//! Mechanical candidate frame comparisons inside explicitly reviewed MusicTruth coverage

use crate::{labels, labels_cli, music_truth};
use cocobeat_media::{
    NativeBeatEvidence, NativeBeatKind, NativeBeatMetadata, NativeDownbeatAlignment,
};
use music_truth::{Channel, Interval, Track};
use serde::Serialize;
use std::path::Path;

const MAX_REPORT_BYTES: usize = 4 * 1_048_576;

#[derive(Clone, Copy, Debug, Serialize)]
struct Event {
    record_index: usize,
    frame: u64,
}

#[derive(Debug, Serialize)]
struct Match {
    record_index: usize,
    candidate_frame: u64,
    truth_frame: u64,
    signed_error_frames: i64,
}

#[derive(Debug, Serialize)]
struct StreamComparison {
    status: &'static str,
    reviewed: Vec<Interval>,
    matching_components: Vec<Interval>,
    coverage_frames: u64,
    candidate_total: usize,
    candidate_inside_coverage: usize,
    candidate_outside_coverage: usize,
    truth_inside_coverage: usize,
    matched_count: usize,
    unmatched_truth_count: usize,
    unmatched_candidate_inside_count: usize,
    matches: Vec<Match>,
    unmatched_truth_frames: Vec<u64>,
    unmatched_candidate_inside: Vec<Event>,
    candidate_outside: Vec<Event>,
    median_absolute_error_frames: Option<f64>,
    p95_absolute_error_frames: Option<u64>,
    max_absolute_error_frames: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Record {
    kind: NativeBeatKind,
    group_index: usize,
    original_q: f64,
    frame: i64,
    uncalibrated_score: f32,
    alignment: Option<NativeDownbeatAlignment>,
    package_downbeat_score: Option<f32>,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u32,
    validated_content_id: String,
    validated_audio: labels::Source,
    truth_declared_source: music_truth::Source,
    truth_reviewer: String,
    channel: Channel,
    truth_raw_blake3: String,
    native: NativeBeatMetadata,
    tolerance_frames: u64,
    matching_policy: &'static str,
    candidate_state: &'static str,
    confidence: Option<f32>,
    beat_unit: Option<&'static str>,
    meter: Option<&'static str>,
    reference_semantics: &'static str,
    quality_status: &'static str,
    production_admission: bool,
    old_frontend_numeric: &'static str,
    old_music_quality: &'static str,
    onset: &'static str,
    // Stream references index these records; original alignment is never replaced by truth
    records: Vec<Record>,
    raw_beat: StreamComparison,
    raw_downbeat: StreamComparison,
    package_aligned: StreamComparison,
}

fn components(reviewed: &[Interval]) -> Vec<Interval> {
    let mut result: Vec<Interval> = Vec::new();
    for &interval in reviewed {
        if let Some(last) = result.last_mut()
            && last.end_frame == interval.start_frame
        {
            last.end_frame = interval.end_frame;
        } else {
            result.push(interval);
        }
    }
    result
}

fn compare_track(events: &[Event], track: &Track, tolerance: u64) -> StreamComparison {
    let matching_components = components(&track.reviewed);
    let (mut matches, mut unmatched_truth, mut unmatched_inside, mut outside) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut next = 0;
    for interval in &matching_components {
        let start = events.partition_point(|event| event.frame < interval.start_frame);
        let end = events.partition_point(|event| event.frame < interval.end_frame);
        outside.extend_from_slice(&events[next..start]);
        let candidates = &events[start..end];
        let a = track
            .frames
            .partition_point(|&frame| frame < interval.start_frame);
        let b = track
            .frames
            .partition_point(|&frame| frame < interval.end_frame);
        let truth = &track.frames[a..b];
        let (mut c, mut t) = (0, 0);
        // Earliest feasible pairs maximize cardinality, not minimum error; never cross a gap
        while c < candidates.len() && t < truth.len() {
            let candidate = candidates[c];
            if candidate.frame.saturating_add(tolerance) < truth[t] {
                unmatched_inside.push(candidate);
                c += 1;
            } else if truth[t].saturating_add(tolerance) < candidate.frame {
                unmatched_truth.push(truth[t]);
                t += 1;
            } else {
                matches.push(Match {
                    record_index: candidate.record_index,
                    candidate_frame: candidate.frame,
                    truth_frame: truth[t],
                    signed_error_frames: candidate.frame as i64 - truth[t] as i64,
                });
                c += 1;
                t += 1;
            }
        }
        unmatched_inside.extend_from_slice(&candidates[c..]);
        unmatched_truth.extend_from_slice(&truth[t..]);
        next = end;
    }
    outside.extend_from_slice(&events[next..]);
    let mut absolute: Vec<_> = matches
        .iter()
        .map(|pair| pair.signed_error_frames.unsigned_abs())
        .collect();
    absolute.sort_unstable();
    let n = absolute.len();
    StreamComparison {
        status: if matching_components.is_empty() {
            "NO_COMPARABLE_COVERAGE"
        } else {
            "MECHANICAL_FRAME_COMPARISON"
        },
        reviewed: track.reviewed.clone(),
        coverage_frames: matching_components
            .iter()
            .map(|r| r.end_frame - r.start_frame)
            .sum(),
        matching_components,
        candidate_total: events.len(),
        candidate_inside_coverage: matches.len() + unmatched_inside.len(),
        candidate_outside_coverage: outside.len(),
        truth_inside_coverage: track.frames.len(),
        matched_count: matches.len(),
        unmatched_truth_count: unmatched_truth.len(),
        unmatched_candidate_inside_count: unmatched_inside.len(),
        median_absolute_error_frames: (n > 0)
            .then(|| (absolute[(n - 1) / 2] + absolute[n / 2]) as f64 / 2.0),
        p95_absolute_error_frames: (n > 0).then(|| absolute[(95 * n).div_ceil(100) - 1]),
        max_absolute_error_frames: absolute.last().copied(),
        matches,
        unmatched_truth_frames: unmatched_truth,
        unmatched_candidate_inside: unmatched_inside,
        candidate_outside: outside,
    }
}

fn comparison(
    source: &labels::Source,
    native: &NativeBeatEvidence,
    truth: &music_truth::Document,
    truth_hash: blake3::Hash,
    tolerance: u64,
) -> Result<Report, String> {
    if tolerance > source.canonical_frames {
        return Err("Native comparison tolerance must be 0..=canonical_frames".into());
    }
    if !matches!(
        (native.metadata.channel, truth.channel),
        (0, Channel::Left) | (1, Channel::Right)
    ) {
        return Err("Native comparison requires MusicTruth left for channel 0 or right for channel 1; stereo is not implicitly downmixed".into());
    }
    let (mut beats, mut downbeats, mut aligned) = (Vec::new(), Vec::new(), Vec::new());
    let mut records = Vec::with_capacity(native.records.len());
    for (record_index, record) in native.records.iter().enumerate() {
        let event = Event {
            record_index,
            frame: u64::try_from(record.frame).map_err(|_| "Negative native candidate frame")?,
        };
        match record.kind {
            NativeBeatKind::Beat => {
                beats.push(event);
                if record.package_downbeat_score.is_some() {
                    aligned.push(event);
                }
            }
            NativeBeatKind::RawDownbeat => downbeats.push(event),
        }
        records.push(Record {
            kind: record.kind,
            group_index: record.group_index,
            original_q: record.original_q,
            frame: record.frame,
            uncalibrated_score: record.uncalibrated_score,
            alignment: record.alignment.clone(),
            package_downbeat_score: record.package_downbeat_score,
        });
    }
    Ok(Report {
        schema_version: 1,
        validated_content_id: source.content_id.clone(),
        validated_audio: source.clone(),
        truth_declared_source: truth.source.clone(),
        truth_reviewer: truth.reviewer.clone(),
        channel: truth.channel,
        truth_raw_blake3: truth_hash.to_string(),
        native: native.metadata.clone(),
        tolerance_frames: tolerance,
        matching_policy: "earliest_feasible_one_to_one_in_connected_reviewed_components_inclusive_tolerance",
        candidate_state: "Candidate",
        confidence: None,
        beat_unit: None,
        meter: None,
        reference_semantics: "unrecorded_in_music_truth_v1",
        quality_status: "UNASSESSED",
        production_admission: false,
        old_frontend_numeric: "FAIL_PRESERVED_19_OF_28",
        old_music_quality: "FAIL_PRESERVED",
        onset: "NO_CANDIDATE_STREAM",
        records,
        raw_beat: compare_track(&beats, &truth.tracks.beat, tolerance),
        raw_downbeat: compare_track(&downbeats, &truth.tracks.downbeat, tolerance),
        package_aligned: compare_track(&aligned, &truth.tracks.downbeat, tolerance),
    })
}

pub(crate) fn compare_native_beats(
    package: &Path,
    evidence: &Path,
    truth: &Path,
    tolerance_frames: u64,
    destination: &Path,
) -> Result<(), String> {
    let validated = cocobeat_media::validate_package(package)?;
    let initial = labels::Source::from_package(&validated);
    let native = cocobeat_media::read_native_beat_evidence(&validated, evidence)?;
    let (truth, raw_hash) =
        music_truth::load(truth, &music_truth::Source::from_snapshot(&initial))?;
    let report = comparison(&initial, &native, &truth, raw_hash, tolerance_frames)?;
    let destination = labels_cli::outside_package(package, destination)?;
    labels_cli::require_fresh_source(package, &initial)?;
    let written = labels::write_new(&destination, &report, MAX_REPORT_BYTES)?;
    println!(
        "{}",
        serde_json::json!({
            "validated_content_id": initial.content_id,
            "declared_origin_content_id": truth.source.origin_content_id,
            "truth_raw_blake3": raw_hash.to_string(),
            "native_summary_blake3": native.metadata.summary_blake3,
            "tolerance_frames": tolerance_frames,
            "saved_blake3": written.to_string(), "destination": destination,
            "scope": "mechanical_frame_comparison_only", "quality_status": "UNASSESSED",
            "production_admission": false,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_media::NativeBeatRecord;
    use std::fs;

    fn track(intervals: &[(u64, u64)], frames: &[u64]) -> Track {
        Track {
            reviewed: intervals
                .iter()
                .map(|&(start_frame, end_frame)| Interval {
                    start_frame,
                    end_frame,
                })
                .collect(),
            frames: frames.to_vec(),
        }
    }

    fn events(frames: &[u64]) -> Vec<Event> {
        frames
            .iter()
            .enumerate()
            .map(|(record_index, &frame)| Event {
                record_index,
                frame,
            })
            .collect()
    }

    fn conserved(result: &StreamComparison) {
        assert_eq!(
            result.candidate_total,
            result.candidate_inside_coverage + result.candidate_outside_coverage
        );
        assert_eq!(
            result.candidate_inside_coverage,
            result.matched_count + result.unmatched_candidate_inside_count
        );
        assert_eq!(
            result.truth_inside_coverage,
            result.matched_count + result.unmatched_truth_count
        );
    }

    #[test]
    fn earliest_pairs_tolerance_edges_and_distinct_candidate_identities() {
        let truth = track(&[(0, 10)], &[0, 4]);
        let result = compare_track(&events(&[3, 5]), &truth, 3);
        assert_eq!(result.matched_count, 2);
        assert_eq!(
            result
                .matches
                .iter()
                .map(|p| (p.candidate_frame, p.truth_frame))
                .collect::<Vec<_>>(),
            [(3, 0), (5, 4)]
        );
        conserved(&result);
        assert_eq!(
            compare_track(&events(&[3]), &track(&[(0, 10)], &[0]), 2).matched_count,
            0
        );
        assert_eq!(
            compare_track(&events(&[0]), &track(&[(0, 10)], &[0]), 0).matched_count,
            1
        );
        // Constructed duplicate predictions stay two identities; one truth is used only once
        let duplicate = compare_track(&events(&[4, 4]), &track(&[(0, 10)], &[4]), 0);
        assert_eq!(duplicate.matched_count, 1);
        assert_eq!(duplicate.matches[0].record_index, 0);
        assert_eq!(duplicate.unmatched_candidate_inside[0].record_index, 1);
        conserved(&duplicate);
    }

    #[test]
    fn half_open_ends_gaps_and_adjacent_components() {
        let truth = track(&[(0, 10), (12, 20)], &[9, 12]);
        let result = compare_track(&events(&[0, 10, 11, 12, 19, 20]), &truth, 0);
        assert_eq!(result.matches[0].truth_frame, 12);
        assert_eq!(
            result
                .candidate_outside
                .iter()
                .map(|p| p.frame)
                .collect::<Vec<_>>(),
            [10, 11, 20]
        );
        assert_eq!(result.coverage_frames, 18);
        conserved(&result);
        let gap = compare_track(&events(&[12]), &track(&[(0, 10), (12, 20)], &[9]), 100);
        assert_eq!(gap.matched_count, 0);
        assert_eq!(gap.unmatched_truth_frames, [9]);
        let adjacent = compare_track(&events(&[10]), &track(&[(0, 10), (10, 20)], &[9]), 1);
        assert_eq!(adjacent.matched_count, 1);
        assert_eq!(adjacent.reviewed.len(), 2);
        assert_eq!(adjacent.matching_components.len(), 1);
    }

    #[test]
    fn unreviewed_reviewed_zero_and_empty_error_statistics_are_distinct() {
        let unreviewed = compare_track(&events(&[0]), &track(&[], &[]), 0);
        assert_eq!(unreviewed.status, "NO_COMPARABLE_COVERAGE");
        assert_eq!(unreviewed.candidate_outside_coverage, 1);
        assert_eq!(unreviewed.unmatched_candidate_inside_count, 0);
        let reviewed = compare_track(&events(&[0]), &track(&[(0, 10)], &[]), 0);
        assert_eq!(reviewed.status, "MECHANICAL_FRAME_COMPARISON");
        assert_eq!(reviewed.unmatched_candidate_inside_count, 1);
        let empty = compare_track(&[], &track(&[(0, 10)], &[]), 0);
        assert_eq!(empty.status, "MECHANICAL_FRAME_COMPARISON");
        assert!(empty.median_absolute_error_frames.is_none());
        assert!(empty.p95_absolute_error_frames.is_none());
        assert!(empty.max_absolute_error_frames.is_none());
        for result in [&unreviewed, &reviewed, &empty] {
            conserved(result);
        }
        let truth = track(&[(0, 2200)], &(0..20).map(|i| i * 100).collect::<Vec<_>>());
        let result = compare_track(
            &events(&(0..20).map(|i| i * 101).collect::<Vec<_>>()),
            &truth,
            19,
        );
        assert_eq!(result.matched_count, 20);
        assert_eq!(result.median_absolute_error_frames, Some(9.5));
        assert_eq!(result.p95_absolute_error_frames, Some(18));
        assert_eq!(result.max_absolute_error_frames, Some(19));
    }

    fn native(source: &labels::Source) -> NativeBeatEvidence {
        let alignment = Some(NativeDownbeatAlignment {
            beat_group_index: 0,
            beat_q: 0.125,
            beat_frame: 120,
        });
        let record = |kind, group_index, frame| NativeBeatRecord {
            kind,
            group_index,
            original_q: frame as f64 / 960.0,
            frame,
            uncalibrated_score: 0.75,
            members: Vec::new(),
            alignment: if kind == NativeBeatKind::RawDownbeat {
                alignment.clone()
            } else {
                None
            },
            package_downbeat_score: Some(0.9),
        };
        NativeBeatEvidence {
            metadata: NativeBeatMetadata {
                profile: "constructed-software-oracle-not-model-output".into(),
                channel: 0,
                canonical_frames: source.canonical_frames,
                resampled_frames: 2205,
                spectrogram_frames: 6,
                audio_blake3: source.audio_blake3.clone(),
                model_blake3: "2".repeat(64),
                summary_blake3: "3".repeat(64),
            },
            records: vec![
                record(NativeBeatKind::RawDownbeat, 0, 100),
                record(NativeBeatKind::RawDownbeat, 1, 110),
                record(NativeBeatKind::Beat, 0, 120),
            ],
        }
    }

    fn manual(source: &labels::Source) -> serde_json::Value {
        serde_json::json!({"schema_version":1,"source":music_truth::Source::from_snapshot(source),"reviewer":"constructed-oracle","channel":"left",
            "tracks":{"onset":{"reviewed":[],"frames":[]},"beat":{"reviewed":[{"start_frame":0,"end_frame":200}],"frames":[120]},
            "downbeat":{"reviewed":[{"start_frame":0,"end_frame":200}],"frames":[100]}}})
    }

    #[test]
    fn raw_alignment_package_once_and_unknown_semantics_remain_separate() {
        let (root, package) = crate::anchors::tests::fixture("native-beat-compare-raw");
        let source = labels::Source::from_package(&package);
        let truth = serde_json::from_value(manual(&source)).unwrap();
        let native = native(&source);
        let report = comparison(&source, &native, &truth, blake3::hash(b"constructed"), 0).unwrap();
        assert_eq!(report.raw_downbeat.candidate_total, 2);
        assert_eq!(report.package_aligned.candidate_total, 1);
        assert_eq!(report.raw_downbeat.matched_count, 1);
        assert_eq!(report.package_aligned.matched_count, 0);
        assert_eq!(report.raw_beat.matched_count, 1);
        assert_eq!(report.records[0].frame, 100);
        assert_eq!(
            report.records[0].alignment.as_ref().unwrap().beat_frame,
            120
        );
        assert_eq!(report.records[0].original_q, native.records[0].original_q);
        for result in [
            &report.raw_beat,
            &report.raw_downbeat,
            &report.package_aligned,
        ] {
            conserved(result);
        }
        let value = serde_json::to_value(&report).unwrap();
        for key in ["confidence", "beat_unit", "meter"] {
            assert!(value[key].is_null());
        }
        assert_eq!(value["quality_status"], "UNASSESSED");
        assert_eq!(value["production_admission"], false);
        assert_eq!(value["onset"], "NO_CANDIDATE_STREAM");
        assert!(
            comparison(
                &source,
                &native,
                &truth,
                blake3::hash(b"constructed"),
                source.canonical_frames + 1
            )
            .is_err()
        );
        assert!(
            comparison(
                &source,
                &native,
                &truth,
                blake3::hash(b"constructed"),
                u64::MAX
            )
            .is_err()
        );
        let mut cross_kind = native.clone();
        cross_kind.records[2].frame = 100;
        cross_kind.records[2].original_q = cross_kind.records[0].original_q;
        let mut value = manual(&source);
        value["tracks"]["beat"]["frames"] = serde_json::json!([100]);
        let truth = serde_json::from_value(value).unwrap();
        let report = comparison(
            &source,
            &cross_kind,
            &truth,
            blake3::hash(b"constructed"),
            0,
        )
        .unwrap();
        assert_eq!(report.raw_beat.matched_count, 1);
        assert_eq!(report.raw_downbeat.matched_count, 1);
        assert_ne!(
            report.raw_beat.matches[0].record_index,
            report.raw_downbeat.matches[0].record_index
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn truth_source_channel_and_duplicate_validation_use_the_existing_loader() {
        let (root, package) = crate::anchors::tests::fixture("native-beat-compare-source");
        let source = labels::Source::from_package(&package);
        let expected = music_truth::Source::from_snapshot(&source);
        let input = root.join("truth.json");
        let initial = manual(&source);
        fs::write(&input, initial.to_string()).unwrap();
        let (truth, hash) = music_truth::load(&input, &expected).unwrap();
        let native = native(&source);
        comparison(&source, &native, &truth, hash, 0).unwrap();
        for (key, bad) in [
            ("audio_blake3", serde_json::json!("4".repeat(64))),
            (
                "canonical_frames",
                serde_json::json!(source.canonical_frames + 1),
            ),
            ("canonical_sample_rate", serde_json::json!(44100)),
            ("channels", serde_json::json!(1)),
        ] {
            let mut value = initial.clone();
            value["source"][key] = bad;
            fs::write(&input, value.to_string()).unwrap();
            assert!(music_truth::load(&input, &expected).is_err(), "{key}");
        }
        let mut duplicate = initial.clone();
        duplicate["tracks"]["beat"]["frames"] = serde_json::json!([120, 120]);
        fs::write(&input, duplicate.to_string()).unwrap();
        assert!(music_truth::load(&input, &expected).is_err());
        for channel in ["stereo", "right"] {
            let mut value = initial.clone();
            value["channel"] = serde_json::json!(channel);
            fs::write(&input, value.to_string()).unwrap();
            let (truth, hash) = music_truth::load(&input, &expected).unwrap();
            assert!(comparison(&source, &native, &truth, hash, 0).is_err());
            if channel == "right" {
                let mut right = native.clone();
                right.metadata.channel = 1;
                comparison(&source, &right, &truth, hash, 0).unwrap();
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_audio_cross_chart_keeps_historical_origin_and_current_content_id() {
        let (root, package) = crate::anchors::tests::fixture("native-beat-compare-cid");
        let original = labels::Source::from_package(&package);
        let input = root.join("truth.json");
        let raw = manual(&original).to_string();
        fs::write(&input, &raw).unwrap();
        let revised_path = root.join("revised");
        let revised = cocobeat_media::export_anchors(
            root.join("source"),
            package.manifest.package_hash,
            &[],
            &revised_path,
        )
        .unwrap();
        let current = labels::Source::from_package(&revised);
        assert_ne!(original.content_id, current.content_id);
        let (truth, hash) =
            music_truth::load(&input, &music_truth::Source::from_snapshot(&current)).unwrap();
        let report = comparison(&current, &native(&current), &truth, hash, 0).unwrap();
        assert_eq!(report.validated_content_id, current.content_id);
        assert_eq!(
            report.truth_declared_source.origin_content_id,
            original.content_id
        );
        assert_eq!(
            report.truth_raw_blake3,
            blake3::hash(raw.as_bytes()).to_string()
        );
        assert!(labels_cli::require_fresh_source(&revised_path, &original).is_err());
        assert!(
            labels_cli::outside_package(&revised_path, &revised_path.join("report.json")).is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_report_protects_existing_output_and_rejects_oversize_without_truncation() {
        let (root, package) = crate::anchors::tests::fixture("native-beat-compare-output");
        let source = labels::Source::from_package(&package);
        let truth = serde_json::from_value(manual(&source)).unwrap();
        let report = comparison(
            &source,
            &native(&source),
            &truth,
            blake3::hash(b"constructed"),
            0,
        )
        .unwrap();
        let output = root.join("report.json");
        fs::write(&output, b"owned sentinel").unwrap();
        assert!(labels::write_new(&output, &report, MAX_REPORT_BYTES).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"owned sentinel");
        let huge = root.join("oversize.json");
        // Exercise the actual fixed report ceiling through the shared writer, not a truncated report
        let oversized = "x".repeat(MAX_REPORT_BYTES);
        assert!(labels::write_new(&huge, &oversized, MAX_REPORT_BYTES).is_err());
        assert!(!huge.exists());
        let exact = root.join("exact-limit.json");
        let small = serde_json::json!({"mechanical":true});
        let bytes = serde_json::to_vec_pretty(&small).unwrap().len() + 1;
        labels::write_new(&exact, &small, bytes).unwrap();
        assert_eq!(fs::metadata(&exact).unwrap().len(), bytes as u64);
        fs::remove_dir_all(root).unwrap();
    }
}
