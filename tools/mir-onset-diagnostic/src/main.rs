use oximedia_mir::{
    beat,
    onset_peak::{OnsetPeakConfig, OnsetPeakDetector},
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::ExitCode, time::Instant};

// Reuse the frozen matching rules, including their independent regression check
#[allow(dead_code)]
mod baseline {
    pub fn score(truth: &[usize], predicted: &[usize]) -> Value {
        metrics(truth, predicted)
    }
    include!("../../mir-onset-probe/src/main.rs");
}

const RATE: usize = 48_000;

fn detector(hop: usize) -> OnsetPeakDetector {
    OnsetPeakDetector::new(OnsetPeakConfig {
        hop_size: hop,
        sample_rate: RATE as f32,
        min_peak_distance: 384 / hop,
        ..Default::default()
    })
}

fn block_energy(samples: &[f32]) -> f64 {
    samples.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / samples.len() as f64
}

fn bounded_energy_peaks(samples: &[f32], hop: usize) -> Vec<usize> {
    let detector = detector(hop);
    let mut flux = detector.spectral_flux(samples);
    // Upstream ignores a partial final hop when at least one complete hop exists
    let tail = samples.len() / hop * hop;
    if tail >= hop && tail < samples.len() {
        let previous = block_energy(&samples[tail - hop..tail]);
        flux.push((block_energy(&samples[tail..]) - previous).max(0.0));
    }
    let threshold = detector.adaptive_threshold(&flux);
    let mut peaks = Vec::new();
    for (i, &value) in flux.iter().enumerate() {
        let left = i.checked_sub(1).map_or(0.0, |j| flux[j]);
        let right = flux.get(i + 1).copied().unwrap_or(0.0);
        if value > left
            && value >= right
            && value > threshold[i]
            && peaks.last().is_none_or(|&last| i * hop - last >= 384)
        {
            peaks.push(i * hop);
        }
    }
    // ponytail: hop RMS loses spectral evidence; keep this isolated until music/noise gates pass
    peaks
}

fn evaluate(name: &str, samples: &[f32], truth: &[usize]) -> Value {
    let mut runs = Vec::new();
    let started = Instant::now();
    let seconds = beat::OnsetDetector::new(RATE as f32, 1024, 128)
        .detect(samples)
        .unwrap();
    let frames: Vec<_> = seconds
        .iter()
        .map(|&s| (f64::from(s) * RATE as f64).round() as usize)
        .collect();
    let windows: Vec<_> = frames
        .iter()
        .map(|&start| {
            let end = (start + 1024).min(samples.len());
            let attack = (start..end).max_by(|&a, &b| {
                let delta = |i: usize| {
                    f64::from(samples[i]).powi(2)
                        - i.checked_sub(1)
                            .map_or(0.0, |j| f64::from(samples[j]).powi(2))
                };
                delta(a).total_cmp(&delta(b))
            });
            json!({"start": start, "end_exclusive": end, "maximum_energy_difference_frame": attack})
        })
        .collect();
    runs.push(
        json!({"algorithm": "beat-upstream-1024-128", "seconds": seconds, "frames": frames,
        "elapsed_seconds": started.elapsed().as_secs_f64(), "peak_support_windows": windows,
        "metrics": baseline::score(truth, &frames)}),
    );
    for hop in [128, 64] {
        for fixed in [false, true] {
            let started = Instant::now();
            let frames = if fixed {
                bounded_energy_peaks(samples, hop)
            } else {
                detector(hop)
                    .detect(samples)
                    .iter()
                    .map(|p| p.frame * hop)
                    .collect()
            };
            assert!(
                frames.windows(2).all(|p| p[0] < p[1]) && frames.iter().all(|&f| f < samples.len())
            );
            runs.push(json!({"algorithm": format!("energy-{}-{hop}", if fixed {"bounded"} else {"upstream"}),
                "frames": frames, "elapsed_seconds": started.elapsed().as_secs_f64(),
                "metrics": baseline::score(truth, &frames)}));
        }
    }
    json!({"case": name, "sample_frames": samples.len(), "truth_frames": truth, "runs": runs})
}

fn write_json(path: &Path, value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}

fn run(source: &Path, output: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    fs::create_dir(output)?;
    let mut results = Vec::new();
    for name in [
        "pulse-120",
        "pulse-137.5",
        "first-last",
        "silence",
        "opposite-polarity",
    ] {
        let truth: Value =
            serde_json::from_slice(&fs::read(source.join(name).join("truth.json"))?)?;
        let truth_frames: Vec<usize> =
            serde_json::from_value(truth["onset_frames_each_channel"].clone())?;
        let pcm = fs::read(source.join(name).join("fixture.f32le"))?;
        let (frames, remainder) = pcm.as_chunks::<8>();
        assert!(remainder.is_empty() && frames.len() == truth["frames"].as_u64().unwrap() as usize);
        for channel in 0..2 {
            let samples: Vec<_> = frames
                .iter()
                .map(|f| f32::from_le_bytes(f[channel * 4..channel * 4 + 4].try_into().unwrap()))
                .collect();
            assert!(samples.iter().all(|s| s.is_finite()));
            results.push(evaluate(
                &format!("{name}/channel-{channel}"),
                &samples,
                &truth_frames,
            ));
        }
    }
    // Independent holdout definitions: impulses cover every phase and the truncated tail
    let mut holdout = Vec::new();
    for (name, gain) in [
        ("impulse-phase-scan", 0.5),
        ("quiet-impulse-phase-scan", 0.005),
    ] {
        let truth: Vec<_> = (0..128).map(|phase| 4096 + phase * 2048 + phase).collect();
        let mut samples = vec![0.0; 270_001];
        for &frame in &truth {
            samples[frame] = gain;
        }
        holdout.push((name, samples, truth));
    }
    let mut partial = vec![0.0; 48_001];
    partial[48_000] = 0.5;
    holdout.push(("partial-hop-last-sample", partial, vec![48_000]));
    holdout.push(("single-sample-boundary", vec![0.5], vec![0]));
    holdout.push((
        "constant-level-one-boundary-attack",
        vec![0.1; 48_001],
        vec![0],
    ));
    // These negative controls expose RMS-only false positives, not musical ground truth
    let mut state = 0x5eed_u32;
    let noise: Vec<_> = (0..48_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (f64::from(state) / f64::from(u32::MAX) * 0.02 - 0.01) as f32
        })
        .collect();
    holdout.push(("stationary-noise-no-interior-attack", noise, vec![0]));
    let tone: Vec<_> = (0..48_000)
        .map(|i| (std::f64::consts::TAU * 440.0 * i as f64 / RATE as f64).cos() as f32 * 0.1)
        .collect();
    holdout.push(("continuous-tone-one-boundary-attack", tone, vec![0]));
    let high_tone: Vec<_> = (0..48_000)
        .map(|i| (std::f64::consts::TAU * 9973.0 * i as f64 / RATE as f64).cos() as f32 * 0.1)
        .collect();
    holdout.push(("high-tone-one-boundary-attack", high_tone, vec![0]));
    let mut holdout_results = Vec::new();
    for (name, samples, truth) in holdout {
        let bytes: Vec<_> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
        let path = output.join(format!("{name}.f32le"));
        fs::write(&path, bytes)?;
        let bytes = fs::read(path)?;
        let samples: Vec<_> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| f32::from_le_bytes(*v))
            .collect();
        holdout_results.push(evaluate(name, &samples, &truth));
    }
    let passed = results.iter().chain(&holdout_results).all(|case| {
        case["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|run| run["metrics"]["status"] == "PASS")
    });
    let report = json!({
        "status": if passed { "PASS" } else { "FAIL" },
        "scope": "diagnosis only; original five-case failure retained; no production adapter",
        "time_origin": "original PCM frame zero; no padding, trim, output translation or fabricated confidence",
        "sample_rate": RATE, "shared_metrics_source": "tools/mir-onset-probe/src/main.rs",
        "upstream_dependency": "oximedia-mir 0.2.1, Cargo.lock", "source_fixture_directory": source,
        "candidate": "public upstream RMS flux and adaptive median; include boundary peaks and partial tail; timestamps are block starts",
        "candidate_hops": [128, 64], "minimum_separation_samples": 384,
        "median_window_blocks": 11, "median_sensitivity": 1.5,
        "gate": "unchanged: all truth matched once, no extras, inclusive 480-frame tolerance, median absolute error <= 96 frames",
        "holdout_license": "CC0-1.0", "holdout_labels": "impulses are literal generated sample positions; sustained signals have only a boundary attack by construction",
        "music_quality": "NOT RUN", "canonical_ogg": "NOT RUN", "results": results, "holdout": holdout_results
    });
    write_json(&output.join("report.json"), &report)?;
    println!("{}", output.join("report.json").display());
    Ok(passed)
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [source, output] = args.as_slice() else {
        eprintln!(
            "Usage: cocobeat-mir-onset-diagnostic <original-fixtures-directory> <new-output-directory>"
        );
        return ExitCode::FAILURE;
    };
    match run(Path::new(source), Path::new(output)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn energy_boundaries_and_unaligned_impulses() {
        for len in [1, 63, 64, 65, 127, 128, 129, 1025] {
            let mut samples = vec![0.0; len];
            samples[len - 1] = 0.5;
            // A <= 2-block clip has no median context; keep that quality failure visible
            let expected = if len <= 128 {
                vec![]
            } else {
                vec![(len - 1) / 64 * 64]
            };
            assert_eq!(bounded_energy_peaks(&samples, 64), expected);
            assert!(bounded_energy_peaks(&vec![0.0; len], 64).is_empty());
        }
        let mut samples = vec![0.0; 4096];
        for frame in [0, 1023, 2047, 4095] {
            samples[frame] = -0.5;
        }
        assert_eq!(bounded_energy_peaks(&samples, 64), vec![0, 960, 1984, 4032]);
    }
}
