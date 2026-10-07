//! Optional local software observations bound to exact Replay bytes
//!
//! These records are neither device timestamps nor a physical latency measurement

use crate::{Replay, invalid};
use cocobeat_schema::{DuoInput, MAX_CANONICAL_FRAMES};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

pub const MAX_TIMING_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_AUDIO_READS: usize = 12_000;
pub const AUDIO_SAMPLING_INTERVAL_NS: u64 = 50_000_000;
const FORMAT: &str = "CoCoBeat Local Timing";
const VERSION: u32 = 1;
const TIMESTAMP_BASIS: &str = "input_process_monotonic_origin";
const INPUT_TIMESTAMP_KIND: &str = "software_message_observed";
const CLOCK_OBSERVATION_KIND: &str = "kira_position_polled";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingSidecar {
    pub format: String,
    pub version: u32,
    pub replay_blake3: String,
    pub content_id: String,
    pub rules_id: String,
    pub build_id: String,
    #[serde(deserialize_with = "required_nullable")]
    pub stage_compiler_version: Option<u32>,
    pub epoch: u64,
    pub canonical_frames: u64,
    /// Explicit local seats, including a seat which recorded no Hit
    pub capture_players: Vec<u8>,
    pub timestamp_basis: String,
    pub input_timestamp_kind: String,
    pub clock_observation_kind: String,
    pub clock: TimingClock,
    pub audio_sampling_interval_ns: u64,
    pub audio_history_status: AudioHistoryStatus,
    pub captures: Vec<CaptureTiming>,
    pub audio_history: Vec<AudioRead>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingClock {
    pub max_extrapolation_ns: u64,
    pub max_drift_ppm: u32,
    pub history_capacity: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    KeyboardMessage,
    GamepadMessage,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingAnchor {
    pub monotonic_ns: u64,
    pub song_time_frames: i64,
    pub uncertainty_frames: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTiming {
    /// One-based position in the exact associated Replay, including Watermarks
    pub fact_index: u64,
    pub player: u8,
    pub seq: u64,
    pub song_time_frames: i64,
    pub observed_ns: u64,
    pub consumed_ns: u64,
    pub uncertainty_frames: u64,
    pub input_kind: InputKind,
    /// The anchor used by the original capture query, not the latest anchor at save
    pub mapping_anchor: MappingAnchor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioHistoryStatus {
    Sampled,
    LimitReached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingPhase {
    Starting,
    Running,
    Pausing,
    Paused,
    Recovering,
    Finishing,
    Finished,
    Fault,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioRead {
    pub read_before_ns: u64,
    pub read_after_ns: u64,
    pub phase: TimingPhase,
    /// These are independent historical reads, not a paired backend callback
    #[serde(deserialize_with = "required_nullable")]
    pub source: Option<SourcePublication>,
    #[serde(deserialize_with = "required_nullable")]
    pub callback: Option<CallbackPublication>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePublication {
    pub generation: u64,
    pub source_id: u64,
    pub sequence: u64,
    pub position_seconds: f64,
    pub published_before_ns: u64,
    pub published_after_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallbackPublication {
    pub generation: u64,
    pub sequence: u64,
    pub observed_ns: u64,
    #[serde(deserialize_with = "required_nullable")]
    pub previous_observed_ns: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    pub previous_frames: Option<u64>,
    pub output_sample_rate: u32,
}

fn required_nullable<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}

fn raw_replay_hash(replay: &Replay, bytes: &[u8]) -> io::Result<String> {
    if bytes.len() as u64 > crate::MAX_FILE_BYTES || Replay::decode(bytes)? != *replay {
        return Err(invalid(
            "Timing requires the same bounded raw Replay and decoded history",
        ));
    }
    Ok(blake3::hash(bytes).to_hex().to_string())
}

impl TimingSidecar {
    /// Creates the fixed identity; callers append real observations before validate/save
    pub fn new(
        replay: &Replay,
        raw_replay: &[u8],
        canonical_frames: u64,
        capture_players: Vec<u8>,
        clock: TimingClock,
    ) -> io::Result<Self> {
        let identity = replay.identity();
        let result = Self {
            format: FORMAT.into(),
            version: VERSION,
            replay_blake3: raw_replay_hash(replay, raw_replay)?,
            content_id: identity.content_id.clone(),
            rules_id: identity.rules_id.clone(),
            build_id: identity.build_id.clone(),
            stage_compiler_version: identity.stage_compiler_version,
            epoch: replay.epoch().0,
            canonical_frames,
            capture_players,
            timestamp_basis: TIMESTAMP_BASIS.into(),
            input_timestamp_kind: INPUT_TIMESTAMP_KIND.into(),
            clock_observation_kind: CLOCK_OBSERVATION_KIND.into(),
            clock,
            audio_sampling_interval_ns: AUDIO_SAMPLING_INTERVAL_NS,
            audio_history_status: AudioHistoryStatus::Sampled,
            captures: Vec::new(),
            audio_history: Vec::new(),
        };
        result.validate_header(replay, canonical_frames)?;
        Ok(result)
    }

    fn validate_header(&self, replay: &Replay, canonical_frames: u64) -> io::Result<()> {
        let identity = replay.identity();
        if self.format != FORMAT
            || self.version != VERSION
            || self.content_id != identity.content_id
            || self.rules_id != identity.rules_id
            || self.build_id != identity.build_id
            || self.stage_compiler_version != identity.stage_compiler_version
            || self.epoch != replay.epoch().0
            || self.canonical_frames != canonical_frames
            || !(1..=MAX_CANONICAL_FRAMES).contains(&canonical_frames)
            || !matches!(self.capture_players.as_slice(), [1] | [2] | [1, 2])
            || self.timestamp_basis != TIMESTAMP_BASIS
            || self.input_timestamp_kind != INPUT_TIMESTAMP_KIND
            || self.clock_observation_kind != CLOCK_OBSERVATION_KIND
            || self.clock.max_extrapolation_ns == 0
            || self.clock.max_drift_ppm >= 1_000_000
            || !(1..=crate::MAX_FACTS as u64).contains(&self.clock.history_capacity)
            || self.audio_sampling_interval_ns != AUDIO_SAMPLING_INTERVAL_NS
        {
            return Err(invalid(
                "Timing header, identity, local seats or clock method differs",
            ));
        }
        Ok(())
    }

    /// Callers also validate the song package and Replay through the existing core path
    /// This checks association and software-observation structure, not physical accuracy
    pub fn validate(
        &self,
        replay: &Replay,
        raw_replay: &[u8],
        canonical_frames: u64,
    ) -> io::Result<()> {
        self.validate_header(replay, canonical_frames)?;
        if self.replay_blake3 != raw_replay_hash(replay, raw_replay)? {
            return Err(invalid("Timing Replay byte identity differs"));
        }
        if self.captures.len() > crate::MAX_FACTS
            || self.audio_history.len() > MAX_AUDIO_READS
            || (self.audio_history_status == AudioHistoryStatus::LimitReached
                && self.audio_history.len() != MAX_AUDIO_READS)
        {
            return Err(invalid(
                "Timing observation count or limit status is invalid",
            ));
        }
        let mut captures = self.captures.iter();
        let mut previous_consumed = None;
        for (index, fact) in replay.facts().iter().enumerate() {
            let epoch = match fact {
                DuoInput::Hit(hit) => hit.epoch,
                DuoInput::Watermark { epoch, .. } => *epoch,
            };
            if epoch != replay.epoch() {
                return Err(invalid("Timing Replay fact belongs to another epoch"));
            }
            let DuoInput::Hit(hit) = fact else { continue };
            if !(0..canonical_frames as i64).contains(&hit.song_time.frames()) {
                return Err(invalid("Timing Replay Hit is outside the song timeline"));
            }
            let player = hit.player.index() as u8 + 1;
            if !self.capture_players.contains(&player) {
                continue;
            }
            let row = captures
                .next()
                .ok_or_else(|| invalid("Timing is missing a local Hit"))?;
            if row.fact_index != index as u64 + 1
                || row.player != player
                || row.seq != hit.seq
                || row.song_time_frames != hit.song_time.frames()
            {
                return Err(invalid(
                    "Timing Capture does not match the exact local Replay Hit",
                ));
            }
            let anchor = row.mapping_anchor;
            if row.observed_ns > row.consumed_ns
                || previous_consumed.is_some_and(|previous| row.consumed_ns < previous)
                || row
                    .observed_ns
                    .checked_sub(anchor.monotonic_ns)
                    .is_none_or(|age| age > self.clock.max_extrapolation_ns)
                || !(0..=canonical_frames as i64).contains(&anchor.song_time_frames)
                || row.song_time_frames < anchor.song_time_frames
                || anchor
                    .uncertainty_frames
                    .checked_add(1)
                    .is_none_or(|minimum| row.uncertainty_frames < minimum)
            {
                return Err(invalid(
                    "Timing Capture clock anchor or observation interval is invalid",
                ));
            }
            previous_consumed = Some(row.consumed_ns);
        }
        if captures.next().is_some() {
            return Err(invalid(
                "Timing contains a duplicate, remote or extra Capture",
            ));
        }
        self.validate_audio()
    }

    fn validate_audio(&self) -> io::Result<()> {
        let mut previous_read: Option<&AudioRead> = None;
        let mut sources: BTreeMap<(u64, u64), &SourcePublication> = BTreeMap::new();
        let mut callbacks: BTreeMap<u64, &CallbackPublication> = BTreeMap::new();
        for read in &self.audio_history {
            if read.read_before_ns > read.read_after_ns
                || previous_read.is_some_and(|previous| {
                    read.read_before_ns < previous.read_after_ns
                        || read
                            .read_before_ns
                            .checked_sub(previous.read_before_ns)
                            .is_none_or(|gap| gap < AUDIO_SAMPLING_INTERVAL_NS)
                })
            {
                return Err(invalid(
                    "Timing audio reads overlap or exceed the sampling cadence",
                ));
            }
            if let Some(source) = &read.source {
                if source.generation == 0
                    || source.source_id == 0
                    || source.sequence == 0
                    || !source.position_seconds.is_finite()
                    || !(0.0..=self.canonical_frames as f64 / 48_000.0)
                        .contains(&source.position_seconds)
                    || source.published_before_ns > source.published_after_ns
                    || source.published_after_ns > read.read_after_ns
                {
                    return Err(invalid("Timing source publication is invalid"));
                }
                if let Some(previous) =
                    sources.insert((source.generation, source.source_id), source)
                    && (source.sequence < previous.sequence
                        || (source.sequence == previous.sequence && source != previous)
                        || (source.sequence > previous.sequence
                            && (source.published_before_ns < previous.published_after_ns
                                || source.position_seconds < previous.position_seconds)))
                {
                    return Err(invalid(
                        "Timing source publication changed or moved backwards",
                    ));
                }
            }
            if let Some(callback) = &read.callback {
                let previous_valid = match (callback.previous_observed_ns, callback.previous_frames)
                {
                    (None, None) => callback.sequence == 1,
                    (previous, Some(_)) => {
                        callback.sequence > 1
                            && previous.is_none_or(|at| at <= callback.observed_ns)
                    }
                    _ => false,
                };
                if callback.generation == 0
                    || callback.sequence == 0
                    || callback.output_sample_rate == 0
                    || !previous_valid
                    || callback.observed_ns > read.read_after_ns
                {
                    return Err(invalid("Timing callback publication is invalid"));
                }
                if let Some(previous) = callbacks.insert(callback.generation, callback)
                    && (callback.sequence < previous.sequence
                        || (callback.sequence == previous.sequence && callback != previous)
                        || (callback.sequence > previous.sequence
                            && (callback.observed_ns < previous.observed_ns
                                || callback.output_sample_rate != previous.output_sample_rate
                                || callback
                                    .previous_observed_ns
                                    .is_none_or(|at| at < previous.observed_ns)
                                || (previous.sequence.checked_add(1) == Some(callback.sequence)
                                    && callback.previous_observed_ns
                                        != Some(previous.observed_ns)))))
                {
                    return Err(invalid(
                        "Timing callback publication changed or moved backwards",
                    ));
                }
            }
            previous_read = Some(read);
        }
        Ok(())
    }

    pub fn decode(
        reader: impl Read,
        replay: &Replay,
        raw_replay: &[u8],
        canonical_frames: u64,
    ) -> io::Result<Self> {
        let mut bytes = Vec::new();
        reader.take(MAX_TIMING_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_TIMING_BYTES {
            return Err(invalid("Timing sidecar byte limit exceeded"));
        }
        let sidecar: Self = serde_json::from_slice(&bytes).map_err(invalid)?;
        sidecar.validate(replay, raw_replay, canonical_frames)?;
        Ok(sidecar)
    }

    pub fn load(
        path: &Path,
        replay: &Replay,
        raw_replay: &[u8],
        canonical_frames: u64,
    ) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() > MAX_TIMING_BYTES {
            return Err(invalid("Timing input must be a bounded regular file"));
        }
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_TIMING_BYTES {
            return Err(invalid(
                "Opened Timing input must be a bounded regular file",
            ));
        }
        Self::decode(file, replay, raw_replay, canonical_frames)
    }

    /// Only this new sidecar is removed on failure; the paired Replay is never changed
    pub fn save_new(
        &self,
        path: &Path,
        replay: &Replay,
        raw_replay: &[u8],
        canonical_frames: u64,
    ) -> io::Result<()> {
        self.validate(replay, raw_replay, canonical_frames)?;
        let bytes = serde_json::to_vec(self).map_err(invalid)?;
        if bytes.len() as u64 > MAX_TIMING_BYTES {
            return Err(invalid("Timing sidecar byte limit exceeded"));
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        let result = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = result {
            fs::remove_file(path).map_err(|cleanup| {
                io::Error::other(format!(
                    "Save Timing: {error}; remove partial sidecar: {cleanup}"
                ))
            })?;
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReplayIdentity;
    use cocobeat_schema::{Hit, PlayerId, SessionEpoch, SongTime};

    fn fixture() -> (Replay, Vec<u8>, TimingSidecar) {
        let mut replay = Replay::new(
            ReplayIdentity {
                content_id: "package-blake3:fixture".into(),
                rules_id: "duo-watermark-v1".into(),
                build_id: "fixture".into(),
                stage_compiler_version: Some(2),
            },
            SessionEpoch(7),
        )
        .unwrap();
        replay
            .record(DuoInput::Watermark {
                epoch: SessionEpoch(7),
                player: PlayerId::P1,
                through: SongTime::from_frames(-1),
            })
            .unwrap();
        for player in [PlayerId::P1, PlayerId::P2] {
            replay
                .record(DuoInput::Hit(Hit {
                    epoch: SessionEpoch(7),
                    player,
                    seq: 0,
                    song_time: SongTime::from_frames(480),
                }))
                .unwrap();
        }
        let raw = replay.encode().unwrap();
        let mut sidecar = TimingSidecar::new(
            &replay,
            &raw,
            48_000,
            vec![1],
            TimingClock {
                max_extrapolation_ns: 250_000_000,
                max_drift_ppm: 1_000,
                history_capacity: 256,
            },
        )
        .unwrap();
        sidecar.captures.push(CaptureTiming {
            fact_index: 2,
            player: 1,
            seq: 0,
            song_time_frames: 480,
            observed_ns: 10_000_000,
            consumed_ns: 20_000_000,
            uncertainty_frames: 2_402,
            input_kind: InputKind::KeyboardMessage,
            mapping_anchor: MappingAnchor {
                monotonic_ns: 0,
                song_time_frames: 0,
                uncertainty_frames: 2_400,
            },
        });
        sidecar.audio_history.push(AudioRead {
            read_before_ns: 20_000_000,
            read_after_ns: 21_000_000,
            phase: TimingPhase::Running,
            source: Some(SourcePublication {
                generation: 1,
                source_id: 1,
                sequence: 3,
                position_seconds: 0.01,
                published_before_ns: 9_000_000,
                published_after_ns: 10_000_000,
            }),
            callback: Some(CallbackPublication {
                generation: 1,
                sequence: 7,
                observed_ns: 11_000_000,
                previous_observed_ns: Some(8_000_000),
                previous_frames: Some(144),
                output_sample_rate: 48_000,
            }),
        });
        (replay, raw, sidecar)
    }

    #[test]
    fn exact_raw_identity_local_coverage_and_hit_join_are_required() {
        let (replay, raw, sidecar) = fixture();
        let encoded = serde_json::to_vec(&sidecar).unwrap();
        assert_eq!(
            TimingSidecar::decode(encoded.as_slice(), &replay, &raw, 48_000).unwrap(),
            sidecar
        );
        let changed_raw = [raw.as_slice(), b"\n"].concat();
        assert_eq!(Replay::decode(changed_raw.as_slice()).unwrap(), replay);
        assert!(sidecar.validate(&replay, &changed_raw, 48_000).is_err());
        for modify in [
            |value: &mut TimingSidecar| value.epoch += 1,
            |value: &mut TimingSidecar| value.stage_compiler_version = Some(1),
            |value: &mut TimingSidecar| value.content_id.push('x'),
            |value: &mut TimingSidecar| value.build_id.push('x'),
            |value: &mut TimingSidecar| value.captures[0].fact_index = 1,
            |value: &mut TimingSidecar| value.captures[0].seq = 1,
            |value: &mut TimingSidecar| value.captures[0].song_time_frames += 1,
            |value: &mut TimingSidecar| value.captures.push(value.captures[0].clone()),
            |value: &mut TimingSidecar| value.captures.clear(),
            |value: &mut TimingSidecar| value.capture_players.push(2),
            |value: &mut TimingSidecar| value.capture_players.clear(),
            |value: &mut TimingSidecar| value.captures[0].consumed_ns = 1,
            |value: &mut TimingSidecar| value.captures[0].mapping_anchor.monotonic_ns = 11_000_000,
            |value: &mut TimingSidecar| value.captures[0].uncertainty_frames = 1,
        ] {
            let mut changed = sidecar.clone();
            modify(&mut changed);
            assert!(
                changed.validate(&replay, &raw, 48_000).is_err(),
                "{changed:?}"
            );
        }
        assert!(sidecar.validate(&replay, &raw, 48_001).is_err());
        let mut changed_replay = replay.clone();
        changed_replay
            .record(DuoInput::Watermark {
                epoch: SessionEpoch(7),
                player: PlayerId::P2,
                through: SongTime::from_frames(1),
            })
            .unwrap();
        assert!(sidecar.validate(&changed_replay, &raw, 48_000).is_err());
    }

    #[test]
    fn strict_nullable_stage_and_independent_publications_preserve_gaps() {
        let (replay, raw, mut sidecar) = fixture();
        let mut repeated = sidecar.audio_history[0].clone();
        repeated.read_before_ns += AUDIO_SAMPLING_INTERVAL_NS;
        repeated.read_after_ns += AUDIO_SAMPLING_INTERVAL_NS;
        sidecar.audio_history.push(repeated.clone());
        sidecar.audio_history.push(AudioRead {
            read_before_ns: 300_000_000,
            read_after_ns: 300_000_001,
            phase: TimingPhase::Paused,
            source: None,
            callback: None,
        });
        sidecar.validate(&replay, &raw, 48_000).unwrap();
        let mut adjacent = sidecar.clone();
        let callback = adjacent.audio_history[1].callback.as_mut().unwrap();
        callback.sequence = 8;
        callback.observed_ns = 60_000_000;
        callback.previous_observed_ns = Some(11_000_000);
        adjacent.validate(&replay, &raw, 48_000).unwrap();
        adjacent.audio_history[1]
            .callback
            .as_mut()
            .unwrap()
            .previous_observed_ns = Some(12_000_000);
        assert!(adjacent.validate(&replay, &raw, 48_000).is_err());
        adjacent.audio_history[1]
            .callback
            .as_mut()
            .unwrap()
            .sequence = 9;
        adjacent.validate(&replay, &raw, 48_000).unwrap();
        for modify in [
            |value: &mut TimingSidecar| {
                value.audio_history[1].source.as_mut().unwrap().sequence = 2
            },
            |value: &mut TimingSidecar| {
                value.audio_history[1]
                    .source
                    .as_mut()
                    .unwrap()
                    .position_seconds = 0.02
            },
            |value: &mut TimingSidecar| {
                value.audio_history[1]
                    .source
                    .as_mut()
                    .unwrap()
                    .position_seconds = f64::NAN
            },
            |value: &mut TimingSidecar| {
                value.audio_history[1]
                    .callback
                    .as_mut()
                    .unwrap()
                    .previous_frames = None
            },
            |value: &mut TimingSidecar| value.audio_history[1].read_before_ns = 21_000_000,
            |value: &mut TimingSidecar| {
                value.audio_history_status = AudioHistoryStatus::LimitReached
            },
        ] {
            let mut changed = sidecar.clone();
            modify(&mut changed);
            assert!(changed.validate(&replay, &raw, 48_000).is_err());
        }
        for key in ["stage_compiler_version", "source", "unexpected"] {
            let mut json = serde_json::to_value(&sidecar).unwrap();
            match key {
                "stage_compiler_version" => {
                    json.as_object_mut().unwrap().remove(key);
                }
                "source" => {
                    json["audio_history"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove(key);
                }
                _ => {
                    json[key] = true.into();
                }
            }
            assert!(
                TimingSidecar::decode(
                    serde_json::to_vec(&json).unwrap().as_slice(),
                    &replay,
                    &raw,
                    48_000
                )
                .is_err()
            );
        }
        let mut legacy_identity = replay.identity().clone();
        legacy_identity.stage_compiler_version = None;
        let mut legacy = Replay::new(legacy_identity, replay.epoch()).unwrap();
        for &fact in replay.facts() {
            legacy.record(fact).unwrap();
        }
        let legacy_raw = legacy.encode().unwrap();
        let mut legacy_sidecar =
            TimingSidecar::new(&legacy, &legacy_raw, 48_000, vec![1], sidecar.clock).unwrap();
        legacy_sidecar.captures = sidecar.captures.clone();
        assert!(serde_json::to_value(&legacy_sidecar).unwrap()["stage_compiler_version"].is_null());
        let legacy_encoded = serde_json::to_vec(&legacy_sidecar).unwrap();
        assert_eq!(
            TimingSidecar::decode(legacy_encoded.as_slice(), &legacy, &legacy_raw, 48_000).unwrap(),
            legacy_sidecar
        );
        let mut prior_origin = fixture().2;
        prior_origin.audio_history[0]
            .callback
            .as_mut()
            .unwrap()
            .previous_observed_ns = None;
        prior_origin.validate(&replay, &raw, 48_000).unwrap();
    }

    #[test]
    fn new_file_roundtrip_does_not_overwrite_and_oversize_is_rejected_before_read() {
        let (replay, raw, sidecar) = fixture();
        let root = std::env::temp_dir().join(format!(
            "cocobeat-timing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("capture.timing.json");
        sidecar.save_new(&path, &replay, &raw, 48_000).unwrap();
        assert_eq!(
            TimingSidecar::load(&path, &replay, &raw, 48_000).unwrap(),
            sidecar
        );
        let saved = fs::read(&path).unwrap();
        assert!(sidecar.save_new(&path, &replay, &raw, 48_000).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        let sparse = root.join("oversized.json");
        File::create(&sparse)
            .unwrap()
            .set_len(MAX_TIMING_BYTES + 1)
            .unwrap();
        assert!(TimingSidecar::load(&sparse, &replay, &raw, 48_000).is_err());
        assert!(TimingSidecar::load(&root, &replay, &raw, 48_000).is_err());
        #[cfg(unix)]
        {
            let link = root.join("linked.json");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(TimingSidecar::load(&link, &replay, &raw, 48_000).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
