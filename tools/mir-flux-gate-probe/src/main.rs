use oximedia_mir::onset_strength::{OnsetDetector, OnsetFunction};
use serde_json::{Value, json};
use std::{fs, path::Path, process::ExitCode, time::Instant};

#[allow(dead_code)]
mod frozen {
    pub use previous::{load, magnitudes, score, windows};
    pub fn native(samples: &[f32]) -> Value {
        analyze(samples, OnsetFunction::SpectralFlux)
    }
    include!("../../mir-flux-probe/src/main.rs");
}

const RADIUS: usize = 8;
const MINIMUM_CHANGE: f64 = 0.5;
const DECLARATION: &str = include_str!(
    "../../../testdata/synthetic/mir-flux-gate-probe/declared-experiment-20261003.json"
);

const NEXT_CONTROLS: &str = include_str!(
    "../../../testdata/synthetic/mir-flux-gate-probe/declared-next-controls-20261003.json"
);

const BAND_DECLARATION: &str = include_str!(
    "../../../testdata/synthetic/mir-flux-gate-probe/declared-band-local-20261003.json"
);
const BANDS: [(usize, usize); 6] = [(0, 2), (2, 4), (4, 8), (8, 16), (16, 32), (32, 65)];
const LOCAL_RADIUS: usize = 2;

fn evidence(strength: f64, mass: &[f64], index: usize) -> (f64, f64) {
    let lo = index.saturating_sub(RADIUS);
    let hi = (index + RADIUS + 1).min(mass.len());
    let denominator = mass[lo..hi].iter().copied().fold(0.0, f64::max);
    let normalized = if denominator > 0.0 {
        strength / denominator
    } else {
        0.0
    };
    (denominator, normalized)
}

fn analyze(samples: &[f32]) -> Value {
    let started = Instant::now();
    let mut native = frozen::native(samples);
    // Reuse the frozen FFT; a second pass keeps the original analyzer intact
    let mass: Vec<f64> = frozen::windows(samples.len())
        .iter()
        .map(|&(start, end)| {
            let magnitudes = frozen::magnitudes(&samples[start..end]);
            magnitudes[..=magnitudes.len() / 2]
                .iter()
                .map(|&value| f64::from(value))
                .sum()
        })
        .collect();
    let mut accepted = Vec::new();
    for (index, frame) in native["envelope"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        let (denominator, normalized) = evidence(frame["strength"].as_f64().unwrap(), &mass, index);
        let keep = frame["peak"] == true && denominator > 0.0 && normalized >= MINIMUM_CHANGE;
        if keep {
            accepted.push(frame["coordinate_frame"].as_u64().unwrap() as usize);
        }
        frame["one_sided_spectrum_mass"] = json!(mass[index]);
        frame["context_maximum_spectrum_mass"] = json!(denominator);
        frame["normalized_positive_change"] = json!(normalized);
        frame["accepted"] = json!(keep);
    }
    native["accepted_frames"] = json!(accepted);
    native["adapter_elapsed_seconds"] = json!(started.elapsed().as_secs_f64());
    native
}

fn verify_native(current: &Value, reference: &Value) {
    assert_eq!(current["predicted_frames"], reference["predicted_frames"]);
    let current = current["envelope"].as_array().unwrap();
    let reference = reference["envelope"].as_array().unwrap();
    assert_eq!(current.len(), reference.len());
    for (current, reference) in current.iter().zip(reference) {
        for key in [
            "analysis_index",
            "support_start",
            "support_end_exclusive",
            "coordinate_frame",
            "peak",
        ] {
            assert_eq!(current[key], reference[key], "native field changed: {key}");
        }
        for key in ["strength", "api_time_seconds"] {
            assert_eq!(
                (current[key].as_f64().unwrap() as f32).to_bits(),
                (reference[key].as_f64().unwrap() as f32).to_bits(),
                "native f32 field changed: {key}"
            );
        }
    }
}

fn run(reference: &Path, output: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    fs::create_dir(output)?;
    fs::write(output.join("declaration.json"), DECLARATION)?;
    let old: Value = serde_json::from_slice(&fs::read(reference)?)?;
    let mut cases = Vec::new();
    for group in ["existing", "new_controls"] {
        for case in old[group].as_array().unwrap() {
            let name = case["case"].as_str().unwrap();
            let (path, channels, channel) =
                if let Some((name, channel)) = name.split_once("/channel-") {
                    (
                        Path::new(old["source_clean"].as_str().unwrap())
                            .join(name)
                            .join("fixture.f32le"),
                        2,
                        channel.parse()?,
                    )
                } else if group == "existing" {
                    (
                        Path::new(old["source_controls"].as_str().unwrap())
                            .join(format!("{name}.f32le")),
                        1,
                        0,
                    )
                } else {
                    (
                        reference.parent().unwrap().join(format!("{name}.f32le")),
                        1,
                        0,
                    )
                };
            let samples = frozen::load(
                &path,
                channels,
                channel,
                case["sample_frames"].as_u64().unwrap() as usize,
            )?;
            let mut result = analyze(&samples);
            let native_reference = case["runs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|run| run["algorithm"] == "spectral_flux")
                .unwrap();
            verify_native(&result, native_reference);
            let native: Vec<usize> = serde_json::from_value(result["predicted_frames"].clone())?;
            let accepted: Vec<usize> = serde_json::from_value(result["accepted_frames"].clone())?;
            assert!(accepted.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(
                accepted
                    .iter()
                    .all(|frame| native.binary_search(frame).is_ok())
            );
            // Labels enter only after both native analysis and the fixed gate
            let truth: Option<Vec<usize>> = serde_json::from_value(case["truth_frames"].clone())?;
            let native_metrics = truth
                .as_ref()
                .map_or(Value::Null, |truth| frozen::score(truth, &native));
            // Compare both through the same JSON f64 parser, without a tolerance
            let parsed_metrics: Value =
                serde_json::from_str(&serde_json::to_string(&native_metrics)?)?;
            assert_eq!(parsed_metrics, native_reference["metrics"]);
            result["metrics"] = truth
                .as_ref()
                .map_or(Value::Null, |truth| frozen::score(truth, &accepted));
            result["native_metrics"] = native_metrics;
            result["case"] = case["case"].clone();
            result["group"] = json!(group);
            result["pcm_path"] = json!(path);
            result["channel"] = json!(channel);
            result["sample_frames"] = case["sample_frames"].clone();
            result["truth_frames"] = case["truth_frames"].clone();
            cases.push(result);
        }
    }
    assert_eq!(cases.len(), 25);
    assert_eq!(
        cases
            .iter()
            .filter(|case| !case["metrics"].is_null())
            .count(),
        24
    );
    let passed = cases
        .iter()
        .all(|case| case["metrics"].is_null() || case["metrics"]["status"] == "PASS");
    let report = json!({"status": if passed {"PASS"} else {"FAIL"}, "declaration": serde_json::from_str::<Value>(DECLARATION)?,
        "native_reproduction": "PASS: every prediction, peak flag, support, native f32 value and metric matches frozen Flux",
        "prediction_subset": "PASS: filter only, identical original coordinates", "confidence": null,
        "reference": reference, "cases": cases});
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

fn generate(control: &Value) -> Vec<f32> {
    let frames = control["frames"].as_u64().unwrap() as usize;
    let mut samples = vec![0.0_f64; frames];
    for voice in control["voices"].as_array().unwrap() {
        let start = voice["start_frame"].as_u64().unwrap() as usize;
        let frequency = voice["frequency_hz"].as_f64().unwrap();
        let amplitude = voice["amplitude"].as_f64().unwrap();
        let duration = voice["duration_frames"]
            .as_u64()
            .map_or(frames - start, |value| value as usize);
        for (age, sample) in samples[start..start + duration].iter_mut().enumerate() {
            let envelope = match control["generator"].as_str().unwrap() {
                "burst" => (-((age as f64) / voice["decay_frames"].as_f64().unwrap())).exp(),
                "hard_tone" => 1.0,
                "slow_tone" => {
                    let rise = voice["rise_frames"].as_u64().unwrap() as usize;
                    if age < rise {
                        0.5 * (1.0 - (std::f64::consts::PI * age as f64 / rise as f64).cos())
                    } else {
                        1.0
                    }
                }
                other => panic!("Undeclared generator {other}"),
            };
            *sample += amplitude
                * envelope
                * (std::f64::consts::TAU * frequency * age as f64 / 48_000.0).cos();
        }
    }
    samples.into_iter().map(|sample| sample as f32).collect()
}

fn run_controls(output: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    fs::create_dir(output)?;
    fs::write(output.join("declaration.json"), NEXT_CONTROLS)?;
    let declaration: Value = serde_json::from_str(NEXT_CONTROLS)?;
    let mut cases = Vec::new();
    for control in declaration["controls"].as_array().unwrap() {
        let name = control["name"].as_str().unwrap();
        let samples = generate(control);
        let path = output.join(format!("{name}.f32le"));
        fs::write(
            &path,
            samples
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect::<Vec<_>>(),
        )?;
        let samples = frozen::load(&path, 1, 0, samples.len())?;
        let mut result = analyze(&samples);
        let native: Vec<usize> = serde_json::from_value(result["predicted_frames"].clone())?;
        let accepted: Vec<usize> = serde_json::from_value(result["accepted_frames"].clone())?;
        assert!(accepted.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            accepted
                .iter()
                .all(|frame| native.binary_search(frame).is_ok())
        );
        // Labels enter only after the same frozen analyzer and fixed gate
        let truth: Option<Vec<usize>> = serde_json::from_value(control["truth_frames"].clone())?;
        result["native_metrics"] = truth
            .as_ref()
            .map_or(Value::Null, |truth| frozen::score(truth, &native));
        result["metrics"] = truth
            .as_ref()
            .map_or(Value::Null, |truth| frozen::score(truth, &accepted));
        result["case"] = json!(name);
        result["pcm_path"] = json!(path);
        result["sample_frames"] = json!(samples.len());
        result["truth_frames"] = control["truth_frames"].clone();
        result["uncertain_interval_frames_inclusive"] =
            control["uncertain_interval_frames_inclusive"].clone();
        cases.push(result);
    }
    let passed = cases
        .iter()
        .all(|case| case["metrics"].is_null() || case["metrics"]["status"] == "PASS");
    let report = json!({"status": if passed {"PASS"} else {"FAIL"}, "declaration": declaration,
        "scope": "New engineering controls only; earlier FAIL cases remain authoritative and are not replaced",
        "prediction_subset": "PASS: filter only, identical original coordinates", "confidence": null,
        "real_music": "NOT RUN", "canonical_ogg": "NOT RUN", "human_labels": "NOT RUN", "cases": cases});
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

fn local_peak(values: &[f64], index: usize) -> bool {
    let value = values[index];
    let lo = index.saturating_sub(LOCAL_RADIUS);
    let hi = (index + LOCAL_RADIUS + 1).min(values.len());
    value >= MINIMUM_CHANGE
        && values[lo..index].iter().all(|&other| other < value)
        && values[index + 1..hi].iter().all(|&other| other <= value)
}

fn analyze_bands(samples: &[f32]) -> Value {
    let started = Instant::now();
    let windows = frozen::windows(samples.len());
    let mut detectors: [_; 6] =
        std::array::from_fn(|_| OnsetDetector::new(OnsetFunction::SpectralFlux, 48_000.0, 64, 1.5));
    let mut strengths = Vec::new();
    let mut masses = Vec::new();
    for &(start, end) in &windows {
        let magnitudes = frozen::magnitudes(&samples[start..end]);
        let bins = magnitudes.len() / 2 + 1;
        let mut strength = [0.0; 6];
        let mut mass = [0.0; 6];
        for (band, &(lo, hi)) in BANDS.iter().enumerate() {
            let slice = &magnitudes[lo.min(bins)..hi.min(bins)];
            strength[band] = detectors[band].add_frame(slice, &[]);
            mass[band] = slice.iter().map(|&value| f64::from(value)).sum::<f64>();
        }
        strengths.push(strength);
        masses.push(mass);
    }
    let mut envelope = Vec::new();
    let mut evidence = Vec::new();
    for (index, &(start, end)) in windows.iter().enumerate() {
        let lo = index.saturating_sub(LOCAL_RADIUS);
        let hi = (index + LOCAL_RADIUS + 1).min(windows.len());
        let mut denominator = [0.0; 6];
        let mut ratio = [0.0; 6];
        let mut winner = 0;
        for band in 0..BANDS.len() {
            denominator[band] = masses[lo..hi]
                .iter()
                .map(|mass| mass[band])
                .fold(0.0, f64::max);
            if denominator[band] > 0.0 {
                ratio[band] = f64::from(strengths[index][band]) / denominator[band];
            }
            if ratio[band] > ratio[winner] {
                winner = band;
            }
        }
        evidence.push(ratio[winner]);
        envelope.push(json!({"analysis_index": index, "support_start": start,
            "support_end_exclusive": end, "coordinate_frame": start + (end - start) / 2,
            "band_strength": strengths[index], "band_mass": masses[index],
            "band_denominator": denominator, "band_ratio": ratio,
            "winning_band": winner, "evidence": ratio[winner]}));
    }
    let mut predicted = Vec::new();
    for (index, row) in envelope.iter_mut().enumerate() {
        let peak = local_peak(&evidence, index);
        row["peak"] = json!(peak);
        if peak {
            predicted.push(row["coordinate_frame"].as_u64().unwrap() as usize);
        }
    }
    assert!(predicted.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(predicted.iter().all(|&frame| frame < samples.len()));
    json!({"predicted_frames": predicted, "envelope": envelope,
        "elapsed_seconds": started.elapsed().as_secs_f64()})
}

fn run_bands(
    old: &Path,
    controls: &Path,
    output: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    fs::create_dir(output)?;
    fs::write(output.join("declaration.json"), BAND_DECLARATION)?;
    let declaration: Value = serde_json::from_str(BAND_DECLARATION)?;
    let old: Value = serde_json::from_slice(&fs::read(old)?)?;
    let controls: Value = serde_json::from_slice(&fs::read(controls)?)?;
    let references: Vec<_> = old["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(controls["cases"].as_array().unwrap())
        .collect();
    let inputs = declaration["inputs"].as_array().unwrap();
    assert_eq!(references.len(), 34);
    assert_eq!(references.len(), inputs.len());
    let mut cases = Vec::new();
    for (reference, input) in references.into_iter().zip(inputs) {
        for key in ["case", "pcm_path", "sample_frames", "truth_frames"] {
            assert_eq!(reference[key], input[key], "declared input changed: {key}");
        }
        let samples = frozen::load(
            Path::new(input["pcm_path"].as_str().unwrap()),
            input["channels"].as_u64().unwrap() as usize,
            input["channel"].as_u64().unwrap() as usize,
            input["sample_frames"].as_u64().unwrap() as usize,
        )?;
        let mut baseline = analyze(&samples);
        verify_native(&baseline, reference);
        let mut result = analyze_bands(&samples);
        // Both algorithms finish before labels enter the unchanged matcher
        let truth: Option<Vec<usize>> = serde_json::from_value(reference["truth_frames"].clone())?;
        for (predictions, metrics) in [
            ("predicted_frames", "native_metrics"),
            ("accepted_frames", "metrics"),
        ] {
            let frames: Vec<usize> = serde_json::from_value(baseline[predictions].clone())?;
            baseline[metrics] = truth
                .as_ref()
                .map_or(Value::Null, |truth| frozen::score(truth, &frames));
        }
        let parsed: Value = serde_json::from_str(&serde_json::to_string(&baseline)?)?;
        for key in [
            "envelope",
            "predicted_frames",
            "accepted_frames",
            "native_metrics",
            "metrics",
        ] {
            assert_eq!(
                parsed[key], reference[key],
                "frozen baseline changed: {key}"
            );
        }
        let predicted: Vec<usize> = serde_json::from_value(result["predicted_frames"].clone())?;
        result["metrics"] = truth
            .as_ref()
            .map_or(Value::Null, |truth| frozen::score(truth, &predicted));
        result["baseline"] = baseline;
        result["input"] = input.clone();
        cases.push(result);
    }
    assert_eq!(
        cases
            .iter()
            .filter(|case| !case["metrics"].is_null())
            .count(),
        31
    );
    let passed = cases
        .iter()
        .all(|case| case["metrics"].is_null() || case["metrics"]["status"] == "PASS");
    let report = json!({"status": if passed {"PASS"} else {"FAIL"}, "declaration": declaration,
        "baseline_reproduction": "PASS: all34 native and full-spectrum-gate predictions, windows and metrics match frozen reports",
        "independent_picker": true, "confidence": null,
        "real_music": "NOT RUN", "canonical_ogg": "NOT RUN", "human_labels": "NOT RUN", "cases": cases});
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
    let result = match args.as_slice() {
        [mode, old, controls, output] if mode == "--band-candidate" => {
            run_bands(Path::new(old), Path::new(controls), Path::new(output))
        }
        [mode, output] if mode == "--controls" => run_controls(Path::new(output)),
        [reference, output] => run(Path::new(reference), Path::new(output)),
        _ => {
            eprintln!(
                "Usage: cocobeat-mir-flux-gate-probe <frozen-flux-report.json> <new-output-directory>\n       cocobeat-mir-flux-gate-probe --controls <new-output-directory>\n       cocobeat-mir-flux-gate-probe --band-candidate <frozen-gate-report.json> <frozen-controls-report.json> <new-output-directory>"
            );
            return ExitCode::FAILURE;
        }
    };
    match result {
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
    fn band_evidence_selects_one_earliest_real_support_without_inventing_history() {
        let declaration: Value = serde_json::from_str(BAND_DECLARATION).unwrap();
        assert_eq!(
            declaration["parameters"]["half_open_bin_bands"],
            json!(BANDS)
        );
        assert_eq!(
            declaration["parameters"]["normalization_radius"],
            LOCAL_RADIUS
        );
        assert_eq!(declaration["parameters"]["peak_radius"], LOCAL_RADIUS);
        assert_eq!(
            declaration["parameters"]["minimum_change_inclusive"],
            MINIMUM_CHANGE
        );
        let values: Vec<f64> =
            serde_json::from_value(declaration["software_test_only"]["selection_values"].clone())
                .unwrap();
        let peaks: Vec<_> = (0..values.len())
            .filter(|&i| local_peak(&values, i))
            .collect();
        assert_eq!(
            json!(peaks),
            declaration["software_test_only"]["expected_peak_indices"]
        );
        let short = analyze_bands(&[1.0]);
        assert_eq!(short["predicted_frames"], json!([]));
        assert_eq!(short["envelope"][0]["coordinate_frame"], 0);
        assert_eq!(short["envelope"][0]["band_ratio"], json!(vec![0.0; 6]));
        assert_eq!(short["envelope"][0]["band_mass"][1], 0.0);
        assert_eq!(analyze_bands(&[0.0; 128])["predicted_frames"], json!([]));
        let mut samples = vec![0.0; 2048];
        samples[512] = 0.25;
        let result = analyze_bands(&samples);
        assert_eq!(result["predicted_frames"], json!([512]));
        let peak = result["envelope"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["peak"] == true)
            .unwrap();
        assert!(
            peak["band_ratio"]
                .as_array()
                .unwrap()
                .iter()
                .all(|ratio| ratio.as_f64().unwrap() >= MINIMUM_CHANGE)
        );
        assert_eq!(peak["support_start"], 448);
        assert_eq!(peak["support_end_exclusive"], 576);
        let scaled: Vec<_> = samples.iter().map(|value| value * 0.5).collect();
        assert_eq!(
            result["predicted_frames"],
            analyze_bands(&scaled)["predicted_frames"]
        );
    }

    #[test]
    fn constructed_entries_rises_and_mixture_have_the_declared_support() {
        let declaration: Value = serde_json::from_str(NEXT_CONTROLS).unwrap();
        assert_eq!(declaration["controls"].as_array().unwrap().len(), 9);
        for control in declaration["controls"].as_array().unwrap() {
            let samples = generate(control);
            assert_eq!(samples.len(), 48_000);
            assert!(
                samples
                    .iter()
                    .all(|value| value.is_finite() && value.abs() < 1.0)
            );
            let voices = control["voices"].as_array().unwrap();
            let first = voices[0]["start_frame"].as_u64().unwrap() as usize;
            assert!(samples[..first].iter().all(|&value| value == 0.0));
            if control["generator"] == "slow_tone" {
                assert!(control["truth_frames"].is_null());
                assert_eq!(samples[first], 0.0);
                assert!(samples[first + 1] > 0.0);
                let rise = voices[0]["rise_frames"].as_u64().unwrap() as usize;
                assert_eq!(
                    control["uncertain_interval_frames_inclusive"],
                    json!([first, first + rise])
                );
                let full = 0.2 * (std::f64::consts::TAU * 440.0 * rise as f64 / 48_000.0).cos();
                assert_eq!(samples[first + rise], full as f32);
            } else {
                assert_eq!(
                    samples[first],
                    voices[0]["amplitude"].as_f64().unwrap() as f32
                );
                assert_eq!(
                    control["truth_frames"].as_array().unwrap().len(),
                    voices.len()
                );
                if control["generator"] == "burst" {
                    for voice in voices {
                        let start = voice["start_frame"].as_u64().unwrap() as usize;
                        assert_ne!(samples[start + 127], 0.0);
                        assert_eq!(samples[start + 128], 0.0);
                    }
                }
            }
        }
        let controls = declaration["controls"].as_array().unwrap();
        let mixture = generate(&controls[7]);
        let isolated = generate(&controls[8]);
        assert_eq!(isolated[24_000], 0.02);
        // At age 18000 the base completes exactly 165 periods, and the added voice begins at cos(0)
        assert_eq!(mixture[24_000], 0.22);
    }

    #[test]
    fn normalization_rejects_phase_minimum_and_preserves_gain_and_zero() {
        let declaration: Value = serde_json::from_str(DECLARATION).unwrap();
        assert_eq!(declaration["parameters"]["normalization_radius"], RADIUS);
        assert_eq!(
            declaration["parameters"]["minimum_normalized_positive_change_inclusive"],
            MINIMUM_CHANGE
        );
        let mut mass = vec![0.0; 19];
        mass[0] = 1000.0; // Outside index 9's declared context
        mass[1] = 4.0;
        mass[9] = 0.1;
        let (_, normalized) = evidence(1.0, &mass, 9);
        assert_eq!(normalized, 0.25);
        assert!(normalized < MINIMUM_CHANGE);
        let scaled: Vec<_> = mass.iter().map(|value| value * 0.01).collect();
        assert_eq!(evidence(0.01, &scaled, 9).1, normalized);
        assert_eq!(evidence(0.0, &[0.0], 0), (0.0, 0.0));
        assert_eq!(evidence(2.0, &[4.0], 0).1, MINIMUM_CHANGE);
    }
}
