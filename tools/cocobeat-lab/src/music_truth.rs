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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    schema_version: u32,
    pub(crate) source: Source,
    pub(crate) reviewer: String,
    pub(crate) channel: Channel,
    pub(crate) tracks: Tracks,
}

impl Document {
    fn validate(&self, expected: &Source) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported music truth schema".into());
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
    let channel = match channel {
        "stereo" => Channel::Stereo,
        "left" => Channel::Left,
        "right" => Channel::Right,
        _ => return Err("Music truth channel must be stereo, left or right".into()),
    };
    let initial = labels::Source::from_package(&cocobeat_media::validate_package(package)?);
    let source = Source::from_snapshot(&initial);
    let document = Document {
        schema_version: 1,
        source: source.clone(),
        reviewer: reviewer.into(),
        channel,
        tracks: Tracks::default(),
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
            "scope": "empty_manual_template_only",
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
    Ok(Comparison {
        schema_version: 1,
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
        }
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
