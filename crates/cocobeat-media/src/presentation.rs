//! Offline presentation hints; algorithm scores never imply MIR admission

use cocobeat_schema::content::{
    AnalysisCapability, AnalysisSource, AnalysisState, ChordCandidate, KeyCandidate,
    MusicPresentation, PresentationWindow,
};
use cocobeat_schema::{CANONICAL_SAMPLE_RATE, MAX_CANONICAL_FRAMES, SongTime};
use oximedia_audio::spectrum::fft::{FftProcessor, WindowFunction};
use std::path::Path;

pub const PRESENTATION_ANALYSIS_PROFILE: &str = "canonical-salience-8192-hop3072-v2-candidate";
const FFT_FRAMES: usize = 8192;
const WINDOW_FRAMES: usize = FFT_FRAMES * 3;
const HOP_FRAMES: usize = 3072;

/// Analyze final canonical PCM while importing, never on the render/audio callback
pub fn analyze_presentation(
    path: impl AsRef<Path>,
    frames: u64,
) -> Result<MusicPresentation, String> {
    analyze_checked(path.as_ref(), frames, &|| Ok(()))
}

pub(crate) fn analyze_checked(
    path: &Path,
    frames: u64,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<MusicPresentation, String> {
    let mut analyzer = Analyzer::new();
    crate::decode_canonical(path, frames, |block| {
        check()?;
        analyzer.push(block)
    })?;
    analyzer.finish()
}

struct Analyzer {
    fft: FftProcessor,
    pending: Vec<[f32; 2]>,
    consumed: usize,
    previous_energy: f64,
    attacks: u8,
    windows: Vec<PresentationWindow>,
}

impl Analyzer {
    fn new() -> Self {
        Self {
            fft: FftProcessor::new(FFT_FRAMES, WindowFunction::Rectangle),
            pending: Vec::with_capacity(WINDOW_FRAMES),
            consumed: 0,
            previous_energy: 0.0,
            attacks: 0,
            windows: Vec::new(),
        }
    }

    fn push(&mut self, frames: &[[f32; 2]]) -> Result<(), String> {
        if frames.len() as u64 > MAX_CANONICAL_FRAMES.saturating_sub(self.consumed as u64) {
            return Err("Presentation PCM exceeds canonical frame limit".into());
        }
        for frame in frames {
            if frame.iter().any(|sample| !sample.is_finite()) {
                return Err("Presentation PCM contains nonfinite samples".into());
            }
            self.pending.push(*frame);
            self.consumed += 1;
            if self.pending.len() == WINDOW_FRAMES {
                self.flush()?;
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let channel_energy = [0, 1].map(|channel| {
            self.pending
                .iter()
                .map(|frame| f64::from(frame[channel]).powi(2))
                .sum::<f64>()
        });
        // Select the more energetic channel per window, preserving antiphase stereo
        let channel = usize::from(channel_energy[1] > channel_energy[0]);
        let hop = HOP_FRAMES.min(self.pending.len());
        let energy = self.pending[..hop]
            .iter()
            .map(|frame| f64::from(frame[channel]).powi(2))
            .sum::<f64>()
            / hop as f64;
        let rms = energy.sqrt();
        self.attacks =
            (self.attacks << 1) | u8::from(energy > 1e-5 && energy > self.previous_energy * 1.65);
        self.previous_energy = energy;
        let mut chroma = [0.0_f64; 12];
        let mut spectral_power = 0.0;
        let mut weighted_frequency = 0.0;
        let mut tonal_power = 0.0;
        for block in self.pending.chunks(FFT_FRAMES) {
            // Partial tails contribute energy/density, never invented padded pitch evidence
            if block.len() != FFT_FRAMES {
                continue;
            }
            let mut samples = vec![0.0; FFT_FRAMES];
            for (index, sample) in samples.iter_mut().enumerate() {
                *sample = f64::from(block[index][channel])
                    * (0.5
                        - 0.5
                            * (std::f64::consts::TAU * index as f64 / (FFT_FRAMES - 1) as f64)
                                .cos());
            }
            let spectrum = self.fft.magnitude_spectrum(&samples);
            if spectrum.len() != FFT_FRAMES || spectrum.iter().any(|value| !value.is_finite()) {
                return Err("Presentation FFT returned invalid spectrum".into());
            }
            let bins = &spectrum[14..1025]; // 82 Hz through 6 kHz
            let total = bins.iter().map(|v| v * v).sum::<f64>();
            if total < 1e-12 {
                continue;
            }
            let geometric =
                (bins.iter().map(|v| (v * v + 1e-20).ln()).sum::<f64>() / bins.len() as f64).exp();
            let flatness = geometric / (total / bins.len() as f64);
            for (index, amplitude) in bins.iter().enumerate() {
                let power = amplitude * amplitude;
                spectral_power += power;
                weighted_frequency +=
                    power * (index + 14) as f64 * f64::from(CANONICAL_SAMPLE_RATE)
                        / FFT_FRAMES as f64;
            }
            if flatness > 0.15 {
                continue;
            }
            let mut peaks = Vec::new();
            let mut supported = [false; 1027];
            for bin in 15..1024 {
                let magnitude = spectrum[bin];
                if magnitude <= spectrum[bin - 1]
                    || magnitude < spectrum[bin + 1]
                    || magnitude * magnitude < total * 0.00001
                {
                    continue;
                }
                // The local median preserves quiet pad fundamentals under a loud arpeggio
                let mut neighbors = [0.0_f64; 12];
                for (i, distance) in (3..=8).enumerate() {
                    neighbors[i * 2] = spectrum[bin - distance].powi(2);
                    neighbors[i * 2 + 1] = spectrum[bin + distance].powi(2);
                }
                neighbors.sort_unstable_by(f64::total_cmp);
                if magnitude.powi(2) < 10.0 * (neighbors[5] + neighbors[6]) * 0.5 {
                    continue;
                }
                let left = spectrum[bin - 1].max(1e-20).ln();
                let center = magnitude.max(1e-20).ln();
                let right = spectrum[bin + 1].max(1e-20).ln();
                let offset =
                    (0.5 * (left - right) / (left - 2.0 * center + right)).clamp(-0.5, 0.5);
                let frequency =
                    (bin as f64 + offset) * f64::from(CANONICAL_SAMPLE_RATE) / FFT_FRAMES as f64;
                let midi = 69.0 + 12.0 * (frequency / 440.0).log2();
                if (midi - midi.round()).abs() > 0.3 {
                    continue;
                }
                let power =
                    magnitude.powi(2) + spectrum[bin - 1].powi(2) + spectrum[bin + 1].powi(2);
                peaks.push((
                    frequency,
                    (midi.round() as i32).rem_euclid(12) as usize,
                    power,
                ));
                // Enveloped tones spread beyond three bins; count each supported bin once
                supported[bin - 2..=bin + 2].fill(true);
            }
            for &(frequency, pitch_class, power) in &peaks {
                // Only simultaneous lower peaks support harmonic suppression
                if peaks.iter().any(|&(lower, _, _)| {
                    lower < frequency * 0.51
                        && (frequency / lower - (frequency / lower).round()).abs() < 0.03
                }) {
                    continue;
                }
                chroma[pitch_class] += power.sqrt().sqrt();
            }
            tonal_power += supported
                .iter()
                .zip(&spectrum)
                .filter(|(supported, _)| **supported)
                .map(|(_, amplitude)| amplitude * amplitude)
                .sum::<f64>();
        }
        let tonal_confidence = if rms < 0.001 {
            0.0
        } else {
            (tonal_power / spectral_power.max(1e-20)).clamp(0.0, 1.0) as f32
        };
        let sum = chroma.iter().sum::<f64>();
        let chroma = if tonal_confidence < 0.55 || sum <= 0.0 {
            [0.0; 12]
        } else {
            chroma.map(|value| (value / sum) as f32)
        };
        let chord = chord_candidate(&chroma, tonal_confidence);
        let key = key_candidate(&chroma, tonal_confidence);
        self.windows.push(PresentationWindow {
            start: SongTime::from_frames((self.consumed - self.pending.len()) as i64),
            end: SongTime::from_frames((self.consumed - self.pending.len() + hop) as i64),
            chroma,
            chord,
            key,
            tonal_confidence,
            onset_density: self.attacks.count_ones() as f32 * CANONICAL_SAMPLE_RATE as f32
                / WINDOW_FRAMES as f32,
            brightness: (weighted_frequency / spectral_power.max(1e-20) / 6000.0).clamp(0.0, 1.0)
                as f32,
            energy: rms.min(1.0) as f32,
        });
        self.pending.drain(..hop);
        Ok(())
    }

    fn finish(mut self) -> Result<MusicPresentation, String> {
        if self.consumed == 0 {
            return Err("Presentation analysis requires PCM".into());
        }
        while !self.pending.is_empty() {
            self.flush()?;
        }
        let chords: Vec<_> = self.windows.iter().map(|window| window.chord).collect();
        for (index, window) in self.windows.iter_mut().enumerate() {
            let Some(chord) = window.chord else {
                continue;
            };
            // Wait out the spectral lookahead, then leave a hop unknown at changing boundaries
            if chords
                [index.saturating_sub(WINDOW_FRAMES / HOP_FRAMES)..(index + 2).min(chords.len())]
                .iter()
                .any(|other| {
                    other.is_none_or(|other| other.root != chord.root || other.minor != chord.minor)
                })
            {
                window.chord = None;
                window.key = None;
            }
        }
        Ok(MusicPresentation {
            capability: AnalysisCapability {
                state: AnalysisState::Candidate,
                source: AnalysisSource::Algorithm,
                confidence: None,
            },
            windows: self.windows,
        })
    }
}

fn chord_candidate(chroma: &[f32; 12], tonal: f32) -> Option<ChordCandidate> {
    // ponytail: conservative major/minor triads only; extend after annotated repertoire evaluation
    let mut best = None;
    let mut best_score = 0.0;
    let mut runner_up = 0.0;
    for root in 0..12 {
        for minor in [false, true] {
            let tones = [
                root,
                (root + if minor { 3 } else { 4 }) % 12,
                (root + 7) % 12,
            ];
            let score = tones.iter().map(|&tone| chroma[tone]).sum::<f32>();
            if tones.iter().any(|&tone| chroma[tone] < 0.09) {
                continue;
            }
            if score > best_score {
                runner_up = best_score;
                best_score = score;
                best = Some((root, minor));
            } else {
                runner_up = runner_up.max(score);
            }
        }
    }
    let (root, minor) = best?;
    (tonal >= 0.65 && best_score >= 0.82 && best_score - runner_up >= 0.12).then_some(
        ChordCandidate {
            root: root as u8,
            minor,
            confidence: best_score * tonal,
        },
    )
}

fn key_candidate(chroma: &[f32; 12], tonal: f32) -> Option<KeyCandidate> {
    // Key needs broader pitch evidence than one triad; ambiguous windows stay unknown
    if tonal < 0.7 || chroma.iter().filter(|&&v| v > 0.035).count() < 5 {
        return None;
    }
    let mut best = (0, false, 0.0_f32);
    let mut runner_up = 0.0_f32;
    for root in 0..12 {
        for minor in [false, true] {
            let scale = if minor {
                [0, 2, 3, 5, 7, 8, 10]
            } else {
                [0, 2, 4, 5, 7, 9, 11]
            };
            let score = scale
                .iter()
                .map(|offset| chroma[(root + offset) % 12])
                .sum::<f32>();
            // Relative major/minor share notes: tonic evidence must break the tie
            let score = score * 0.8 + chroma[root] * 0.2;
            if score > best.2 {
                runner_up = best.2;
                best = (root, minor, score);
            } else {
                runner_up = runner_up.max(score);
            }
        }
    }
    (best.2 > 0.8 && best.2 - runner_up > 0.035).then_some(KeyCandidate {
        root: best.0 as u8,
        minor: best.1,
        confidence: best.2 * tonal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tones(notes: &[f64], frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|frame| {
                let sample = notes
                    .iter()
                    .map(|frequency| {
                        (std::f64::consts::TAU * frequency * frame as f64 / 48000.0).sin() * 0.12
                    })
                    .sum::<f64>() as f32;
                [sample, -sample]
            })
            .collect()
    }
    #[test]
    fn major_minor_changes_antiphase_silence_noise_and_partial_tail() {
        let segment = WINDOW_FRAMES * 4;
        let mut analyzer = Analyzer::new();
        analyzer
            .push(&tones(&[261.6256, 329.6276, 391.9954], segment))
            .unwrap();
        analyzer
            .push(&tones(&[220.0, 261.6256, 329.6276], segment))
            .unwrap();
        analyzer.push(&vec![[0.0; 2]; segment]).unwrap();
        let mut rng = 7_u32;
        let noise = (0..segment)
            .map(|_| {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                let v = (rng as f64 / u32::MAX as f64 * 0.2 - 0.1) as f32;
                [v, v]
            })
            .collect::<Vec<_>>();
        analyzer.push(&noise).unwrap();
        analyzer.push(&tones(&[440.0], 77)).unwrap();
        let result = analyzer.finish().unwrap();
        let first = result.windows[0].chord.expect("C major triad");
        assert_eq!((first.root, first.minor), (0, false));
        let second = result
            .windows
            .iter()
            .find(|window| window.start.frames() >= segment as i64 && window.chord.is_some())
            .unwrap()
            .chord
            .unwrap();
        assert_eq!((second.root, second.minor), (9, true));
        assert!(
            result
                .windows
                .iter()
                .filter(|window| window.start.frames() >= (segment * 2) as i64)
                .all(|window| window.chord.is_none() && window.key.is_none())
        );
        assert_eq!(result.windows[segment * 2 / HOP_FRAMES].chroma, [0.0; 12]);
        assert_eq!(
            result.windows.last().unwrap().end.frames(),
            (segment * 4 + 77) as i64
        );
        assert_eq!(result.capability.state, AnalysisState::Candidate);
        let mut invalid = Analyzer::new();
        assert!(invalid.push(&[[f32::NAN, 0.0]]).is_err());
    }

    #[test]
    fn isolated_pitch_is_not_a_chord_and_keys_need_unambiguous_scale_evidence() {
        for note in 60..72 {
            let frequency = 440.0 * 2.0_f64.powf((f64::from(note) - 69.0) / 12.0);
            let mut analyzer = Analyzer::new();
            analyzer.push(&tones(&[frequency], WINDOW_FRAMES)).unwrap();
            let result = analyzer.finish().unwrap();
            let window = &result.windows[0];
            assert!(
                window.chroma[(note % 12) as usize] > 0.95,
                "note {note}: {:?}",
                window.chroma
            );
            assert!(window.chord.is_none() && window.key.is_none());
        }
        let c_major = [0.4, 0.0, 0.1, 0.0, 0.1, 0.1, 0.0, 0.1, 0.0, 0.1, 0.0, 0.1];
        let key = key_candidate(&c_major, 1.0).unwrap();
        assert_eq!((key.root, key.minor), (0, false));
        assert!(key_candidate(&[1.0 / 12.0; 12], 1.0).is_none());
    }

    #[test]
    fn quiet_pad_loud_arpeggio_and_key_change_keep_safe_pitch_spans() {
        let duration = 48_000 * 10;
        let mut analyzer = Analyzer::new();
        let mut noise = 13_u32;
        let pcm: Vec<_> = (0..duration)
            .map(|frame| {
                let notes = if frame < 48_000 * 4 {
                    [261.6256, 329.6276, 391.9954]
                } else {
                    [246.9417, 293.6648, 369.9944]
                };
                let sine = |frequency: f64| {
                    (std::f64::consts::TAU * frequency * frame as f64 / 48_000.0).sin()
                };
                let envelope = (1.0 - (frame % 12_000) as f64 / 12_000.0).powi(2);
                let pluck = notes[(frame / 12_000) % 3] * 2.0;
                let pad = notes
                    .iter()
                    .map(|&frequency| sine(frequency) * 0.012)
                    .sum::<f64>();
                let sample = if frame < 48_000 * 8 {
                    pad + (sine(pluck) + sine(pluck * 3.0) * 0.15) * envelope * 0.14
                } else if frame < 48_000 * 9 {
                    noise = noise.wrapping_mul(1664525).wrapping_add(1013904223);
                    f64::from(noise) / f64::from(u32::MAX) * 0.2 - 0.1
                } else {
                    0.0
                } as f32;
                [sample, -sample]
            })
            .collect();
        analyzer.push(&pcm).unwrap();
        let presentation = analyzer.finish().unwrap();
        let mut pitched = [0; 2];
        for frame in (0..48_000 * 8).step_by(480) {
            let end = frame + 47_281;
            let mut mask = 0xfff;
            for window in presentation
                .windows
                .iter()
                .filter(|w| w.end.frames() > frame as i64 && w.start.frames() < end as i64)
            {
                let Some(chord) = window
                    .chord
                    .filter(|chord| chord.confidence >= 0.75 && window.tonal_confidence >= 0.82)
                else {
                    mask = 0;
                    break;
                };
                mask &= [0, if chord.minor { 3 } else { 4 }, 7]
                    .into_iter()
                    .fold(0, |mask, offset| mask | (1 << ((chord.root + offset) % 12)));
            }
            if mask == 0 {
                continue;
            }
            let expected = if frame < 48_000 * 4 {
                (1 << 0) | (1 << 4) | (1 << 7)
            } else {
                (1 << 11) | (1 << 2) | (1 << 6)
            };
            assert_eq!(mask & expected, mask, "start {frame}");
            if frame < 48_000 * 4 && end >= 48_000 * 4 {
                assert_eq!(
                    mask & ((1 << 11) | (1 << 2) | (1 << 6)),
                    mask,
                    "crossed modulation at {frame}"
                );
            }
            assert!(end < 48_000 * 8, "tonal tail crossed into noise");
            pitched[usize::from(frame >= 48_000 * 4)] += 1;
        }
        assert!(
            pitched.into_iter().all(|count| count > 0),
            "each sustained harmony needs a safe audible span"
        );
        assert!(
            presentation
                .windows
                .iter()
                .filter(|w| w.start.frames() >= 48_000 * 8)
                .all(|w| w.chord.is_none())
        );
        let mut mixed = Analyzer::new();
        mixed
            .push(&tones(
                &[261.6256, 329.6276, 391.9954, 277.1826, 349.2282, 415.3047],
                48_000 * 3,
            ))
            .unwrap();
        assert!(
            mixed
                .finish()
                .unwrap()
                .windows
                .iter()
                .all(|w| w.chord.is_none()),
            "simultaneous incompatible triads remain unknown"
        );
    }
}
