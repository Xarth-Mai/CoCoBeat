//! Bounded canonical spectral descriptors, without musical section or repetition adoption

use crate::{ValidatedPackage, read_package};
use cocobeat_schema::{CANONICAL_SAMPLE_RATE, MAX_CANONICAL_FRAMES};
use oximedia_audio::spectrum::fft::{FftProcessor, WindowFunction};
use serde::Serialize;
use std::path::Path;

const FFT_FRAMES: usize = 1024;
const WINDOWS_PER_BIN: usize = 24;
const BIN_FRAMES: u64 = (FFT_FRAMES * WINDOWS_PER_BIN) as u64;
const MAX_BINS: usize = MAX_CANONICAL_FRAMES.div_ceil(BIN_FRAMES) as usize;
const BAND_EDGES: [usize; 9] = [0, 4, 8, 16, 32, 64, 128, 256, 513];
const NEIGHBORS: usize = 4;

#[derive(Debug, Serialize)]
pub struct StructureFeatureEvidence {
    pub profile: &'static str,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel: usize,
    pub canonical_frames: u64,
    pub fft_frames: usize,
    pub windows_per_bin: usize,
    pub bin_frames: u64,
    pub band_edges: [usize; 9],
    pub window: &'static str,
    pub coordinate: &'static str,
    pub power_convention: &'static str,
    pub padded_frames: u64,
    pub complete_windows: u64,
    pub partial_tail_frames: u64,
    pub beat_unit: Option<&'static str>,
    pub meter: Option<&'static str>,
    pub confidence: Option<f32>,
    pub quality_status: &'static str,
    pub production_admission: bool,
    pub bins: Vec<StructureFeatureBin>,
    pub adjacent: Vec<StructureAdjacentChange>,
}

#[derive(Debug, Serialize)]
pub struct StructureFeatureBin {
    pub index: usize,
    pub start_frame: u64,
    pub end_frame: u64,
    pub rms: f64,
    pub peak: f64,
    pub spectral_frames: u64,
    pub partial_tail_frames: u64,
    pub spectrum_status: &'static str,
    pub mean_band_power: Option<[f64; 8]>,
    pub log_band_power: Option<[f64; 8]>,
    pub neighbors: Vec<StructureNeighbor>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StructureNeighbor {
    pub index: usize,
    pub start_frame: u64,
    pub end_frame: u64,
    pub raw_cosine: f64,
}

#[derive(Debug, Serialize)]
pub struct StructureAdjacentChange {
    pub left_index: usize,
    pub right_index: usize,
    pub raw_cosine: Option<f64>,
    pub descriptor_distance: Option<f64>,
}

/// All features remain provisional until the same owned package snapshot passes validation
pub fn inspect_structure_features_package(
    root: impl AsRef<Path>,
    channel: usize,
) -> Result<(ValidatedPackage, StructureFeatureEvidence), String> {
    let mut features = Features::new(channel)?;
    let package = read_package(root, |frames| features.push(frames))?;
    if features.consumed != package.manifest.canonical_frames {
        return Err("Structure features differ from the validated canonical extent".into());
    }
    Ok((package, features.finish()?))
}

struct Features {
    channel: usize,
    fft: FftProcessor,
    tail: [f64; FFT_FRAMES],
    filled: usize,
    consumed: u64,
    bin_samples: u64,
    square_sum: f64,
    peak: f64,
    band_sum: [f64; 8],
    windows: u64,
    bins: Vec<StructureFeatureBin>,
}

impl Features {
    fn new(channel: usize) -> Result<Self, String> {
        if channel > 1 {
            return Err("Structure features require explicit left (0) or right (1) channel".into());
        }
        let mut fft = FftProcessor::new(FFT_FRAMES, WindowFunction::Rectangle);
        let mut impulse = [0.0; FFT_FRAMES];
        impulse[0] = 1.0;
        let probe = fft.magnitude_spectrum(&impulse);
        if probe.len() != FFT_FRAMES
            || probe
                .iter()
                .any(|v| !v.is_finite() || (v - 1.0).abs() > 1e-9)
        {
            return Err("Structure FFT forward impulse / scale check failed".into());
        }
        Ok(Self {
            channel,
            fft,
            tail: [0.0; FFT_FRAMES],
            filled: 0,
            consumed: 0,
            bin_samples: 0,
            square_sum: 0.0,
            peak: 0.0,
            band_sum: [0.0; 8],
            windows: 0,
            bins: Vec::new(),
        })
    }

    fn push(&mut self, frames: &[[f32; 2]]) -> Result<(), String> {
        if frames.len() as u64 > MAX_CANONICAL_FRAMES - self.consumed {
            return Err("Structure PCM exceeds the canonical frame limit".into());
        }
        for frame in frames {
            let sample = f64::from(frame[self.channel]);
            if !sample.is_finite() {
                return Err("Structure PCM contains a non-finite selected sample".into());
            }
            self.tail[self.filled] = sample;
            self.filled += 1;
            self.consumed += 1;
            self.bin_samples += 1;
            self.square_sum += sample * sample;
            self.peak = self.peak.max(sample.abs());
            if self.filled == FFT_FRAMES {
                let power = band_power(&mut self.fft, &self.tail)?;
                for (sum, value) in self.band_sum.iter_mut().zip(power) {
                    *sum += value;
                }
                self.windows += 1;
                self.filled = 0;
            }
            if self.bin_samples == BIN_FRAMES {
                self.finish_bin()?;
            }
        }
        Ok(())
    }

    fn finish_bin(&mut self) -> Result<(), String> {
        if self.bins.len() >= MAX_BINS {
            return Err("Structure feature count exceeds the canonical limit".into());
        }
        let mean = (self.windows > 0).then(|| self.band_sum.map(|v| v / self.windows as f64));
        let log = mean.map(|v| v.map(f64::ln_1p));
        self.bins
            .try_reserve(1)
            .map_err(|_| "Cannot reserve bounded structure bins")?;
        self.bins.push(StructureFeatureBin {
            index: self.bins.len(),
            start_frame: self.consumed - self.bin_samples,
            end_frame: self.consumed,
            rms: (self.square_sum / self.bin_samples as f64).sqrt(),
            peak: self.peak,
            spectral_frames: self.windows * FFT_FRAMES as u64,
            partial_tail_frames: self.bin_samples - self.windows * FFT_FRAMES as u64,
            spectrum_status: if self.windows == 0 {
                "INSUFFICIENT_FULL_WINDOW"
            } else if self.band_sum.iter().all(|v| *v == 0.0) {
                "EXACT_ZERO_SPECTRUM"
            } else {
                "MEASURED"
            },
            mean_band_power: mean,
            log_band_power: log,
            neighbors: Vec::new(),
        });
        self.bin_samples = 0;
        self.square_sum = 0.0;
        self.peak = 0.0;
        self.band_sum = [0.0; 8];
        self.windows = 0;
        Ok(())
    }

    fn finish(mut self) -> Result<StructureFeatureEvidence, String> {
        if self.consumed == 0 {
            return Err("Structure features require nonempty canonical PCM".into());
        }
        if self.bin_samples > 0 {
            // The actual partial FFT tail contributes energy only, never a padded spectrum
            self.finish_bin()?;
        }
        let adjacent = relations(&mut self.bins)?;
        Ok(StructureFeatureEvidence {
            profile: "canonical-spectrum-1024x24-v1-candidate",
            sample_rate: CANONICAL_SAMPLE_RATE,
            channels: 2,
            channel: self.channel,
            canonical_frames: self.consumed,
            fft_frames: FFT_FRAMES,
            windows_per_bin: WINDOWS_PER_BIN,
            bin_frames: BIN_FRAMES,
            band_edges: BAND_EDGES,
            window: "Rectangle_nonoverlapping_actual_1024_frames",
            coordinate: "canonical_pcm_half_open_frames",
            power_convention: "mean_window_sum_unscaled_magnitude_squared_then_log1p_no_doubling",
            padded_frames: 0,
            complete_windows: self.consumed / FFT_FRAMES as u64,
            partial_tail_frames: self.consumed % FFT_FRAMES as u64,
            beat_unit: None,
            meter: None,
            confidence: None,
            quality_status: "UNASSESSED",
            production_admission: false,
            bins: self.bins,
            adjacent,
        })
    }
}

fn band_power(fft: &mut FftProcessor, window: &[f64; FFT_FRAMES]) -> Result<[f64; 8], String> {
    let spectrum = fft.magnitude_spectrum(window);
    if spectrum.len() != FFT_FRAMES || spectrum.iter().any(|v| !v.is_finite()) {
        return Err("Structure FFT produced invalid magnitudes".into());
    }
    if window.iter().any(|v| *v != 0.0) && spectrum.iter().all(|v| *v == 0.0) {
        return Err("Structure FFT produced all zeros for nonzero PCM".into());
    }
    Ok(std::array::from_fn(|band| {
        spectrum[BAND_EDGES[band]..BAND_EDGES[band + 1]]
            .iter()
            .map(|v| v * v)
            .sum()
    }))
}

fn cosine(left: Option<[f64; 8]>, right: Option<[f64; 8]>) -> Option<f64> {
    let (left, right) = (left?, right?);
    let dot: f64 = left.iter().zip(right).map(|(a, b)| a * b).sum();
    let left_norm: f64 = left.iter().map(|v| v * v).sum();
    let right_norm: f64 = right.iter().map(|v| v * v).sum();
    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }
    Some(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

fn relations(bins: &mut [StructureFeatureBin]) -> Result<Vec<StructureAdjacentChange>, String> {
    let mut adjacent = Vec::new();
    adjacent
        .try_reserve_exact(bins.len().saturating_sub(1))
        .map_err(|_| "Cannot reserve bounded structure changes")?;
    for pair in bins.windows(2) {
        let score = cosine(pair[0].log_band_power, pair[1].log_band_power);
        adjacent.push(StructureAdjacentChange {
            left_index: pair[0].index,
            right_index: pair[1].index,
            raw_cosine: score,
            descriptor_distance: score.map(|v| 1.0 - v),
        });
    }
    let mut row = Vec::new();
    row.try_reserve_exact(bins.len().saturating_sub(1))
        .map_err(|_| "Cannot reserve bounded structure neighbor row")?;
    // ponytail: at most1172 bins, one bounded row; add an index only if this ceiling changes
    for i in 0..bins.len() {
        row.clear();
        for (j, candidate) in bins.iter().enumerate() {
            if i != j
                && let Some(raw_cosine) = cosine(bins[i].log_band_power, candidate.log_band_power)
            {
                row.push(StructureNeighbor {
                    index: j,
                    start_frame: candidate.start_frame,
                    end_frame: candidate.end_frame,
                    raw_cosine,
                });
            }
        }
        row.sort_unstable_by(|a, b| {
            b.raw_cosine
                .total_cmp(&a.raw_cosine)
                .then(a.index.cmp(&b.index))
        });
        bins[i]
            .neighbors
            .try_reserve_exact(row.len().min(NEIGHBORS))
            .map_err(|_| "Cannot reserve bounded structure neighbors")?;
        bins[i]
            .neighbors
            .extend(row.iter().take(NEIGHBORS).cloned());
    }
    Ok(adjacent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn inspect(frames: &[[f32; 2]], channel: usize, chunk: usize) -> StructureFeatureEvidence {
        let mut features = Features::new(channel).unwrap();
        for block in frames.chunks(chunk) {
            features.push(block).unwrap();
        }
        features.finish().unwrap()
    }

    #[test]
    fn actual_tail_energy_and_spectral_support_do_not_pad_or_depend_on_chunks() {
        for n in [1, 1023, 1024, 1025, 24575, 24576, 24577] {
            let mut frames = vec![[0.0; 2]; n];
            frames[0][0] = 1.0;
            frames[n - 1][0] += 2.0;
            for chunk in [1, 137, 1024, 24577] {
                let evidence = inspect(&frames, 0, chunk);
                assert_eq!(evidence.canonical_frames, n as u64);
                assert_eq!(evidence.complete_windows, (n / FFT_FRAMES) as u64);
                assert_eq!(evidence.partial_tail_frames, (n % FFT_FRAMES) as u64);
                assert_eq!(evidence.bins.len(), n.div_ceil(BIN_FRAMES as usize));
                for bin in &evidence.bins {
                    let samples = &frames[bin.start_frame as usize..bin.end_frame as usize];
                    let squares: f64 = samples.iter().map(|v| f64::from(v[0]).powi(2)).sum();
                    let expected_peak = if n == 1 {
                        3.0
                    } else if bin.end_frame == n as u64 {
                        2.0
                    } else {
                        1.0
                    };
                    assert_eq!(bin.peak, expected_peak);
                    assert!((bin.rms - (squares / samples.len() as f64).sqrt()).abs() < 1e-12);
                    assert_eq!(
                        bin.spectral_frames,
                        (samples.len() / FFT_FRAMES * FFT_FRAMES) as u64
                    );
                    assert_eq!(bin.partial_tail_frames, (samples.len() % FFT_FRAMES) as u64);
                    assert_eq!(bin.mean_band_power.is_none(), samples.len() < FFT_FRAMES);
                }
            }
        }
    }

    #[test]
    fn rectangular_fft_band_formula_matches_independent_dft_and_nyquist() {
        let window = std::array::from_fn(|i| if i < 7 { (i as f64 - 3.0) / 8.0 } else { 0.0 });
        let mut fft = FftProcessor::new(FFT_FRAMES, WindowFunction::Rectangle);
        let actual = band_power(&mut fft, &window).unwrap();
        let mut expected = [0.0; 8];
        for k in 0..=FFT_FRAMES / 2 {
            let (mut real, mut imaginary) = (0.0, 0.0);
            for (i, sample) in window[..7].iter().enumerate() {
                let phase = TAU * (i * k) as f64 / FFT_FRAMES as f64;
                real += sample * phase.cos();
                imaginary -= sample * phase.sin();
            }
            let band = BAND_EDGES.partition_point(|v| *v <= k) - 1;
            expected[band] += real * real + imaginary * imaginary;
        }
        for (a, e) in actual.into_iter().zip(expected) {
            assert!((a - e).abs() <= 1e-10 * e.max(1.0));
        }
        let mut unequal_windows = vec![[1.0, 0.0]; FFT_FRAMES];
        unequal_windows.extend(vec![[2.0, 0.0]; FFT_FRAMES]);
        let evidence = inspect(&unequal_windows, 0, 127);
        let expected_mean = 2.5 * (FFT_FRAMES * FFT_FRAMES) as f64;
        assert_eq!(evidence.bins[0].mean_band_power.unwrap()[0], expected_mean);
        assert_eq!(
            evidence.bins[0].log_band_power.unwrap()[0],
            expected_mean.ln_1p()
        );
        let alternating = std::array::from_fn(|i| if i % 2 == 0 { 1.0 } else { -1.0 });
        let powers = band_power(&mut fft, &alternating).unwrap();
        assert_eq!(powers[7], (FFT_FRAMES * FFT_FRAMES) as f64);
        assert!(powers[..7].iter().all(|v| v.abs() < 1e-20));
    }

    #[test]
    fn frequency_not_rms_and_explicit_channels_determine_descriptors() {
        let dc = vec![[1.0, 0.0]; FFT_FRAMES];
        let nyquist: Vec<_> = (0..FFT_FRAMES)
            .map(|i| [if i % 2 == 0 { 1.0 } else { -1.0 }, 0.0])
            .collect();
        let a = inspect(&dc, 0, 127);
        let b = inspect(&nyquist, 0, 127);
        assert_eq!(a.bins[0].rms, 1.0);
        assert_eq!(b.bins[0].rms, 1.0);
        assert_eq!(
            cosine(a.bins[0].log_band_power, b.bins[0].log_band_power),
            Some(0.0)
        );
        let antiphase = vec![[1.0, -1.0]; FFT_FRAMES];
        let right = inspect(&antiphase, 1, 133);
        assert_eq!(right.bins[0].mean_band_power, a.bins[0].mean_band_power);
        let silent = inspect(&dc, 1, 133);
        assert_eq!(silent.bins[0].spectrum_status, "EXACT_ZERO_SPECTRUM");
        assert_eq!(
            cosine(silent.bins[0].log_band_power, silent.bins[0].log_band_power),
            None
        );
    }

    #[test]
    fn neighbors_exclude_self_tie_by_original_index_and_stay_bounded() {
        let evidence = inspect(&vec![[1.0, 0.0]; BIN_FRAMES as usize * 6], 0, 997);
        for bin in &evidence.bins {
            let expected: Vec<_> = (0..6).filter(|i| *i != bin.index).take(4).collect();
            assert_eq!(
                bin.neighbors.iter().map(|v| v.index).collect::<Vec<_>>(),
                expected
            );
            for neighbor in &bin.neighbors {
                assert!((neighbor.raw_cosine - 1.0).abs() < 1e-12);
                assert_eq!(neighbor.start_frame, neighbor.index as u64 * BIN_FRAMES);
                assert_eq!(neighbor.end_frame, (neighbor.index + 1) as u64 * BIN_FRAMES);
            }
        }
        assert_eq!(evidence.adjacent.len(), 5);
        assert!(
            evidence
                .adjacent
                .iter()
                .all(|v| (v.raw_cosine.unwrap() - 1.0).abs() < 1e-12
                    && v.descriptor_distance.unwrap().abs() < 1e-12)
        );
        let short = inspect(&[[1.0, 0.0]], 0, 1);
        assert_eq!(short.bins[0].spectrum_status, "INSUFFICIENT_FULL_WINDOW");
        assert!(short.bins[0].neighbors.is_empty());
    }

    #[test]
    fn invalid_channel_nonfinite_extent_and_empty_pcm_are_rejected() {
        assert!(Features::new(2).is_err());
        assert!(Features::new(0).unwrap().finish().is_err());
        let mut features = Features::new(0).unwrap();
        assert!(features.push(&[[f32::NAN, 0.0]]).is_err());
        assert_eq!(features.consumed, 0);
        features.consumed = MAX_CANONICAL_FRAMES;
        assert!(features.push(&[[0.0; 2]]).is_err());
        assert_eq!(features.consumed, MAX_CANONICAL_FRAMES);
    }
}
