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

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [reference, output] = args.as_slice() else {
        eprintln!(
            "Usage: cocobeat-mir-flux-gate-probe <frozen-flux-report.json> <new-output-directory>"
        );
        return ExitCode::FAILURE;
    };
    match run(Path::new(reference), Path::new(output)) {
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
