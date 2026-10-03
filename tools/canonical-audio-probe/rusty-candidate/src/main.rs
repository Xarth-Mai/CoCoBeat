use cocobeat_media::{decode_source, resample_source};
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use rusty_vorbis::{Error, VorbisEncoder, VorbisEncoderConfig, quality01_from_vorbis_q};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

const SAMPLE_RATE: u32 = 48_000;
const MAX_FRAMES: u64 = 600 * SAMPLE_RATE as u64;
const HOP: u64 = 1024;

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: cocobeat-rusty-candidate <source-audio> <new-output-dir>");
        std::process::exit(2);
    }
    if let Err(error) = run(Path::new(&args[0]), Path::new(&args[1])) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(source: &Path, output: &Path) -> Result<(), String> {
    fs::create_dir(output).map_err(|e| format!("create new output directory: {e}"))?;
    let result = pipeline(source, output);
    let report = match &result {
        Ok(evidence) => json!({"status": "PASS_SOFTWARE_CANDIDATE", "evidence": evidence}),
        Err(error) => json!({"status": "FAIL", "error": error}),
    };
    let report = json!({
        "schema": 1,
        "candidate": "rusty_vorbis-0.1.1-max-abs-q10",
        "source_path": source.to_string_lossy(),
        "host": {"os": std::env::consts::OS, "arch": std::env::consts::ARCH},
        "result": report,
    });
    let save_report = (|| -> Result<(), String> {
        let pending = output.join("report.pending.json");
        let mut writer = create_new(&pending)?;
        serde_json::to_writer_pretty(&mut writer, &report).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        drop(writer);
        fs::rename(pending, output.join("report.json")).map_err(|e| e.to_string())
    })();
    match (result, save_report) {
        (Err(error), Err(report_error)) => Err(format!("{error}; write report: {report_error}")),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(format!("write report: {error}")),
        (Ok(_), Ok(())) => Ok(()),
    }
}

fn pipeline(source: &Path, output: &Path) -> Result<Value, String> {
    let metadata = fs::metadata(source).map_err(|e| format!("source metadata: {e}"))?;
    if !metadata.is_file() || metadata.len() > 512 * 1024 * 1024 {
        return Err("source must be a regular file of at most 512 MiB".into());
    }
    let source_hash = file_sha256(source)?;
    let quality = quality01_from_vorbis_q(10.0);
    let mut encoder = VorbisEncoder::new(VorbisEncoderConfig {
        quality,
        ..Default::default()
    });
    encoder
        .push_pcm_f32(&[0.0; 2048], 2, SAMPLE_RATE)
        .map_err(|e| format!("encoder priming: {e}"))?;
    let mut pcm = create_new(&output.join("resampled.f32le"))?;
    let mut input = PcmStats::default();
    let resampled = resample_source(source, |frames| {
        if input.frames + frames.len() as u64 > MAX_FRAMES {
            return Err("candidate exceeds 28800000 output frames (600 seconds)".into());
        }
        // The upstream API documents [-1, 1]; media legitimately accepts wider finite PCM
        for frame in frames {
            if frame.iter().any(|value| !(-1.0..=1.0).contains(value)) {
                return Err("candidate unsupported input domain: resampled PCM must be finite and in [-1, 1] per rusty_vorbis::push_pcm_f32; no normalization or clipping applied".into());
            }
        }
        write_frames(&mut pcm, frames)?;
        let interleaved: Vec<_> = frames.iter().flatten().copied().collect();
        encoder
            .push_pcm_f32(&interleaved, 2, SAMPLE_RATE)
            .map_err(|e| format!("encoder input: {e}"))?;
        input.observe(frames)?;
        Ok(())
    })?;
    pcm.flush()
        .map_err(|e| format!("flush resampled PCM: {e}"))?;
    if input.frames == 0 || resampled.output_frames != input.frames {
        return Err(
            "resampled PCM is empty or callback frame count differs from media result".into(),
        );
    }
    if file_sha256(source)? != source_hash {
        return Err("source bytes changed during resampling".into());
    }
    encoder
        .push_pcm_f32(&[0.0; 2048], 2, SAMPLE_RATE)
        .map_err(|e| format!("encoder suffix: {e}"))?;
    encoder.finish();
    let keep = 3 + input.frames.div_ceil(HOP) + 1;
    let mut produced = 0_u64;
    let mut muxer = PacketWriter::new(create_new(&output.join("canonical.ogg"))?);
    loop {
        let packet = match encoder.next_packet() {
            Ok(packet) => packet,
            Err(Error::Eof) => break,
            Err(error) => return Err(format!("encode final packets: {error}")),
        };
        verify_profile(produced, &packet.data)?;
        if produced < keep {
            let granule = produced
                .saturating_sub(3)
                .saturating_mul(HOP)
                .min(input.frames);
            let end = if produced + 1 == keep {
                PacketWriteEndInfo::EndStream
            } else {
                PacketWriteEndInfo::EndPage
            };
            muxer
                .write_packet(packet.data.into_boxed_slice(), 0x434f434f, end, granule)
                .map_err(|e| format!("mux Ogg packet: {e}"))?;
        }
        produced += 1;
    }
    if produced < keep {
        return Err(format!(
            "fixed profile needs {keep} packets, encoder produced {produced}"
        ));
    }
    muxer
        .into_inner()
        .flush()
        .map_err(|e| format!("flush Ogg: {e}"))?;
    let readback = readback(output, input.frames)?;
    let mut artifacts = serde_json::Map::new();
    for name in ["resampled.f32le", "canonical.ogg", "decoded.f32le"] {
        let path = output.join(name);
        artifacts.insert(
            name.into(),
            json!({
                "sha256": file_sha256(&path)?,
                "bytes": fs::metadata(path).map_err(|e| e.to_string())?.len(),
            }),
        );
    }
    Ok(json!({
        "source": {"bytes": metadata.len(), "sha256": source_hash,
            "sample_rate": resampled.source_sample_rate, "frames": resampled.source_frames},
        "resampled": input.report(),
        "encoder": {"vorbis_q": 10, "quality": quality, "sample_rate": SAMPLE_RATE,
            "channels": 2, "long_block": 2048, "hop": HOP, "padding_frames_each_side": HOP,
            "produced_packets": produced, "kept_packets": keep,
            "buffers_whole_input": true,
            "upstream_archive_sha256": "6eddf79e697ce9279ef0e012432cd2889a7b6305a45efb5bb16e8a1d6338646f",
            "coupling_patch_sha256": "28c3f3ed5d5f35ce239d0ea00c5aedf567859332cc521f665f46a0a5476848db"},
        "readback": readback,
        "artifacts": artifacts,
    }))
}

fn verify_profile(index: u64, data: &[u8]) -> Result<(), String> {
    let valid = match index {
        0 => {
            data.len() == 30
                && &data[..11] == b"\x01vorbis\0\0\0\0"
                && data[11] == 2
                && data[12..16] == SAMPLE_RATE.to_le_bytes()
                && data[28] == 0xb8
                && data[29] == 1
        }
        1 => data.starts_with(b"\x03vorbis"),
        2 => data.starts_with(b"\x05vorbis"),
        _ => data.first().is_some_and(|byte| byte & 15 == 14),
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "packet {index} differs from fixed stereo 48 kHz long-block profile"
        ))
    }
}

fn readback(output: &Path, expected_frames: u64) -> Result<Value, String> {
    let mut reference =
        BufReader::new(File::open(output.join("resampled.f32le")).map_err(|e| e.to_string())?);
    let mut pcm = create_new(&output.join("decoded.f32le"))?;
    let mut stats = PcmStats::default();
    let mut signal_power = [0.0_f64; 2];
    let mut error_power = [0.0_f64; 2];
    let decoded = decode_source(output.join("canonical.ogg"), |rate, frames| {
        if rate != SAMPLE_RATE || stats.frames + frames.len() as u64 > expected_frames {
            return Err(
                "full readback sample rate or frame count differs from canonical input".into(),
            );
        }
        stats.observe(frames)?;
        for frame in frames {
            for channel in 0..2 {
                let mut bytes = [0; 4];
                reference
                    .read_exact(&mut bytes)
                    .map_err(|e| format!("read reference PCM: {e}"))?;
                let source = f64::from(f32::from_le_bytes(bytes));
                signal_power[channel] += source * source;
                error_power[channel] += (source - f64::from(frame[channel])).powi(2);
            }
        }
        write_frames(&mut pcm, frames)
    })?;
    if decoded.sample_rate != SAMPLE_RATE
        || decoded.source_frames != expected_frames
        || stats.frames != expected_frames
    {
        return Err(format!(
            "full readback frame mismatch: expected {expected_frames}, media {}, callback {}",
            decoded.source_frames, stats.frames
        ));
    }
    if reference.read(&mut [0]).map_err(|e| e.to_string())? != 0 {
        return Err("reference PCM has trailing samples".into());
    }
    pcm.flush()
        .map_err(|e| format!("flush readback PCM: {e}"))?;
    let snr: Vec<_> = (0..2)
        .map(|channel| {
            if signal_power[channel] > 0.0 && error_power[channel] > 0.0 {
                json!(10.0 * (signal_power[channel] / error_power[channel]).log10())
            } else {
                Value::Null
            }
        })
        .collect();
    Ok(
        json!({"pcm": stats.report(), "sample_rate": decoded.sample_rate,
        "snr_db": snr, "signal_power": signal_power, "error_power": error_power}),
    )
}

#[derive(Default)]
struct PcmStats {
    frames: u64,
    peak: [f32; 2],
    overs: [u64; 2],
}

impl PcmStats {
    fn observe(&mut self, frames: &[[f32; 2]]) -> Result<(), String> {
        for frame in frames {
            for (channel, value) in frame.iter().enumerate() {
                if !value.is_finite() {
                    return Err("PCM contains non-finite sample".into());
                }
                self.peak[channel] = self.peak[channel].max(value.abs());
                self.overs[channel] += u64::from(value.abs() > 1.0);
            }
        }
        self.frames += frames.len() as u64;
        Ok(())
    }

    fn report(&self) -> Value {
        json!({"frames": self.frames, "peak": self.peak, "overs": self.overs, "non_finite": 0})
    }
}

fn create_new(path: &Path) -> Result<BufWriter<File>, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(BufWriter::new)
        .map_err(|e| format!("create {}: {e}", path.display()))
}

fn write_frames(writer: &mut impl Write, frames: &[[f32; 2]]) -> Result<(), String> {
    for value in frames.iter().flatten() {
        writer
            .write_all(&value.to_le_bytes())
            .map_err(|e| format!("write PCM: {e}"))?;
    }
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("hash {}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("hash {}: {e}", path.display()))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests;
