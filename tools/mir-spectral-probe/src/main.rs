use oximedia_mir::onset_strength::{OnsetDetector, OnsetFunction};
use serde_json::{Value, json};
use std::{fs, path::Path, process::ExitCode, time::Instant};

#[allow(dead_code)]
mod baseline {
    pub fn score(truth: &[usize], predicted: &[usize]) -> Value {
        metrics(truth, predicted)
    }
    include!("../../mir-onset-probe/src/main.rs");
}

const RATE: usize = 48_000;
const WINDOW: usize = 128;
const HOP: usize = 64;
const THRESHOLD_FACTOR: f32 = 1.5;

fn detector() -> OnsetDetector {
    OnsetDetector::new(OnsetFunction::HfcEnergy, RATE as f32, HOP, THRESHOLD_FACTOR)
}

fn spectrum(samples: &[f32]) -> Vec<f32> {
    let input: Vec<_> = samples
        .iter()
        .zip(oxifft::streaming::hamming::<f32>(samples.len()))
        .map(|(&sample, window)| oxifft::Complex::new(sample * window, 0.0))
        .collect();
    oxifft::fft(&input).iter().map(|c| c.norm()).collect()
}

fn spans(frames: usize) -> Vec<(usize, usize)> {
    if frames == 0 {
        return vec![];
    }
    if frames < WINDOW {
        return vec![(0, frames)];
    }
    let mut spans: Vec<_> = (0..=frames - WINDOW)
        .step_by(HOP)
        .map(|start| (start, start + WINDOW))
        .collect();
    if spans.last().unwrap().1 < frames {
        spans.push((frames - WINDOW, frames));
    }
    spans
}

fn center(start: usize, end: usize) -> usize {
    // Round the center of the actual discrete sample support upward at a half frame
    start + (end - start) / 2
}

fn analyze(samples: &[f32]) -> Value {
    let started = Instant::now();
    let spans = spans(samples.len());
    let mut detector = detector();
    for &(start, end) in &spans {
        let magnitudes = spectrum(&samples[start..end]);
        // Mirrored negative-frequency bins would cancel most HFC frequency weighting
        detector.add_frame(&magnitudes[..=magnitudes.len() / 2], &[]);
    }
    detector.pick_onsets();
    let envelope: Vec<_> = detector
        .frames()
        .iter()
        .zip(&spans)
        .map(|(frame, &(start, end))| {
            json!({"analysis_index": frame.frame_index, "support_start": start,
            "support_end_exclusive": end, "coordinate_frame": center(start, end),
            "api_time_seconds": frame.time_s, "hfc": frame.value, "peak": frame.onset_flag})
        })
        .collect();
    let frames: Vec<_> = detector
        .onset_frames()
        .iter()
        .map(|frame| {
            let (start, end) = spans[frame.frame_index];
            center(start, end)
        })
        .collect();
    assert!(frames.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(frames.iter().all(|&frame| frame < samples.len()));
    // ponytail: short windows trade spectral resolution for time resolution; music gates remain separate
    json!({"predicted_frames": frames, "envelope": envelope, "elapsed_seconds": started.elapsed().as_secs_f64()})
}

fn frequency_weight_check() -> Value {
    let mut rows = Vec::new();
    for bin in [4, 20] {
        let samples: Vec<_> = (0..WINDOW)
            .map(|i| {
                (std::f64::consts::TAU * bin as f64 * i as f64 / WINDOW as f64).cos() as f32 * 0.1
            })
            .collect();
        let energy: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
        let magnitudes = spectrum(&samples);
        let hfc = detector().add_frame(&magnitudes[..=WINDOW / 2], &[]);
        let mirrored_hfc = detector().add_frame(&magnitudes, &[]);
        rows.push(json!({"bin": bin, "frequency_hz": bin * RATE / WINDOW,
            "pcm_square_sum": energy, "one_sided_hfc": hfc, "full_mirrored_hfc": mirrored_hfc}));
    }
    let low = &rows[0];
    let high = &rows[1];
    let energy_delta =
        (low["pcm_square_sum"].as_f64().unwrap() - high["pcm_square_sum"].as_f64().unwrap()).abs();
    let ratio = high["one_sided_hfc"].as_f64().unwrap() / low["one_sided_hfc"].as_f64().unwrap();
    assert!(
        energy_delta < 1e-6 && ratio > 3.5,
        "HFC must distinguish equal-energy frequencies"
    );
    json!({"status": "PASS", "pcm_energy_difference": energy_delta, "high_to_low_hfc_ratio": ratio, "rows": rows})
}

fn read_channel(
    path: &Path,
    channels: usize,
    channel: usize,
    frames: usize,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    if bytes.len() != frames * channels * 4 {
        return Err(format!("{}: unexpected PCM size", path.display()).into());
    }
    let samples: Vec<_> = bytes
        .chunks_exact(channels * 4)
        .map(|frame| f32::from_le_bytes(frame[channel * 4..channel * 4 + 4].try_into().unwrap()))
        .collect();
    if samples.iter().any(|s| !s.is_finite()) {
        return Err(format!("{}: non-finite PCM", path.display()).into());
    }
    Ok(samples)
}

fn evaluate(name: &str, samples: &[f32], truth: &[usize]) -> Value {
    let mut result = analyze(samples);
    let predicted: Vec<usize> = serde_json::from_value(result["predicted_frames"].clone()).unwrap();
    result["metrics"] = baseline::score(truth, &predicted);
    result["case"] = json!(name);
    result["sample_frames"] = json!(samples.len());
    result["truth_frames"] = json!(truth);
    result
}

fn run(clean: &Path, controls: &Path, output: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    fs::create_dir(output)?;
    let weight_check = frequency_weight_check();
    let mut results = Vec::new();
    for name in [
        "pulse-120",
        "pulse-137.5",
        "first-last",
        "silence",
        "opposite-polarity",
    ] {
        let truth: Value = serde_json::from_slice(&fs::read(clean.join(name).join("truth.json"))?)?;
        let labels: Vec<usize> =
            serde_json::from_value(truth["onset_frames_each_channel"].clone())?;
        for channel in 0..2 {
            let samples = read_channel(
                &clean.join(name).join("fixture.f32le"),
                2,
                channel,
                truth["frames"].as_u64().unwrap() as usize,
            )?;
            results.push(evaluate(
                &format!("{name}/channel-{channel}"),
                &samples,
                &labels,
            ));
        }
    }
    let original_controls: Value =
        serde_json::from_slice(&fs::read(controls.join("report.json"))?)?;
    let mut holdout = Vec::new();
    for name in [
        "impulse-phase-scan",
        "quiet-impulse-phase-scan",
        "partial-hop-last-sample",
        "single-sample-boundary",
        "constant-level-one-boundary-attack",
        "stationary-noise-no-interior-attack",
        "continuous-tone-one-boundary-attack",
        "high-tone-one-boundary-attack",
    ] {
        let case = original_controls["holdout"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["case"] == name)
            .ok_or("missing holdout case")?;
        let labels: Vec<usize> = serde_json::from_value(case["truth_frames"].clone())?;
        let samples = read_channel(
            &controls.join(format!("{name}.f32le")),
            1,
            0,
            case["sample_frames"].as_u64().unwrap() as usize,
        )?;
        holdout.push(evaluate(name, &samples, &labels));
    }
    let passed = results
        .iter()
        .chain(&holdout)
        .all(|case| case["metrics"]["status"] == "PASS");
    let report = json!({
        "status": if passed {"PASS"} else {"FAIL"}, "algorithm": "oxifft FFT + one-sided Hamming magnitudes + oximedia_mir::onset_strength::HfcEnergy",
        "scope": "isolated research candidate, not production MIR admission", "sample_rate": RATE,
        "window_frames": WINDOW, "hop_frames": HOP, "threshold_factor": THRESHOLD_FACTOR,
        "time_origin": "original PCM frame zero; no padding, trim, truth-driven tuning or translated outputs",
        "coordinate_mapping": "ceil((support_start + support_end_exclusive - 1)/2); API time_s is recorded but not treated as PCM time",
        "tail_policy": "append a complete real window ending at PCM EOF when regular hops do not reach EOF",
        "short_file_policy": "one actual short window, no invented samples; upstream single-frame picker failure preserved",
        "frequency_weight_check": weight_check, "shared_metrics_source": "tools/mir-onset-probe/src/main.rs",
        "gate": "unchanged: all truth matched once, no extras, inclusive 480-frame tolerance, median absolute error <= 96 frames",
        "confidence": "NOT PROVIDED", "source_clean": clean, "source_controls": controls,
        "real_music": "NOT RUN", "canonical_ogg": "NOT RUN", "results": results, "holdout": holdout
    });
    fs::write(
        output.join("report.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    println!(
        "{}: {}",
        output.join("report.json").display(),
        report["status"]
    );
    Ok(passed)
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [clean, controls, output] = args.as_slice() else {
        eprintln!(
            "Usage: cocobeat-mir-spectral-probe <original-clean-directory> <diagnostic-controls-directory> <new-output-directory>"
        );
        return ExitCode::FAILURE;
    };
    match run(Path::new(clean), Path::new(controls), Path::new(output)) {
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
    fn native_frequency_weight_and_real_support_coordinates() {
        assert_eq!(frequency_weight_check()["status"], "PASS");
        assert_eq!(spans(0), vec![]);
        assert_eq!(spans(1), vec![(0, 1)]);
        assert_eq!(spans(128), vec![(0, 128)]);
        assert_eq!(spans(129), vec![(0, 128), (1, 129)]);
        assert_eq!(
            spans(257),
            vec![(0, 128), (64, 192), (128, 256), (129, 257)]
        );
        assert_eq!(center(129, 257), 193);
        assert_eq!(analyze(&[0.5])["predicted_frames"], json!([]));
        assert_eq!(analyze(&[0.0; 257])["predicted_frames"], json!([]));
    }
}
