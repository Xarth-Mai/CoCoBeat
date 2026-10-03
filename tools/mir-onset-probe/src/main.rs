use oximedia_mir::beat::OnsetDetector;
use serde_json::{Value, json};
use std::{fs, path::Path, process::ExitCode, time::Instant};

const RATE: usize = 48_000;
const WINDOW: usize = 1024;
const HOP: usize = 128;
const TOLERANCE: usize = 480;
// Original project research report, line 1861: median absolute timing error <= 2 ms
const MEDIAN_LIMIT: f64 = 96.0;
const REGULAR: [usize; 8] = [
    24_000, 48_000, 72_000, 96_000, 120_000, 144_000, 168_000, 192_000,
];
// Independently checked: round(24000 + n * 230400 / 11), not rounded-period accumulation
const FRACTIONAL: [usize; 8] = [
    24_000, 44_945, 65_891, 86_836, 107_782, 128_727, 149_673, 170_618,
];

fn pulse_frames(bpm_numerator: usize, bpm_denominator: usize) -> Vec<usize> {
    (0..8)
        .map(|beat| {
            RATE / 2 + (beat * RATE * 60 * bpm_denominator + bpm_numerator / 2) / bpm_numerator
        })
        .collect()
}

fn signal(frames: usize, events: &[usize], polarity: f32) -> Vec<[f32; 2]> {
    let mut samples = vec![[0.0; 2]; frames];
    for &start in events {
        // Causal 64-frame pulse: the event frame is the first nonzero sample
        for (age, frame) in samples.iter_mut().skip(start).take(64).enumerate() {
            let value = (64 - age) as f32 / 128.0;
            *frame = [value, value * polarity];
        }
    }
    samples
}

fn metrics(truth: &[usize], predictions: &[usize]) -> Value {
    let (mut t, mut p) = (0, 0);
    let mut pairs = Vec::new();
    let mut errors = Vec::new();
    // Earliest feasible chronological pairs maximise cardinality at a fixed tolerance
    while t < truth.len() && p < predictions.len() {
        if predictions[p].saturating_add(TOLERANCE) < truth[t] {
            p += 1;
        } else if truth[t].saturating_add(TOLERANCE) < predictions[p] {
            t += 1;
        } else {
            errors.push(predictions[p] as i64 - truth[t] as i64);
            pairs.push([truth[t], predictions[p]]);
            t += 1;
            p += 1;
        }
    }
    let tp = pairs.len();
    let ratio = |n: usize, d: usize| (d != 0).then(|| n as f64 / d as f64);
    let mut absolute: Vec<_> = errors.iter().map(|error| error.unsigned_abs()).collect();
    absolute.sort_unstable();
    let median = (!absolute.is_empty())
        .then(|| (absolute[(absolute.len() - 1) / 2] + absolute[absolute.len() / 2]) as f64 / 2.0);
    let pass = tp == truth.len()
        && tp == predictions.len()
        && (truth.is_empty() || median.is_some_and(|value| value <= MEDIAN_LIMIT));
    json!({
        "status": if pass { "PASS" } else { "FAIL" },
        "true_positive": tp, "false_positive": predictions.len() - tp,
        "false_negative": truth.len() - tp,
        "precision": ratio(tp, predictions.len()), "recall": ratio(tp, truth.len()),
        "f1": ratio(2 * tp, truth.len() + predictions.len()),
        "matched_frames": pairs, "signed_error_frames": errors,
        "median_absolute_error_frames": median,
        "p95_absolute_error_frames": (!absolute.is_empty()).then(|| absolute[(95 * absolute.len()).div_ceil(100) - 1]),
    })
}

fn predictions(
    detector: &OnsetDetector,
    samples: &[f32],
) -> Result<(Vec<f32>, Vec<usize>), String> {
    let seconds = detector
        .detect(samples)
        .map_err(|error| error.to_string())?;
    if seconds.iter().any(|time| !time.is_finite() || *time < 0.0)
        || seconds.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Err("Detector returned non-finite, negative or unordered timestamps".into());
    }
    let frames: Vec<_> = seconds
        .iter()
        .map(|&time| (f64::from(time) * RATE as f64).round() as usize)
        .collect();
    if frames.iter().any(|&frame| frame >= samples.len()) {
        return Err("Detector returned an event outside the PCM frame bounds".into());
    }
    Ok((seconds, frames))
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .map_err(|error| format!("Write {}: {error}", path.display()))
}

fn run(output: &Path) -> Result<bool, String> {
    fs::create_dir(output).map_err(|error| format!("Create new {}: {error}", output.display()))?;
    fs::write(output.join("Cargo.lock"), include_bytes!("../Cargo.lock"))
        .map_err(|error| error.to_string())?;
    let cases = [
        (
            "pulse-120",
            RATE * 5,
            pulse_frames(120, 1),
            REGULAR.to_vec(),
            1.0,
        ),
        (
            "pulse-137.5",
            RATE * 5,
            pulse_frames(275, 2),
            FRACTIONAL.to_vec(),
            1.0,
        ),
        (
            "first-last",
            RATE,
            vec![0, 24_000, 47_999],
            vec![0, 24_000, 47_999],
            1.0,
        ),
        ("silence", RATE, vec![], vec![], 1.0),
        (
            "opposite-polarity",
            RATE * 5,
            pulse_frames(120, 1),
            REGULAR.to_vec(),
            -1.0,
        ),
    ];
    let detector = OnsetDetector::new(RATE as f32, WINDOW, HOP);
    let mut results = Vec::new();
    let mut passed = true;
    for (name, frames, events, truth, polarity) in cases {
        let directory = output.join(name);
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let pcm = signal(frames, &events, polarity);
        let bytes: Vec<_> = pcm
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let audio_path = directory.join("fixture.f32le");
        fs::write(&audio_path, bytes).map_err(|error| error.to_string())?;
        write_json(
            &directory.join("truth.json"),
            &json!({
                "sample_rate": RATE, "channels": 2, "frames": frames,
                "pcm_format": "f32le_interleaved_stereo",
                "onset_frames_each_channel": truth, "license": "CC0-1.0",
                "definition": "first nonzero PCM frame of each causal pulse; no beat or Anchor labels",
            }),
        )?;
        // Analyze the written PCM artifact; the detector never receives labels
        let bytes = fs::read(&audio_path).map_err(|error| error.to_string())?;
        if bytes.len() != frames * 8 {
            return Err(format!("{name}: PCM length changed after write"));
        }
        let mut channels = Vec::new();
        for channel in 0..2 {
            let samples: Vec<_> = bytes
                .as_chunks::<8>()
                .0
                .iter()
                .map(|frame| {
                    f32::from_le_bytes(frame[channel * 4..channel * 4 + 4].try_into().unwrap())
                })
                .collect();
            if samples.iter().any(|value| !value.is_finite()) {
                return Err(format!("{name}: non-finite PCM"));
            }
            let actual_starts: Vec<_> = samples
                .iter()
                .enumerate()
                .filter_map(|(frame, &value)| {
                    (value != 0.0 && (frame == 0 || samples[frame - 1] == 0.0)).then_some(frame)
                })
                .collect();
            if actual_starts != truth {
                return Err(format!(
                    "{name}: waveform starts disagree with independent literal truth"
                ));
            }
            let started = Instant::now();
            let prediction = predictions(&detector, &samples);
            let elapsed_seconds = started.elapsed().as_secs_f64();
            let result = match prediction {
                Ok((seconds, predicted)) => json!({
                    "channel": channel, "elapsed_seconds": elapsed_seconds,
                    "predicted_seconds": seconds, "predicted_frames": predicted,
                    "metrics": metrics(&truth, &predicted),
                }),
                Err(error) => json!({"channel": channel, "elapsed_seconds": elapsed_seconds,
                    "error": error, "metrics": {"status": "ERROR"}}),
            };
            passed &= result["metrics"]["status"] == "PASS";
            channels.push(result);
        }
        let result = json!({"case": name, "source_frames": frames, "channels": channels});
        write_json(&directory.join("predictions.json"), &result)?;
        results.push(result);
    }
    let report = json!({
        "status": if passed { "PASS" } else { "FAIL" }, "scope": "five synthetic raw PCM cases only",
        "canonical_ogg_readback": "NOT RUN", "confidence": "NOT PROVIDED BY API",
        "algorithm": "oximedia_mir::beat::OnsetDetector", "dependency_identity": "Cargo.lock",
        "pcm_format": "f32le_interleaved_stereo",
        "sample_rate": RATE, "window_frames": WINDOW, "hop_frames": HOP,
        "match_tolerance_frames": TOLERANCE, "median_absolute_error_limit_frames": MEDIAN_LIMIT,
        "median_gate_source": "original deep-research-report.md:1861, median absolute timing error <= 2 ms",
        "gates": "all literal truth matched once, no extra predictions, median <= 96 frames; silence requires zero predictions",
        "matching": "sorted earliest feasible one-to-one pairs; signed error = predicted - truth; inclusive tolerance",
        "empty_metrics": "zero denominators produce null; silence status is evaluated separately",
        "time_origin": "original frame zero; no intro trim, padding, shift compensation or confidence fabrication",
        "results": results,
    });
    write_json(&output.join("report.json"), &report)?;
    println!(
        "{}: {}",
        output.join("report.json").display(),
        report["status"]
    );
    Ok(passed)
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [output] = args.as_slice() else {
        eprintln!("Usage: cocobeat-mir-onset-probe <new-output-directory>");
        return ExitCode::FAILURE;
    };
    match run(Path::new(output)) {
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
    fn literal_truth_waveform_and_one_to_one_metrics() {
        assert_eq!(pulse_frames(120, 1), REGULAR);
        assert_eq!(pulse_frames(275, 2), FRACTIONAL);
        let pcm = signal(48_000, &[0, 24_000, 47_999], -1.0);
        for frame in [0, 24_000, 47_999] {
            assert_eq!(pcm[frame], [0.5, -0.5]);
        }
        assert_eq!(pcm[64], [0.0; 2]);
        let duplicate = metrics(&[1000], &[1000, 1001]);
        assert_eq!(duplicate["true_positive"], 1);
        assert_eq!(duplicate["false_positive"], 1);
        assert_eq!(duplicate["f1"], 2.0 / 3.0);
        assert_eq!(metrics(&[1000, 1600], &[1400, 1900])["true_positive"], 2);
        assert_eq!(metrics(&[1000], &[1480])["true_positive"], 1);
        assert_eq!(metrics(&[1000], &[1481])["true_positive"], 0);
        assert_eq!(metrics(&[1000], &[1100])["status"], "FAIL");
        assert_eq!(metrics(&[], &[])["status"], "PASS");
        assert!(metrics(&[], &[])["f1"].is_null());
        assert_eq!(metrics(&[1000], &[])["status"], "FAIL");
        assert_eq!(metrics(&[], &[1000])["status"], "FAIL");
    }
}
