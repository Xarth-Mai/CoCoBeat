//! Offline musical evidence selects presentation without changing gameplay facts

use std::sync::Arc;

use cocobeat_schema::{
    MusicAnalysis, SongTime,
    content::{MusicPresentation, PresentationWindow},
};
use serde::{Deserialize, Serialize};

use crate::feedback_audio::{
    FeedbackContext, FeedbackTimbre, MAX_SHORT_TONAL_SECONDS, MAX_TONAL_SECONDS,
};

pub(crate) const REPLAY_FEEDBACK_FRESHNESS_FRAMES: i64 = 4_800;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorldTheme {
    #[default]
    Neon,
    Forest,
    Candy,
    StarSea,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MotionStyle {
    Gentle,
    Playful,
    #[default]
    Energetic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PresentationOverrides {
    pub world: Option<WorldTheme>,
    pub timbre: Option<FeedbackTimbre>,
    pub motion: Option<MotionStyle>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PresentationPlan {
    pub music: Option<MusicPresentation>,
    pub sections: Vec<SongTime>,
    pub world: WorldTheme,
    pub seed: u64,
    pub tempo: Vec<cocobeat_schema::TempoRegion>,
    pub energy_reference: f32,
}

impl PresentationPlan {
    pub fn compile(content_id: &str, analysis: &MusicAnalysis) -> Self {
        let music = analysis.presentation.clone();
        let mut energies: Vec<_> = music
            .iter()
            .flat_map(|music| &music.windows)
            .map(|window| window.energy)
            .filter(|energy| energy.is_finite() && *energy > 0.0)
            .collect();
        energies.sort_unstable_by(f32::total_cmp);
        let energy_reference = energies
            .get((energies.len() * 95).div_ceil(100).saturating_sub(1))
            .copied()
            .unwrap_or(0.01)
            .max(0.01);
        // These are transparent aesthetic defaults, not a music-genre classifier
        let world =
            music
                .as_ref()
                .filter(|m| !m.windows.is_empty())
                .map_or(WorldTheme::Neon, |music| {
                    let count = music.windows.len() as f32;
                    let brightness =
                        music.windows.iter().map(|w| w.brightness).sum::<f32>() / count;
                    let density =
                        music.windows.iter().map(|w| w.onset_density).sum::<f32>() / count;
                    let energy = music.windows.iter().map(|w| w.energy).sum::<f32>() / count;
                    if density < 1.2 && energy < 0.3 {
                        WorldTheme::StarSea
                    } else if brightness < 0.32 {
                        WorldTheme::Forest
                    } else if brightness > 0.62 && density < 4.0 {
                        WorldTheme::Candy
                    } else {
                        WorldTheme::Neon
                    }
                });
        let mut sections: Vec<_> = analysis.sections.iter().map(|s| s.start).collect();
        sections.push(SongTime::ZERO);
        sections.sort_unstable();
        sections.dedup();
        Self {
            music,
            sections,
            world,
            tempo: analysis.tempo_regions.clone(),
            energy_reference,
            seed: content_id.bytes().fold(0xcbf29ce484222325, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            }),
        }
    }

    pub fn context(
        &self,
        time: SongTime,
        event_id: u64,
        family: FeedbackTimbre,
    ) -> FeedbackContext {
        let beat_seconds = self
            .tempo
            .iter()
            .find(|t| t.start <= time && time < t.end)
            .map_or(0.5, |t| (60.0 / t.bpm).clamp(0.2, 2.0));
        let window = self.music.as_ref().and_then(|music| {
            let index = music.windows.partition_point(|w| w.start <= time);
            index
                .checked_sub(1)
                .and_then(|i| music.windows.get(i))
                .filter(|w| time < w.end)
        });
        let (chord_mask, short_tonal) =
            match self.phrase_chord_mask(time, beat_seconds, MAX_TONAL_SECONDS) {
                Some(mask) => (Some(mask), false),
                None => {
                    let mask = self.phrase_chord_mask(time, beat_seconds, MAX_SHORT_TONAL_SECONDS);
                    (mask, mask.is_some())
                }
            };
        FeedbackContext {
            event_id: event_id ^ self.seed,
            song_time: time,
            family,
            chord_mask,
            short_tonal,
            energy: window.map_or(0.4, |w| {
                (w.energy / self.energy_reference.max(0.01)).clamp(0.0, 1.0)
            }),
            density: window.map_or(0.3, |w| (w.onset_density / 8.0).clamp(0.0, 1.0)),
            beat_seconds,
        }
    }

    fn phrase_chord_mask(
        &self,
        time: SongTime,
        beat_seconds: f32,
        tail_seconds: f32,
    ) -> Option<u16> {
        // Cover the last duo onset plus the selected bank at its slowest pitch rate
        let frames =
            ((f64::from(beat_seconds) * 0.25 + f64::from(tail_seconds)) * 48_000.0).ceil() as i64;
        let end = time.checked_add_frames(frames + REPLAY_FEEDBACK_FRESHNESS_FRAMES)?;
        let music = self.music.as_ref()?;
        let index = music.windows.partition_point(|window| window.end <= time);
        let mut through = time;
        let mut mask = 0xfff;
        for window in &music.windows[index..] {
            if window.start > through || window.end <= through {
                return None;
            }
            mask &= gated_chord_mask(window)?;
            if mask == 0 {
                return None;
            }
            through = window.end;
            if through >= end {
                return Some(mask);
            }
        }
        None
    }
}

fn gated_chord_mask(window: &PresentationWindow) -> Option<u16> {
    let chord = window.chord?;
    // Conservative heuristic gate; candidates remain explicitly uncalibrated
    (window.tonal_confidence >= 0.82 && chord.confidence >= 0.75).then(|| {
        [0, if chord.minor { 3 } else { 4 }, 7]
            .into_iter()
            .fold(0, |mask, interval| {
                mask | (1 << ((chord.root + interval) % 12))
            })
    })
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PresentationVisual {
    pub plan: Arc<PresentationPlan>,
    pub world: WorldTheme,
    pub section_index: usize,
    pub section_elapsed: f32,
    pub event_seed: u64,
    pub sync_streak: u32,
    pub overrides: PresentationOverrides,
}

impl PresentationVisual {
    pub fn recommendation(world: WorldTheme) -> PresentationOverrides {
        PresentationOverrides {
            world: Some(world),
            timbre: Some(match world {
                WorldTheme::Neon => FeedbackTimbre::Elastic,
                WorldTheme::Forest => FeedbackTimbre::Wood,
                WorldTheme::Candy => FeedbackTimbre::Crisp,
                WorldTheme::StarSea => FeedbackTimbre::Glass,
            }),
            motion: Some(match world {
                WorldTheme::Neon => MotionStyle::Energetic,
                WorldTheme::Forest | WorldTheme::StarSea => MotionStyle::Gentle,
                WorldTheme::Candy => MotionStyle::Playful,
            }),
        }
    }

    pub fn context(&self, time: SongTime, id: u64) -> FeedbackContext {
        let family = self
            .overrides
            .timbre
            .unwrap_or_else(|| Self::recommendation(self.world).timbre.unwrap());
        self.plan.context(time, id, family)
    }
    pub fn update(
        &mut self,
        plan: &Arc<PresentationPlan>,
        time: SongTime,
        overrides: PresentationOverrides,
    ) {
        self.plan = Arc::clone(plan);
        self.overrides = overrides;
        self.world = overrides.world.unwrap_or(plan.world);
        self.section_index = plan
            .sections
            .partition_point(|start| *start <= time)
            .saturating_sub(1);
        let start = plan
            .sections
            .get(self.section_index)
            .copied()
            .unwrap_or_default();
        self.section_elapsed = (time.as_seconds_f64() - start.as_seconds_f64()).max(0.0) as f32;
    }

    pub fn motion_style(&self) -> MotionStyle {
        self.overrides
            .motion
            .unwrap_or_else(|| Self::recommendation(self.world).motion.unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_schema::content::{
        ANALYSIS_SCHEMA_VERSION, AnalysisCapabilities, AnalysisCapability, AnalysisSource,
        AnalysisState, ChordCandidate, EnergySample,
    };

    fn window(start: i64, end: i64, root: Option<u8>) -> PresentationWindow {
        let mut chroma = [0.0; 12];
        if let Some(root) = root {
            for interval in [0, 4, 7] {
                chroma[((root + interval) % 12) as usize] = 1.0 / 3.0;
            }
        }
        PresentationWindow {
            start: SongTime::from_frames(start),
            end: SongTime::from_frames(end),
            chroma,
            chord: root.map(|root| ChordCandidate {
                root,
                minor: false,
                confidence: 0.95,
            }),
            key: None,
            tonal_confidence: if root.is_some() { 0.95 } else { 0.0 },
            onset_density: 4.0,
            brightness: 0.4,
            energy: 0.25,
        }
    }

    fn analysis(windows: Option<Vec<PresentationWindow>>) -> MusicAnalysis {
        MusicAnalysis {
            schema_version: ANALYSIS_SCHEMA_VERSION,
            audio_hash: [7; 32],
            capabilities: Some(AnalysisCapabilities::authored()),
            presentation: windows.map(|windows| MusicPresentation {
                capability: AnalysisCapability {
                    state: AnalysisState::Candidate,
                    source: AnalysisSource::Algorithm,
                    confidence: None,
                },
                windows,
            }),
            tempo_regions: vec![],
            repetitions: vec![],
            beats: vec![],
            onsets: vec![],
            sections: vec![],
            energy: vec![EnergySample {
                start: SongTime::ZERO,
                frames: 72_000,
                rms: [0.25; 2],
                peak: [0.5; 2],
            }],
            diagnostics: "Synthetic presentation test; music quality is unvalidated".into(),
        }
    }

    #[test]
    fn legacy_and_unknown_harmony_keep_percussion_fallback() {
        let mut legacy = analysis(None);
        legacy.schema_version = 1;
        legacy.capabilities = None;
        legacy.validate(72_000).unwrap();
        let plan = PresentationPlan::compile("legacy", &legacy);
        let context = plan.context(SongTime::ZERO, 71, FeedbackTimbre::Wood);
        assert_eq!(context.chord_mask, None);
        assert_eq!(context.energy, 0.4);
        assert_eq!(context.beat_seconds, 0.5);
        let unknown = analysis(Some(vec![window(0, 72_000, None)]));
        unknown.validate(72_000).unwrap();
        let plan = PresentationPlan::compile("unknown", &unknown);
        assert_eq!(
            plan.context(SongTime::ZERO, 71, FeedbackTimbre::Glass)
                .chord_mask,
            None
        );
    }

    #[test]
    fn candidate_chords_intersect_across_entire_phrase_and_unknown_tail_is_unpitched() {
        let mut candidate = analysis(Some(vec![
            window(0, 48_000, Some(0)),
            window(48_000, 72_000, Some(7)),
        ]));
        candidate.validate(72_000).unwrap();
        let plan = PresentationPlan::compile("candidate", &candidate);
        let context = plan.context(SongTime::ZERO, 5, FeedbackTimbre::Plucks);
        assert_eq!(context.chord_mask, Some((1 << 0) | (1 << 4) | (1 << 7)));
        assert!(!context.short_tonal);
        assert_eq!(context.event_id, 5 ^ plan.seed);
        assert_eq!(
            plan.context(SongTime::from_frames(16_800), 5, FeedbackTimbre::Plucks)
                .chord_mask,
            Some(1 << 7)
        );
        assert_eq!(
            plan.context(SongTime::from_frames(68_000), 5, FeedbackTimbre::Plucks)
                .chord_mask,
            None
        );
        let repeated = plan.context(SongTime::ZERO, 5, FeedbackTimbre::Plucks);
        assert_eq!(
            (context.event_id, context.chord_mask),
            (repeated.event_id, repeated.chord_mask)
        );
        for next_root in [None, Some(6)] {
            candidate.presentation.as_mut().unwrap().windows[1] = window(48_000, 72_000, next_root);
            let plan = PresentationPlan::compile("candidate", &candidate);
            assert_eq!(
                plan.context(SongTime::from_frames(28_800), 5, FeedbackTimbre::Plucks)
                    .chord_mask,
                None
            );
        }
        candidate.presentation.as_mut().unwrap().windows[0].tonal_confidence = 0.81;
        let plan = PresentationPlan::compile("candidate", &candidate);
        assert_eq!(
            plan.context(SongTime::ZERO, 5, FeedbackTimbre::Plucks)
                .chord_mask,
            None
        );
    }

    #[test]
    fn short_tonal_requires_its_entire_tail_and_keeps_real_dynamics() {
        let long =
            PresentationPlan::compile("tail", &analysis(Some(vec![window(0, 72_000, Some(0))])));
        let original = long.context(SongTime::ZERO, 9, FeedbackTimbre::Wood);
        assert!(original.chord_mask.is_some() && !original.short_tonal);
        for next_root in [None, Some(6)] {
            let short = PresentationPlan::compile(
                "tail",
                &analysis(Some(vec![
                    window(0, 26_400, Some(0)),
                    window(26_400, 72_000, next_root),
                ])),
            );
            let adapted = short.context(SongTime::ZERO, 9, FeedbackTimbre::Wood);
            assert!(adapted.short_tonal);
            assert_eq!(adapted.chord_mask, original.chord_mask);
            assert_eq!(
                (adapted.energy, adapted.density, adapted.event_id),
                (original.energy, original.density, original.event_id)
            );
            let crossing = short.context(SongTime::from_frames(480), 9, FeedbackTimbre::Wood);
            assert_eq!(crossing.chord_mask, None);
            assert!(!crossing.short_tonal);
        }
    }

    #[test]
    fn relative_energy_preserves_dynamics_gain_invariance_and_silence() {
        let mut source = analysis(Some(vec![
            window(0, 24_000, None),
            window(24_000, 48_000, None),
            window(48_000, 72_000, None),
        ]));
        for (window, energy) in source
            .presentation
            .as_mut()
            .unwrap()
            .windows
            .iter_mut()
            .zip([0.0, 0.015, 0.05])
        {
            window.energy = energy;
        }
        let plan = PresentationPlan::compile("dynamics", &source);
        let sample = |plan: &PresentationPlan, frame| {
            plan.context(SongTime::from_frames(frame), 1, FeedbackTimbre::Wood)
                .energy
        };
        assert_eq!(sample(&plan, 0), 0.0);
        assert!((sample(&plan, 24_000) - 0.3).abs() < 1e-6);
        assert_eq!(sample(&plan, 48_000), 1.0);
        for window in &mut source.presentation.as_mut().unwrap().windows {
            window.energy *= 2.0;
        }
        let amplified = PresentationPlan::compile("dynamics", &source);
        for frame in [0, 24_000, 48_000] {
            assert!((sample(&plan, frame) - sample(&amplified, frame)).abs() < 1e-6);
        }
        for window in &mut source.presentation.as_mut().unwrap().windows {
            window.energy = 0.0;
        }
        let silence = PresentationPlan::compile("silence", &source);
        assert_eq!(silence.energy_reference, 0.01);
        assert_eq!(sample(&silence, 0), 0.0);
    }

    #[derive(Deserialize)]
    struct AuthoredEvent {
        event_id: u64,
        time_seconds: f64,
        kind: String,
        player: Option<String>,
        precise: Option<bool>,
        family: Option<FeedbackTimbre>,
    }

    /// Resolves authored audition events through the real imported package plan
    #[test]
    #[ignore = "requires explicit imported package, authored events and target output path"]
    fn export_imported_contexts() {
        let package = std::env::var("COCOBEAT_PRESENTATION_PACKAGE").unwrap();
        let events = std::env::var("COCOBEAT_PRESENTATION_EVENTS").unwrap();
        let output = std::env::var("COCOBEAT_PRESENTATION_OUT").unwrap();
        let output = std::path::Path::new(&output);
        let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .canonicalize()
            .unwrap();
        assert!(
            output
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .starts_with(target)
        );
        assert!(!output.is_symlink());
        let (content, _) = crate::content::load_package(std::path::Path::new(&package)).unwrap();
        let events: Vec<AuthoredEvent> =
            serde_json::from_slice(&std::fs::read(events).unwrap()).unwrap();
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].time_seconds <= pair[1].time_seconds)
        );
        let mut resolved = Vec::with_capacity(events.len());
        for event in events {
            assert!(matches!(event.kind.as_str(), "hit" | "duo"));
            if event.kind == "hit" {
                assert!(matches!(event.player.as_deref(), Some("P1" | "P2")));
            }
            let time = SongTime::try_from_seconds_f64(event.time_seconds).unwrap();
            assert!(time >= SongTime::ZERO && time < content.end);
            let family = event.family.unwrap_or_else(|| {
                PresentationVisual::recommendation(content.presentation.world)
                    .timbre
                    .unwrap()
            });
            let context = content.presentation.context(time, event.event_id, family);
            resolved.push(serde_json::json!({
                "event_id":context.event_id,"time_seconds":event.time_seconds,"kind":event.kind,
                "player":event.player,"precise":event.precise,"family":context.family,
                "chord_mask":context.chord_mask,"short_tonal":context.short_tonal,"energy":context.energy,"density":context.density,"beat_seconds":context.beat_seconds,
            }));
        }
        std::fs::write(output, serde_json::to_vec_pretty(&resolved).unwrap()).unwrap();
        eprintln!(
            "Resolved {} authored events from {} ({} pitched); offline audition input, not actual gameplay events",
            resolved.len(),
            content.content_id,
            resolved
                .iter()
                .filter(|event| !event["chord_mask"].is_null())
                .count()
        );
    }
}
