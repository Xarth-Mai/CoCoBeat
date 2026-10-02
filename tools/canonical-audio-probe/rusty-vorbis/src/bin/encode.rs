use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use rusty_vorbis::{Error, VorbisEncoder, VorbisEncoderConfig, quality01_from_vorbis_q};
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let frames: usize = args[2].parse()?;
    let q: f64 = args[3].parse()?;
    assert!(frames <= 48_000 * 64 && (-1.0..=10.0).contains(&q));
    std::fs::create_dir_all(&root)?;
    let quality = quality01_from_vorbis_q(q);
    let mut encoder = VorbisEncoder::new(VorbisEncoderConfig {
        quality,
        ..Default::default()
    });
    let mut input = BufWriter::new(File::create(root.join("input.f32le"))?);
    // Initialize the declared format even for the empty-input boundary
    encoder.push_pcm_f32(&[], 2, 48_000)?;
    for start in (0..frames).step_by(1_000) {
        let pcm: Vec<f32> = (start..(start + 1_000).min(frames))
            .flat_map(|i| {
                let left =
                    0.125 * (std::f64::consts::TAU * 440.0 * i as f64 / 48_000.0).sin() as f32;
                let right = if i >= frames.saturating_sub(128) {
                    if i % 2 == 0 { 0.75 } else { -0.75 }
                } else {
                    0.0
                };
                [left, right]
            })
            .collect();
        for value in &pcm {
            input.write_all(&value.to_le_bytes())?;
        }
        encoder.push_pcm_f32(&pcm, 2, 48_000)?;
    }
    input.flush()?;
    encoder.finish();
    let mut packets = Vec::new();
    loop {
        match encoder.next_packet() {
            Ok(packet) => packets.push(packet),
            Err(Error::Eof) => break,
            Err(error) => return Err(error.into()),
        }
    }
    let mut timing = BufWriter::new(File::create(root.join("packets.jsonl"))?);
    let mut writer = PacketWriter::new(BufWriter::new(File::create(root.join("encoded.ogg"))?));
    let count = packets.len();
    for (i, packet) in packets.into_iter().enumerate() {
        writeln!(
            timing,
            "{{\"index\":{i},\"bytes\":{},\"pts\":{},\"duration\":{}}}",
            packet.data.len(),
            packet.pts,
            packet.duration
        )?;
        // Keep official packet granules unchanged; one packet per page matches its published test mux
        let end = if i + 1 == count {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::EndPage
        };
        writer.write_packet(
            packet.data.into_boxed_slice(),
            0x434f434f,
            end,
            u64::try_from(packet.pts)?,
        )?;
    }
    timing.flush()?;
    writer.into_inner().flush()?;
    println!(
        "{{\"source_frames\":{frames},\"sample_rate\":48000,\"channels\":2,\"vorbis_q\":{q},\"quality\":{quality},\"packets\":{count}}}"
    );
    Ok(())
}
