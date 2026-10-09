//! Mechanical candidate frame comparisons inside explicitly reviewed MusicTruth coverage

use crate::{labels, labels_cli, music_truth};
use cocobeat_media::{
    NativeBeatEvidence, NativeBeatKind, NativeBeatMetadata, NativeDownbeatAlignment,
};
use cocobeat_schema::{AnalysisSource, AnalysisState, MusicAnalysis};
use music_truth::{Channel, Interval, Track};
use serde::Serialize;
use std::path::Path;

const MAX_REPORT_BYTES: usize = 4 * 1_048_576;
const AUTO_ANALYSIS_PROFILE: &str = "native-small0-hfc1024-interbeat-v1-candidate";

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
#[serde(untagged)]
enum Onset {
    Absent(&'static str),
    Stream(Box<OnsetComparison>),
}

#[derive(Debug, Serialize)]
struct OnsetRecord {
    frame: i64,
    strength: f32,
    confidence: Option<f32>,
}

#[derive(Debug, Serialize)]
struct OnsetComparison {
    candidate_state: &'static str,
    unsupported_reason: Option<&'static str>,
    coordinate_policy: &'static str,
    strength_policy: &'static str,
    // Only onset comparisons index these records, never the native beat records
    records: Vec<OnsetRecord>,
    comparison: Option<StreamComparison>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    reference_metrical_context: Option<Vec<music_truth::MetricalContext>>,
    quality_status: &'static str,
    production_admission: bool,
    old_frontend_numeric: &'static str,
    old_music_quality: &'static str,
    onset: Onset,
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
        schema_version: if truth.metrical_context.is_some() {
            3
        } else {
            1
        },
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
        reference_semantics: if truth.metrical_context.is_some() {
            "manual_declarations_in_music_truth_v2_not_musical_admission"
        } else {
            "unrecorded_in_music_truth_v1"
        },
        reference_metrical_context: truth.metrical_context.clone(),
        quality_status: "UNASSESSED",
        production_admission: false,
        old_frontend_numeric: "FAIL_PRESERVED_19_OF_28",
        old_music_quality: "FAIL_PRESERVED",
        onset: Onset::Absent("NO_CANDIDATE_STREAM"),
        records,
        raw_beat: compare_track(&beats, &truth.tracks.beat, tolerance),
        raw_downbeat: compare_track(&downbeats, &truth.tracks.downbeat, tolerance),
        package_aligned: compare_track(&aligned, &truth.tracks.downbeat, tolerance),
    })
}

// Called only after the package and its fixed-profile native resources have been verified
fn include_onsets(
    report: &mut Report,
    analysis: &MusicAnalysis,
    truth: &music_truth::Document,
) -> Result<(), String> {
    if report.native.profile != AUTO_ANALYSIS_PROFILE {
        return Ok(());
    }
    let capability = analysis
        .capabilities
        .ok_or("Automatic onset comparison requires v2 capabilities")?
        .onset;
    if capability.source != AnalysisSource::Algorithm
        || capability.confidence.is_some()
        || !matches!(
            capability.state,
            AnalysisState::Candidate | AnalysisState::Unsupported
        )
        || (capability.state == AnalysisState::Unsupported && !analysis.onsets.is_empty())
    {
        return Err("Automatic onset comparison requires Candidate or Unsupported/Algorithm with unknown confidence".into());
    }
    let events = analysis
        .onsets
        .iter()
        .enumerate()
        .map(|(record_index, onset)| {
            Ok(Event {
                record_index,
                frame: u64::try_from(onset.time.frames())
                    .map_err(|_| "Negative automatic onset candidate frame")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let supported = capability.state == AnalysisState::Candidate;
    report.onset = Onset::Stream(Box::new(OnsetComparison {
        candidate_state: if supported {
            "Candidate"
        } else {
            "Unsupported"
        },
        unsupported_reason: (!supported).then_some("insufficient_analysis_frames"),
        coordinate_policy: "canonical_left_window_start_1024_frames_hop_512_no_alignment",
        strength_policy: "original_hfc_normalized_strength_not_confidence",
        records: analysis
            .onsets
            .iter()
            .map(|onset| OnsetRecord {
                frame: onset.time.frames(),
                strength: onset.strength,
                confidence: onset.confidence,
            })
            .collect(),
        comparison: supported
            .then(|| compare_track(&events, &truth.tracks.onset, report.tolerance_frames)),
    }));
    report.schema_version = report.schema_version.max(2);
    Ok(())
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
    let mut report = comparison(&initial, &native, &truth, raw_hash, tolerance_frames)?;
    include_onsets(&mut report, &validated.analysis, &truth)?;
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
    fn metrical_reference_does_not_relabel_candidate_semantics_or_change_frame_matching() {
        let (root, package) = crate::anchors::tests::fixture("metrical-native-compare");
        let source = labels::Source::from_package(&package);
        let candidate = native(&source);
        let old: music_truth::Document = serde_json::from_value(manual(&source)).unwrap();
        let hash = blake3::hash(b"constructed mechanism only");
        let legacy = comparison(&source, &candidate, &old, hash, 0).unwrap();
        let legacy = serde_json::to_value(legacy).unwrap();
        assert!(legacy.get("reference_metrical_context").is_none());
        let mut value = manual(&source);
        value["schema_version"] = 2.into();
        value["metrical_context"] = serde_json::json!([{
            "start_frame": 0, "end_frame": source.canonical_frames,
            "beat_unit": "dotted_quarter", "meter": null,
            "provenance": { "basis": "human_listening", "note": "Constructed declaration, not a human reference" },
        }]);
        let truth: music_truth::Document = serde_json::from_value(value).unwrap();
        let mut report = comparison(&source, &candidate, &truth, hash, 0).unwrap();
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["schema_version"], 3);
        assert_eq!(
            value["reference_metrical_context"][0]["beat_unit"],
            "dotted_quarter"
        );
        for field in [
            "raw_beat",
            "raw_downbeat",
            "package_aligned",
            "records",
            "tolerance_frames",
            "matching_policy",
        ] {
            assert_eq!(value[field], legacy[field]);
        }
        for field in ["beat_unit", "meter", "confidence"] {
            assert!(value[field].is_null());
        }
        assert_eq!(value["quality_status"], "UNASSESSED");
        assert_eq!(value["production_admission"], false);
        report.native.profile = AUTO_ANALYSIS_PROFILE.into();
        include_onsets(&mut report, &onset_analysis(&package, &[512]), &truth).unwrap();
        assert_eq!(report.schema_version, 3);
        std::fs::remove_dir_all(root).unwrap();
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

    fn onset_analysis(package: &cocobeat_media::ValidatedPackage, frames: &[i64]) -> MusicAnalysis {
        let mut analysis = package.analysis.clone();
        analysis.schema_version = 2;
        analysis.presentation = None;
        let mut capabilities = cocobeat_schema::AnalysisCapabilities::authored();
        capabilities.onset = cocobeat_schema::AnalysisCapability {
            state: AnalysisState::Candidate,
            source: AnalysisSource::Algorithm,
            confidence: None,
        };
        analysis.capabilities = Some(capabilities);
        analysis.onsets = frames
            .iter()
            .enumerate()
            .map(|(index, &frame)| cocobeat_schema::OnsetFeature {
                time: cocobeat_schema::SongTime::from_frames(frame),
                strength: (index + 1) as f32 / (frames.len() + 1) as f32,
                confidence: None,
            })
            .collect();
        analysis
    }

    #[test]
    fn automatic_onsets_keep_original_records_coverage_errors_and_kind_indices() {
        let (root, package) = crate::anchors::tests::fixture("automatic-onset-compare");
        let source = labels::Source::from_package(&package);
        let mut native = native(&source);
        native.metadata.profile = AUTO_ANALYSIS_PROFILE.into();
        let mut truth: music_truth::Document = serde_json::from_value(manual(&source)).unwrap();
        // Independently constructed matching oracle, not copied candidate output or music truth
        truth.tracks.onset = track(&[(0, 1300), (2000, 3500)], &[500, 1100, 2100, 2300, 3400]);
        let analysis = onset_analysis(&package, &[512, 1024, 1536, 2048, 2560, 3072]);
        let mut report = comparison(&source, &native, &truth, blake3::hash(b"oracle"), 64).unwrap();
        include_onsets(&mut report, &analysis, &truth).unwrap();
        assert_eq!(report.schema_version, 2);
        let Onset::Stream(onset) = &report.onset else {
            panic!("Missing onset stream")
        };
        assert_eq!(onset.candidate_state, "Candidate");
        let result = onset.comparison.as_ref().unwrap();
        conserved(result);
        assert_eq!(result.candidate_total, 6);
        assert_eq!(result.matched_count, 2);
        assert_eq!(result.unmatched_candidate_inside_count, 3);
        assert_eq!(result.unmatched_truth_count, 3);
        assert_eq!(result.candidate_outside[0].record_index, 2);
        assert_eq!(result.matches[0].record_index, 0);
        assert_eq!(result.matches[0].signed_error_frames, 12);
        assert_eq!(result.matches[1].record_index, 3);
        assert_eq!(result.matches[1].signed_error_frames, -52);
        for (record, original) in onset.records.iter().zip(&analysis.onsets) {
            assert_eq!(record.frame, original.time.frames());
            assert_eq!(record.strength.to_bits(), original.strength.to_bits());
            assert_eq!(record.confidence, original.confidence);
        }
        assert_eq!(report.raw_beat.matches[0].record_index, 2);
        assert_eq!(report.records[2].frame, 120);
        assert_eq!(onset.records[2].frame, 1536);
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["onset"]["records"][3]["frame"], 2048);
        assert!(value["onset"]["records"][3]["confidence"].is_null());
        assert_eq!(value["production_admission"], false);
        // Even a wide tolerance cannot match across disconnected reviewed components
        truth.tracks.onset = track(&[(0, 600), (700, 2000)], &[550]);
        let mut gap = comparison(&source, &native, &truth, blake3::hash(b"oracle"), 500).unwrap();
        include_onsets(&mut gap, &onset_analysis(&package, &[1024]), &truth).unwrap();
        let Onset::Stream(onset) = gap.onset else {
            panic!("Missing onset stream")
        };
        let result = onset.comparison.unwrap();
        assert_eq!(result.matched_count, 0);
        assert_eq!(result.unmatched_truth_frames, [550]);
        conserved(&result);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_automatic_candidate_is_a_stream_with_reviewed_false_negatives() {
        let (root, package) = crate::anchors::tests::fixture("automatic-onset-empty");
        let source = labels::Source::from_package(&package);
        let mut native = native(&source);
        native.metadata.profile = AUTO_ANALYSIS_PROFILE.into();
        let mut truth: music_truth::Document = serde_json::from_value(manual(&source)).unwrap();
        truth.tracks.onset = track(&[(0, 4800)], &[1500]);
        let analysis = onset_analysis(&package, &[]);
        let mut report = comparison(&source, &native, &truth, blake3::hash(b"oracle"), 0).unwrap();
        include_onsets(&mut report, &analysis, &truth).unwrap();
        let Onset::Stream(onset) = report.onset else {
            panic!("Missing empty candidate stream")
        };
        assert_eq!(onset.candidate_state, "Candidate");
        assert!(onset.records.is_empty());
        let result = onset.comparison.unwrap();
        assert_eq!(result.status, "MECHANICAL_FRAME_COMPARISON");
        assert_eq!(result.unmatched_truth_frames, [1500]);
        conserved(&result);
        truth.tracks.onset = Track::default();
        let mut report = comparison(&source, &native, &truth, blake3::hash(b"oracle"), 0).unwrap();
        include_onsets(&mut report, &analysis, &truth).unwrap();
        let Onset::Stream(onset) = report.onset else {
            panic!("Missing empty candidate stream")
        };
        assert_eq!(onset.comparison.unwrap().status, "NO_COMPARABLE_COVERAGE");
        // Constructed short-sample report oracle, not an ORT import or resource-reader acceptance
        let mut short_source = source.clone();
        short_source.canonical_frames = 1536;
        let mut short_native = native.clone();
        short_native.metadata.canonical_frames = short_source.canonical_frames;
        short_native.metadata.profile = AUTO_ANALYSIS_PROFILE.into();
        let mut short_truth: music_truth::Document =
            serde_json::from_value(manual(&short_source)).unwrap();
        short_truth.tracks.onset = track(&[(0, 1536)], &[1000]);
        let mut unsupported = analysis.clone();
        unsupported.capabilities.as_mut().unwrap().onset.state = AnalysisState::Unsupported;
        unsupported.energy[0].frames = 1536;
        unsupported.validate(1536).unwrap();
        let mut report = comparison(
            &short_source,
            &short_native,
            &short_truth,
            blake3::hash(b"oracle"),
            0,
        )
        .unwrap();
        include_onsets(&mut report, &unsupported, &short_truth).unwrap();
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["onset"]["candidate_state"], "Unsupported");
        assert_eq!(
            value["onset"]["unsupported_reason"],
            "insufficient_analysis_frames"
        );
        assert!(value["onset"]["comparison"].is_null());
        assert!(value["onset"]["records"].as_array().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_report_remains_byte_identical_when_package_has_unrelated_onsets() {
        let (root, package) = crate::anchors::tests::fixture("native-onset-legacy");
        let source = labels::Source::from_package(&package);
        let truth = serde_json::from_value(manual(&source)).unwrap();
        let mut native = native(&source);
        native.metadata.profile = "native-small0-high22050-f64fma-minimal-v1-candidate".into();
        let mut report = comparison(&source, &native, &truth, blake3::hash(b"oracle"), 0).unwrap();
        let original = serde_json::to_vec_pretty(&report).unwrap();
        include_onsets(&mut report, &package.analysis, &truth).unwrap();
        assert_eq!(serde_json::to_vec_pretty(&report).unwrap(), original);
        assert_eq!(report.schema_version, 1);
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["onset"], "NO_CANDIDATE_STREAM");
        fs::remove_dir_all(root).unwrap();
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
