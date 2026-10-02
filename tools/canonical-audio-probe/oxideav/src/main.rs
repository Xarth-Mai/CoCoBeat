use oxideav_vorbis::{StreamEncoderConfig, encode_pcm_to_ogg};
use std::{error::Error, fs, io::Write, path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(4..=5).contains(&args.len()) {
        return Err(
            "usage: cocobeat-oxideav-probe OUTPUT_DIR FRAMES QUALITY_0_TO_1 [tail|burst]".into(),
        );
    }
    let root = PathBuf::from(&args[1]);
    let n: usize = args[2].parse()?;
    let quality: f32 = args[3].parse()?;
    fs::create_dir_all(&root)?;
    let mut pcm = vec![
        (0..n)
            .map(|i| (0.125 * (std::f64::consts::TAU * 440.0 * i as f64 / 48_000.0).sin()) as f32)
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
    ];
    let signal = args.get(4).map(String::as_str).unwrap_or("tail");
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
    } else if signal != "tail" {
        return Err("unknown signal".into());
    }
    fs::write(root.join("signal.txt"), format!("{signal}\n"))?;
    let mut input = fs::File::create(root.join("input.f32le"))?;
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
