//! Independent manual labels for one validated canonical package
//! Positions are explicit reviewer entries, not captured input or audio timestamps

use cocobeat_media::ValidatedPackage;
use cocobeat_schema::MAX_CANONICAL_FRAMES;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MAX_BYTES: usize = 1_048_576;
const MAX_LABELS: usize = 1024;
// Two bounded inputs plus pretty comparison rows and decision flags
const MAX_COMPARISON_BYTES: usize = 4 * MAX_BYTES;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) content_id: String,
    pub(crate) audio_blake3: String,
    pub(crate) canonical_frames: u64,
    pub(crate) audio_basis: AudioBasis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AudioBasis {
    CanonicalDecoded,
}

impl Source {
    pub(crate) fn from_package(package: &ValidatedPackage) -> Self {
        Self {
            content_id: format!(
                "package-blake3:{}",
                blake3::Hash::from(package.manifest.package_hash)
            ),
            audio_blake3: blake3::Hash::from(package.manifest.audio.blake3).to_string(),
            canonical_frames: package.manifest.canonical_frames,
            audio_basis: AudioBasis::CanonicalDecoded,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    pub(crate) schema_version: u32,
    pub(crate) source: Source,
    pub(crate) reviewer: String,
    pub(crate) labels: Vec<Label>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Label {
    pub(crate) item_id: u64,
    pub(crate) location: Location,
    pub(crate) anchor_decision: AnchorDecision,
    pub(crate) reason: String,
    pub(crate) playback: Playback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Location {
    Point { frame: i64 },
    Interval { start_frame: i64, end_frame: i64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnchorDecision {
    ShouldAnchor,
    ShouldNotAnchor,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Playback {
    Stereo,
}

impl Document {
    pub(crate) fn validate(&self, expected: &Source) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported independent label schema".into());
        }
        if self.source != *expected {
            return Err(
                "Label source identity or audio basis differs from the validated package".into(),
            );
        }
        if expected.canonical_frames == 0 || expected.canonical_frames > MAX_CANONICAL_FRAMES {
            return Err("Label source has invalid canonical extent".into());
        }
        if self.reviewer.trim().is_empty()
            || self.reviewer.trim() != self.reviewer
            || self.reviewer.len() > 64
        {
            return Err(
                "Reviewer alias must contain 1..64 UTF-8 bytes without outer whitespace".into(),
            );
        }
        if self.labels.len() > MAX_LABELS {
            return Err("Label document exceeds 1024 records".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        let end = expected.canonical_frames as i64;
        for label in &self.labels {
            if !ids.insert(label.item_id) {
                return Err("Label item_id is duplicated".into());
            }
            let valid = match label.location {
                Location::Point { frame } => (0..end).contains(&frame),
                Location::Interval {
                    start_frame,
                    end_frame,
                } => 0 <= start_frame && start_frame < end_frame && end_frame <= end,
            };
            if !valid {
                return Err(format!(
                    "Label {} position must be inside the canonical song; intervals are half-open",
                    label.item_id
                ));
            }
            if label.reason.trim().is_empty() || label.reason.len() > 2048 {
                return Err(format!(
                    "Label {} reason must contain 1..2048 UTF-8 bytes",
                    label.item_id
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn load(path: &Path, expected: &Source) -> Result<Document, String> {
    load_identified(path, expected).map(|(document, _)| document)
}

pub(crate) fn load_identified(
    path: &Path,
    expected: &Source,
) -> Result<(Document, blake3::Hash), String> {
    if !std::fs::symlink_metadata(path)
        .map_err(|error| format!("Inspect independent labels: {error}"))?
        .is_file()
    {
        return Err("Independent labels must be a regular file".into());
    }
    let file = File::open(path).map_err(|error| format!("Open independent labels: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Independent labels must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Label document exceeds 1 MiB".into());
    }
    let document: Document = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid independent labels: {error}"))?;
    document.validate(expected)?;
    Ok((document, blake3::hash(&bytes)))
}

// Caller revalidates the package immediately before save and keeps output outside it
pub(crate) fn save(
    path: &Path,
    expected: &Source,
    document: &Document,
) -> Result<blake3::Hash, String> {
    document.validate(expected)?;
    write_new(path, document, MAX_BYTES)
}

#[derive(Debug, Serialize)]
pub(crate) struct Comparison {
    pub(crate) schema_version: u32,
    pub(crate) source: Source,
    pub(crate) left_reviewer: String,
    pub(crate) right_reviewer: String,
    pub(crate) common: Vec<ComparedLabel>,
    pub(crate) left_only: Vec<Label>,
    pub(crate) right_only: Vec<Label>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ComparedLabel {
    pub(crate) left: Label,
    pub(crate) right: Label,
    pub(crate) same_location: bool,
    pub(crate) same_anchor_decision: bool,
    pub(crate) same_reason: bool,
    pub(crate) same_playback: bool,
}

pub(crate) fn compare(
    expected: &Source,
    left: &Document,
    right: &Document,
) -> Result<Comparison, String> {
    left.validate(expected)?;
    right.validate(expected)?;
    if left.reviewer == right.reviewer {
        return Err("Comparison requires different reviewer aliases".into());
    }
    let right_items: BTreeMap<_, _> = right
        .labels
        .iter()
        .map(|label| (label.item_id, label))
        .collect();
    let left_items: BTreeMap<_, _> = left
        .labels
        .iter()
        .map(|label| (label.item_id, label))
        .collect();
    let mut result = Comparison {
        schema_version: 1,
        source: expected.clone(),
        left_reviewer: left.reviewer.clone(),
        right_reviewer: right.reviewer.clone(),
        common: Vec::new(),
        left_only: Vec::new(),
        right_only: Vec::new(),
    };
    for (&id, &a) in &left_items {
        if let Some(&b) = right_items.get(&id) {
            result.common.push(ComparedLabel {
                left: a.clone(),
                right: b.clone(),
                same_location: a.location == b.location,
                same_anchor_decision: a.anchor_decision == b.anchor_decision,
                same_reason: a.reason == b.reason,
                same_playback: a.playback == b.playback,
            });
        } else {
            result.left_only.push(a.clone());
        }
    }
    for (&id, &label) in &right_items {
        if !left_items.contains_key(&id) {
            result.right_only.push(label.clone());
        }
    }
    Ok(result)
}

pub(crate) fn save_comparison(path: &Path, comparison: &Comparison) -> Result<(), String> {
    write_new(path, comparison, MAX_COMPARISON_BYTES).map(|_| ())
}

pub(crate) fn write_new(
    path: &Path,
    value: &impl Serialize,
    limit: usize,
) -> Result<blake3::Hash, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    if bytes.len() > limit {
        return Err("Serialized independent label document exceeds its size limit".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Create new independent label document: {error}"))?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        // Only this function's newly created file is removed after a returned write error
        let cleanup = std::fs::remove_file(path);
        return Err(match cleanup {
            Ok(()) => error.to_string(),
            Err(cleanup) => format!("{error}; partial output cleanup failed: {cleanup}"),
        });
    }
    Ok(blake3::hash(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Source {
        Source {
            content_id: format!("package-blake3:{}", "1".repeat(64)),
            audio_blake3: "2".repeat(64),
            canonical_frames: 4800,
            audio_basis: AudioBasis::CanonicalDecoded,
        }
    }
    fn document(reviewer: &str) -> Document {
        Document {
            schema_version: 1,
            source: source(),
            reviewer: reviewer.into(),
            labels: vec![Label {
                item_id: u64::MAX,
                location: Location::Point { frame: 0 },
                anchor_decision: AnchorDecision::ShouldNotAnchor,
                reason: "Audible change, unsuitable as an Anchor".into(),
                playback: Playback::Stereo,
            }],
        }
    }
    #[test]
    fn positions_and_source_identity_reject_invalid_records() {
        let mut d = document("reviewer-a");
        d.validate(&source()).unwrap();
        for location in [
            Location::Point { frame: -1 },
            Location::Point { frame: 4800 },
            Location::Interval {
                start_frame: 2,
                end_frame: 2,
            },
            Location::Interval {
                start_frame: 3,
                end_frame: 2,
            },
            Location::Interval {
                start_frame: 0,
                end_frame: 4801,
            },
        ] {
            d.labels[0].location = location;
            assert!(d.validate(&source()).is_err());
        }
        d.labels[0].location = Location::Interval {
            start_frame: 0,
            end_frame: 4800,
        };
        d.validate(&source()).unwrap();
        d.source.audio_blake3 = "3".repeat(64);
        assert!(d.validate(&source()).is_err());
        d.source = source();
        d.labels.push(d.labels[0].clone());
        assert!(d.validate(&source()).is_err());
    }
    #[test]
    fn comparison_preserves_disagreement_and_unmatched_records() {
        let left = document("reviewer-a");
        let mut right = document("reviewer-b");
        right.labels[0].location = Location::Point { frame: 1 };
        right.labels[0].anchor_decision = AnchorDecision::Uncertain;
        right.labels[0].reason = "Timing ambiguous".into();
        let mut extra = right.labels[0].clone();
        extra.item_id = 7;
        right.labels.push(extra);
        let c = compare(&source(), &left, &right).unwrap();
        assert_eq!(c.common.len(), 1);
        assert!(!c.common[0].same_location);
        assert!(!c.common[0].same_anchor_decision);
        assert!(!c.common[0].same_reason);
        assert_eq!(c.common[0].left, left.labels[0]);
        assert_eq!(c.common[0].right, right.labels[0]);
        assert_eq!(c.right_only, vec![right.labels[1].clone()]);
        right.reviewer = left.reviewer.clone();
        assert!(compare(&source(), &left, &right).is_err());
    }
    #[test]
    fn json_unknown_duplicate_and_extra_location_fields_are_rejected() {
        let d = document("reviewer-a");
        let bytes = serde_json::to_string(&d).unwrap();
        for invalid in [
            bytes.replacen('{', "{\"extra\":1,", 1),
            bytes.replacen('{', "{\"schema_version\":1,", 1),
            bytes.replace("\"frame\":0", "\"frame\":0,\"end_frame\":1"),
        ] {
            assert!(serde_json::from_str::<Document>(&invalid).is_err());
        }
        assert_eq!(serde_json::from_str::<Document>(&bytes).unwrap(), d);
    }
    #[test]
    fn near_limit_valid_inputs_export_the_full_comparison() {
        let mut left = document("reviewer-a");
        left.labels = (0..448)
            .map(|id| Label {
                item_id: id,
                location: Location::Point { frame: id as i64 },
                anchor_decision: AnchorDecision::ShouldNotAnchor,
                reason: "r".repeat(2048),
                playback: Playback::Stereo,
            })
            .collect();
        let mut right = left.clone();
        right.reviewer = "reviewer-b".into();
        assert!(serde_json::to_vec_pretty(&left).unwrap().len() < MAX_BYTES);
        assert!(serde_json::to_vec_pretty(&right).unwrap().len() < MAX_BYTES);
        let result = compare(&source(), &left, &right).unwrap();
        assert!(serde_json::to_vec_pretty(&result).unwrap().len() > MAX_BYTES * 2);
        let path = std::env::temp_dir().join(format!(
            "cocobeat-label-report-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        save_comparison(&path, &result).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() <= MAX_COMPARISON_BYTES);
        let restored: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored["common"].as_array().unwrap().len(), 448);
        assert_eq!(restored["common"][447]["left"]["reason"], "r".repeat(2048));
        assert_eq!(restored["common"][447]["right"]["reason"], "r".repeat(2048));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn new_file_roundtrip_conflict_and_validation_leave_existing_bytes() {
        let dir = std::env::temp_dir().join(format!(
            "cocobeat-labels-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("a.json");
        let d = document("reviewer-a");
        save(&path, &source(), &d).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert_eq!(load(&path, &source()).unwrap(), d);
        assert!(save(&path, &source(), &d).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let mut bad = d;
        bad.reviewer.clear();
        let output = dir.join("bad.json");
        assert!(save(&output, &source(), &bad).is_err());
        assert!(!output.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
