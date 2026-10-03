//! Session facts and final PCM are loaded together before a song can start

use crate::{audio::sound_data, dev_song};
use cocobeat_schema::{Anchor, SongTime};
use kira::{Frame, sound::static_sound::StaticSoundData};
use std::path::Path;

pub const CONTENT_ID: &str = "dev64-pcm16-3390dd080cb536fd4-anchors-v1";
pub const RULES_ID: &str = "duo-watermark-v1";

#[derive(Clone, Debug)]
pub struct SongContent {
    pub content_id: String,
    pub end: SongTime,
    pub anchors: Vec<Anchor>,
}

impl SongContent {
    pub fn development() -> Self {
        Self {
            content_id: CONTENT_ID.into(),
            end: SongTime::from_frames(i64::from(dev_song::FRAMES)),
            anchors: dev_song::anchors(),
        }
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
    Ok((
        SongContent {
            content_id: format!("package-blake3:{hash}"),
            end: SongTime::from_frames(package.manifest.canonical_frames as i64),
            anchors: package.chart.anchors,
        },
        sound_data(pcm),
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

        fn package(&self, name: &str, ruleset_id: &str, anchor_frame: i64) -> PathBuf {
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
                            schema_version: CONTENT_SCHEMA_VERSION,
                            audio_hash: prepared.asset.blake3,
                            beats: vec![],
                            onsets: vec![],
                            sections: vec![],
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
                            sections: vec![],
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
    fn package_pcm_is_exact_and_shared_while_identity_binds_the_chart() {
        let root = TestDirectory::new();
        let first = root.package("first", RULES_ID, 1_200);
        let second = root.package("second", RULES_ID, 2_400);
        let (content, sound) = load_package(&first).unwrap();
        let (other_content, other_sound) = load_package(&second).unwrap();
        assert_ne!(content.content_id, other_content.content_id);
        assert!(content.content_id.starts_with("package-blake3:"));
        assert_eq!(content.content_id.len(), "package-blake3:".len() + 64);
        assert_eq!(content.end.frames(), 4_800);
        assert_eq!(content.anchors.len(), 1);
        assert_eq!(content.anchors[0].id, 5);
        assert_eq!(content.anchors[0].song_time.frames(), 1_200);
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
        let (_, sound) = load_package(&root.package("playback", RULES_ID, 1_200)).unwrap();
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
        let unknown = root.package("unknown", "unknown-rules-v1", 1_200);
        assert!(
            load_package(&unknown)
                .unwrap_err()
                .contains("Unsupported song ruleset")
        );
        let corrupt = root.package("corrupt", RULES_ID, 1_200);
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
        let sound = development_sound();
        assert_eq!(sound.sample_rate, dev_song::SAMPLE_RATE);
        assert_eq!(sound.frames.len(), dev_song::FRAMES as usize);
        for frame in [0, 1, 24_000, 1_919_999, 1_920_000, 2_304_000, 3_071_999] {
            let [left, right] = dev_song::sample(frame).map(|sample| f32::from(sample) / 32_768.0);
            assert_eq!(sound.frames[frame as usize], Frame::new(left, right));
        }
    }
}
