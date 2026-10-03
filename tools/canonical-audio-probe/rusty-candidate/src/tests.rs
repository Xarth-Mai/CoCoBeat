use super::*;
use std::{
    io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cocobeat-rusty-candidate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wav(path: &Path, rate: u32, channels: u16, pcm: &[f32]) {
    let bytes = (pcm.len() * 4) as u32;
    let mut output = Vec::new();
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&(bytes + 36).to_le_bytes());
    output.extend_from_slice(b"WAVEfmt ");
    output.extend_from_slice(&16_u32.to_le_bytes());
    output.extend_from_slice(&3_u16.to_le_bytes());
    output.extend_from_slice(&channels.to_le_bytes());
    output.extend_from_slice(&rate.to_le_bytes());
    output.extend_from_slice(&(rate * u32::from(channels) * 4).to_le_bytes());
    output.extend_from_slice(&(channels * 4).to_le_bytes());
    output.extend_from_slice(&32_u16.to_le_bytes());
    output.extend_from_slice(b"data");
    output.extend_from_slice(&bytes.to_le_bytes());
    for value in pcm {
        output.extend_from_slice(&value.to_le_bytes());
    }
    fs::write(path, output).unwrap();
}

fn report(output: &Path) -> Value {
    serde_json::from_slice(&fs::read(output.join("report.json")).unwrap()).unwrap()
}

#[test]
fn short_mono_48k_preserves_exact_source_and_full_readback_frames() {
    let scratch = Scratch::new();
    for frames in [1_usize, 1024, 1025] {
        let source = scratch.0.join(format!("{frames}.wav"));
        let output = scratch.0.join(format!("{frames}-output"));
        let samples: Vec<_> = (0..frames)
            .map(|i| 0.25 * (i as f32 * 0.125).cos())
            .collect();
        wav(&source, SAMPLE_RATE, 1, &samples);
        run(&source, &output).unwrap();
        let expected: Vec<_> = samples
            .iter()
            .flat_map(|v| [*v, *v])
            .flat_map(f32::to_le_bytes)
            .collect();
        assert_eq!(fs::read(output.join("resampled.f32le")).unwrap(), expected);
        assert_eq!(
            fs::metadata(output.join("decoded.f32le")).unwrap().len(),
            frames as u64 * 8
        );
        let result = report(&output);
        assert_eq!(result["result"]["status"], "PASS_SOFTWARE_CANDIDATE");
        assert_eq!(result["result"]["evidence"]["source"]["frames"], frames);
        assert_eq!(
            result["result"]["evidence"]["readback"]["pcm"]["frames"],
            frames
        );
        assert_eq!(
            result["result"]["evidence"]["readback"]["pcm"]["non_finite"],
            0
        );
    }
}

#[test]
fn stereo_44100_uses_media_resampling_before_final_ogg_readback() {
    let scratch = Scratch::new();
    let source = scratch.0.join("source.wav");
    let output = scratch.0.join("output");
    let samples: Vec<_> = (0..2205)
        .flat_map(|i| [0.2 * (i as f32 * 0.06).sin(), 0.1 * (i as f32 * 0.1).cos()])
        .collect();
    wav(&source, 44_100, 2, &samples);
    let mut expected = Vec::new();
    let resampled = resample_source(&source, |frames| write_frames(&mut expected, frames)).unwrap();
    assert_eq!(resampled.output_frames, 2400);
    run(&source, &output).unwrap();
    assert_eq!(fs::read(output.join("resampled.f32le")).unwrap(), expected);
    let result = report(&output);
    assert_eq!(
        result["result"]["evidence"]["source"]["sample_rate"],
        44_100
    );
    assert_eq!(
        result["result"]["evidence"]["readback"]["pcm"]["frames"],
        2400
    );
}

#[test]
fn source_errors_and_supported_media_outside_encoder_domain_cannot_pass() {
    let scratch = Scratch::new();
    let source = scratch.0.join("source.wav");
    for (case, value) in [("overunity", 1.125_f32), ("nonfinite", f32::NAN)] {
        wav(&source, SAMPLE_RATE, 1, &[value; 32]);
        if case == "overunity" {
            assert_eq!(
                decode_source(&source, |_, _| Ok(())).unwrap().source_frames,
                32
            );
        }
        let output = scratch.0.join(case);
        let error = run(&source, &output).unwrap_err();
        assert_eq!(report(&output)["result"]["status"], "FAIL");
        if case == "overunity" {
            assert!(error.contains("unsupported input domain"), "{error}");
        }
        assert!(!output.join("canonical.ogg").exists());
    }
    fs::write(&source, b"not an audio file").unwrap();
    let output = scratch.0.join("invalid");
    assert!(run(&source, &output).is_err());
    assert_eq!(report(&output)["result"]["status"], "FAIL");
}

#[test]
fn in_range_source_can_resample_outside_encoder_domain_without_becoming_invalid_media() {
    let scratch = Scratch::new();
    for rate in [44_100, 96_000] {
        let source = scratch.0.join(format!("{rate}.wav"));
        let samples: Vec<_> = (0..rate / 10)
            .map(|i| if i < rate / 20 { -0.9979248 } else { 0.9979248 })
            .collect();
        wav(&source, rate, 1, &samples);
        let mut peak = 0.0_f32;
        let result = resample_source(&source, |frames| {
            for value in frames.iter().flatten() {
                peak = peak.max(value.abs());
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(result.output_frames, 4800);
        assert!(peak > 1.0, "expected finite sinc overshoot, got {peak}");
        let output = scratch.0.join(format!("{rate}-output"));
        assert!(
            run(&source, &output)
                .unwrap_err()
                .contains("unsupported input domain")
        );
        assert_eq!(report(&output)["result"]["status"], "FAIL");
        assert!(!output.join("canonical.ogg").exists());
    }
}

#[test]
fn existing_output_is_preserved_and_consumer_readback_errors_propagate() {
    let scratch = Scratch::new();
    let sentinel = scratch.0.join("sentinel");
    fs::write(&sentinel, b"preserve").unwrap();
    assert!(
        run(Path::new("missing.wav"), &scratch.0)
            .unwrap_err()
            .contains("create new output")
    );
    assert_eq!(fs::read(sentinel).unwrap(), b"preserve");
    assert!(!scratch.0.join("report.json").exists());

    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("consumer failed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert!(
        write_frames(&mut FailingWriter, &[[0.0; 2]])
            .unwrap_err()
            .contains("consumer failed")
    );
    fs::write(scratch.0.join("resampled.f32le"), [0; 8]).unwrap();
    fs::write(scratch.0.join("canonical.ogg"), b"corrupt Ogg").unwrap();
    assert!(readback(&scratch.0, 1).is_err());
    assert!(verify_profile(3, &[0]).is_err());
}
