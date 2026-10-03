use oximedia_mir::onset_strength::{OnsetDetector, OnsetFunction};
use serde_json::{Value, json};
use std::{fs, path::Path, process::ExitCode, time::Instant};

// Keep the exact native FFT, support coordinates and original matching rules
#[allow(dead_code)]
mod previous {
    pub fn windows(frames: usize) -> Vec<(usize, usize)> {
        spans(frames)
    }
    pub fn magnitudes(samples: &[f32]) -> Vec<f32> {
        spectrum(samples)
    }
    pub fn coordinate(start: usize, end: usize) -> usize {
        center(start, end)
    }
    pub fn score(truth: &[usize], predicted: &[usize]) -> Value {
        baseline::score(truth, predicted)
    }
    pub fn load(
        path: &Path,
        channels: usize,
        channel: usize,
        frames: usize,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        read_channel(path, channels, channel, frames)
    }
    include!("../../mir-spectral-probe/src/main.rs");
}

const RATE: usize = 48_000;
const HOP: usize = 64;
const FACTOR: f32 = 1.5;
const DECLARATION: &str =
    include_str!("../../../testdata/synthetic/mir-flux-probe/declared-controls-20261003.json");

fn analyze(samples: &[f32], function: OnsetFunction) -> Value {
    let started = Instant::now();
    let windows = previous::windows(samples.len());
    let mut detector = OnsetDetector::new(function, RATE as f32, HOP, FACTOR);
    for &(start, end) in &windows {
        let magnitudes = previous::magnitudes(&samples[start..end]);
        detector.add_frame(&magnitudes[..=magnitudes.len() / 2], &[]);
    }
    detector.pick_onsets();
    let envelope: Vec<_> = detector.frames().iter().zip(&windows).map(|(frame, &(start, end))| {
        json!({"analysis_index": frame.frame_index, "support_start": start, "support_end_exclusive": end,
            "coordinate_frame": previous::coordinate(start, end), "api_time_seconds": frame.time_s,
            "strength": frame.value, "peak": frame.onset_flag})
    }).collect();
    let predicted: Vec<_> = detector
        .onset_frames()
        .iter()
        .map(|frame| {
            let (start, end) = windows[frame.frame_index];
            previous::coordinate(start, end)
        })
        .collect();
    assert!(predicted.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(predicted.iter().all(|&frame| frame < samples.len()));
    json!({"predicted_frames": predicted, "envelope": envelope, "elapsed_seconds": started.elapsed().as_secs_f64()})
}

fn evaluate(name: &str, samples: &[f32], truth: Option<&[usize]>) -> Value {
    let mut runs = Vec::new();
    for (name, function) in [
        ("hfc", OnsetFunction::HfcEnergy),
        ("spectral_flux", OnsetFunction::SpectralFlux),
    ] {
        let mut result = analyze(samples, function);
        let predicted: Vec<usize> =
            serde_json::from_value(result["predicted_frames"].clone()).unwrap();
        result["algorithm"] = json!(name);
        result["metrics"] = truth.map_or(Value::Null, |truth| previous::score(truth, &predicted));
        result["emitted_peaks"] = json!(predicted.len());
        runs.push(result);
    }
    json!({"case": name, "sample_frames": samples.len(), "truth_frames": truth, "runs": runs,
        "scoring": if truth.is_some() {"discrete_constructed_events"} else {"continuous_observation_only"}})
}

fn noise(state: &mut u32) -> f64 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    f64::from(*state) / f64::from(u32::MAX) * 2.0 - 1.0
}

fn generate(control: &Value) -> Vec<f32> {
    let frames = control["frames"].as_u64().unwrap() as usize;
    let p = &control["parameters"];
    let number = |key: &str| p[key].as_f64().unwrap();
    let index = |key: &str| p[key].as_u64().unwrap() as usize;
    let mut samples = vec![0.0; frames];
    match control["name"].as_str().unwrap() {
        "leading-silence-tone-440" | "leading-silence-tone-9973" => {
            for (age, sample) in samples.iter_mut().skip(index("start_frame")).enumerate() {
                *sample = (number("amplitude")
                    * (std::f64::consts::TAU * number("frequency_hz") * age as f64 / RATE as f64)
                        .cos()) as f32;
            }
        }
        "equal-energy-note-switch" => {
            for (note, (start, end)) in [
                (index("start_frame"), index("switch_frame")),
                (index("switch_frame"), frames),
            ]
            .into_iter()
            .enumerate()
            {
                let frequency = p["frequencies_hz"][note].as_f64().unwrap();
                for (age, sample) in samples[start..end].iter_mut().enumerate() {
                    *sample = (number("amplitude")
                        * (std::f64::consts::TAU * frequency * age as f64 / RATE as f64).cos())
                        as f32;
                }
            }
        }
        "leading-silence-noise-burst" => {
            let start = index("start_frame");
            let end = index("end_frame_exclusive");
            let release = index("release_frames");
            let mut state = index("seed") as u32;
            for (frame, sample) in samples.iter_mut().enumerate().take(end).skip(start) {
                let envelope = if frame < end - release {
                    1.0
                } else {
                    0.5 * (1.0
                        + (std::f64::consts::PI * (frame - (end - release)) as f64
                            / (release - 1) as f64)
                            .cos())
                };
                *sample = (number("amplitude") * envelope * noise(&mut state)) as f32;
            }
        }
        "sparse-kick-like" | "sparse-snare-like" => {
            let kick = control["name"] == "sparse-kick-like";
            for (event, start) in p["start_frames"].as_array().unwrap().iter().enumerate() {
                let start = start.as_u64().unwrap() as usize;
                let duration = index("duration_frames");
                let mut state = if kick {
                    0
                } else {
                    p["seed_per_note"][event].as_u64().unwrap() as u32
                };
                for (age, sample) in samples[start..start + duration].iter_mut().enumerate() {
                    let carrier = if kick {
                        let t = age as f64 / RATE as f64;
                        let f = number("frequency_start_hz");
                        let drop = f - number("frequency_end_hz");
                        (std::f64::consts::TAU
                            * (f * t - 0.5 * drop * t * t / (duration as f64 / RATE as f64)))
                            .cos()
                    } else {
                        noise(&mut state)
                    };
                    *sample = (number("amplitude")
                        * (-(age as f64) / number("decay_samples")).exp()
                        * carrier) as f32;
                }
            }
        }
        "continuous-fade-440" => {
            for (i, sample) in samples.iter_mut().enumerate().take(frames - 1).skip(1) {
                let envelope = (std::f64::consts::PI * i as f64 / (frames - 1) as f64)
                    .sin()
                    .powi(2);
                *sample = (number("amplitude")
                    * envelope
                    * (std::f64::consts::TAU * number("frequency_hz") * i as f64 / RATE as f64)
                        .cos()) as f32;
            }
        }
        other => panic!("Undeclared generator {other}"),
    }
    assert!(samples.iter().all(|s| s.is_finite()));
    samples
}

fn repeated_pick_control() -> Value {
    let values: [f32; 9] = [0.0, 0.0, 1.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0];
    let create = || OnsetDetector::new(OnsetFunction::HfcEnergy, RATE as f32, HOP, FACTOR);
    let add = |d: &mut OnsetDetector, values: &[f32]| {
        for value in values {
            d.add_frame(&[value.sqrt()], &[]);
        }
    };
    let indices = |d: &OnsetDetector| {
        d.onset_frames()
            .iter()
            .map(|f| f.frame_index)
            .collect::<Vec<_>>()
    };
    let mut repeated = create();
    add(&mut repeated, &values[..5]);
    repeated.pick_onsets();
    let before = indices(&repeated);
    add(&mut repeated, &values[5..]);
    repeated.pick_onsets();
    let mut fresh = create();
    add(&mut fresh, &values);
    fresh.pick_onsets();
    assert_eq!(before, [2]);
    assert_eq!(indices(&fresh), [5]);
    assert_eq!(indices(&repeated), [2, 5]);
    json!({"status": "UPSTREAM_STATE_BUG_REPRODUCED", "strength_values": values,
        "prefix_peaks": before, "repeated_peaks": indices(&repeated), "fresh_once_peaks": indices(&fresh),
        "cause": "pick_onsets only sets true flags and never clears stale flags before recomputing",
        "minimal_upstream_fix": "reset each frame.onset_flag before evaluating its current threshold",
        "candidate_affected": false, "candidate_policy": "one call after all spectral frames; vendor unchanged"})
}

fn run(
    clean: &Path,
    controls: &Path,
    reference: &Path,
    output: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    fs::create_dir(output)?;
    fs::write(output.join("declaration.json"), DECLARATION)?;
    let declaration: Value = serde_json::from_str(DECLARATION)?;
    let old: Value = serde_json::from_slice(&fs::read(reference)?)?;
    let mut existing = Vec::new();
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
            let samples = previous::load(
                &clean.join(name).join("fixture.f32le"),
                2,
                channel,
                truth["frames"].as_u64().unwrap() as usize,
            )?;
            existing.push(evaluate(
                &format!("{name}/channel-{channel}"),
                &samples,
                Some(&labels),
            ));
        }
    }
    let control_report: Value = serde_json::from_slice(&fs::read(controls.join("report.json"))?)?;
    for case in control_report["holdout"].as_array().unwrap() {
        let name = case["case"].as_str().unwrap();
        let labels: Vec<usize> = serde_json::from_value(case["truth_frames"].clone())?;
        let samples = previous::load(
            &controls.join(format!("{name}.f32le")),
            1,
            0,
            case["sample_frames"].as_u64().unwrap() as usize,
        )?;
        existing.push(evaluate(name, &samples, Some(&labels)));
    }
    assert_eq!(existing.len(), 18);
    for case in &existing {
        let baseline = old["results"]
            .as_array()
            .unwrap()
            .iter()
            .chain(old["holdout"].as_array().unwrap())
            .find(|c| c["case"] == case["case"])
            .unwrap();
        let hfc = &case["runs"][0];
        assert_eq!(hfc["predicted_frames"], baseline["predicted_frames"]);
        assert_eq!(hfc["metrics"], baseline["metrics"]);
        assert_eq!(
            hfc["envelope"].as_array().unwrap().len(),
            baseline["envelope"].as_array().unwrap().len()
        );
        for (current, previous) in hfc["envelope"]
            .as_array()
            .unwrap()
            .iter()
            .zip(baseline["envelope"].as_array().unwrap())
        {
            // The API returns f32; JSON's f64 decimal parser can differ by one f64 ULP
            assert_eq!(
                (current["strength"].as_f64().unwrap() as f32).to_bits(),
                (previous["hfc"].as_f64().unwrap() as f32).to_bits()
            );
        }
    }
    let mut added = Vec::new();
    for control in declaration["new_controls"].as_array().unwrap() {
        let name = control["name"].as_str().unwrap();
        let samples = generate(control);
        let path = output.join(format!("{name}.f32le"));
        fs::write(
            &path,
            samples
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )?;
        let samples = previous::load(&path, 1, 0, samples.len())?;
        let labels: Option<Vec<usize>> =
            serde_json::from_value(control["literal_truth_frames"].clone())?;
        added.push(evaluate(name, &samples, labels.as_deref()));
    }
    let passed = existing.iter().chain(&added).all(|case| {
        case["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|run| run["metrics"].is_null() || run["metrics"]["status"] == "PASS")
    });
    let report = json!({"status": if passed {"PASS"} else {"FAIL"}, "scope": "fixed HFC versus true spectral-flux comparison; no production admission",
        "declaration": declaration, "existing_hfc_reproduction": "PASS: all18 predictions, metrics and HFC strength values match frozen reference",
        "confidence": "NOT PROVIDED", "continuous_metrics": "null: no unique discrete attack truth declared; peak observations only",
        "existing": existing, "new_controls": added, "upstream_repeated_pick": repeated_pick_control(),
        "source_clean": clean, "source_controls": controls, "hfc_reference": reference,
        "real_music": "NOT RUN", "canonical_ogg": "NOT RUN"});
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
    let [clean, controls, reference, output] = args.as_slice() else {
        eprintln!(
            "Usage: cocobeat-mir-flux-probe <clean-directory> <control-directory> <frozen-hfc-report.json> <new-output-directory>"
        );
        return ExitCode::FAILURE;
    };
    match run(
        Path::new(clean),
        Path::new(controls),
        Path::new(reference),
        Path::new(output),
    ) {
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
    fn declared_controls_and_upstream_state_regression() {
        let declaration: Value = serde_json::from_str(DECLARATION).unwrap();
        for control in declaration["new_controls"].as_array().unwrap() {
            let samples = generate(control);
            if control["name"] == "continuous-fade-440" {
                assert!(control["literal_truth_frames"].is_null());
                assert_eq!(samples[0], 0.0);
                assert_eq!(samples[samples.len() - 1], 0.0);
            } else {
                for value in control["literal_truth_frames"].as_array().unwrap() {
                    let frame = value.as_u64().unwrap() as usize;
                    assert_ne!(samples[frame], 0.0);
                    if control["name"] != "equal-energy-note-switch" || frame == 12_000 {
                        assert_eq!(samples[frame - 1], 0.0);
                    }
                }
            }
        }
        assert_eq!(
            repeated_pick_control()["status"],
            "UPSTREAM_STATE_BUG_REPRODUCED"
        );
    }
}
