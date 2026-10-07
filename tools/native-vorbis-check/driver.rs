//! QA only: the product adapter and two independent complete decoders

use cocobeat_media::{decode_canonical, encode_canonical_audio, resample_source};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() < 3 {
        return Err("encode SOURCE NEW_OGG | resample SOURCE NEW_PCM | readback OGG N NEW_PCM | reference OGG NEW_PCM".into());
    }
    let input = Path::new(&args[1]);
    match (args[0].to_str(), args.len()) {
        (Some("encode"), 3) => {
            let summary = encode_canonical_audio(input, Path::new(&args[2]))?;
            println!(
                "{{\"source_rate\":{},\"source_frames\":{},\"frames\":{}}}",
                summary.source_sample_rate, summary.source_frames, summary.output_frames
            );
        }
        (Some("resample"), 3) => {
            let mut writer = new_pcm(Path::new(&args[2]))?;
            let mut peak = [0.0_f32; 2];
            let mut observed = 0_u64;
            let summary = resample_source(input, |frames| {
                for frame in frames {
                    for channel in 0..2 {
                        peak[channel] = peak[channel].max(frame[channel].abs());
                    }
                }
                write_pcm(&mut writer, frames)?;
                observed += frames.len() as u64;
                Ok(())
            })?;
            writer.flush()?;
            if observed != summary.output_frames {
                return Err("Resample callback frame count mismatch".into());
            }
            println!(
                "{{\"source_rate\":{},\"source_frames\":{},\"frames\":{},\"peak\":[{},{}]}}",
                summary.source_sample_rate, summary.source_frames, observed, peak[0], peak[1]
            );
        }
        (Some("readback"), 4) => {
            let expected: u64 = args[2]
                .to_str()
                .ok_or("Expected frames must be ASCII")?
                .parse()?;
            let mut writer = new_pcm(Path::new(&args[3]))?;
            let frames =
                decode_canonical(input, expected, |frames| write_pcm(&mut writer, frames))?;
            writer.flush()?;
            println!("{{\"frames\":{frames},\"channels\":2,\"sample_rate\":48000}}");
        }
        (Some("reference"), 3) => {
            let mut decoder = vorbis_rs::VorbisDecoder::new(File::open(input)?)?;
            let rate = decoder.sampling_frequency().get();
            let channels = decoder.channels().get();
            if rate != 48_000 || channels != 2 {
                return Err("Reference decoder found a noncanonical format".into());
            }
            let mut writer = new_pcm(Path::new(&args[2]))?;
            let mut frames = 0_u64;
            while let Some(block) = decoder.decode_audio_block()? {
                let planar = block.samples();
                if planar.len() != 2 || planar[0].len() != planar[1].len() {
                    return Err("Reference decoder returned inconsistent channels".into());
                }
                for (&left, &right) in planar[0].iter().zip(planar[1].iter()) {
                    write_pcm(&mut writer, &[[left, right]])?;
                }
                frames += planar[0].len() as u64;
            }
            writer.flush()?;
            println!("{{\"frames\":{frames},\"channels\":{channels},\"sample_rate\":{rate}}}");
        }
        _ => return Err("Invalid native Vorbis QA command".into()),
    }
    Ok(())
}

fn new_pcm(path: &Path) -> std::io::Result<BufWriter<File>> {
    Ok(BufWriter::new(
        OpenOptions::new().write(true).create_new(true).open(path)?,
    ))
}

fn write_pcm(writer: &mut impl Write, frames: &[[f32; 2]]) -> Result<(), String> {
    for sample in frames.iter().flatten() {
        if !sample.is_finite() {
            return Err("QA decoder returned non-finite PCM".into());
        }
        writer
            .write_all(&sample.to_le_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
