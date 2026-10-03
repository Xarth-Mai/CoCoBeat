use oxideav_vorbis::{StreamEncoderConfig, encode_pcm_to_ogg};
use std::{
    error::Error,
    fs,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

const MAX_FRAMES: usize = 28_800_000;

fn checked_frames(frames: u64) -> Result<usize, Box<dyn Error>> {
    if frames > MAX_FRAMES as u64 {
        return Err("input exceeds the 28800000-frame experimental limit".into());
    }
    Ok(frames as usize)
}

fn read_pcm(path: &Path) -> Result<Vec<Vec<f32>>, Box<dyn Error>> {
    let file = fs::File::open(path)?;
    let bytes = file.metadata()?.len();
    if bytes == 0 || bytes % 8 != 0 {
        return Err("input must contain nonempty complete stereo F32LE frames".into());
    }
    let frames = checked_frames(bytes / 8)?;
    let mut input = BufReader::new(file);
    let mut pcm = vec![vec![0.0; frames]; 2];
    for frame in 0..frames {
        for channel in &mut pcm {
            let mut bytes = [0; 4];
            input.read_exact(&mut bytes)?;
            let sample = f32::from_le_bytes(bytes);
            if !sample.is_finite() || sample.abs() > 1.0 {
                return Err("input samples must be finite and within [-1, 1]".into());
            }
            channel[frame] = sample;
        }
    }
    if input.read(&mut [0])? != 0 {
        return Err("input length changed while reading".into());
    }
    Ok(pcm)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(4..=5).contains(&args.len()) || (args[2] == "--input" && args.len() != 5) {
        return Err(
            "usage: cocobeat-oxideav-probe OUTPUT_DIR FRAMES QUALITY_0_TO_1 [tail|burst|near-full]\n       cocobeat-oxideav-probe OUTPUT_DIR --input INPUT_F32LE QUALITY_0_TO_1"
                .into(),
        );
    }
    let root = PathBuf::from(&args[1]);
    let external = args[2] == "--input";
    let quality: f32 = args[if external { 4 } else { 3 }].parse()?;
    if !quality.is_finite() || !(0.0..=1.0).contains(&quality) {
        return Err("quality must be finite and within [0, 1]".into());
    }
    let mut pcm = if external {
        read_pcm(Path::new(&args[3]))?
    } else {
        let n = checked_frames(args[2].parse()?)?;
        vec![
            (0..n)
                .map(|i| {
                    (0.125 * (std::f64::consts::TAU * 440.0 * i as f64 / 48_000.0).sin()) as f32
                })
                .collect::<Vec<_>>(),
            (0..n)
                .map(|i| {
                    if i < n.saturating_sub(128) {
                        0.0
                    } else if i % 2 == 0 {
                        0.75
                    } else {
                        -0.75
                    }
                })
                .collect::<Vec<_>>(),
        ]
    };
    let n = pcm[0].len();
    let signal = if external {
        "external"
    } else {
        args.get(4).map(String::as_str).unwrap_or("tail")
    };
    if signal == "burst" {
        if n < 512 {
            return Err("burst requires at least 512 frames".into());
        }
        let mut burst: Vec<f32> = (0..256)
            .map(|i| {
                ((std::f64::consts::PI * i as f64 / 255.0).sin().powi(2)
                    * (std::f64::consts::TAU * 1_000.0 * i as f64 / 48_000.0).sin())
                    as f32
            })
            .collect();
        let peak = burst.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
        for value in &mut burst {
            *value *= 0.5 / peak;
        }
        pcm[1].fill(0.0);
        pcm[1][..256].copy_from_slice(&burst);
        pcm[1][n - 256..].copy_from_slice(&burst);
    } else if signal == "near-full" {
        // Match the Rusty probe's f32 rounding so both encode identical PCM
        for (channel, frequency) in pcm.iter_mut().zip([440.0, 1000.0]) {
            for (i, sample) in channel.iter_mut().enumerate() {
                *sample =
                    0.99 * (std::f64::consts::TAU * frequency * i as f64 / 48_000.0).sin() as f32;
            }
        }
    } else if signal != "tail" && !external {
        return Err("unknown signal".into());
    }
    fs::create_dir_all(&root)?;
    fs::write(root.join("signal.txt"), format!("{signal}\n"))?;
    let mut input = BufWriter::new(fs::File::create(root.join("input.f32le"))?);
    for i in 0..n {
        for channel in &pcm {
            input.write_all(&channel[i].to_le_bytes())?;
        }
    }
    input.flush()?;
    let mut config = StreamEncoderConfig::new(48_000, 2);
    config.quality = quality;
    fs::write(root.join("config.txt"), format!("{config:#?}\n"))?;
    let started = Instant::now();
    let encoded = encode_pcm_to_ogg(&pcm, &config)?;
    let elapsed = started.elapsed().as_secs_f64();
    fs::write(root.join("encoded.ogg"), &encoded)?;
    println!(
        "{{\"frames\":{n},\"quality\":{quality},\"bytes\":{},\"encode_seconds\":{elapsed}}}",
        encoded.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_input_preserves_channels_and_rejects_invalid_frames() {
        assert_eq!(checked_frames(MAX_FRAMES as u64).unwrap(), MAX_FRAMES);
        assert!(checked_frames(MAX_FRAMES as u64 + 1).is_err());
        assert!(checked_frames(u64::MAX).is_err());
        let path = std::env::temp_dir().join(format!(
            "cocobeat-oxideav-input-check-{}.f32le",
            std::process::id()
        ));
        let valid: Vec<u8> = [0.25f32, -0.5, 1.0, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        fs::write(&path, &valid).unwrap();
        assert_eq!(read_pcm(&path).unwrap(), [vec![0.25, 1.0], vec![-0.5, 0.0]]);
        for bytes in [Vec::new(), valid[..7].to_vec()] {
            fs::write(&path, bytes).unwrap();
            assert!(read_pcm(&path).is_err());
        }
        for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.01, -1.01] {
            let bytes: Vec<u8> = [sample, 0.0]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect();
            fs::write(&path, bytes).unwrap();
            assert!(read_pcm(&path).is_err());
        }
        fs::File::create(&path)
            .unwrap()
            .set_len((MAX_FRAMES as u64 + 1) * 8)
            .unwrap();
        assert!(read_pcm(&path).unwrap_err().to_string().contains("limit"));
        fs::remove_file(path).unwrap();
    }
}
