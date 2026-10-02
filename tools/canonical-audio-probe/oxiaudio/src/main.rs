use oxiaudio_core::{AudioBuffer, ChannelLayout, SampleFormat};
use oxiaudio_encode::{VorbisQuality, encode_vorbis_with_quality};
use std::{fs::File, io::Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: cocobeat-canonical-probe OUTPUT_DIR")?,
    );
    std::fs::create_dir_all(&root)?;
    for frames in [48000_usize, 48128, 1024, 0] {
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let sine = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin() * 0.125;
            let tail = if i >= frames.saturating_sub(128) {
                if i % 2 == 0 { 0.75 } else { -0.75 }
            } else {
                0.0
            };
            samples.extend([sine, tail]);
        }
        let mut raw = File::create(root.join(format!("source-{frames}.f32le")))?;
        for v in &samples {
            raw.write_all(&v.to_le_bytes())?;
        }
        raw.sync_all()?;
        let buf = AudioBuffer {
            samples,
            sample_rate: 48000,
            channels: ChannelLayout::Stereo,
            format: SampleFormat::F32,
        };
        for q in [0, 5, 10] {
            let mut output = File::create(root.join(format!("encoded-{frames}-q{q}.ogg")))?;
            encode_vorbis_with_quality(&buf, &mut output, VorbisQuality::from_level(q))?;
            output.flush()?;
            output.sync_all()?;
            println!(
                "encoded source_frames={frames} quality={q} bytes={}",
                output.metadata()?.len()
            );
        }
    }
    Ok(())
}
