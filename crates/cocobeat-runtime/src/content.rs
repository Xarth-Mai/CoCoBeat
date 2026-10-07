//! Session facts and final PCM are loaded together before a song can start

use crate::{audio::sound_data, dev_song};
use cocobeat_schema::{Anchor, SectionCue, SongTime};
use kira::{Frame, sound::static_sound::StaticSoundData};
use std::{path::Path, sync::Arc};

pub const CONTENT_ID: &str = "dev64-pcm16-3390dd080cb536fd4-anchors-v1";
pub const RULES_ID: &str = "duo-watermark-v1";

#[derive(Clone, Debug)]
pub struct SongContent {
    pub content_id: String,
    pub end: SongTime,
    pub anchors: Vec<Anchor>,
    pub sections: Vec<SectionCue>,
    pub stage: Option<Arc<cocobeat_stage::StagePlan>>,
}

impl SongContent {
    pub fn development() -> Self {
        Self {
            content_id: CONTENT_ID.into(),
            end: SongTime::from_frames(i64::from(dev_song::FRAMES)),
            anchors: dev_song::anchors(),
            sections: dev_song::sections(),
            stage: None,
        }
    }

    pub fn section_cues(&self, time: SongTime) -> (Option<&SectionCue>, Option<&SectionCue>) {
        if time < SongTime::ZERO || time >= self.end {
            return (None, None);
        }
        let split = self.sections.partition_point(|cue| cue.time <= time);
        let latest = split.checked_sub(1).map(|index| &self.sections[index]);
        let next = self.sections.get(split).map(|first| {
            // Validated cues sort by (time, id); preview and arrival select the same ID
            let end = self.sections.partition_point(|cue| cue.time <= first.time);
            &self.sections[end - 1]
        });
        (latest, next)
    }
}

pub fn development_sound() -> StaticSoundData {
    sound_data(
        dev_song::samples()
            .into_iter()
            .map(|[left, right]| Frame::new(left, right))
            .collect(),
    )
}

pub fn load_package(path: &Path) -> Result<(SongContent, StaticSoundData), String> {
    load_package_version(path, cocobeat_stage::COMPILER_VERSION)
}

pub(crate) fn load_package_version(
    path: &Path,
    version: u32,
) -> Result<(SongContent, StaticSoundData), String> {
    load_package_named(path, version).map(|(content, sound, _)| (content, sound))
}

pub(crate) fn load_package_named(
    path: &Path,
    version: u32,
) -> Result<(SongContent, StaticSoundData, String), String> {
    let mut pcm = Vec::new();
    let package = cocobeat_media::read_package(path, |block| {
        pcm.try_reserve(block.len())
            .map_err(|error| format!("Cannot allocate song PCM: {error}"))?;
        pcm.extend(block.iter().map(|&[left, right]| Frame::new(left, right)));
        Ok(())
    })?;
    if package.chart.ruleset_id != RULES_ID {
        return Err(format!(
            "Unsupported song ruleset: {}",
            package.chart.ruleset_id
        ));
    }
    let hash: String = package
        .manifest
        .package_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let content_id = format!("package-blake3:{hash}");
    let end = SongTime::from_frames(package.manifest.canonical_frames as i64);
    let stage =
        cocobeat_stage::compile_version(&content_id, end, &package.analysis.sections, version)?;
    Ok((
        SongContent {
            content_id,
            end,
            anchors: package.chart.anchors,
            sections: package.chart.sections,
            stage: Some(Arc::new(stage)),
        },
        sound_data(pcm),
        package.manifest.song_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_media::{PackageBuildInput, build_package, decode_canonical};
    use cocobeat_schema::{CONTENT_SCHEMA_VERSION, CompiledChart, EnergySample, MusicAnalysis};
    use std::{
        fs,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    const STEREO: &[u8] =
        include_bytes!("../../../testdata/synthetic/media-import/stereo-canonical.ogg");
    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cocobeat-runtime-content-test-{}-{}",
                std::process::id(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            fs::write(path.join("source.ogg"), STEREO).unwrap();
            Self(path)
        }

        fn package(
            &self,
            name: &str,
            ruleset_id: &str,
            anchor_frame: i64,
            sections: Vec<SectionCue>,
        ) -> PathBuf {
            let destination = self.0.join(name);
            build_package(
                self.0.join("source.ogg"),
                4_800,
                &destination,
                |staged, prepared| {
                    let mut sum = [0.0_f64; 2];
                    let mut peak = [0.0_f32; 2];
                    decode_canonical(staged, prepared.canonical_frames, |block| {
                        for frame in block {
                            for channel in 0..2 {
                                sum[channel] += f64::from(frame[channel]).powi(2);
                                peak[channel] = peak[channel].max(frame[channel].abs());
                            }
                        }
                        Ok(())
                    })?;
                    Ok(PackageBuildInput {
                        song_id: "runtime-stereo-fixture".into(),
                        importer_version: "fixture-v1".into(),
                        analysis_version: "measured-energy-v1".into(),
                        chart_version: "hand-authored-v1".into(),
                        analysis: MusicAnalysis {
                            capabilities: None,
                            tempo_regions: Vec::new(),
                            repetitions: Vec::new(),
                            schema_version: CONTENT_SCHEMA_VERSION,
                            audio_hash: prepared.asset.blake3,
                            beats: vec![],
                            onsets: vec![],
                            sections: vec![cocobeat_schema::SectionFeature {
                                start: SongTime::from_frames(1_200),
                                end: SongTime::from_frames(3_600),
                                confidence: None,
                                label: "Analysis interval independent of chart cues".into(),
                            }],
                            energy: vec![EnergySample {
                                start: SongTime::ZERO,
                                frames: 4_800,
                                rms: sum.map(|value| (value / 4_800.0).sqrt() as f32),
                                peak,
                            }],
                            diagnostics: "Measured stereo energy; manually authored test Anchor"
                                .into(),
                        },
                        chart: CompiledChart {
                            schema_version: CONTENT_SCHEMA_VERSION,
                            audio_hash: prepared.asset.blake3,
                            ruleset_id: ruleset_id.into(),
                            anchors: vec![Anchor {
                                id: 5,
                                song_time: SongTime::from_frames(anchor_frame),
                            }],
                            sections,
                        },
                    })
                },
            )
            .unwrap();
            destination
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn recorded_stage_versions_load_the_same_validated_pcm_and_analysis_intervals() {
        let root = TestDirectory::new();
        let path = root.package("versioned", RULES_ID, 1_200, vec![]);
        let (old, old_pcm) = load_package_version(&path, 1).unwrap();
        let (current, current_pcm) = load_package_version(&path, 2).unwrap();
        assert_eq!(old.content_id, current.content_id);
        assert_eq!(old_pcm.frames, current_pcm.frames);
        for (content, version) in [(old, 1), (current, 2)] {
            let stage = content.stage.unwrap();
            assert_eq!(stage.compiler_version(), version);
            assert_eq!(stage.segments().len(), 3);
            assert!(content.sections.is_empty());
        }
        assert!(load_package_version(&path, 3).is_err());
    }

    #[test]
    fn library_named_load_uses_the_same_validated_manifest_and_pcm() {
        let root = TestDirectory::new();
        let path = root.package("named", RULES_ID, 1_200, vec![]);
        let (content, sound, song_id) =
            load_package_named(&path, cocobeat_stage::COMPILER_VERSION).unwrap();
        let (ordinary, ordinary_sound) = load_package(&path).unwrap();
        assert_eq!(song_id, "runtime-stereo-fixture");
        assert_eq!(content.content_id, ordinary.content_id);
        assert_eq!(content.end, ordinary.end);
        assert_eq!(sound.frames, ordinary_sound.frames);
        assert!(load_package_named(&path, 3).is_err());
        let unknown = root.package("unknown-named", "unknown-rules-v1", 1_200, vec![]);
        assert!(
            load_package_named(&unknown, cocobeat_stage::COMPILER_VERSION)
                .unwrap_err()
                .contains("Unsupported song ruleset")
        );
        let mut bytes = fs::read(path.join("song.audio.ogg")).unwrap();
        bytes.pop();
        fs::write(path.join("song.audio.ogg"), bytes).unwrap();
        assert!(load_package_named(&path, cocobeat_stage::COMPILER_VERSION).is_err());
    }

    #[test]
    fn package_pcm_is_exact_and_shared_while_identity_binds_the_chart() {
        let root = TestDirectory::new();
        let first = root.package("first", RULES_ID, 1_200, vec![]);
        let second = root.package("second", RULES_ID, 2_400, vec![]);
        let (content, sound) = load_package(&first).unwrap();
        let (other_content, other_sound) = load_package(&second).unwrap();
        assert_ne!(content.content_id, other_content.content_id);
        assert!(content.content_id.starts_with("package-blake3:"));
        assert_eq!(content.content_id.len(), "package-blake3:".len() + 64);
        assert_eq!(content.end.frames(), 4_800);
        assert_eq!(content.anchors.len(), 1);
        assert_eq!(content.anchors[0].id, 5);
        assert_eq!(content.anchors[0].song_time.frames(), 1_200);
        assert!(content.sections.is_empty());
        let stage = content.stage.as_ref().unwrap();
        assert_eq!(stage.content_id(), content.content_id);
        assert_eq!(stage.end(), content.end);
        assert_eq!(stage.segments().len(), 3);
        assert_eq!(
            stage
                .sample(SongTime::from_frames(2_400))
                .unwrap()
                .half_width_mm,
            3_575
        );
        assert!(Arc::ptr_eq(stage, content.clone().stage.as_ref().unwrap()));
        assert_eq!(sound.sample_rate, 48_000);
        assert_eq!(sound.frames.len(), 4_800);
        assert_eq!(sound.frames, other_sound.frames);
        assert!(Arc::ptr_eq(&sound.frames, &sound.clone().frames));
        let mut offset = 0;
        decode_canonical(root.0.join("source.ogg"), 4_800, |block| {
            for frame in block {
                assert_eq!(sound.frames[offset], Frame::new(frame[0], frame[1]));
                offset += 1;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(offset, sound.frames.len());
    }

    #[test]
    fn package_cues_are_preserved_and_alone_change_content_identity() {
        let root = TestDirectory::new();
        let sections = vec![
            SectionCue {
                id: 2,
                time: SongTime::ZERO,
                label: "雨夜\n\t{label}".into(),
            },
            SectionCue {
                id: 4,
                time: SongTime::ZERO,
                label: " \t ".into(),
            },
            SectionCue {
                id: 7,
                time: SongTime::from_frames(4_799),
                label: "ending".into(),
            },
        ];
        let mut changed = sections.clone();
        changed[0].label = "雨后".into();
        let (content, sound) =
            load_package(&root.package("cues", RULES_ID, 1_200, sections.clone())).unwrap();
        let (other, other_sound) =
            load_package(&root.package("changed-cues", RULES_ID, 1_200, changed.clone())).unwrap();
        assert_eq!(content.sections, sections);
        assert_eq!(other.sections, changed);
        assert_eq!(content.anchors, other.anchors);
        assert_eq!(sound.frames, other_sound.frames);
        assert_ne!(content.content_id, other.content_id);
    }

    #[test]
    fn section_queries_use_point_times_and_same_frame_highest_ids() {
        let mut content = SongContent {
            content_id: "cue-query".into(),
            end: SongTime::from_frames(4_800),
            anchors: vec![],
            stage: None,
            sections: [(4, 100), (7, 100), (9, 500), (3, 4_799), (10, 4_799)]
                .into_iter()
                .map(|(id, frame)| SectionCue {
                    id,
                    time: SongTime::from_frames(frame),
                    label: "marker".into(),
                })
                .collect(),
        };
        for (frame, expected) in [
            (-1, (None, None)),
            (0, (None, Some(7))),
            (99, (None, Some(7))),
            (100, (Some(7), Some(9))),
            (499, (Some(7), Some(9))),
            (500, (Some(9), Some(10))),
            (4_798, (Some(9), Some(10))),
            (4_799, (Some(10), None)),
            (4_800, (None, None)),
            (i64::MAX, (None, None)),
            (0, (None, Some(7))),
            (4_799, (Some(10), None)),
            (100, (Some(7), Some(9))),
        ] {
            let (latest, next) = content.section_cues(SongTime::from_frames(frame));
            assert_eq!((latest.map(|cue| cue.id), next.map(|cue| cue.id)), expected);
        }
        content.sections.clear();
        assert_eq!(content.section_cues(SongTime::ZERO), (None, None));
    }

    #[test]
    fn package_pcm_plays_to_eof_and_restarts_from_zero_through_kira() {
        use kira::{
            AudioManager, AudioManagerSettings,
            backend::mock::{MockBackend, MockBackendSettings},
            effect::Effect,
            info::Info,
            sound::PlaybackState,
            track::MainTrackBuilder,
        };
        use std::sync::Mutex;

        // MockBackend keeps its output private; observe its main track without changing samples
        struct Capture(Arc<Mutex<Vec<Frame>>>);
        impl Effect for Capture {
            fn process(&mut self, input: &mut [Frame], _: f64, _: &Info) {
                self.0.lock().unwrap().extend_from_slice(input);
            }
        }

        let root = TestDirectory::new();
        let (_, sound) = load_package(&root.package("playback", RULES_ID, 1_200, vec![])).unwrap();
        // Kira's four-frame interpolation history can drain after the final source sample
        let maximum_frames = sound.frames.len() + 4;
        let captured = Arc::new(Mutex::new(Vec::with_capacity(maximum_frames)));
        let mut audio = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: sound.sample_rate,
            },
            internal_buffer_size: 1,
            main_track_builder: MainTrackBuilder::new()
                .with_built_effect(Box::new(Capture(captured.clone()))),
            ..Default::default()
        })
        .unwrap();
        for _ in 0..2 {
            captured.lock().unwrap().clear();
            let handle = audio.play(sound.clone()).unwrap();
            assert_eq!(handle.position(), 0.0);
            for _ in 0..maximum_frames {
                audio.backend_mut().on_start_processing();
                audio.backend_mut().process();
                if handle.state() == PlaybackState::Stopped {
                    break;
                }
            }
            assert_eq!(handle.state(), PlaybackState::Stopped);
            let output = captured.lock().unwrap();
            assert!((sound.frames.len()..=maximum_frames).contains(&output.len()));
            assert_eq!(&output[..sound.frames.len()], sound.frames.as_ref());
            assert!(
                output[sound.frames.len()..]
                    .iter()
                    .all(|frame| *frame == Frame::ZERO)
            );
            assert!(output.iter().any(|frame| frame.left.abs() > 0.01));
            assert!(
                output
                    .iter()
                    .any(|frame| (frame.right - frame.left).abs() > 0.01)
            );
            assert!(
                output
                    .iter()
                    .all(|frame| frame.left.abs() < 1.0 && frame.right.abs() < 1.0)
            );
        }
    }

    #[test]
    fn invalid_packages_and_unknown_rules_cannot_return_playback_data() {
        let root = TestDirectory::new();
        let unknown = root.package("unknown", "unknown-rules-v1", 1_200, vec![]);
        assert!(
            load_package(&unknown)
                .unwrap_err()
                .contains("Unsupported song ruleset")
        );
        let corrupt = root.package("corrupt", RULES_ID, 1_200, vec![]);
        let mut bytes = STEREO.to_vec();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(corrupt.join("song.audio.ogg"), bytes).unwrap();
        assert!(load_package(&corrupt).is_err());
    }

    #[test]
    fn development_content_preserves_the_original_identity_and_pcm() {
        let content = SongContent::development();
        assert_eq!(
            content.content_id,
            "dev64-pcm16-3390dd080cb536fd4-anchors-v1"
        );
        assert_eq!(content.end.frames(), 3_072_000);
        assert_eq!(content.anchors, dev_song::anchors());
        assert!(content.stage.is_none());
        let authoring: serde_json::Value = serde_json::from_str(include_str!(
            "../../../assets/dev/vertical_slice/authoring.json"
        ))
        .unwrap();
        let authored_sections = authoring["sections"].as_array().unwrap();
        assert_eq!(content.sections.len(), authored_sections.len());
        for (cue, authored) in content.sections.iter().zip(authored_sections) {
            assert_eq!(Some(cue.id), authored["id"].as_u64());
            assert_eq!(Some(cue.time.frames()), authored["start_frame"].as_i64());
            assert_eq!(Some(cue.label.as_str()), authored["label"].as_str());
        }
        let sound = development_sound();
        assert_eq!(sound.sample_rate, dev_song::SAMPLE_RATE);
        assert_eq!(sound.frames.len(), dev_song::FRAMES as usize);
        for frame in [0, 1, 24_000, 1_919_999, 1_920_000, 2_304_000, 3_071_999] {
            let [left, right] = dev_song::sample(frame).map(|sample| f32::from(sample) / 32_768.0);
            assert_eq!(sound.frames[frame as usize], Frame::new(left, right));
        }
    }
}
