//! Versioned music facts and authored content, independent of codecs and engines

use crate::{Anchor, AssetRef, CANONICAL_SAMPLE_RATE, SongTime};
use std::collections::BTreeSet;

pub const CONTENT_SCHEMA_VERSION: u32 = 1;
pub const ANALYSIS_SCHEMA_VERSION: u32 = 2;
pub const MAX_CONTENT_ITEMS: usize = 100_000;
pub const MAX_CONTENT_TEXT_BYTES: usize = 256;
pub const MAX_CONTENT_DIAGNOSTICS_BYTES: usize = 4_096;
pub const MAX_CANONICAL_FRAMES: u64 = 28_800_000;

#[derive(Clone, Debug, PartialEq)]
pub struct MusicAnalysis {
    pub schema_version: u32,
    pub audio_hash: [u8; 32],
    pub capabilities: Option<AnalysisCapabilities>,
    pub tempo_regions: Vec<TempoRegion>,
    pub repetitions: Vec<RepetitionFeature>,
    pub beats: Vec<BeatFeature>,
    pub onsets: Vec<OnsetFeature>,
    pub sections: Vec<SectionFeature>,
    pub energy: Vec<EnergySample>,
    pub diagnostics: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisState {
    NotRun,
    Unsupported,
    Candidate,
    Validated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisSource {
    Algorithm,
    Authored,
    Measured,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnalysisCapability {
    pub state: AnalysisState,
    pub source: AnalysisSource,
    pub confidence: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnalysisCapabilities {
    pub tempo: AnalysisCapability,
    pub onset: AnalysisCapability,
    pub beat: AnalysisCapability,
    pub downbeat: AnalysisCapability,
    pub sections: AnalysisCapability,
    pub repetition: AnalysisCapability,
    pub energy: AnalysisCapability,
}

impl AnalysisCapabilities {
    /// Metadata for final PCM energy and manual sections, without automatic MIR admission
    pub fn authored() -> Self {
        let not_run = AnalysisCapability {
            state: AnalysisState::NotRun,
            source: AnalysisSource::Algorithm,
            confidence: None,
        };
        let unsupported = AnalysisCapability {
            state: AnalysisState::Unsupported,
            ..not_run
        };
        Self {
            tempo: unsupported,
            onset: not_run,
            beat: not_run,
            downbeat: not_run,
            sections: AnalysisCapability {
                source: AnalysisSource::Authored,
                ..not_run
            },
            repetition: unsupported,
            energy: AnalysisCapability {
                state: AnalysisState::Validated,
                source: AnalysisSource::Measured,
                confidence: None,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempoBeatUnit {
    Quarter,
    Eighth,
    DottedQuarter,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoRegion {
    pub start: SongTime,
    pub end: SongTime,
    pub bpm: f32,
    pub beat_unit: Option<TempoBeatUnit>,
    pub confidence: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RepetitionFeature {
    pub source_start: SongTime,
    pub source_end: SongTime,
    pub target_start: SongTime,
    pub target_end: SongTime,
    pub confidence: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatFeature {
    pub time: SongTime,
    pub strength: f32,
    pub downbeat_probability: Option<f32>,
    pub confidence: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnsetFeature {
    pub time: SongTime,
    pub strength: f32,
    pub confidence: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SectionFeature {
    pub start: SongTime,
    pub end: SongTime,
    pub confidence: Option<f32>,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnergySample {
    pub start: SongTime,
    pub frames: u32,
    pub rms: [f32; 2],
    pub peak: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledChart {
    pub schema_version: u32,
    pub audio_hash: [u8; 32],
    pub ruleset_id: String,
    pub anchors: Vec<Anchor>,
    pub sections: Vec<SectionCue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionCue {
    pub id: u64,
    pub time: SongTime,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SongPackage {
    pub schema_version: u32,
    pub song_id: String,
    pub audio: AssetRef,
    pub analysis: AssetRef,
    pub chart: AssetRef,
    pub canonical_sample_rate: u32,
    pub channels: u8,
    pub canonical_frames: u64,
    pub importer_version: String,
    pub analysis_version: String,
    pub chart_version: String,
    pub package_hash: [u8; 32],
}

fn validate_header(version: u32, frames: u64) -> Result<(), String> {
    if version != CONTENT_SCHEMA_VERSION {
        return Err(format!("Unsupported content schema version: {version}"));
    }
    if !(1..=MAX_CANONICAL_FRAMES).contains(&frames) {
        return Err("Canonical content must cover more than zero and at most ten minutes".into());
    }
    Ok(())
}

fn validate_items(count: usize) -> Result<(), String> {
    if count > MAX_CONTENT_ITEMS {
        return Err("Content item limit exceeded".into());
    }
    Ok(())
}

fn validate_text(text: &str) -> Result<(), String> {
    if text.is_empty() || text.len() > MAX_CONTENT_TEXT_BYTES {
        return Err("Content identifiers and labels must contain 1 to 256 UTF-8 bytes".into());
    }
    Ok(())
}

fn validate_time(time: SongTime, frames: u64) -> Result<(), String> {
    if time.frames() < 0 || time.frames() as u64 >= frames {
        return Err("Content event lies outside the canonical audio frames".into());
    }
    Ok(())
}

fn validate_probability(value: Option<f32>) -> Result<(), String> {
    // None means unmeasured, rather than an invented calibrated confidence
    if value.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        return Err("Content probabilities must be unknown or finite values within 0..=1".into());
    }
    Ok(())
}

fn validate_strength(value: f32) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 {
        return Err("Content strength and energy must be finite and nonnegative".into());
    }
    Ok(())
}

impl MusicAnalysis {
    pub fn validate(&self, canonical_frames: u64) -> Result<(), String> {
        match (self.schema_version, self.capabilities) {
            (CONTENT_SCHEMA_VERSION, None)
                if self.tempo_regions.is_empty() && self.repetitions.is_empty() => {}
            (ANALYSIS_SCHEMA_VERSION, Some(capabilities)) => {
                for (capability, has_payload) in [
                    (capabilities.tempo, !self.tempo_regions.is_empty()),
                    (capabilities.onset, !self.onsets.is_empty()),
                    (capabilities.beat, !self.beats.is_empty()),
                    (
                        capabilities.downbeat,
                        self.beats
                            .iter()
                            .any(|beat| beat.downbeat_probability.is_some()),
                    ),
                    (capabilities.sections, !self.sections.is_empty()),
                    (capabilities.repetition, !self.repetitions.is_empty()),
                    (capabilities.energy, !self.energy.is_empty()),
                ] {
                    validate_probability(capability.confidence)?;
                    if matches!(
                        capability.state,
                        AnalysisState::NotRun | AnalysisState::Unsupported
                    ) && (capability.confidence.is_some()
                        || (has_payload && capability.source != AnalysisSource::Authored))
                    {
                        return Err("Unavailable algorithm capability cannot contain measured output or confidence".into());
                    }
                }
                if capabilities.energy.source != AnalysisSource::Measured
                    || !matches!(
                        capabilities.energy.state,
                        AnalysisState::Candidate | AnalysisState::Validated
                    )
                {
                    return Err("Analysis energy must describe measured PCM output".into());
                }
            }
            _ => return Err("Analysis version and capability metadata do not match".into()),
        }
        validate_header(CONTENT_SCHEMA_VERSION, canonical_frames)?;
        for count in [
            self.beats.len(),
            self.onsets.len(),
            self.sections.len(),
            self.energy.len(),
            self.tempo_regions.len(),
            self.repetitions.len(),
        ] {
            validate_items(count)?;
        }
        if self.diagnostics.len() > MAX_CONTENT_DIAGNOSTICS_BYTES {
            return Err("Content diagnostics exceed 4096 UTF-8 bytes".into());
        }
        for beat in &self.beats {
            validate_time(beat.time, canonical_frames)?;
            validate_strength(beat.strength)?;
            validate_probability(beat.downbeat_probability)?;
            validate_probability(beat.confidence)?;
        }
        for onset in &self.onsets {
            validate_time(onset.time, canonical_frames)?;
            validate_strength(onset.strength)?;
            validate_probability(onset.confidence)?;
        }
        if self
            .beats
            .windows(2)
            .any(|pair| pair[0].time >= pair[1].time)
            || self
                .onsets
                .windows(2)
                .any(|pair| pair[0].time >= pair[1].time)
        {
            return Err("Beat and onset times must be strictly increasing".into());
        }
        for section in &self.sections {
            validate_time(section.start, canonical_frames)?;
            if section.end <= section.start || section.end.frames() as u64 > canonical_frames {
                return Err("Analysis sections must be nonempty intervals within the audio".into());
            }
            validate_probability(section.confidence)?;
            validate_text(&section.label)?;
        }
        if self
            .sections
            .windows(2)
            .any(|pair| pair[0].end > pair[1].start)
        {
            return Err("Analysis sections must be ordered and non-overlapping".into());
        }
        for region in &self.tempo_regions {
            validate_time(region.start, canonical_frames)?;
            if region.end <= region.start
                || region.end.frames() as u64 > canonical_frames
                || !region.bpm.is_finite()
                || region.bpm <= 0.0
            {
                return Err(
                    "Tempo regions require finite positive BPM and bounded nonempty intervals"
                        .into(),
                );
            }
            validate_probability(region.confidence)?;
        }
        if self
            .tempo_regions
            .windows(2)
            .any(|pair| pair[0].end > pair[1].start)
        {
            return Err("Tempo regions must be ordered and non-overlapping".into());
        }
        for repetition in &self.repetitions {
            validate_time(repetition.source_start, canonical_frames)?;
            validate_time(repetition.target_start, canonical_frames)?;
            if repetition.source_end <= repetition.source_start
                || repetition.source_end > repetition.target_start
                || repetition.target_end <= repetition.target_start
                || repetition.target_end.frames() as u64 > canonical_frames
            {
                return Err(
                    "Repetition spans require two ordered nonempty intervals within the audio"
                        .into(),
                );
            }
            validate_probability(repetition.confidence)?;
        }
        if self.repetitions.windows(2).any(|pair| {
            (pair[0].source_start, pair[0].target_start)
                >= (pair[1].source_start, pair[1].target_start)
        }) {
            return Err("Repetition relations must be ordered and unique".into());
        }
        let mut through = 0u64;
        for energy in &self.energy {
            if energy.frames == 0 || energy.start.frames() != through as i64 {
                return Err(
                    "Energy intervals must cover the audio continuously from frame zero".into(),
                );
            }
            through += u64::from(energy.frames);
            if through > canonical_frames {
                return Err("Energy interval extends beyond the canonical audio".into());
            }
            for channel in 0..2 {
                validate_strength(energy.rms[channel])?;
                validate_strength(energy.peak[channel])?;
                if energy.rms[channel] > energy.peak[channel] {
                    return Err("Energy RMS cannot exceed its channel peak".into());
                }
            }
        }
        if through != canonical_frames {
            return Err("Energy intervals must cover the entire canonical audio".into());
        }
        Ok(())
    }
}

impl CompiledChart {
    pub fn validate(&self, canonical_frames: u64) -> Result<(), String> {
        validate_header(self.schema_version, canonical_frames)?;
        validate_text(&self.ruleset_id)?;
        validate_items(self.anchors.len())?;
        validate_items(self.sections.len())?;
        let mut anchor_ids = BTreeSet::new();
        for anchor in &self.anchors {
            validate_time(anchor.song_time, canonical_frames)?;
            if !anchor_ids.insert(anchor.id) {
                return Err("Chart Anchor IDs must be unique".into());
            }
        }
        let mut section_ids = BTreeSet::new();
        for section in &self.sections {
            validate_time(section.time, canonical_frames)?;
            validate_text(&section.label)?;
            if !section_ids.insert(section.id) {
                return Err("Chart section IDs must be unique".into());
            }
        }
        if self
            .anchors
            .windows(2)
            .any(|pair| (pair[0].song_time, pair[0].id) >= (pair[1].song_time, pair[1].id))
            || self
                .sections
                .windows(2)
                .any(|pair| (pair[0].time, pair[0].id) >= (pair[1].time, pair[1].id))
        {
            return Err("Chart events must be ordered by time and then ID".into());
        }
        Ok(())
    }
}

impl SongPackage {
    pub fn validate(&self) -> Result<(), String> {
        validate_header(self.schema_version, self.canonical_frames)?;
        if self.canonical_sample_rate != CANONICAL_SAMPLE_RATE || self.channels != 2 {
            return Err("Song packages require 48000 Hz stereo canonical audio".into());
        }
        for text in [
            &self.song_id,
            &self.importer_version,
            &self.analysis_version,
            &self.chart_version,
        ] {
            validate_text(text)?;
        }
        for (asset, name) in [
            (&self.audio, "song.audio.ogg"),
            (&self.analysis, "analysis.bin"),
            (&self.chart, "chart.bin"),
        ] {
            if asset.file_name != name || asset.byte_len == 0 {
                return Err(format!(
                    "Song package object must be named {name} and contain bytes"
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis() -> MusicAnalysis {
        MusicAnalysis {
            capabilities: None,
            tempo_regions: Vec::new(),
            repetitions: Vec::new(),
            schema_version: CONTENT_SCHEMA_VERSION,
            audio_hash: [0; 32],
            beats: vec![BeatFeature {
                time: SongTime::ZERO,
                strength: 2.0,
                downbeat_probability: None,
                confidence: None,
            }],
            onsets: vec![OnsetFeature {
                time: SongTime::from_frames(99),
                strength: 1.5,
                confidence: Some(0.5),
            }],
            sections: vec![SectionFeature {
                start: SongTime::ZERO,
                end: SongTime::from_frames(100),
                confidence: None,
                label: "hand-authored fixture".into(),
            }],
            energy: vec![EnergySample {
                start: SongTime::ZERO,
                frames: 100,
                rms: [0.0, 1.1],
                peak: [0.0, 1.5],
            }],
            diagnostics: "Fixture facts; confidence has not been calibrated".into(),
        }
    }

    #[test]
    fn analysis_accepts_unknown_confidence_and_checks_the_real_timeline_and_energy() {
        let original = analysis();
        assert_eq!(original.validate(100), Ok(()));
        assert!(original.validate(0).is_err());
        assert!(original.validate(MAX_CANONICAL_FRAMES + 1).is_err());
        for value in [f32::NAN, f32::INFINITY, -0.1] {
            let mut changed = original.clone();
            changed.beats[0].strength = value;
            assert!(changed.validate(100).is_err());
            changed = original.clone();
            changed.energy[0].peak[0] = value;
            assert!(changed.validate(100).is_err());
        }
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            let mut changed = original.clone();
            changed.onsets[0].confidence = Some(value);
            assert!(changed.validate(100).is_err());
            changed = original.clone();
            changed.beats[0].downbeat_probability = Some(value);
            assert!(changed.validate(100).is_err());
        }
        for time in [-1, 100, i64::MAX] {
            let mut changed = original.clone();
            changed.onsets[0].time = SongTime::from_frames(time);
            assert!(changed.validate(100).is_err());
        }
        for frames in [0, 99, 101, u32::MAX] {
            let mut changed = original.clone();
            changed.energy[0].frames = frames;
            assert!(changed.validate(100).is_err());
        }
        let mut changed = original.clone();
        changed.energy[0].start = SongTime::from_frames(1);
        assert!(changed.validate(100).is_err());
        changed = original.clone();
        changed.energy[0].rms[0] = 0.1;
        assert!(changed.validate(100).is_err());
        changed.energy.clear();
        assert!(changed.validate(100).is_err());
        changed = original.clone();
        changed.beats.push(changed.beats[0]);
        assert!(changed.validate(100).is_err());
        changed = original.clone();
        changed.sections.push(changed.sections[0].clone());
        assert!(changed.validate(100).is_err());
        changed = original;
        changed.diagnostics = "x".repeat(MAX_CONTENT_DIAGNOSTICS_BYTES + 1);
        assert!(changed.validate(100).is_err());
    }

    #[test]
    fn charts_keep_existing_anchor_semantics_and_reject_ambiguous_or_out_of_bounds_ids() {
        let mut chart = CompiledChart {
            schema_version: CONTENT_SCHEMA_VERSION,
            audio_hash: [0; 32],
            ruleset_id: "duo-v1".into(),
            anchors: vec![
                Anchor {
                    id: 0,
                    song_time: SongTime::ZERO,
                },
                Anchor {
                    id: 1,
                    song_time: SongTime::ZERO,
                },
            ],
            sections: vec![SectionCue {
                id: 0,
                time: SongTime::ZERO,
                label: "start".into(),
            }],
        };
        assert_eq!(chart.validate(100), Ok(()));
        chart.anchors.swap(0, 1);
        assert!(chart.validate(100).is_err());
        chart.anchors.swap(0, 1);
        chart.anchors[1].id = 0;
        assert!(chart.validate(100).is_err());
        chart.anchors.clear();
        assert_eq!(chart.validate(100), Ok(()));
        chart.sections[0].time = SongTime::from_frames(100);
        assert!(chart.validate(100).is_err());
        chart.sections[0].time = SongTime::ZERO;
        chart.sections.push(chart.sections[0].clone());
        assert!(chart.validate(100).is_err());
        chart.sections.clear();
        chart.ruleset_id = "中".repeat(86);
        assert!(chart.validate(100).is_err());
        chart.ruleset_id = "duo-v1".into();
        chart.anchors = vec![
            Anchor {
                id: 0,
                song_time: SongTime::ZERO
            };
            MAX_CONTENT_ITEMS + 1
        ];
        assert!(chart.validate(100).unwrap_err().contains("limit"));
    }

    #[test]
    fn package_metadata_has_fixed_objects_and_rejects_unsupported_versions_and_limits() {
        let asset = |name: &str| AssetRef {
            file_name: name.into(),
            byte_len: 1,
            blake3: [0; 32],
        };
        let original = SongPackage {
            schema_version: CONTENT_SCHEMA_VERSION,
            song_id: "manual-fixture".into(),
            audio: asset("song.audio.ogg"),
            analysis: asset("analysis.bin"),
            chart: asset("chart.bin"),
            canonical_sample_rate: CANONICAL_SAMPLE_RATE,
            channels: 2,
            canonical_frames: MAX_CANONICAL_FRAMES,
            importer_version: "fixture-v1".into(),
            analysis_version: "measured-energy-v1".into(),
            chart_version: "hand-authored-v1".into(),
            package_hash: [0; 32],
        };
        assert_eq!(original.validate(), Ok(()));
        let mut changed = original.clone();
        changed.schema_version += 1;
        assert!(changed.validate().is_err());
        changed = original.clone();
        changed.canonical_frames += 1;
        assert!(changed.validate().is_err());
        changed = original.clone();
        changed.channels = 1;
        assert!(changed.validate().is_err());
        changed = original.clone();
        changed.canonical_sample_rate = 44_100;
        assert!(changed.validate().is_err());
        for name in [
            "../song.audio.ogg",
            "/song.audio.ogg",
            "song.audio.ogg/child",
        ] {
            changed = original.clone();
            changed.audio.file_name = name.into();
            assert!(changed.validate().is_err());
        }
        changed = original.clone();
        changed.analysis.byte_len = 0;
        assert!(changed.validate().is_err());
        changed = original;
        changed.chart_version.clear();
        assert!(changed.validate().is_err());
    }
}
