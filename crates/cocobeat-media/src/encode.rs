//! One bounded Vorbis candidate path, with strict final readback before success

use crate::{
    ResampledSource,
    decode::{MAX_SOURCE_BYTES, MAX_SOURCE_SECONDS},
    decode_canonical, resample_source,
};
use cocobeat_schema::CANONICAL_SAMPLE_RATE;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    num::{NonZeroU8, NonZeroU32},
    path::Path,
};
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

pub const CANONICAL_ENCODER_PROFILE: &str = "aotuv-lancer-vorbis-q10-v1";
// Candidate admission contract, not a proof that every legal source fits the codec
pub const MAX_ENCODER_PCM_PEAK: f32 = 4.0;

/// Encodes actual resampled frames to a new Ogg file, then strictly reads the final file
/// Neither amplitude nor timeline is adjusted; samples outside the codec domain fail
/// A successful readback proves structure and actual length, not listening acceptance
/// A returned error removes only the newly created output and retains the original error
pub fn encode_canonical_audio(
    source: impl AsRef<Path>,
    new_output: impl AsRef<Path>,
) -> Result<ResampledSource, String> {
    let output = new_output.as_ref();
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("Cannot create canonical audio output: {error}"))?;
    let result: Result<ResampledSource, String> = (|| {
        let mut builder = VorbisEncoderBuilder::new_with_serial(
            NonZeroU32::new(CANONICAL_SAMPLE_RATE).unwrap(),
            NonZeroU8::new(2).unwrap(),
            BoundedFile {
                file,
                remaining: MAX_SOURCE_BYTES,
            },
            // A canonical object contains exactly one logical stream, with no chaining
            0x4343_4231,
        );
        builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
            target_quality: 1.0,
        });
        let mut encoder = builder
            .build()
            .map_err(|error| format!("Cannot initialize canonical Vorbis encoder: {error}"))?;
        let mut planar = [Vec::with_capacity(1024), Vec::with_capacity(1024)];
        let mut input_frames = 0_u64;
        let source = resample_source(source, |frames| {
            let next_frames = input_frames
                .checked_add(frames.len() as u64)
                .filter(|frames| *frames <= u64::from(CANONICAL_SAMPLE_RATE) * MAX_SOURCE_SECONDS)
                .ok_or("Canonical encoder input exceeds ten minutes")?;
            if frames
                .iter()
                .flatten()
                .any(|sample| !sample.is_finite() || sample.abs() > MAX_ENCODER_PCM_PEAK)
            {
                return Err("Canonical encoder unsupported input domain: actual resampled PCM must be finite and within [-4, 4]; source media may still be legal; no clipping or normalization applied".into());
            }
            for channel in &mut planar {
                channel.clear();
            }
            for frame in frames {
                planar[0].push(frame[0]);
                planar[1].push(frame[1]);
            }
            encoder
                .encode_audio_block([planar[0].as_slice(), planar[1].as_slice()])
                .map_err(|error| format!("Cannot encode canonical audio block: {error}"))?;
            input_frames = next_frames;
            Ok(())
        })?;
        if input_frames == 0 || input_frames != source.output_frames {
            return Err("Canonical encoder input count differs from resampler output".into());
        }
        let writer = encoder
            .finish()
            .map_err(|error| format!("Cannot finish canonical Vorbis stream: {error}"))?;
        writer
            .file
            .sync_all()
            .map_err(|error| format!("Cannot sync canonical audio output: {error}"))?;
        drop(writer);
        decode_canonical(output, input_frames, |_| Ok(()))?;
        Ok(source)
    })();
    result.map_err(|mut error| {
        if let Err(cleanup) = fs::remove_file(output) {
            error.push_str(&format!(
                "; remove canonical audio output {}: {cleanup}",
                output.display()
            ));
        }
        error
    })
}

struct BoundedFile {
    file: File,
    remaining: u64,
}

impl Write for BoundedFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::other(
                "Canonical audio exceeds the 512 MiB output limit",
            ));
        }
        let written = self.file.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        f64::consts::TAU,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cocobeat-encode-{}-{}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn wav(path: &Path, rate: u32, frames: &[[f32; 2]]) {
        let bytes = (frames.len() * 8) as u32;
        let mut file = File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&3_u16.to_le_bytes()).unwrap();
        file.write_all(&2_u16.to_le_bytes()).unwrap();
        file.write_all(&rate.to_le_bytes()).unwrap();
        file.write_all(&(rate * 8).to_le_bytes()).unwrap();
        file.write_all(&8_u16.to_le_bytes()).unwrap();
        file.write_all(&32_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&bytes.to_le_bytes()).unwrap();
        for sample in frames.iter().flatten() {
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn exact_short_tail_stereo_and_resampled_lengths_are_strictly_read_back() {
        let root = TestDirectory::new();
        let source = root.0.join("source.wav");
        let output = root.0.join("canonical.ogg");
        for (rate, length) in [(48_000, 1), (48_000, 1024), (48_000, 1025), (44_100, 4410)] {
            let frames: Vec<_> = (0..length)
                .map(|index| {
                    [
                        (TAU * 440.0 * index as f64 / rate as f64).sin() as f32 * 0.99,
                        (TAU * 1000.0 * index as f64 / rate as f64).sin() as f32 * 0.99,
                    ]
                })
                .collect();
            wav(&source, rate, &frames);
            let metadata = encode_canonical_audio(&source, &output).unwrap();
            assert_eq!(metadata.source_sample_rate, rate);
            assert_eq!(metadata.source_frames, length as u64);
            assert_eq!(
                metadata.output_frames,
                (length as u64 * 48_000).div_ceil(u64::from(rate))
            );
            assert_eq!(
                decode_canonical(&output, metadata.output_frames, |_| Ok(())).unwrap(),
                metadata.output_frames
            );
            fs::remove_file(&output).unwrap();
        }
    }

    #[test]
    fn codec_domain_and_bad_source_fail_without_overwriting_or_leaking_output() {
        let root = TestDirectory::new();
        let source = root.0.join("source.wav");
        let output = root.0.join("canonical.ogg");
        let peak = MAX_ENCODER_PCM_PEAK;
        wav(&source, 48_000, &vec![[peak, -peak]; 4096]);
        let metadata = encode_canonical_audio(&source, &output).unwrap();
        assert_eq!(metadata.output_frames, 4096);
        let existing = fs::read(&output).unwrap();
        assert!(encode_canonical_audio(&source, &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), existing);
        fs::remove_file(&output).unwrap();

        for sample in [peak.next_up(), -peak.next_up(), f32::INFINITY, f32::NAN] {
            wav(&source, 48_000, &[[sample, 0.0]; 1025]);
            let original = fs::read(&source).unwrap();
            assert!(encode_canonical_audio(&source, &output).is_err());
            assert!(!output.exists());
            assert_eq!(fs::read(&source).unwrap(), original);
        }
        fs::write(&source, b"invalid source audio").unwrap();
        assert!(encode_canonical_audio(&source, &output).is_err());
        assert!(!output.exists());
        assert!(encode_canonical_audio(source.join("missing"), &output).is_err());
        assert!(!output.exists());
    }

    #[test]
    fn output_byte_budget_rejects_before_exposing_an_over_limit_write() {
        let root = TestDirectory::new();
        let output = root.0.join("bounded");
        let mut writer = BoundedFile {
            file: File::create(&output).unwrap(),
            remaining: 3,
        };
        writer.write_all(b"ab").unwrap();
        assert!(writer.write_all(b"cd").is_err());
        assert_eq!(writer.remaining, 1);
        writer.write_all(b"c").unwrap();
        writer.flush().unwrap();
        assert_eq!(fs::read(output).unwrap(), b"abc");
    }
}
