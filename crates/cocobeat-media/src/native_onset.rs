//! Whole-channel HFC candidates with the detector's original left-window coordinates

use crate::MAX_ENCODER_PCM_PEAK;
use cocobeat_schema::{
    AnalysisState, CANONICAL_SAMPLE_RATE, MAX_CANONICAL_FRAMES, OnsetFeature, SongTime,
};
use oximedia_audio::transient_detector::{TransientConfig, TransientDetector};
use serde::{Deserialize, Serialize};

pub(crate) const ONSET_PROFILE: &str = "oximedia-hfc-48k-1024-512-v1";
pub(crate) const WINDOW_FRAMES: u64 = 1024;
pub(crate) const HOP_FRAMES: u64 = 512;
pub(crate) const MIN_ONSET_FRAMES: u64 = WINDOW_FRAMES + 2 * HOP_FRAMES;
pub(crate) const ONSET_THRESHOLD: f32 = 1.5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeOnsetRecord {
    pub time_ms: f64,
    pub grid_index: u64,
    pub frame: i64,
    pub support_end_frame: i64,
    pub strength: f32,
    #[serde(deserialize_with = "Option::<f32>::deserialize")]
    pub confidence: Option<f32>,
}

#[derive(Debug)]
pub(crate) struct NativeOnsetResult {
    pub state: AnalysisState,
    pub unsupported_reason: Option<&'static str>,
    pub records: Vec<NativeOnsetRecord>,
    pub onsets: Vec<OnsetFeature>,
}

/// The caller owns channel selection, bounded collection and cancellation before/after this call
pub(crate) fn analyze_onsets(mono: &[f32]) -> Result<NativeOnsetResult, String> {
    let n = mono.len() as u64;
    validate_extent(n)?;
    if mono
        .iter()
        .any(|sample| !sample.is_finite() || sample.abs() > MAX_ENCODER_PCM_PEAK)
    {
        return Err("Native onset PCM is non-finite or exceeds the canonical peak limit".into());
    }
    if n < MIN_ONSET_FRAMES {
        return Ok(NativeOnsetResult {
            state: AnalysisState::Unsupported,
            unsupported_reason: Some("insufficient_analysis_frames"),
            records: Vec::new(),
            onsets: Vec::new(),
        });
    }
    let detector = TransientDetector::new(TransientConfig {
        threshold: ONSET_THRESHOLD,
        hop_size: HOP_FRAMES as usize,
        window_size: WINDOW_FRAMES as usize,
    });
    let events = detector.detect_with_config(mono, CANONICAL_SAMPLE_RATE);
    let mut records = Vec::with_capacity(events.len());
    for event in events {
        if !event.time_ms.is_finite()
            || event.time_ms < 0.0
            || event.time_ms > n as f64 / f64::from(CANONICAL_SAMPLE_RATE) * 1000.0
        {
            return Err("Native onset time lies outside the finite canonical extent".into());
        }
        let grid_index = (event.time_ms * f64::from(CANONICAL_SAMPLE_RATE)
            / (1000.0 * HOP_FRAMES as f64))
            .round() as u64;
        let frame = (grid_index * HOP_FRAMES) as i64;
        records.push(NativeOnsetRecord {
            time_ms: event.time_ms,
            grid_index,
            frame,
            support_end_frame: frame + WINDOW_FRAMES as i64,
            strength: event.strength,
            confidence: None,
        });
    }
    let onsets = onsets_from_records(&records, n)?;
    Ok(NativeOnsetResult {
        state: AnalysisState::Candidate,
        unsupported_reason: None,
        records,
        onsets,
    })
}

/// Checks stored evidence without rerunning DSP; strength remains uncalibrated
pub(crate) fn onsets_from_records(
    records: &[NativeOnsetRecord],
    n: u64,
) -> Result<Vec<OnsetFeature>, String> {
    validate_extent(n)?;
    let odf_frames = if n < WINDOW_FRAMES {
        0
    } else {
        (n - WINDOW_FRAMES) / HOP_FRAMES + 1
    };
    if records.len() as u64 > odf_frames.saturating_sub(2) {
        return Err("Native onset count exceeds the interior full-window support".into());
    }
    let mut onsets = Vec::with_capacity(records.len());
    let mut previous = None;
    for record in records {
        if record.grid_index == 0 || record.grid_index >= odf_frames.saturating_sub(1) {
            return Err("Native onset must use an interior full-window grid point".into());
        }
        let frame = (record.grid_index * HOP_FRAMES) as i64;
        let expected_ms = (record.grid_index as f64 * HOP_FRAMES as f64
            / f64::from(CANONICAL_SAMPLE_RATE))
            * 1000.0;
        // Only f64 representation roundoff is accepted, never a musical timing tolerance
        let roundoff = 4.0 * f64::EPSILON * expected_ms.abs().max(1.0);
        if !record.time_ms.is_finite()
            || (record.time_ms - expected_ms).abs() > roundoff
            || record.frame != frame
            || record.support_end_frame != frame + WINDOW_FRAMES as i64
            || previous.is_some_and(|previous| frame <= previous)
            || !record.strength.is_finite()
            || !(0.0..=1.0).contains(&record.strength)
            || record.confidence.is_some()
        {
            return Err(
                "Native onset evidence changes the raw grid, support, strength or confidence"
                    .into(),
            );
        }
        onsets.push(OnsetFeature {
            time: SongTime::from_frames(frame),
            strength: record.strength,
            confidence: None,
        });
        previous = Some(frame);
    }
    Ok(onsets)
}

fn validate_extent(n: u64) -> Result<(), String> {
    if !(1..=MAX_CANONICAL_FRAMES).contains(&n) {
        return Err("Native onset requires a nonempty bounded canonical extent".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(grid_index: u64) -> NativeOnsetRecord {
        let frame = (grid_index * HOP_FRAMES) as i64;
        NativeOnsetRecord {
            time_ms: (frame as f64 / f64::from(CANONICAL_SAMPLE_RATE)) * 1000.0,
            grid_index,
            frame,
            support_end_frame: frame + WINDOW_FRAMES as i64,
            strength: 0.75,
            confidence: None,
        }
    }

    #[test]
    fn short_input_is_unsupported_and_full_window_silence_is_candidate_empty() {
        for n in [1, 1023, 1024, 1536, 2047] {
            let result = analyze_onsets(&vec![0.0; n]).unwrap();
            assert_eq!(result.state, AnalysisState::Unsupported);
            assert_eq!(
                result.unsupported_reason,
                Some("insufficient_analysis_frames")
            );
            assert!(result.records.is_empty() && result.onsets.is_empty());
        }
        for n in [2048, 2559, 2560, 2561] {
            let result = analyze_onsets(&vec![0.0; n]).unwrap();
            assert_eq!(result.state, AnalysisState::Candidate);
            assert_eq!(result.unsupported_reason, None);
            assert!(result.records.is_empty() && result.onsets.is_empty());
        }
        assert!(analyze_onsets(&[]).is_err());
        for value in [f32::NAN, f32::INFINITY, -f32::INFINITY, 4.01, -4.01] {
            assert!(analyze_onsets(&[value]).is_err());
        }
        assert!(onsets_from_records(&[], MAX_CANONICAL_FRAMES + 1).is_err());
    }

    #[test]
    fn evidence_preserves_raw_grid_strength_and_excludes_uncovered_edges() {
        let record = row(1);
        let onsets = onsets_from_records(std::slice::from_ref(&record), 2048).unwrap();
        assert_eq!(onsets[0].time, SongTime::from_frames(512));
        assert_eq!(onsets[0].strength.to_bits(), record.strength.to_bits());
        assert_eq!(onsets[0].confidence, None);
        let mut json = serde_json::to_value(&record).unwrap();
        assert!(json["confidence"].is_null());
        json.as_object_mut().unwrap().remove("confidence");
        assert!(serde_json::from_value::<NativeOnsetRecord>(json).is_err());
        for n in [2047, 2048, 2559, 2560, 2561] {
            assert_eq!(onsets_from_records(&[row(2)], n).is_ok(), n >= 2560);
        }
        assert!(onsets_from_records(&[row(0)], 4096).is_err());
        assert!(onsets_from_records(&[row(1), row(1)], 4096).is_err());
        assert!(onsets_from_records(&[row(2), row(1)], 4096).is_err());
        for field in 0..7 {
            let mut changed = record.clone();
            match field {
                0 => changed.time_ms += 0.000_001,
                1 => changed.time_ms = f64::NAN,
                2 => changed.frame += 1,
                3 => changed.support_end_frame += 1,
                4 => changed.strength = f32::NAN,
                5 => changed.strength = 1.01,
                _ => changed.confidence = Some(0.75),
            }
            assert!(onsets_from_records(&[changed], 2048).is_err());
        }
    }

    #[test]
    fn a_local_transient_keeps_its_window_and_antiphase_channel_strength() {
        let mut left = vec![0.0; 8192];
        left[4096] = 1.0;
        let right: Vec<_> = left.iter().map(|sample| -sample).collect();
        let left = analyze_onsets(&left).unwrap();
        let right = analyze_onsets(&right).unwrap();
        assert_eq!(left.records.len(), 1);
        assert_eq!(left.records, right.records);
        assert_eq!(left.records[0].frame, 3584);
        assert_eq!(left.records[0].support_end_frame, 4608);
        assert_eq!(left.records[0].strength, 1.0);
        assert_eq!(left.onsets[0].confidence, None);
    }
}
