//! Bounded, versioned input histories played through the same core as live input
//!
//! Files contain integer song frames and local provenance, never user audio

use cocobeat_core::DuoEngine;
use cocobeat_schema::{Anchor, DuoInput, DuoRules, Hit, PlayerId, SessionEpoch, SongTime};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

const FORMAT: &str = "CoCoBeat Replay";
const CORE_VERSION: u32 = 1;
const STAGE_VERSION: u32 = 2;
// Ten minutes of 10 ms player watermarks leave room for 39,996 hits
// The byte bound also fits every fact and identity at their maximum encoded width
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_FACTS: usize = 160_000;
const MAX_IDENTITY_BYTES: usize = 256;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayIdentity {
    pub content_id: String,
    pub rules_id: String,
    pub build_id: String,
    /// None preserves legacy core histories without inferring a historical stage
    pub stage_compiler_version: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    identity: ReplayIdentity,
    epoch: SessionEpoch,
    facts: Vec<DuoInput>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    format: String,
    version: u32,
    content_id: String,
    rules_id: String,
    build_id: String,
    // The outer option distinguishes a missing field from an explicit null
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "stage_field"
    )]
    stage_compiler_version: Option<Option<u32>>,
    epoch: u64,
    facts: Vec<Fact>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Fact {
    Hit {
        epoch: u64,
        player: u8,
        seq: u64,
        song_time_frames: i64,
    },
    Watermark {
        epoch: u64,
        player: u8,
        through_frames: i64,
    },
}

fn stage_field<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<u32>>, D::Error> {
    Option::<u32>::deserialize(deserializer).map(Some)
}

fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn player(number: u8) -> io::Result<PlayerId> {
    match number {
        1 => Ok(PlayerId::P1),
        2 => Ok(PlayerId::P2),
        _ => Err(invalid("Replay player must be 1 or 2")),
    }
}

fn player_number(player: PlayerId) -> u8 {
    match player {
        PlayerId::P1 => 1,
        PlayerId::P2 => 2,
    }
}

impl Replay {
    pub fn new(identity: ReplayIdentity, epoch: SessionEpoch) -> io::Result<Self> {
        for value in [&identity.content_id, &identity.rules_id, &identity.build_id] {
            if value.is_empty() || value.len() > MAX_IDENTITY_BYTES {
                return Err(invalid(
                    "Replay identities must contain 1 to 256 UTF-8 bytes",
                ));
            }
        }
        if identity
            .stage_compiler_version
            .is_some_and(|version| !matches!(version, 1 | 2))
        {
            return Err(invalid("Unsupported Stage compiler version"));
        }
        Ok(Self {
            identity,
            epoch,
            facts: Vec::new(),
        })
    }

    pub fn identity(&self) -> &ReplayIdentity {
        &self.identity
    }

    pub fn epoch(&self) -> SessionEpoch {
        self.epoch
    }

    pub fn facts(&self) -> &[DuoInput] {
        &self.facts
    }

    /// Preserves the supplied fact verbatim; core validates its gameplay meaning
    /// Callers must surface failure; a full recorder never silently drops input
    pub fn record(&mut self, fact: DuoInput) -> io::Result<()> {
        if self.facts.len() == MAX_FACTS {
            return Err(invalid("Replay fact limit exceeded"));
        }
        self.facts.push(fact);
        Ok(())
    }

    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let document = Document {
            format: FORMAT.into(),
            version: if self.identity.stage_compiler_version.is_some() {
                STAGE_VERSION
            } else {
                CORE_VERSION
            },
            content_id: self.identity.content_id.clone(),
            rules_id: self.identity.rules_id.clone(),
            build_id: self.identity.build_id.clone(),
            stage_compiler_version: self.identity.stage_compiler_version.map(Some),
            epoch: self.epoch.0,
            facts: self
                .facts
                .iter()
                .map(|fact| match *fact {
                    DuoInput::Hit(hit) => Fact::Hit {
                        epoch: hit.epoch.0,
                        player: player_number(hit.player),
                        seq: hit.seq,
                        song_time_frames: hit.song_time.frames(),
                    },
                    DuoInput::Watermark {
                        epoch,
                        player,
                        through,
                    } => Fact::Watermark {
                        epoch: epoch.0,
                        player: player_number(player),
                        through_frames: through.frames(),
                    },
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&document).map_err(invalid)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid("Replay byte limit exceeded"));
        }
        Ok(bytes)
    }

    pub fn decode(reader: impl Read) -> io::Result<Self> {
        let mut bytes = Vec::new();
        reader.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid("Replay byte limit exceeded"));
        }
        let document: Document = serde_json::from_slice(&bytes).map_err(invalid)?;
        let stage_compiler_version = match (document.version, document.stage_compiler_version) {
            (CORE_VERSION, None) => None,
            (STAGE_VERSION, Some(Some(version @ (1 | 2)))) => Some(version),
            _ => return Err(invalid("Invalid Replay version or Stage compiler identity")),
        };
        if document.format != FORMAT {
            return Err(invalid("Invalid Replay header or unsupported version"));
        }
        if document.facts.len() > MAX_FACTS {
            return Err(invalid("Replay fact limit exceeded"));
        }
        let mut replay = Self::new(
            ReplayIdentity {
                content_id: document.content_id,
                rules_id: document.rules_id,
                build_id: document.build_id,
                stage_compiler_version,
            },
            SessionEpoch(document.epoch),
        )?;
        for fact in document.facts {
            replay.record(match fact {
                Fact::Hit {
                    epoch,
                    player: number,
                    seq,
                    song_time_frames,
                } => DuoInput::Hit(Hit {
                    epoch: SessionEpoch(epoch),
                    player: player(number)?,
                    seq,
                    song_time: SongTime::from_frames(song_time_frames),
                }),
                Fact::Watermark {
                    epoch,
                    player: number,
                    through_frames,
                } => DuoInput::Watermark {
                    epoch: SessionEpoch(epoch),
                    player: player(number)?,
                    through: SongTime::from_frames(through_frames),
                },
            })?;
        }
        Ok(replay)
    }

    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::decode(fs::File::open(path)?)
    }

    /// Writes a same-directory temporary file, syncs its data, then renames it
    /// An existing destination is never deleted first; replacement follows the
    /// platform's rename semantics, and rename errors leave the old file intact
    /// This does not promise directory-entry durability after power loss
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let bytes = self.encode()?;
        let path = path.as_ref();
        let name = path
            .file_name()
            .ok_or_else(|| invalid("Replay path requires a file name"))?;
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let temporary = path.with_file_name(temporary_name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    /// The caller resolves content and rules identities before constructing the
    /// engine inputs; build identity is provenance rather than a compatibility gate
    pub fn replay(
        &self,
        expected_content_id: &str,
        expected_rules_id: &str,
        anchors: Vec<Anchor>,
        rules: DuoRules,
    ) -> io::Result<DuoEngine> {
        if self.identity.content_id != expected_content_id
            || self.identity.rules_id != expected_rules_id
        {
            return Err(invalid("Replay content or rules identity mismatch"));
        }
        let mut engine = DuoEngine::new(self.epoch, anchors, rules).map_err(invalid)?;
        for (index, fact) in self.facts.iter().enumerate() {
            engine
                .ingest(*fact)
                .map_err(|error| invalid(format!("Replay fact {}: {error}", index + 1)))?;
        }
        Ok(engine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Replay {
        Replay::new(
            ReplayIdentity {
                content_id: "fixture-64s-v1".into(),
                rules_id: "duo-v1".into(),
                build_id: "test-build".into(),
                stage_compiler_version: None,
            },
            SessionEpoch(7),
        )
        .unwrap()
    }

    fn hit(player: PlayerId, seq: u64, frames: i64) -> DuoInput {
        DuoInput::Hit(Hit {
            epoch: SessionEpoch(7),
            player,
            seq,
            song_time: SongTime::from_frames(frames),
        })
    }

    fn watermark(player: PlayerId, frames: i64) -> DuoInput {
        DuoInput::Watermark {
            epoch: SessionEpoch(7),
            player,
            through: SongTime::from_frames(frames),
        }
    }

    #[test]
    fn roundtrip_preserves_integer_frames_identities_and_duplicate_deliveries() {
        let mut replay = empty();
        for fact in [
            hit(PlayerId::P1, u64::MAX, i64::MIN),
            hit(PlayerId::P1, u64::MAX, i64::MIN),
            hit(PlayerId::P2, 9, i64::MAX),
            watermark(PlayerId::P1, -48_000),
            watermark(PlayerId::P2, i64::MAX),
        ] {
            replay.record(fact).unwrap();
        }
        let bytes = replay.encode().unwrap();
        assert_eq!(Replay::decode(bytes.as_slice()).unwrap(), replay);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("-9223372036854775808"));
        assert!(text.contains("9223372036854775807"));
        assert!(text.contains("18446744073709551615"));
    }

    #[test]
    fn stage_identity_requires_explicit_v2_and_keeps_legacy_bytes() {
        let legacy = empty();
        let legacy_bytes = legacy.encode().unwrap();
        assert_eq!(legacy_bytes, br#"{"format":"CoCoBeat Replay","version":1,"content_id":"fixture-64s-v1","rules_id":"duo-v1","build_id":"test-build","epoch":7,"facts":[]}"#);
        assert_eq!(
            Replay::decode(legacy_bytes.as_slice())
                .unwrap()
                .identity()
                .stage_compiler_version,
            None
        );
        for version in [1, 2] {
            let mut identity = legacy.identity().clone();
            identity.stage_compiler_version = Some(version);
            let explicit = Replay::new(identity, legacy.epoch()).unwrap();
            let bytes = explicit.encode().unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["version"], 2);
            assert_eq!(value["stage_compiler_version"], version);
            assert_eq!(Replay::decode(bytes.as_slice()).unwrap(), explicit);
        }
        let original: serde_json::Value = serde_json::from_slice(&legacy_bytes).unwrap();
        for (document_version, field) in [
            (1, Some(serde_json::json!(null))),
            (1, Some(serde_json::json!(1))),
            (2, None),
            (2, Some(serde_json::json!(null))),
            (2, Some(serde_json::json!(0))),
            (2, Some(serde_json::json!(3))),
        ] {
            let mut value = original.clone();
            value["version"] = document_version.into();
            if let Some(field) = field {
                value["stage_compiler_version"] = field;
            }
            assert!(Replay::decode(serde_json::to_vec(&value).unwrap().as_slice()).is_err());
        }
        let mut identity = legacy.identity().clone();
        identity.stage_compiler_version = Some(3);
        assert!(Replay::new(identity, legacy.epoch()).is_err());
    }

    #[test]
    fn rejects_truncation_bad_headers_unknown_fields_and_invalid_numbers() {
        let mut replay = empty();
        replay.record(hit(PlayerId::P1, 1, -48)).unwrap();
        let bytes = replay.encode().unwrap();
        for end in 0..bytes.len() {
            assert!(
                Replay::decode(&bytes[..end]).is_err(),
                "accepted prefix {end}"
            );
        }
        let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for (key, value) in [
            ("format", serde_json::json!("other")),
            ("version", serde_json::json!(2)),
            ("unexpected", serde_json::json!(true)),
            ("content_id", serde_json::json!("")),
            ("build_id", serde_json::json!("x".repeat(257))),
        ] {
            let mut changed = original.clone();
            changed[key] = value;
            assert!(Replay::decode(serde_json::to_vec(&changed).unwrap().as_slice()).is_err());
        }
        for (key, value) in [
            ("player", serde_json::json!(3)),
            ("song_time_frames", serde_json::json!(1.5)),
            ("seq", serde_json::json!(-1)),
            ("unexpected", serde_json::json!(true)),
        ] {
            let mut changed = original.clone();
            changed["facts"][0][key] = value;
            assert!(Replay::decode(serde_json::to_vec(&changed).unwrap().as_slice()).is_err());
        }
    }

    #[test]
    fn bounded_reader_and_recorder_report_limits() {
        let mut source = io::Cursor::new(vec![b' '; MAX_FILE_BYTES as usize + 100]);
        assert!(
            Replay::decode(&mut source)
                .unwrap_err()
                .to_string()
                .contains("byte limit")
        );
        assert_eq!(source.position(), MAX_FILE_BYTES + 1);
        let mut replay = Replay::new(
            ReplayIdentity {
                content_id: "\0".repeat(MAX_IDENTITY_BYTES),
                rules_id: "\0".repeat(MAX_IDENTITY_BYTES),
                build_id: "\0".repeat(MAX_IDENTITY_BYTES),
                stage_compiler_version: None,
            },
            SessionEpoch(u64::MAX),
        )
        .unwrap();
        let fact = DuoInput::Hit(Hit {
            epoch: SessionEpoch(u64::MAX),
            player: PlayerId::P1,
            seq: u64::MAX,
            song_time: SongTime::from_frames(i64::MIN),
        });
        for _ in 0..MAX_FACTS {
            replay.record(fact).unwrap();
        }
        assert!(replay.record(fact).is_err());
        assert_eq!(replay.facts().len(), MAX_FACTS);
        let bytes = replay.encode().unwrap();
        assert!(bytes.len() as u64 <= MAX_FILE_BYTES);
        assert_eq!(Replay::decode(bytes.as_slice()).unwrap(), replay);
        let mut document: Document = serde_json::from_slice(&bytes).unwrap();
        document.facts.push(Fact::Hit {
            epoch: 7,
            player: 1,
            seq: 0,
            song_time_frames: 0,
        });
        assert!(
            Replay::decode(serde_json::to_vec(&document).unwrap().as_slice())
                .unwrap_err()
                .to_string()
                .contains("fact limit")
        );
    }

    #[test]
    fn saves_complete_files_and_preserves_existing_destination_on_failure() {
        let root = std::env::temp_dir().join(format!(
            "cocobeat-replay-test-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("session.replay");
        let mut replay = empty();
        replay.record(hit(PlayerId::P1, 0, -48)).unwrap();
        replay.save(&path).unwrap();
        assert_eq!(Replay::load(&path).unwrap(), replay);

        let previous = fs::read(&path).unwrap();
        let directory = root.join("occupied");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("keep"), b"previous data").unwrap();
        assert!(replay.save(&directory).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(fs::read(directory.join("keep")).unwrap(), b"previous data");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    fn history(frame_intervals: &[i64], reverse_batch: bool) -> Replay {
        let mut replay = empty();
        let hits = [
            hit(PlayerId::P1, 1, 48_000),
            hit(PlayerId::P2, 1, 48_048),
            hit(PlayerId::P1, 2, 96_000),
            hit(PlayerId::P2, 2, 96_480),
            hit(PlayerId::P1, 3, 144_000),
            hit(PlayerId::P2, 3, 143_520),
        ];
        let mut previous = -1;
        let mut through = 0;
        let mut frame = 0;
        while through <= 240_000 {
            let mut batch: Vec<_> = hits
                .iter()
                .copied()
                .filter(|fact| match fact {
                    DuoInput::Hit(hit) => {
                        hit.song_time.frames() > previous && hit.song_time.frames() <= through
                    }
                    _ => unreachable!(),
                })
                .collect();
            if reverse_batch {
                batch.reverse();
            }
            for fact in batch {
                replay.record(fact).unwrap();
            }
            replay.record(watermark(PlayerId::P1, through)).unwrap();
            replay.record(watermark(PlayerId::P2, through)).unwrap();
            previous = through;
            if through == 240_000 {
                break;
            }
            through = (through + frame_intervals[frame % frame_intervals.len()]).min(240_000);
            frame += 1;
        }
        replay
    }

    #[test]
    fn live_and_serialized_playback_agree_across_frame_batches_and_reordering() {
        let anchors = vec![Anchor {
            id: 1,
            song_time: SongTime::from_frames(96_000),
        }];
        let rules = DuoRules::default();
        let mut expected = DuoEngine::new(SessionEpoch(7), anchors.clone(), rules).unwrap();
        for fact in history(&[240_000], false).facts() {
            expected.ingest(*fact).unwrap();
        }
        assert!(!expected.events().is_empty());
        // Exact 60 Hz, exact 144 Hz, and 500 ms render stalls
        for (intervals, reverse_batch) in [
            (&[800][..], false),
            (&[333, 333, 334][..], true),
            (&[24_000][..], true),
        ] {
            let recorded = history(intervals, reverse_batch);
            let loaded = Replay::decode(recorded.encode().unwrap().as_slice()).unwrap();
            let actual = loaded
                .replay("fixture-64s-v1", "duo-v1", anchors.clone(), rules)
                .unwrap();
            assert_eq!(
                actual.events(),
                expected.events(),
                "intervals={intervals:?}"
            );
            assert_eq!(
                actual.resonance(),
                expected.resonance(),
                "intervals={intervals:?}"
            );
        }
        assert!(
            empty()
                .replay("wrong-content", "duo-v1", anchors.clone(), rules)
                .is_err()
        );
        assert!(
            empty()
                .replay("fixture-64s-v1", "wrong-rules", anchors, rules)
                .is_err()
        );
    }

    #[test]
    fn ten_minute_duet_survives_save_and_replay_with_dense_input() {
        let mut recorded = empty();
        let rules = DuoRules::default();
        let mut live = DuoEngine::new(recorded.epoch(), Vec::new(), rules).unwrap();
        let mut record = |fact| {
            live.ingest(fact).unwrap();
            recorded.record(fact).unwrap();
        };
        // Twenty hits per second per player, with the runtime's 10 ms checkpoints
        for step in 0..=60_000 {
            if step < 60_000 && step % 5 == 0 {
                for player in [PlayerId::P1, PlayerId::P2] {
                    record(hit(player, step as u64 / 5, step * 480));
                }
            }
            for player in [PlayerId::P1, PlayerId::P2] {
                record(watermark(player, step * 480 - 2_400));
            }
        }
        let end = 28_800_000 + rules.confirmation_delay_frames().unwrap() + 1;
        for player in [PlayerId::P1, PlayerId::P2] {
            record(watermark(player, end));
        }
        assert_eq!(recorded.facts().len(), 144_004);
        assert_eq!(live.events().len(), 12_000);
        let loaded = Replay::decode(recorded.encode().unwrap().as_slice()).unwrap();
        assert_eq!(loaded, recorded);
        let restored = loaded
            .replay("fixture-64s-v1", "duo-v1", Vec::new(), rules)
            .unwrap();
        assert_eq!(restored.events(), live.events());
        assert_eq!(restored.resonance(), live.resonance());
    }

    #[test]
    fn semantic_errors_identify_the_original_fact_without_reordering() {
        for (facts, cause) in [
            (
                vec![hit(PlayerId::P1, 1, 0), hit(PlayerId::P1, 1, 0)],
                "DuplicateHit",
            ),
            (
                vec![hit(PlayerId::P1, 1, 0), hit(PlayerId::P1, 1, 1)],
                "ConflictingHit",
            ),
            (
                vec![watermark(PlayerId::P1, 1), watermark(PlayerId::P1, 0)],
                "WatermarkRegression",
            ),
            (
                vec![watermark(PlayerId::P1, 1), hit(PlayerId::P1, 1, 1)],
                "ClosedHistory",
            ),
            (
                vec![
                    watermark(PlayerId::P1, -1),
                    DuoInput::Watermark {
                        epoch: SessionEpoch(8),
                        player: PlayerId::P2,
                        through: SongTime::ZERO,
                    },
                ],
                "EpochMismatch",
            ),
        ] {
            let mut replay = empty();
            for fact in facts {
                replay.record(fact).unwrap();
            }
            let error = replay
                .replay("fixture-64s-v1", "duo-v1", vec![], DuoRules::default())
                .unwrap_err()
                .to_string();
            assert!(error.starts_with("Replay fact 2:"), "{error}");
            assert!(error.contains(cause), "{error}");
        }
        let error = empty()
            .replay("wrong", "duo-v1", vec![], DuoRules::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("identity mismatch"));
        assert!(!error.contains("Replay fact"));
    }
}
