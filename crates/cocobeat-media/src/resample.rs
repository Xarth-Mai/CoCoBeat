//! Finite 48 kHz stereo source blocks, before canonical encoding

use crate::decode::{MAX_SOURCE_SAMPLE_RATE, decode_source};
use cocobeat_schema::CANONICAL_SAMPLE_RATE;
use oximedia_audio::{AudioBuffer, AudioFrame, ChannelLayout, Resampler, ResamplerQuality};
use oximedia_core::SampleFormat;
use std::path::Path;

const BLOCK_FRAMES: usize = 1024;
// OxiMedia 0.2.1 High retains at most half its 192-tap kernel for flush
// At 1 Hz this is 4,608,000 output frames: ~74 MB of library buffers plus capacity
const HIGH_TAIL_FRAMES: u64 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResampledSource {
    pub source_sample_rate: u32,
    pub source_frames: u64,
    pub output_frames: u64,
}

/// Delivers finite 48 kHz stereo blocks of at most 1024 frames using High quality
/// Same-rate input is bit-exact; no clipping, normalization or silence trimming
/// Callback errors stop immediately, and all delivered blocks remain provisional
/// until this function succeeds, so consumers must discard partial imports on error
pub fn resample_source(
    path: impl AsRef<Path>,
    mut consume: impl FnMut(&[[f32; 2]]) -> Result<(), String>,
) -> Result<ResampledSource, String> {
    let mut resampler = None;
    let decoded = decode_source(path, |rate, frames| {
        if resampler.is_none() {
            resampler = Some(SourceResampler::new(rate)?);
        }
        resampler.as_mut().unwrap().push(frames, &mut consume)
    })?;
    let resampler = resampler.ok_or("Audio source contains no audio frames")?;
    if resampler.rate != decoded.sample_rate || resampler.source_frames != decoded.source_frames {
        return Err("Decoded source metadata changed during resampling".into());
    }
    resampler.finish(&mut consume)
}

struct SourceResampler {
    rate: u32,
    converter: Option<Resampler>,
    source_frames: u64,
    output_frames: u64,
    block: Vec<[f32; 2]>,
}

impl SourceResampler {
    fn new(rate: u32) -> Result<Self, String> {
        if !(1..=MAX_SOURCE_SAMPLE_RATE).contains(&rate) {
            return Err("Audio source sample rate must be within 1..=192000 Hz".into());
        }
        let converter = if rate == CANONICAL_SAMPLE_RATE {
            None
        } else {
            Some(
                Resampler::new(rate, CANONICAL_SAMPLE_RATE, 2, ResamplerQuality::High)
                    .map_err(|error| format!("Cannot create source resampler: {error}"))?,
            )
        };
        Ok(Self {
            rate,
            converter,
            source_frames: 0,
            output_frames: 0,
            block: Vec::with_capacity(BLOCK_FRAMES),
        })
    }

    fn expected_frames(&self) -> Result<u64, String> {
        self.source_frames
            .checked_mul(u64::from(CANONICAL_SAMPLE_RATE))
            .map(|frames| frames.div_ceil(u64::from(self.rate)))
            .ok_or_else(|| "Resampled frame count overflow".into())
    }

    fn push(
        &mut self,
        frames: &[[f32; 2]],
        consume: &mut impl FnMut(&[[f32; 2]]) -> Result<(), String>,
    ) -> Result<(), String> {
        if frames.iter().flatten().any(|sample| !sample.is_finite()) {
            return Err("Audio source contains non-finite samples".into());
        }
        let chunk_frames = (u64::from(self.rate) * BLOCK_FRAMES as u64
            / u64::from(CANONICAL_SAMPLE_RATE))
        .clamp(1, BLOCK_FRAMES as u64) as usize;
        for chunk in frames.chunks(chunk_frames) {
            self.source_frames = self
                .source_frames
                .checked_add(chunk.len() as u64)
                .ok_or("Source frame count overflow")?;
            if let Some(converter) = &mut self.converter {
                let bytes: Vec<_> = chunk
                    .iter()
                    .flatten()
                    .flat_map(|sample| sample.to_le_bytes())
                    .collect();
                let mut input =
                    AudioFrame::new(SampleFormat::F32, self.rate, ChannelLayout::Stereo);
                input.samples = AudioBuffer::Interleaved(bytes.into());
                let output = converter
                    .resample(&input)
                    .map_err(|error| format!("Cannot resample source block: {error}"))?;
                let max_frames = (chunk.len() as u64 * u64::from(CANONICAL_SAMPLE_RATE))
                    .div_ceil(u64::from(self.rate));
                self.forward(output, max_frames, consume)?;
            } else {
                consume(chunk)?;
                self.output_frames = self.source_frames;
            }
        }
        Ok(())
    }

    fn forward(
        &mut self,
        output: AudioFrame,
        max_frames: u64,
        consume: &mut impl FnMut(&[[f32; 2]]) -> Result<(), String>,
    ) -> Result<(), String> {
        if output.format != SampleFormat::F32
            || output.sample_rate != CANONICAL_SAMPLE_RATE
            || output.channels != ChannelLayout::Stereo
        {
            return Err("Resampler returned an unexpected audio format".into());
        }
        let AudioBuffer::Interleaved(bytes) = output.samples else {
            return Err("Resampler returned non-interleaved samples".into());
        };
        let (frames, remainder) = bytes.as_chunks::<8>();
        let output_frames = self
            .output_frames
            .checked_add(frames.len() as u64)
            .ok_or("Resampled frame count overflow")?;
        if !remainder.is_empty()
            || frames.len() as u64 > max_frames
            || output_frames > self.expected_frames()?
        {
            return Err("Resampler returned an invalid frame count".into());
        }
        // Validate the complete library block before exposing any of its samples
        if bytes
            .as_chunks::<4>()
            .0
            .iter()
            .any(|sample| !f32::from_le_bytes(*sample).is_finite())
        {
            return Err("Resampler returned non-finite samples".into());
        }
        for chunk in frames.chunks(BLOCK_FRAMES) {
            self.block.clear();
            self.block.extend(chunk.iter().map(|frame| {
                [
                    f32::from_le_bytes(frame[..4].try_into().unwrap()),
                    f32::from_le_bytes(frame[4..].try_into().unwrap()),
                ]
            }));
            consume(&self.block)?;
        }
        self.output_frames = output_frames;
        Ok(())
    }

    fn finish(
        mut self,
        consume: &mut impl FnMut(&[[f32; 2]]) -> Result<(), String>,
    ) -> Result<ResampledSource, String> {
        if let Some(mut converter) = self.converter.take() {
            let output = converter
                .flush()
                .map_err(|error| format!("Cannot flush source resampler: {error}"))?;
            let max_frames = (HIGH_TAIL_FRAMES * u64::from(CANONICAL_SAMPLE_RATE))
                .div_ceil(u64::from(self.rate));
            self.forward(output, max_frames, consume)?;
        }
        if self.output_frames != self.expected_frames()? {
            return Err("Resampled frame count does not match the complete source".into());
        }
        Ok(ResampledSource {
            source_sample_rate: self.rate,
            source_frames: self.source_frames,
            output_frames: self.output_frames,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(rate: u32, input: &[[f32; 2]], chunk: usize) -> (ResampledSource, Vec<[f32; 2]>) {
        let mut converter = SourceResampler::new(rate).unwrap();
        let mut output = Vec::new();
        let mut consume = |block: &[[f32; 2]]| {
            assert!(block.len() <= BLOCK_FRAMES);
            output.extend_from_slice(block);
            Ok(())
        };
        for block in input.chunks(chunk) {
            converter.push(block, &mut consume).unwrap();
        }
        (converter.finish(&mut consume).unwrap(), output)
    }

    #[test]
    fn split_short_tail_and_extreme_rates_preserve_integer_frames() {
        for (rate, length) in [
            (1, 1),
            (7, 3),
            (44_100, 1025),
            (96_000, 1025),
            (192_000, 97),
        ] {
            let mut input = vec![[0.0, 0.0]; length];
            input[0][0] = 0.25;
            input[length - 1][1] = -0.5;
            let (metadata, output) = collect(rate, &input, 8192);
            assert_eq!(metadata.source_sample_rate, rate);
            assert_eq!(metadata.source_frames, length as u64);
            assert_eq!(
                metadata.output_frames,
                (length as u64 * 48_000).div_ceil(u64::from(rate))
            );
            assert_eq!(output.len() as u64, metadata.output_frames);
            assert!(output.iter().flatten().all(|value| value.is_finite()));
            assert!(output.iter().any(|frame| frame[0] != 0.0));
            assert!(output.iter().any(|frame| frame[1] != 0.0));
            let (_, split) = collect(rate, &input, 1);
            assert_eq!(output, split);
        }
    }

    #[test]
    fn passthrough_preserves_bits_and_errors_stop_stream_and_flush() {
        let input: Vec<_> = (0..2049)
            .map(|i| [if i % 2 == 0 { -0.0 } else { 1.5 }, -0.25])
            .collect();
        let (metadata, output) = collect(48_000, &input, 8192);
        assert_eq!(metadata.output_frames, input.len() as u64);
        assert!(
            input
                .iter()
                .flatten()
                .zip(output.iter().flatten())
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        for rate in [44_100, 48_000] {
            let mut calls = 0;
            let mut consume = |_: &[[f32; 2]]| {
                calls += 1;
                Err("cancelled".into())
            };
            assert_eq!(
                SourceResampler::new(rate)
                    .unwrap()
                    .push(&input, &mut consume),
                Err("cancelled".into())
            );
            assert_eq!(calls, 1);
        }
        let mut converter = SourceResampler::new(44_100).unwrap();
        converter
            .push(&[[0.25, -0.5]], &mut |_| {
                panic!("short input must remain buffered")
            })
            .unwrap();
        assert_eq!(
            converter.finish(&mut |_| Err("flush cancelled".into())),
            Err("flush cancelled".into())
        );
        for rate in [0, 192_001] {
            assert!(SourceResampler::new(rate).is_err());
        }
        let mut converter = SourceResampler::new(44_100).unwrap();
        assert!(
            converter
                .push(&[[f32::NAN, 0.0]], &mut |_| panic!("invalid input escaped"))
                .is_err()
        );
        converter.source_frames = 2048;
        let mut bytes = vec![0; (BLOCK_FRAMES + 1) * 8];
        bytes[BLOCK_FRAMES * 8..BLOCK_FRAMES * 8 + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        let mut output = AudioFrame::new(
            SampleFormat::F32,
            CANONICAL_SAMPLE_RATE,
            ChannelLayout::Stereo,
        );
        output.samples = AudioBuffer::Interleaved(bytes.into());
        assert!(
            converter
                .forward(output, 2048, &mut |_| panic!("invalid output escaped"))
                .is_err()
        );
    }

    #[test]
    fn source_decode_errors_and_consumer_cancellation_propagate() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/synthetic/media-import/mono.ogg");
        let mut frames = Vec::new();
        let metadata = resample_source(&source, |block| {
            frames.extend_from_slice(block);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            metadata,
            ResampledSource {
                source_sample_rate: 48_000,
                source_frames: 100_800,
                output_frames: 100_800
            }
        );
        assert!(frames.iter().all(|frame| frame[0] == frame[1]));
        assert_eq!(
            resample_source(&source, |_| Err("cancelled".into())),
            Err("cancelled".into())
        );
        assert!(
            resample_source(source.join("missing"), |_| panic!("invalid source escaped")).is_err()
        );
    }
}
