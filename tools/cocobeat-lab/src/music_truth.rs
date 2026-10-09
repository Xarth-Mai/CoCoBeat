//! Independent manual music events on validated canonical audio
//! Coverage is a reviewer declaration; these records do not establish algorithm admission

use crate::{labels, labels_cli};
use cocobeat_schema::{CANONICAL_SAMPLE_RATE, MAX_CANONICAL_FRAMES};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

const MAX_BYTES: usize = 1_048_576;
const MAX_EVENTS: usize = 65_536;
const MAX_INTERVALS: usize = 1024;
const MAX_REPORT_BYTES: usize = 4 * MAX_BYTES;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) origin_content_id: String,
    audio_blake3: String,
    canonical_frames: u64,
    canonical_sample_rate: u32,
    channels: u8,
    audio_basis: labels::AudioBasis,
}

impl Source {
    pub(crate) fn from_snapshot(source: &labels::Source) -> Self {
        Self {
            origin_content_id: source.content_id.clone(),
            audio_blake3: source.audio_blake3.clone(),
            canonical_frames: source.canonical_frames,
            canonical_sample_rate: CANONICAL_SAMPLE_RATE,
            channels: 2,
            audio_basis: labels::AudioBasis::CanonicalDecoded,
        }
    }

    fn validate(&self, expected: &Self) -> Result<(), String> {
        fn hash(value: &str) -> bool {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        if !self
            .origin_content_id
            .strip_prefix("package-blake3:")
            .is_some_and(hash)
            || !hash(&self.audio_blake3)
            || !(1..=MAX_CANONICAL_FRAMES).contains(&self.canonical_frames)
            || self.canonical_sample_rate != CANONICAL_SAMPLE_RATE
            || self.channels != 2
        {
            return Err("Invalid music truth source identity or canonical extent".into());
        }
        // The declared origin CID is provenance, not the cross-chart audio applicability key
        if self.audio_blake3 != expected.audio_blake3
            || self.canonical_frames != expected.canonical_frames
            || self.canonical_sample_rate != expected.canonical_sample_rate
            || self.channels != expected.channels
            || self.audio_basis != expected.audio_basis
        {
            return Err(
                "Music truth audio identity or basis differs from the validated package".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Channel {
    Stereo,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Interval {
    pub(crate) start_frame: u64,
    pub(crate) end_frame: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Track {
    pub(crate) reviewed: Vec<Interval>,
    pub(crate) frames: Vec<u64>,
}

impl Track {
    fn validate(&self, n: u64) -> Result<(), String> {
        if self
            .reviewed
            .iter()
            .any(|r| r.start_frame >= r.end_frame || r.end_frame > n)
            || self
                .reviewed
                .windows(2)
                .any(|pair| pair[0].end_frame > pair[1].start_frame)
        {
            return Err("Music truth coverage must be ordered, nonoverlapping, half-open and inside the song".into());
        }
        if self.frames.iter().any(|&frame| frame >= n)
            || self.frames.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(
                "Music truth frames must be strictly increasing integers inside the song".into(),
            );
        }
        let (_, uncovered) = split_frames(&self.frames, &self.reviewed);
        if !uncovered.is_empty() {
            return Err(
                "Music truth event is outside this kind's declared reviewed coverage".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tracks {
    pub(crate) onset: Track,
    pub(crate) beat: Track,
    pub(crate) downbeat: Track,
}

impl Tracks {
    fn all(&self) -> [&Track; 3] {
        [&self.onset, &self.beat, &self.downbeat]
    }
}

// Private serde adapter for the existing schema beat-unit values
#[derive(Serialize, Deserialize)]
#[serde(remote = "cocobeat_schema::TempoBeatUnit", rename_all = "snake_case")]
enum TempoBeatUnitSerde {
    Quarter,
    Eighth,
    DottedQuarter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct BeatUnit(#[serde(with = "TempoBeatUnitSerde")] cocobeat_schema::TempoBeatUnit);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Meter {
    numerator: u8,
    denominator: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReviewBasis {
    HumanListening,
    ScoreDocument,
    ComposerDeclaration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    basis: ReviewBasis,
    note: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MetricalContext {
    start_frame: u64,
    end_frame: u64,
    // Explicit null means reviewed but unknown; missing fields are rejected
    #[serde(deserialize_with = "required_option")]
    beat_unit: Option<BeatUnit>,
    #[serde(deserialize_with = "required_option")]
    meter: Option<Meter>,
    provenance: Provenance,
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

// Default handles absent v1 fields; a present null array is never an empty review
fn present_context<'de, D>(deserializer: D) -> Result<Option<Vec<MetricalContext>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<MetricalContext>::deserialize(deserializer).map(Some)
}

fn validate_context(context: &[MetricalContext], n: u64) -> Result<(), String> {
    if context
        .iter()
        .any(|r| r.start_frame >= r.end_frame || r.end_frame > n)
        || context
            .windows(2)
            .any(|pair| pair[0].end_frame > pair[1].start_frame)
    {
        return Err("Music truth metrical coverage must be ordered, nonoverlapping, half-open and inside the song".into());
    }
    for record in context {
        if record
            .meter
            .is_some_and(|meter| meter.numerator == 0 || !meter.denominator.is_power_of_two())
        {
            return Err(
                "Music truth meter requires a positive numerator and a power-of-two denominator"
                    .into(),
            );
        }
        // Matches the existing independent-label reason ceiling
        if record.provenance.note.trim().is_empty() || record.provenance.note.len() > 2048 {
            return Err(
                "Music truth metrical provenance note must contain 1..2048 UTF-8 bytes".into(),
            );
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    schema_version: u32,
    pub(crate) source: Source,
    pub(crate) reviewer: String,
    pub(crate) channel: Channel,
    pub(crate) tracks: Tracks,
    #[serde(
        default,
        deserialize_with = "present_context",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) metrical_context: Option<Vec<MetricalContext>>,
}

impl Document {
    fn validate(&self, expected: &Source) -> Result<(), String> {
        match (self.schema_version, &self.metrical_context) {
            (1, None) => {}
            (2, Some(context)) => validate_context(context, self.source.canonical_frames)?,
            _ => {
                return Err(
                    "Music truth v1 forbids metrical context; v2 requires a context array".into(),
                );
            }
        }
        self.source.validate(expected)?;
        if self.reviewer.trim().is_empty()
            || self.reviewer.trim() != self.reviewer
            || self.reviewer.len() > 64
        {
            return Err(
                "Music truth reviewer must contain 1..64 UTF-8 bytes without outer whitespace"
                    .into(),
            );
        }
        let tracks = self.tracks.all();
        if tracks
            .iter()
            .try_fold(0usize, |n, track| n.checked_add(track.frames.len()))
            .is_none_or(|n| n > MAX_EVENTS)
            || tracks
                .iter()
                .try_fold(0usize, |n, track| n.checked_add(track.reviewed.len()))
                .and_then(|n| n.checked_add(self.metrical_context.as_ref().map_or(0, Vec::len)))
                .is_none_or(|n| n > MAX_INTERVALS)
        {
            return Err("Music truth exceeds 65536 events or 1024 coverage intervals".into());
        }
        for track in tracks {
            track.validate(self.source.canonical_frames)?;
        }
        Ok(())
    }
}

pub(crate) fn load(path: &Path, expected: &Source) -> Result<(Document, blake3::Hash), String> {
    if !fs::symlink_metadata(path)
        .map_err(|error| format!("Inspect music truth: {error}"))?
        .is_file()
    {
        return Err("Music truth must be a regular file, not a symlink".into());
    }
    let file = File::open(path).map_err(|error| format!("Open music truth: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Music truth must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Music truth document exceeds 1 MiB".into());
    }
    let document: Document = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid music truth JSON: {error}"))?;
    document.validate(expected)?;
    Ok((document, blake3::hash(&bytes)))
}

pub(crate) fn inspect_source(package: &Path) -> Result<(), String> {
    let snapshot = labels::Source::from_package(&cocobeat_media::validate_package(package)?);
    println!(
        "{}",
        serde_json::to_string_pretty(&Source::from_snapshot(&snapshot))
            .map_err(|error| error.to_string())?
    );
    Ok(())
}

pub(crate) fn template(
    package: &Path,
    reviewer: &str,
    channel: &str,
    destination: &Path,
) -> Result<(), String> {
    template_kind(package, reviewer, channel, destination, false)
}

pub(crate) fn template_metrical(
    package: &Path,
    reviewer: &str,
    channel: &str,
    destination: &Path,
) -> Result<(), String> {
    template_kind(package, reviewer, channel, destination, true)
}

fn template_kind(
    package: &Path,
    reviewer: &str,
    channel: &str,
    destination: &Path,
    metrical: bool,
) -> Result<(), String> {
    let channel = match channel {
        "stereo" => Channel::Stereo,
        "left" => Channel::Left,
        "right" => Channel::Right,
        _ => return Err("Music truth channel must be stereo, left or right".into()),
    };
    let initial = labels::Source::from_package(&cocobeat_media::validate_package(package)?);
    let source = Source::from_snapshot(&initial);
    let document = Document {
        schema_version: if metrical { 2 } else { 1 },
        source: source.clone(),
        reviewer: reviewer.into(),
        channel,
        tracks: Tracks::default(),
        metrical_context: metrical.then(Vec::new),
    };
    document.validate(&source)?;
    let destination = labels_cli::outside_package(package, destination)?;
    labels_cli::require_fresh_source(package, &initial)?;
    let written_hash = labels::write_new(&destination, &document, MAX_BYTES)?;
    println!(
        "{}",
        serde_json::json!({
            "source": source,
            "validated_content_id": initial.content_id,
            "reviewer": reviewer,
            "channel": channel,
            "saved_blake3": written_hash.to_hex().as_str(),
            "destination": destination,
            "scope": if metrical { "empty_manual_template_with_metrical_context_only" } else { "empty_manual_template_only" },
        })
    );
    Ok(())
}

pub(crate) fn import(package: &Path, input: &Path, destination: &Path) -> Result<(), String> {
    let initial = labels::Source::from_package(&cocobeat_media::validate_package(package)?);
    let (document, raw_hash) = load(input, &Source::from_snapshot(&initial))?;
    let destination = labels_cli::outside_package(package, destination)?;
    labels_cli::require_fresh_source(package, &initial)?;
    let written_hash = labels::write_new(&destination, &document, MAX_BYTES)?;
    println!(
        "{}",
        serde_json::json!({
            "source": document.source,
            "validated_content_id": initial.content_id,
            "reviewer": document.reviewer,
            "channel": document.channel,
            "raw_input_blake3": raw_hash.to_hex().as_str(),
            "saved_blake3": written_hash.to_hex().as_str(),
            "event_count": document.tracks.all().iter().map(|track| track.frames.len()).sum::<usize>(),
            "destination": destination,
            "scope": "manual_records_only",
        })
    );
    Ok(())
}

#[derive(Debug, Serialize)]
struct TrackComparison {
    mutual_reviewed: Vec<Interval>,
    same_frames: Vec<u64>,
    left_only_frames: Vec<u64>,
    right_only_frames: Vec<u64>,
    left_uncompared_frames: Vec<u64>,
    right_uncompared_frames: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct Comparison {
    schema_version: u32,
    // This source is the current verified package; declared historical origins stay separate
    source: Source,
    validated_content_id: String,
    channel: Channel,
    left_reviewer: String,
    right_reviewer: String,
    left_declared_origin_content_id: String,
    right_declared_origin_content_id: String,
    left_raw_blake3: String,
    right_raw_blake3: String,
    comparison_basis: &'static str,
    scope: &'static str,
    onset: TrackComparison,
    beat: TrackComparison,
    downbeat: TrackComparison,
    #[serde(skip_serializing_if = "Option::is_none")]
    metrical_context: Option<MetricalComparison>,
}

#[derive(Debug, Serialize)]
struct MetricalComparison {
    left_context: Vec<MetricalContext>,
    right_context: Vec<MetricalContext>,
    overlaps: Vec<MetricalOverlap>,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DeclarationComparison {
    Same,
    Different,
    Unknown,
}

#[derive(Debug, Serialize)]
struct MetricalOverlap {
    start_frame: u64,
    end_frame: u64,
    left_index: usize,
    right_index: usize,
    beat_unit: DeclarationComparison,
    meter: DeclarationComparison,
}

fn declarations<T: Eq>(left: &Option<T>, right: &Option<T>) -> DeclarationComparison {
    match (left, right) {
        (Some(a), Some(b)) if a == b => DeclarationComparison::Same,
        (Some(_), Some(_)) => DeclarationComparison::Different,
        _ => DeclarationComparison::Unknown,
    }
}

fn compare_context(left: &Document, right: &Document) -> Option<MetricalComparison> {
    if left.metrical_context.is_none() && right.metrical_context.is_none() {
        return None;
    }
    let left_context = left.metrical_context.clone().unwrap_or_default();
    let right_context = right.metrical_context.clone().unwrap_or_default();
    let (mut a, mut b, mut overlaps) = (0, 0, Vec::new());
    while a < left_context.len() && b < right_context.len() {
        let l = &left_context[a];
        let r = &right_context[b];
        let start_frame = l.start_frame.max(r.start_frame);
        let end_frame = l.end_frame.min(r.end_frame);
        if start_frame < end_frame {
            overlaps.push(MetricalOverlap {
                start_frame,
                end_frame,
                left_index: a,
                right_index: b,
                beat_unit: declarations(&l.beat_unit, &r.beat_unit),
                meter: declarations(&l.meter, &r.meter),
            });
        }
        match l.end_frame.cmp(&r.end_frame) {
            std::cmp::Ordering::Less => a += 1,
            std::cmp::Ordering::Greater => b += 1,
            std::cmp::Ordering::Equal => {
                a += 1;
                b += 1;
            }
        }
    }
    Some(MetricalComparison {
        left_context,
        right_context,
        overlaps,
    })
}

fn intersection(left: &[Interval], right: &[Interval]) -> Vec<Interval> {
    let mut result = Vec::new();
    let (mut a, mut b) = (0, 0);
    while a < left.len() && b < right.len() {
        let start_frame = left[a].start_frame.max(right[b].start_frame);
        let end_frame = left[a].end_frame.min(right[b].end_frame);
        if start_frame < end_frame {
            result.push(Interval {
                start_frame,
                end_frame,
            });
        }
        match left[a].end_frame.cmp(&right[b].end_frame) {
            std::cmp::Ordering::Less => a += 1,
            std::cmp::Ordering::Greater => b += 1,
            std::cmp::Ordering::Equal => {
                a += 1;
                b += 1;
            }
        }
    }
    result
}

fn split_frames(frames: &[u64], reviewed: &[Interval]) -> (Vec<u64>, Vec<u64>) {
    let (mut covered, mut uncovered) = (Vec::new(), Vec::new());
    let mut index = 0;
    for &frame in frames {
        while index < reviewed.len() && reviewed[index].end_frame <= frame {
            index += 1;
        }
        if reviewed.get(index).is_some_and(|r| r.start_frame <= frame) {
            covered.push(frame);
        } else {
            uncovered.push(frame);
        }
    }
    (covered, uncovered)
}

fn compare_track(left: &Track, right: &Track) -> TrackComparison {
    let mutual_reviewed = intersection(&left.reviewed, &right.reviewed);
    let (a, left_uncompared_frames) = split_frames(&left.frames, &mutual_reviewed);
    let (b, right_uncompared_frames) = split_frames(&right.frames, &mutual_reviewed);
    let (mut same_frames, mut left_only_frames, mut right_only_frames) =
        (Vec::new(), Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                left_only_frames.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                right_only_frames.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                same_frames.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    left_only_frames.extend_from_slice(&a[i..]);
    right_only_frames.extend_from_slice(&b[j..]);
    TrackComparison {
        mutual_reviewed,
        same_frames,
        left_only_frames,
        right_only_frames,
        left_uncompared_frames,
        right_uncompared_frames,
    }
}

fn comparison(
    expected: &Source,
    left: &Document,
    left_hash: blake3::Hash,
    right: &Document,
    right_hash: blake3::Hash,
) -> Result<Comparison, String> {
    left.validate(expected)?;
    right.validate(expected)?;
    if left.reviewer == right.reviewer || left.channel != right.channel {
        return Err(
            "Music truth comparison requires different reviewers and the same channel".into(),
        );
    }
    let metrical_context = compare_context(left, right);
    Ok(Comparison {
        schema_version: if metrical_context.is_some() { 2 } else { 1 },
        source: expected.clone(),
        validated_content_id: expected.origin_content_id.clone(),
        channel: left.channel,
        left_reviewer: left.reviewer.clone(),
        right_reviewer: right.reviewer.clone(),
        left_declared_origin_content_id: left.source.origin_content_id.clone(),
        right_declared_origin_content_id: right.source.origin_content_id.clone(),
        left_raw_blake3: left_hash.to_string(),
        right_raw_blake3: right_hash.to_string(),
        comparison_basis: "exact_frame_in_mutual_reviewed_coverage",
        scope: "manual_records_only",
        onset: compare_track(&left.tracks.onset, &right.tracks.onset),
        beat: compare_track(&left.tracks.beat, &right.tracks.beat),
        downbeat: compare_track(&left.tracks.downbeat, &right.tracks.downbeat),
        metrical_context,
    })
}

pub(crate) fn compare(
    package: &Path,
    left: &Path,
    right: &Path,
    destination: &Path,
) -> Result<(), String> {
    let initial = labels::Source::from_package(&cocobeat_media::validate_package(package)?);
    let source = Source::from_snapshot(&initial);
    let (left, left_hash) = load(left, &source)?;
    let (right, right_hash) = load(right, &source)?;
    let report = comparison(&source, &left, left_hash, &right, right_hash)?;
    let destination = labels_cli::outside_package(package, destination)?;
    labels_cli::require_fresh_source(package, &initial)?;
    let written_hash = labels::write_new(&destination, &report, MAX_REPORT_BYTES)?;
    println!(
        "{}",
        serde_json::json!({
            "source": source,
            "validated_content_id": initial.content_id,
            "left_raw_blake3": left_hash.to_hex().as_str(),
            "right_raw_blake3": right_hash.to_hex().as_str(),
            "saved_blake3": written_hash.to_hex().as_str(),
            "destination": destination,
            "scope": "manual_records_only",
            "comparison_basis": "exact_frame_in_mutual_reviewed_coverage",
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Source {
        Source {
            origin_content_id: format!("package-blake3:{}", "1".repeat(64)),
            audio_blake3: "2".repeat(64),
            canonical_frames: 200,
            canonical_sample_rate: CANONICAL_SAMPLE_RATE,
            channels: 2,
            audio_basis: labels::AudioBasis::CanonicalDecoded,
        }
    }

    fn document(reviewer: &str) -> Document {
        Document {
            schema_version: 1,
            source: source(),
            reviewer: reviewer.into(),
            channel: Channel::Stereo,
            tracks: Tracks::default(),
            metrical_context: None,
        }
    }

    fn context(start_frame: u64, end_frame: u64) -> MetricalContext {
        MetricalContext {
            start_frame,
            end_frame,
            beat_unit: Some(BeatUnit(cocobeat_schema::TempoBeatUnit::DottedQuarter)),
            meter: Some(Meter {
                numerator: 6,
                denominator: 8,
            }),
            provenance: Provenance {
                basis: ReviewBasis::HumanListening,
                note: "Constructed software declaration, not a human reference".into(),
            },
        }
    }

    #[test]
    fn metrical_schema_retains_v1_bytes_and_requires_explicit_v2_context() {
        let mut d = document("reviewer-a");
        let v1 = serde_json::to_value(&d).unwrap();
        assert!(v1.get("metrical_context").is_none());
        let encoded = serde_json::to_vec_pretty(&d).unwrap();
        assert_eq!(
            serde_json::to_vec_pretty(&serde_json::from_slice::<Document>(&encoded).unwrap())
                .unwrap(),
            encoded
        );
        d.metrical_context = Some(Vec::new());
        assert!(d.validate(&source()).is_err());
        d.schema_version = 2;
        d.validate(&source()).unwrap();
        let mut v2 = serde_json::to_value(&d).unwrap();
        v2["metrical_context"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<Document>(v2).is_err());
        let mut missing = v1;
        missing["schema_version"] = 2.into();
        let missing: Document = serde_json::from_value(missing).unwrap();
        assert!(missing.validate(&source()).is_err());
        d.metrical_context = Some(vec![context(0, 200)]);
        let v2 = serde_json::to_value(&d).unwrap();
        assert_eq!(v2["metrical_context"][0]["beat_unit"], "dotted_quarter");
        assert_eq!(serde_json::from_value::<Document>(v2.clone()).unwrap(), d);
        for field in ["beat_unit", "meter"] {
            let mut bad = v2.clone();
            bad["metrical_context"][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(serde_json::from_value::<Document>(bad).is_err());
        }
        for field in ["beat_unit", "meter", "provenance"] {
            let mut extra = v2.clone();
            extra["metrical_context"][0][field] = serde_json::json!({"unexpected": true});
            assert!(serde_json::from_value::<Document>(extra).is_err());
        }
        for field in ["beat_unit", "meter"] {
            let mut unknown = v2.clone();
            unknown["metrical_context"][0][field] = serde_json::Value::Null;
            serde_json::from_value::<Document>(unknown)
                .unwrap()
                .validate(&source())
                .unwrap();
        }
    }

    #[test]
    fn metrical_validation_rejects_bad_extent_provenance_and_resource_limits() {
        let mut d = document("reviewer-a");
        d.schema_version = 2;
        for ranges in [
            vec![context(0, 0)],
            vec![context(0, 201)],
            vec![context(20, 50), context(10, 20)],
            vec![context(0, 50), context(49, 100)],
        ] {
            d.metrical_context = Some(ranges);
            assert!(d.validate(&source()).is_err());
        }
        for meter in [
            Meter {
                numerator: 0,
                denominator: 8,
            },
            Meter {
                numerator: 6,
                denominator: 0,
            },
            Meter {
                numerator: 6,
                denominator: 3,
            },
        ] {
            let mut r = context(0, 200);
            r.meter = Some(meter);
            d.metrical_context = Some(vec![r]);
            assert!(d.validate(&source()).is_err());
        }
        for note in [" ".into(), "x".repeat(2049)] {
            let mut r = context(0, 200);
            r.provenance.note = note;
            d.metrical_context = Some(vec![r]);
            assert!(d.validate(&source()).is_err());
        }
        d.metrical_context = Some(vec![context(0, 100), context(100, 200)]);
        d.validate(&source()).unwrap();
        let mut extended = source();
        extended.canonical_frames = 2048;
        d.source = extended.clone();
        d.tracks.beat.reviewed = (0..MAX_INTERVALS as u64)
            .map(|i| Interval {
                start_frame: i,
                end_frame: i + 1,
            })
            .collect();
        assert!(d.validate(&extended).unwrap_err().contains("1024"));
    }

    #[test]
    fn metrical_comparison_preserves_independent_unknown_and_half_open_boundaries() {
        let mut a = document("reviewer-a");
        let mut b = document("reviewer-b");
        let hash = blake3::hash(b"constructed mechanism only");
        let legacy =
            serde_json::to_value(comparison(&source(), &a, hash, &b, hash).unwrap()).unwrap();
        assert_eq!(legacy["schema_version"], 1);
        assert!(legacy.get("metrical_context").is_none());
        a.schema_version = 2;
        b.schema_version = 2;
        a.metrical_context = Some(vec![context(0, 100), context(120, 200)]);
        let mut first = context(50, 120);
        first.beat_unit = Some(BeatUnit(cocobeat_schema::TempoBeatUnit::Quarter));
        let mut second = context(120, 180);
        second.meter = None;
        b.metrical_context = Some(vec![first, second]);
        let report = comparison(&source(), &a, hash, &b, hash).unwrap();
        assert_eq!(report.schema_version, 2);
        let report = report.metrical_context.unwrap();
        assert_eq!(report.overlaps.len(), 2);
        let one = &report.overlaps[0];
        assert_eq!(
            (
                one.start_frame,
                one.end_frame,
                one.left_index,
                one.right_index
            ),
            (50, 100, 0, 0)
        );
        assert_eq!(one.beat_unit, DeclarationComparison::Different);
        assert_eq!(one.meter, DeclarationComparison::Same);
        let two = &report.overlaps[1];
        assert_eq!(
            (
                two.start_frame,
                two.end_frame,
                two.left_index,
                two.right_index
            ),
            (120, 180, 1, 1)
        );
        assert_eq!(two.beat_unit, DeclarationComparison::Same);
        assert_eq!(two.meter, DeclarationComparison::Unknown);
        assert_eq!(report.left_context, a.metrical_context.clone().unwrap());
        b = document("reviewer-b");
        let cross_version = comparison(&source(), &a, hash, &b, hash)
            .unwrap()
            .metrical_context
            .unwrap();
        assert!(cross_version.right_context.is_empty() && cross_version.overlaps.is_empty());
    }

    #[test]
    fn coverage_and_frame_validation_rejects_invented_or_ambiguous_records() {
        let expected = source();
        let mut d = document("reviewer-a");
        d.validate(&expected).unwrap();
        d.tracks.onset.reviewed = vec![Interval {
            start_frame: 0,
            end_frame: 100,
        }];
        d.tracks.onset.frames = vec![0, 99];
        d.validate(&expected).unwrap();
        for frames in [vec![100], vec![200], vec![99, 0], vec![0, 0]] {
            d.tracks.onset.frames = frames;
            assert!(d.validate(&expected).is_err());
        }
        d.tracks.onset.frames.clear();
        for reviewed in [
            vec![Interval {
                start_frame: 5,
                end_frame: 5,
            }],
            vec![Interval {
                start_frame: 0,
                end_frame: 201,
            }],
            vec![
                Interval {
                    start_frame: 0,
                    end_frame: 100,
                },
                Interval {
                    start_frame: 99,
                    end_frame: 150,
                },
            ],
        ] {
            d.tracks.onset.reviewed = reviewed;
            assert!(d.validate(&expected).is_err());
        }
        d.tracks.onset.reviewed.clear();
        let bytes = serde_json::to_vec(&d).unwrap();
        for bad in ["NaN", "Infinity", "1.0", "-1"] {
            let json = String::from_utf8(bytes.clone()).unwrap().replacen(
                "\"frames\":[]",
                &format!("\"frames\":[{bad}]"),
                1,
            );
            assert!(serde_json::from_str::<Document>(&json).is_err());
        }
        let mut value = serde_json::to_value(&d).unwrap();
        value["confidence"] = serde_json::json!(1.0);
        assert!(serde_json::from_value::<Document>(value).is_err());
        d.tracks.onset.frames = vec![0; MAX_EVENTS + 1];
        assert!(d.validate(&expected).unwrap_err().contains("65536"));
    }

    #[test]
    fn comparison_limits_disagreement_to_mutual_half_open_coverage() {
        let expected = source();
        let mut a = document("reviewer-a");
        let mut b = document("reviewer-b");
        a.tracks.onset = Track {
            reviewed: vec![Interval {
                start_frame: 0,
                end_frame: 100,
            }],
            frames: vec![0, 50, 60, 99],
        };
        b.tracks.onset = Track {
            reviewed: vec![Interval {
                start_frame: 50,
                end_frame: 150,
            }],
            frames: vec![50, 61, 99, 100, 149],
        };
        let hash = blake3::hash(b"constructed mechanism control, not a human review");
        let report = comparison(&expected, &a, hash, &b, hash).unwrap();
        assert_eq!(
            report.onset.mutual_reviewed,
            vec![Interval {
                start_frame: 50,
                end_frame: 100
            }]
        );
        assert_eq!(report.onset.same_frames, [50, 99]);
        assert_eq!(report.onset.left_only_frames, [60]);
        assert_eq!(report.onset.right_only_frames, [61]);
        assert_eq!(report.onset.left_uncompared_frames, [0]);
        assert_eq!(report.onset.right_uncompared_frames, [100, 149]);
        assert!(report.beat.mutual_reviewed.is_empty());
        b.channel = Channel::Left;
        assert!(comparison(&expected, &a, hash, &b, hash).is_err());
        b.channel = Channel::Stereo;
        b.reviewer = a.reviewer.clone();
        assert!(comparison(&expected, &a, hash, &b, hash).is_err());
    }

    #[test]
    fn event_count_and_serialized_output_limits_are_checked_separately() {
        let mut expected = source();
        expected.canonical_frames = MAX_CANONICAL_FRAMES;
        let mut left = Document {
            source: expected.clone(),
            ..document("reviewer-a")
        };
        left.tracks.onset = Track {
            reviewed: vec![Interval {
                start_frame: 0,
                end_frame: MAX_CANONICAL_FRAMES,
            }],
            frames: (MAX_CANONICAL_FRAMES - MAX_EVENTS as u64..MAX_CANONICAL_FRAMES).collect(),
        };
        left.validate(&expected).unwrap();
        let mut right = left.clone();
        right.reviewer = "reviewer-b".into();
        right.tracks.onset.frames = (0..MAX_EVENTS as u64).collect();
        let hash = blake3::hash(b"constructed limit control, not a human review");
        let report = comparison(&expected, &left, hash, &right, hash).unwrap();
        assert_eq!(report.onset.left_only_frames, left.tracks.onset.frames);
        assert_eq!(report.onset.right_only_frames, right.tracks.onset.frames);
        assert!(serde_json::to_vec_pretty(&report).unwrap().len() < MAX_REPORT_BYTES);
        // Pretty serialization may exceed the byte ceiling even when count is legal
        assert!(serde_json::to_vec_pretty(&left).unwrap().len() + 1 > MAX_BYTES);
        let (root, _) = crate::anchors::tests::fixture("music-truth-limit");
        let output = root.join("count-legal-byte-limit-rejected.json");
        assert!(labels::write_new(&output, &left, MAX_BYTES).is_err());
        assert!(!output.exists());
        let report_output = root.join("full-report.json");
        labels::write_new(&report_output, &report, MAX_REPORT_BYTES).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&report_output).unwrap()).unwrap();
        assert_eq!(
            value["onset"]["left_only_frames"].as_array().unwrap().len(),
            MAX_EVENTS
        );
        assert_eq!(
            value["onset"]["right_only_frames"]
                .as_array()
                .unwrap()
                .len(),
            MAX_EVENTS
        );
        left.tracks.beat.frames.push(0);
        assert!(left.validate(&expected).unwrap_err().contains("65536"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn raw_input_identity_and_same_audio_cross_chart_import_preserve_sources() {
        let (root, package) = crate::anchors::tests::fixture("music-truth");
        let package_path = root.join("source");
        let initial = labels::Source::from_package(&package);
        let source = Source::from_snapshot(&initial);
        let d = Document {
            source: source.clone(),
            ..document("reviewer-a")
        };
        let input = root.join("manual.json");
        let mut raw = serde_json::to_vec(&d).unwrap();
        raw.extend_from_slice(b" \n");
        fs::write(&input, &raw).unwrap();
        let (loaded, raw_hash) = load(&input, &source).unwrap();
        assert_eq!(loaded, d);
        assert_eq!(raw_hash, blake3::hash(&raw));
        let revised_path = root.join("revised");
        let revised = cocobeat_media::export_anchors(
            &package_path,
            package.manifest.package_hash,
            &[],
            &revised_path,
        )
        .unwrap();
        assert_ne!(revised.manifest.package_hash, package.manifest.package_hash);
        let names = cocobeat_media::PACKAGE_OBJECT_NAMES;
        let original = names.map(|name| fs::read(package_path.join(name)).unwrap());
        let revised_original = names.map(|name| fs::read(revised_path.join(name)).unwrap());
        let output = root.join("imported.json");
        import(&revised_path, &input, &output).unwrap();
        let output_before = fs::read(&output).unwrap();
        let (imported, _) = load(
            &output,
            &Source::from_snapshot(&labels::Source::from_package(&revised)),
        )
        .unwrap();
        assert_eq!(imported, d);
        assert!(import(&revised_path, &input, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), output_before);
        let metrical_template = root.join("metrical-template.json");
        template_metrical(
            &revised_path,
            "reviewer-metrical",
            "left",
            &metrical_template,
        )
        .unwrap();
        let template_bytes = fs::read(&metrical_template).unwrap();
        let (template_document, _) = load(
            &metrical_template,
            &Source::from_snapshot(&labels::Source::from_package(&revised)),
        )
        .unwrap();
        assert_eq!(template_document.schema_version, 2);
        assert_eq!(template_document.metrical_context, Some(Vec::new()));
        assert!(
            template_metrical(
                &revised_path,
                "reviewer-metrical",
                "left",
                &metrical_template
            )
            .is_err()
        );
        assert_eq!(fs::read(&metrical_template).unwrap(), template_bytes);
        assert_eq!(fs::read(&input).unwrap(), raw);
        assert_eq!(
            names.map(|name| fs::read(package_path.join(name)).unwrap()),
            original
        );
        assert_eq!(
            names.map(|name| fs::read(revised_path.join(name)).unwrap()),
            revised_original
        );
        assert!(labels_cli::require_fresh_source(&revised_path, &initial).is_err());
        let mut changed = source.clone();
        changed.audio_blake3 = "3".repeat(64);
        assert!(load(&input, &changed).is_err());
        let too_large = root.join("too-large.json");
        fs::write(&too_large, vec![b' '; MAX_BYTES + 1]).unwrap();
        assert!(load(&too_large, &source).unwrap_err().contains("1 MiB"));
        let too_small_output = root.join("size-rejected.json");
        assert!(labels::write_new(&too_small_output, &d, 1).is_err());
        assert!(!too_small_output.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
