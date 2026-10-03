use oximedia_audio::{AudioBuffer, AudioFrame, ChannelLayout, Resampler, ResamplerQuality};
use oximedia_core::SampleFormat;
use std::{
    error::Error,
    f64::consts::TAU,
    fs,
    path::{Path, PathBuf},
};

fn frame(data: &[u8], rate: u32) -> AudioFrame {
    let mut frame = AudioFrame::new(SampleFormat::F32, rate, ChannelLayout::Stereo);
    frame.samples = AudioBuffer::Interleaved(data.to_vec().into());
    frame
}

fn append(frame: AudioFrame, output: &mut Vec<u8>) {
    assert_eq!(frame.format, SampleFormat::F32);
    assert_eq!(frame.sample_rate, 48_000);
    assert_eq!(frame.channels.count(), 2);
    match frame.samples {
        AudioBuffer::Interleaved(data) => output.extend_from_slice(&data),
        _ => panic!("expected interleaved F32"),
    }
}

// All quality inputs are one second; exclude 100 ms at each edge only for
// steady-state metrics and record edge impulses separately without this trim
const TRIM: usize = 4800;
const AMPLITUDE: f64 = 0.5;

fn number(value: f64) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "null".into()
    }
}

fn status(pass: bool) -> &'static str {
    if pass { "PASS" } else { "FAIL" }
}

struct ToneMetrics {
    gain_db: f64,
    phase_error_rad: f64,
    truth_snr_db: f64,
    fitted_snr_db: f64,
    suppression_db: f64,
    output_rms: f64,
    truth_error_rms: f64,
    fitted_error_rms: f64,
}

fn tone_metrics(samples: &[f64], frequency: f64, phase: f64) -> ToneMetrics {
    let mut ss = 0.0;
    let mut cc = 0.0;
    let mut sc = 0.0;
    let mut ys = 0.0;
    let mut yc = 0.0;
    let mut energy = 0.0;
    let mut truth_energy = 0.0;
    let mut error_energy = 0.0;
    for (i, &y) in samples
        .iter()
        .enumerate()
        .take(samples.len() - TRIM)
        .skip(TRIM)
    {
        let (s, c) = (TAU * frequency * i as f64 / 48_000.0).sin_cos();
        let truth = AMPLITUDE * (TAU * frequency * i as f64 / 48_000.0 + phase).sin();
        ss += s * s;
        cc += c * c;
        sc += s * c;
        ys += y * s;
        yc += y * c;
        energy += y * y;
        truth_energy += truth * truth;
        error_energy += (y - truth).powi(2);
    }
    let det = ss * cc - sc * sc;
    let (a, b) = if det.abs() > 1e-9 {
        ((ys * cc - yc * sc) / det, (yc * ss - ys * sc) / det)
    } else {
        // The target Nyquist tone has only one independent basis vector
        (f64::NAN, f64::NAN)
    };
    let mut residual = 0.0;
    let mut fitted_energy = 0.0;
    for (i, &y) in samples
        .iter()
        .enumerate()
        .take(samples.len() - TRIM)
        .skip(TRIM)
    {
        let (s, c) = (TAU * frequency * i as f64 / 48_000.0).sin_cos();
        let fitted = a * s + b * c;
        residual += (y - fitted).powi(2);
        fitted_energy += fitted * fitted;
    }
    ToneMetrics {
        gain_db: 20.0 * (a.hypot(b) / AMPLITUDE).log10(),
        phase_error_rad: (b.atan2(a) - phase + std::f64::consts::PI).rem_euclid(TAU)
            - std::f64::consts::PI,
        truth_snr_db: 10.0 * (truth_energy / error_energy).log10(),
        fitted_snr_db: 10.0 * (fitted_energy / residual).log10(),
        suppression_db: -10.0
            * (energy / ((samples.len() - 2 * TRIM) as f64 * AMPLITUDE.powi(2) / 2.0)).log10(),
        output_rms: (energy / (samples.len() - 2 * TRIM) as f64).sqrt(),
        truth_error_rms: (error_energy / (samples.len() - 2 * TRIM) as f64).sqrt(),
        fitted_error_rms: (residual / (samples.len() - 2 * TRIM) as f64).sqrt(),
    }
}

fn channel(bytes: &[u8], ch: usize) -> Vec<f64> {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(b[ch * 4..ch * 4 + 4].try_into().unwrap()) as f64)
        .collect()
}

fn finite_samples(bytes: &[u8]) -> bool {
    bytes.len().is_multiple_of(8)
        && bytes
            .as_chunks::<4>()
            .0
            .iter()
            .all(|v| f32::from_le_bytes(*v).is_finite())
}

struct StreamOutput {
    output: Vec<u8>,
    before_flush: usize,
    repeated_flush: usize,
    after_flush_rejected: bool,
}

fn stream(
    input: &[u8],
    rate: u32,
    quality: ResamplerQuality,
    chunk: usize,
) -> Result<StreamOutput, Box<dyn Error>> {
    let mut resampler = Resampler::new(rate, 48_000, 2, quality)?;
    let mut output = Vec::new();
    for bytes in input.chunks(chunk * 8) {
        append(resampler.resample(&frame(bytes, rate))?, &mut output);
    }
    let before_flush = output.len() / 8;
    append(resampler.flush()?, &mut output);
    let repeated_flush = resampler.flush()?.sample_count();
    let after_flush_rejected = resampler.resample(&frame(&[], rate)).is_err();
    Ok(StreamOutput {
        output,
        before_flush,
        repeated_flush,
        after_flush_rejected,
    })
}

fn quality_probe(root: &Path, quality: ResamplerQuality) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(root)?;
    let mut all_pass = true;
    println!(
        "{{\"kind\":\"method\",\"quality\":\"{quality:?}\",\"target_rate\":48000,\"channels\":2,\"amplitude\":0.5,\"tone_phases_rad\":[0,{}],\"steady_trim_output_frames_each_edge\":{TRIM},\"thresholds\":{{\"1khz_truth_snr_min_db\":70,\"passband_abs_gain_max_db\":0.1,\"stopband_min_suppression_db\":60}},\"quality_scope\":\"synthetic software measurements, not listening approval\"}}",
        std::f64::consts::FRAC_PI_4
    );
    for rate in [44_100u32, 48_000, 96_000] {
        let mut cases = vec![("silence".to_owned(), None), ("impulses".to_owned(), None)];
        for frequency in [20u32, 1000, 20_000] {
            cases.push((format!("tone-{frequency}"), Some(frequency)));
        }
        if rate == 96_000 {
            for frequency in [24_000u32, 24_500, 25_000, 25_500, 26_000, 30_000, 40_000] {
                cases.push((format!("tone-{frequency}"), Some(frequency)));
            }
        }
        for (signal, frequency) in cases {
            let n = rate as usize;
            let mut input = Vec::with_capacity(n * 8);
            for i in 0..n {
                for ch in 0..2 {
                    let sample = if let Some(frequency) = frequency {
                        (AMPLITUDE
                            * (TAU * frequency as f64 * i as f64 / rate as f64
                                + ch as f64 * std::f64::consts::FRAC_PI_4)
                                .sin()) as f32
                    } else if signal == "impulses"
                        && ((ch == 0 && i == 0) || (ch == 1 && i == n - 1))
                    {
                        AMPLITUDE as f32
                    } else {
                        0.0
                    };
                    input.extend_from_slice(&sample.to_le_bytes());
                }
            }
            let case = root.join(format!("{rate}-{signal}"));
            fs::create_dir_all(&case)?;
            fs::write(case.join("input.f32le"), &input)?;
            let mut reference = None;
            for chunk in [1024, 1, 8192] {
                let StreamOutput {
                    output,
                    before_flush,
                    repeated_flush,
                    after_flush_rejected,
                } = stream(&input, rate, quality, chunk)?;
                let frames = output.len() / 8;
                let finite = finite_samples(&output);
                let identical = reference.as_ref().is_none_or(|bytes| bytes == &output);
                // The same-rate API is a passthrough and has no finished state
                let pass = frames == 48_000
                    && finite
                    && identical
                    && repeated_flush == 0
                    && (rate == 48_000 || after_flush_rejected);
                all_pass &= pass;
                println!(
                    "{{\"kind\":\"stream\",\"source_rate\":{rate},\"signal\":\"{signal}\",\"chunk_frames\":{chunk},\"input_frames\":{n},\"integer_expected_frames\":48000,\"actual_frames\":{frames},\"before_flush_frames\":{before_flush},\"flush_frames\":{},\"repeated_flush_frames\":{repeated_flush},\"after_flush_rejected\":{after_flush_rejected},\"finite\":{finite},\"bit_identical_to_1024\":{identical},\"status\":\"{}\"}}",
                    frames - before_flush,
                    status(pass)
                );
                fs::write(case.join(format!("chunk-{chunk}.f32le")), &output)?;
                if reference.is_none() {
                    reference = Some(output);
                }
            }
            let output = reference.unwrap();
            for ch in 0..2 {
                let samples = channel(&output, ch);
                let peak = samples.iter().copied().map(f64::abs).fold(0.0, f64::max);
                if !samples.iter().all(|v| v.is_finite()) || samples.len() != 48_000 {
                    continue;
                }
                if let Some(frequency) = frequency {
                    let alias = frequency.min(48_000 - frequency);
                    let passband = frequency <= 20_000;
                    let phase = ch as f64 * std::f64::consts::FRAC_PI_4;
                    let metrics = tone_metrics(
                        &samples,
                        alias as f64,
                        if frequency > 24_000 {
                            std::f64::consts::PI - phase
                        } else {
                            phase
                        },
                    );
                    let assessed = passband || frequency >= 26_000;
                    let pass = if passband {
                        metrics.gain_db.abs() <= 0.1
                            && (frequency != 1000 || metrics.truth_snr_db >= 70.0)
                    } else {
                        metrics.suppression_db >= 60.0
                    };
                    if assessed {
                        all_pass &= pass;
                    }
                    println!(
                        "{{\"kind\":\"tone\",\"source_rate\":{rate},\"frequency_hz\":{frequency},\"alias_hz\":{alias},\"channel\":{ch},\"gain_db\":{},\"phase_error_rad\":{},\"truth_snr_db\":{},\"fitted_snr_db\":{},\"suppression_db\":{},\"steady_output_rms\":{},\"truth_error_rms\":{},\"fitted_error_rms\":{},\"whole_signal_peak\":{peak},\"status\":\"{}\"}}",
                        number(metrics.gain_db),
                        number(metrics.phase_error_rad),
                        number(metrics.truth_snr_db),
                        number(metrics.fitted_snr_db),
                        number(metrics.suppression_db),
                        number(metrics.output_rms),
                        number(metrics.truth_error_rms),
                        number(metrics.fitted_error_rms),
                        if assessed {
                            status(pass)
                        } else {
                            "OBSERVED_TRANSITION"
                        }
                    );
                } else if signal == "silence" {
                    all_pass &= peak == 0.0;
                    println!(
                        "{{\"kind\":\"silence\",\"source_rate\":{rate},\"channel\":{ch},\"whole_signal_peak\":{peak},\"status\":\"{}\"}}",
                        status(peak == 0.0)
                    );
                } else {
                    let input_index = if ch == 0 { 0 } else { n - 1 };
                    let expected = input_index as f64 * 48_000.0 / rate as f64;
                    let peak_index = samples
                        .iter()
                        .enumerate()
                        .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
                        .unwrap()
                        .0;
                    let energy: f64 = samples.iter().map(|v| v * v).sum();
                    let centroid = samples
                        .iter()
                        .enumerate()
                        .map(|(i, v)| i as f64 * v * v)
                        .sum::<f64>()
                        / energy;
                    let sum: f64 = samples.iter().sum();
                    all_pass &= energy > 0.0;
                    println!(
                        "{{\"kind\":\"impulse\",\"source_rate\":{rate},\"channel\":{ch},\"input_index\":{input_index},\"expected_output_position\":{expected},\"peak_index\":{peak_index},\"peak_position_error_frames\":{},\"energy_centroid\":{},\"centroid_error_frames\":{},\"sum\":{sum},\"energy\":{energy},\"whole_signal_peak\":{peak},\"first_16_samples\":{:?},\"last_16_samples\":{:?},\"status\":\"{}\"}}",
                        peak_index as f64 - expected,
                        number(centroid),
                        number(centroid - expected),
                        &samples[..16],
                        &samples[samples.len() - 16..],
                        if energy > 0.0 {
                            "OBSERVED_BOUNDARY"
                        } else {
                            "FAIL_ZERO_ENERGY"
                        }
                    );
                }
            }
        }
        for n in [0usize, 1, 2, 3, 95, 96, 97, 127, 128, 129, 1023, 1024, 1025] {
            let input = vec![0u8; n * 8];
            let expected = (n as u64 * 48_000).div_ceil(rate as u64);
            for chunk in [1, 1024, 8192] {
                let StreamOutput {
                    output,
                    repeated_flush,
                    after_flush_rejected,
                    ..
                } = stream(&input, rate, quality, chunk)?;
                let count = output.len() / 8;
                let finite = finite_samples(&output);
                let pass = count as u64 == expected
                    && finite
                    && repeated_flush == 0
                    && (rate == 48_000 || after_flush_rejected);
                all_pass &= pass;
                println!(
                    "{{\"kind\":\"length\",\"source_rate\":{rate},\"input_frames\":{n},\"chunk_frames\":{chunk},\"integer_expected_frames\":{expected},\"actual_frames\":{count},\"finite\":{finite},\"repeated_flush_frames\":{repeated_flush},\"after_flush_rejected\":{after_flush_rejected},\"status\":\"{}\"}}",
                    status(pass)
                );
            }
        }
    }
    println!(
        "{{\"kind\":\"summary\",\"quality\":\"{quality:?}\",\"status\":\"{}\"}}",
        status(all_pass)
    );
    if all_pass {
        Ok(())
    } else {
        Err("quality matrix has failed checks; retain measured results".into())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("quality") {
        let [_, output, quality] = args.as_slice() else {
            return Err("usage: preflight quality OUTPUT_DIR High|Best".into());
        };
        let quality = match quality.as_str() {
            "High" => ResamplerQuality::High,
            "Best" => ResamplerQuality::Best,
            _ => return Err("quality must be High or Best".into()),
        };
        return quality_probe(Path::new(output), quality);
    }
    let [output] = args.as_slice() else {
        return Err("usage: preflight OUTPUT_DIR".into());
    };
    let root = PathBuf::from(output);
    fs::create_dir_all(&root)?;
    for (rate, cases) in [
        (44_100u32, [(0usize, 0u64), (1, 2), (44_100, 48_000)]),
        (96_000u32, [(0usize, 0u64), (1, 1), (96_000, 48_000)]),
    ] {
        for (n, expected) in cases {
            let mut input = Vec::with_capacity(n * 8);
            for i in 0..n {
                let left =
                    (0.125 * (std::f64::consts::TAU * 440.0 * i as f64 / rate as f64).sin()) as f32;
                let right: f32 = if i == 0 {
                    0.5
                } else if i == n - 1 {
                    -0.5
                } else {
                    0.0
                };
                input.extend_from_slice(&left.to_le_bytes());
                input.extend_from_slice(&right.to_le_bytes());
            }
            let mut r = Resampler::new(rate, 48_000, 2, ResamplerQuality::High)?;
            let estimate = r.output_sample_count(n);
            let mut output = Vec::new();
            for chunk in input.chunks(1024 * 8) {
                append(r.resample(&frame(chunk, rate))?, &mut output);
            }
            let before_flush = output.len() / 8;
            append(r.flush()?, &mut output);
            let count = output.len() / 8;
            let repeated_flush = r.flush()?.sample_count();
            let after_flush_rejected = r.resample(&frame(&[], rate)).is_err();
            assert_eq!((n as u64 * 48_000).div_ceil(rate as u64), expected);
            assert_eq!(count as u64, expected);
            assert_eq!(repeated_flush, 0);
            assert!(after_flush_rejected);
            let samples: Vec<f32> = output
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| f32::from_le_bytes(*v))
                .collect();
            let nonfinite = samples.iter().filter(|v| !v.is_finite()).count();
            assert_eq!(nonfinite, 0);
            let peak = samples.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
            let stem = format!("{rate}-{n}");
            fs::write(root.join(format!("{stem}-input.f32le")), input)?;
            fs::write(root.join(format!("{stem}-output.f32le")), output)?;
            println!(
                "{{\"source_rate\":{rate},\"target_rate\":48000,\"channels\":2,\"quality\":\"High\",\"max_chunk_frames\":1024,\"input_frames\":{n},\"estimated_frames\":{estimate},\"integer_expected_frames\":{expected},\"before_flush_frames\":{before_flush},\"flush_frames\":{},\"actual_frames\":{count},\"repeated_flush_frames\":{repeated_flush},\"after_flush_rejected\":{after_flush_rejected},\"nonfinite\":{nonfinite},\"peak\":{peak}}}",
                count - before_flush
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytic_metrics_detect_gain_phase_and_noise() {
        let input: Vec<_> = (0..48_000)
            .map(|i| {
                let t = TAU * i as f64 / 48_000.0;
                AMPLITUDE * (1000.0 * t + 0.2).sin() + 0.05 * (2000.0 * t).sin()
            })
            .collect();
        let m = tone_metrics(&input, 1000.0, 0.2);
        assert!(m.gain_db.abs() < 1e-10);
        assert!(m.phase_error_rad.abs() < 1e-10);
        assert!((m.truth_snr_db - 20.0).abs() < 1e-10);
        assert!((m.fitted_snr_db - 20.0).abs() < 1e-10);
        let scaled: Vec<_> = input.iter().map(|v| v * 0.5).collect();
        assert!((tone_metrics(&scaled, 1000.0, 0.0).gain_db + 6.020599913279624).abs() < 1e-10);
        assert!((tone_metrics(&scaled, 1000.0, 0.0).phase_error_rad - 0.2).abs() < 1e-10);
        let stop: Vec<_> = (0..48_000)
            .map(|i| AMPLITUDE * 0.0001 * (TAU * 8000.0 * i as f64 / 48_000.0).sin())
            .collect();
        assert!((tone_metrics(&stop, 8000.0, 0.0).suppression_db - 80.0).abs() < 1e-10);
        assert_eq!(number(f64::INFINITY), "null");
        assert_eq!(number(f64::NAN), "null");
    }
}
