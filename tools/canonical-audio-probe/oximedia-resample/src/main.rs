use oximedia_audio::{AudioBuffer, AudioFrame, ChannelLayout, Resampler, ResamplerQuality};
use oximedia_core::SampleFormat;
use std::{error::Error, fs, path::PathBuf};

fn frame(data: &[u8], rate: u32) -> AudioFrame {
    let mut frame = AudioFrame::new(SampleFormat::F32, rate, ChannelLayout::Stereo);
    frame.samples = AudioBuffer::Interleaved(data.to_vec().into());
    frame
}

fn append(frame: AudioFrame, output: &mut Vec<u8>) {
    assert_eq!(frame.sample_rate, 48_000);
    assert_eq!(frame.channels.count(), 2);
    match frame.samples {
        AudioBuffer::Interleaved(data) => output.extend_from_slice(&data),
        _ => panic!("expected interleaved F32"),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: preflight OUTPUT_DIR")?,
    );
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
                .chunks_exact(4)
                .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
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
