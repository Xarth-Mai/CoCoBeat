//! Raw causal tempo observations from the same validated canonical package snapshot

use crate::{ValidatedPackage, read_package};
use cocobeat_btt::{BLOCK_FRAMES, Btt48000, TempoEstimate, UPSTREAM_COMMIT};
use cocobeat_schema::{CANONICAL_SAMPLE_RATE, MAX_CANONICAL_FRAMES};
use serde::Serialize;
use std::path::Path;

const WARMUP_FRAMES: u64 = 1024 * BLOCK_FRAMES as u64;
const ZERO_SUPPORT_FRAMES: u64 = 1024 + 128 + (15 + 1 + 1024) * 128;
const MAX_RECORDS: usize = MAX_CANONICAL_FRAMES.div_ceil(BLOCK_FRAMES as u64) as usize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NativeTempoRecord {
    pub consumed_frames: u64,
    pub bpm: f64,
    pub period_frames: u32,
    pub native_certainty: f64,
    pub warmup_complete: bool,
    pub trailing_zero_frames: u64,
    pub stale_status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct NativeTempoEvidence {
    pub profile: &'static str,
    pub upstream_commit: &'static str,
    pub sample_rate: u32,
    pub channel: usize,
    pub canonical_frames: u64,
    pub block_frames: usize,
    pub fft_frames: u32,
    pub hop_frames: u32,
    pub oss_frames: u32,
    pub filter_order: u32,
    pub warmup_frames: u64,
    pub zero_support_frames: u64,
    pub latency_adjustments: [i32; 2],
    pub callbacks: bool,
    pub coordinate: &'static str,
    pub zero_test: &'static str,
    pub beat_unit: Option<&'static str>,
    pub meter: Option<&'static str>,
    pub confidence: Option<f32>,
    pub quality_status: &'static str,
    pub production_admission: bool,
    pub records: Vec<NativeTempoRecord>,
}

/// The package is unchanged; returned rows describe consumption positions, never event times
pub fn inspect_native_tempo_package(
    root: impl AsRef<Path>,
    channel: usize,
) -> Result<(ValidatedPackage, NativeTempoEvidence), String> {
    let mut blocks = CanonicalBlocks::new(channel)?;
    let mut tracker = Btt48000::new().map_err(str::to_owned)?;
    let mut records = Vec::new();
    let mut consume = |samples: &mut [f32], consumed, zeros| {
        if records.len() >= MAX_RECORDS {
            return Err("Native tempo observation count exceeds the canonical extent limit".into());
        }
        let estimate = tracker.process(samples).map_err(str::to_owned)?;
        let record = record(estimate, consumed, zeros)?;
        records
            .try_reserve(1)
            .map_err(|_| "Cannot reserve bounded native tempo observations".to_string())?;
        records.push(record);
        Ok(())
    };
    // All native state and rows remain local until the complete four-object validation succeeds
    let package = read_package(root, |frames| blocks.push(frames, &mut consume))?;
    if blocks.consumed != package.manifest.canonical_frames {
        return Err(
            "Native tempo consumed extent differs from the validated canonical package".into(),
        );
    }
    blocks.finish(&mut consume)?;
    if records.len() as u64 != blocks.consumed.div_ceil(BLOCK_FRAMES as u64)
        || records.last().map(|r| r.consumed_frames) != Some(blocks.consumed)
    {
        return Err(
            "Native tempo final observation count / position differs from the input".into(),
        );
    }
    let evidence = NativeTempoEvidence {
        profile: "btt-48k-tempo-only-v1",
        upstream_commit: UPSTREAM_COMMIT,
        sample_rate: CANONICAL_SAMPLE_RATE,
        channel,
        canonical_frames: blocks.consumed,
        block_frames: BLOCK_FRAMES,
        fft_frames: 1024,
        hop_frames: 128,
        oss_frames: 1024,
        filter_order: 15,
        warmup_frames: WARMUP_FRAMES,
        zero_support_frames: ZERO_SUPPORT_FRAMES,
        latency_adjustments: [0, 0],
        callbacks: false,
        coordinate: "consumed_pcm_frames_not_event_position",
        zero_test: "exact_selected_channel_pcm_not_perceptual_silence",
        beat_unit: None,
        meter: None,
        confidence: None,
        quality_status: "UNSCORED_NO_ADMISSION_THRESHOLD",
        production_admission: false,
        records,
    };
    Ok((package, evidence))
}

fn record(estimate: TempoEstimate, consumed: u64, zeros: u64) -> Result<NativeTempoRecord, String> {
    if consumed < WARMUP_FRAMES && (estimate.bpm != 0.0 || estimate.period_frames != 0) {
        return Err("Native tempo unexpectedly precedes its OSS warmup".into());
    }
    let stale_status = if zeros == 0 {
        "not_in_exact_zero_run"
    } else if estimate.period_frames == 0 {
        "no_retained_tempo"
    } else if zeros >= ZERO_SUPPORT_FRAMES {
        "stale_after_full_zero_support"
    } else {
        "insufficient_zero_history"
    };
    Ok(NativeTempoRecord {
        consumed_frames: consumed,
        bpm: estimate.bpm,
        period_frames: estimate.period_frames,
        native_certainty: estimate.native_certainty,
        warmup_complete: consumed >= WARMUP_FRAMES,
        trailing_zero_frames: zeros,
        stale_status,
    })
}

// Decoder callback boundaries must not change the fixed BTT observation positions
struct CanonicalBlocks {
    channel: usize,
    tail: [f32; BLOCK_FRAMES],
    filled: usize,
    consumed: u64,
    zeros: u64,
}

impl CanonicalBlocks {
    fn new(channel: usize) -> Result<Self, String> {
        if channel > 1 {
            return Err("Native tempo requires explicit left (0) or right (1) channel".into());
        }
        Ok(Self {
            channel,
            tail: [0.0; BLOCK_FRAMES],
            filled: 0,
            consumed: 0,
            zeros: 0,
        })
    }

    fn push(
        &mut self,
        frames: &[[f32; 2]],
        consume: &mut impl FnMut(&mut [f32], u64, u64) -> Result<(), String>,
    ) -> Result<(), String> {
        if frames.len() as u64 > MAX_CANONICAL_FRAMES - self.consumed {
            return Err("Native tempo PCM exceeds the canonical frame limit".into());
        }
        for frame in frames {
            let sample = frame[self.channel];
            if !sample.is_finite() || sample.abs() > 4.0 {
                return Err(
                    "Native tempo PCM is non-finite or exceeds the canonical peak limit".into(),
                );
            }
            self.tail[self.filled] = sample;
            self.filled += 1;
            self.consumed += 1;
            self.zeros = if sample == 0.0 { self.zeros + 1 } else { 0 };
            if self.filled == BLOCK_FRAMES {
                consume(&mut self.tail, self.consumed, self.zeros)?;
                self.filled = 0;
            }
        }
        Ok(())
    }

    fn finish(
        &mut self,
        consume: &mut impl FnMut(&mut [f32], u64, u64) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.filled > 0 {
            consume(&mut self.tail[..self.filled], self.consumed, self.zeros)?;
            self.filled = 0;
        }
        Ok(())
    }
}

pub(crate) const INTERBEAT_PROFILE: &str = "adjacent-native-beat-interval-v1";

/// Each region describes only the interval between two observed beats, with unknown beat unit
pub(crate) fn interbeat_tempo(
    beats: &[cocobeat_schema::BeatFeature],
    n: u64,
) -> Result<Vec<cocobeat_schema::TempoRegion>, String> {
    use cocobeat_schema::{MAX_CONTENT_ITEMS, TempoRegion};

    if !(1..=MAX_CANONICAL_FRAMES).contains(&n) || beats.len() > MAX_CONTENT_ITEMS {
        return Err("Interbeat tempo requires bounded canonical extent and beat count".into());
    }
    let mut previous = None;
    for beat in beats {
        let frame = beat.time.frames();
        if frame < 0
            || frame as u64 >= n
            || previous.is_some_and(|previous| frame <= previous)
            || !beat.strength.is_finite()
            || beat.strength < 0.0
            || [beat.downbeat_probability, beat.confidence]
                .into_iter()
                .flatten()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err(
                "Interbeat tempo requires valid beats strictly increasing within the audio".into(),
            );
        }
        previous = Some(frame);
    }
    Ok(beats
        .windows(2)
        .map(|pair| TempoRegion {
            start: pair[0].time,
            end: pair[1].time,
            bpm: (60.0 * f64::from(CANONICAL_SAMPLE_RATE)
                / (pair[1].time.frames() - pair[0].time.frames()) as f64) as f32,
            beat_unit: None,
            confidence: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_schema::{BeatFeature, SongTime};

    #[test]
    fn decoder_chunks_preserve_selected_samples_and_exact_short_eof() {
        let frames: Vec<_> = (0..257)
            .map(|i| [i as f32 / 100.0, if i < 129 { -1.0 } else { 0.0 }])
            .collect();
        for channel in [0, 1] {
            for chunks in [1, 127, 128, 129, 257] {
                let mut blocks = CanonicalBlocks::new(channel).unwrap();
                let mut seen = Vec::new();
                let mut positions = Vec::new();
                let mut consume = |samples: &mut [f32], n, zeros| {
                    seen.extend_from_slice(samples);
                    positions.push((n, samples.len(), zeros));
                    Ok(())
                };
                for chunk in frames.chunks(chunks) {
                    blocks.push(chunk, &mut consume).unwrap();
                }
                blocks.finish(&mut consume).unwrap();
                blocks.finish(&mut consume).unwrap();
                assert_eq!(seen, frames.iter().map(|v| v[channel]).collect::<Vec<_>>());
                assert_eq!(
                    positions.iter().map(|v| (v.0, v.1)).collect::<Vec<_>>(),
                    [(128, 128), (256, 128), (257, 1)]
                );
                if channel == 1 {
                    assert_eq!(positions[2].2, 128);
                }
            }
        }
    }

    #[test]
    fn invalid_extent_channel_pcm_and_consumer_errors_stop_immediately() {
        assert!(CanonicalBlocks::new(2).is_err());
        let mut blocks = CanonicalBlocks::new(0).unwrap();
        assert!(
            blocks
                .push(&[[f32::NAN, 0.0]], &mut |_, _, _| panic!(
                    "invalid PCM reached native"
                ))
                .is_err()
        );
        blocks.consumed = MAX_CANONICAL_FRAMES - 1;
        assert!(
            blocks
                .push(&[[0.0; 2]; 2], &mut |_, _, _| panic!(
                    "excess reached native"
                ))
                .is_err()
        );
        let mut blocks = CanonicalBlocks::new(0).unwrap();
        let mut calls = 0;
        let error = blocks
            .push(&[[0.0; 2]; 257], &mut |_, _, _| {
                calls += 1;
                Err("stop".into())
            })
            .unwrap_err();
        assert_eq!(error, "stop");
        assert_eq!(calls, 1);
        assert_eq!(blocks.consumed, 128);
    }

    #[test]
    fn raw_histogram_warmup_and_exact_zero_support_are_not_confidence() {
        let retained = TempoEstimate {
            bpm: 60.0 * 48_000.0 / 24_064.0,
            period_frames: 24_064,
            native_certainty: 19.75,
        };
        assert!(record(retained, WARMUP_FRAMES - 1, 0).is_err());
        let row = record(retained, WARMUP_FRAMES, 1).unwrap();
        assert!(row.warmup_complete);
        assert_eq!(row.native_certainty, 19.75);
        assert_eq!(row.stale_status, "insufficient_zero_history");
        assert_eq!(
            record(retained, ZERO_SUPPORT_FRAMES, ZERO_SUPPORT_FRAMES)
                .unwrap()
                .stale_status,
            "stale_after_full_zero_support"
        );
        assert_eq!(
            record(retained, ZERO_SUPPORT_FRAMES, ZERO_SUPPORT_FRAMES - 1)
                .unwrap()
                .stale_status,
            "insufficient_zero_history"
        );
        let zero = TempoEstimate {
            bpm: 0.0,
            period_frames: 0,
            native_certainty: 0.0,
        };
        assert_eq!(
            record(zero, 1, 1).unwrap().stale_status,
            "no_retained_tempo"
        );
    }

    fn beat(frame: i64) -> BeatFeature {
        BeatFeature {
            time: SongTime::from_frames(frame),
            strength: 0.5,
            downbeat_probability: None,
            confidence: None,
        }
    }

    #[test]
    fn adjacent_intervals_preserve_endpoints_without_tails_or_invented_units() {
        let beats = [beat(24_000), beat(48_000), beat(60_000)];
        let regions = interbeat_tempo(&beats, 96_000).unwrap();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].start.frames(), 24_000);
        assert_eq!(regions[0].end.frames(), 48_000);
        assert_eq!(regions[0].bpm, 120.0);
        assert_eq!(regions[1].start.frames(), 48_000);
        assert_eq!(regions[1].end.frames(), 60_000);
        assert_eq!(regions[1].bpm, 240.0);
        assert!(
            regions
                .iter()
                .all(|r| r.beat_unit.is_none() && r.confidence.is_none())
        );
        assert!(interbeat_tempo(&[], 96_000).unwrap().is_empty());
        assert!(interbeat_tempo(&beats[..1], 96_000).unwrap().is_empty());
        assert_eq!(
            interbeat_tempo(&[beat(0), beat(1)], 2).unwrap()[0].bpm,
            2_880_000.0
        );
    }

    #[test]
    fn malformed_beats_or_extents_cannot_become_tempo() {
        for beats in [
            vec![beat(-1)],
            vec![beat(96_000)],
            vec![beat(24_000), beat(24_000)],
            vec![beat(48_000), beat(24_000)],
        ] {
            assert!(interbeat_tempo(&beats, 96_000).is_err());
        }
        for n in [0, MAX_CANONICAL_FRAMES + 1] {
            assert!(interbeat_tempo(&[], n).is_err());
        }
        for field in 0..3 {
            let mut invalid = beat(1);
            match field {
                0 => invalid.strength = f32::NAN,
                1 => invalid.downbeat_probability = Some(1.01),
                _ => invalid.confidence = Some(-0.1),
            }
            assert!(interbeat_tempo(&[invalid], 96_000).is_err());
        }
    }
}
