//! CC0-1.0 test signals streamed as stereo IEEE F32LE WAV

#[path = "../../crates/cocobeat-runtime/src/dev_song.rs"]
#[allow(dead_code)]
mod dev_song;

use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    process::ExitCode,
};

fn original(frame: u32) -> [f32; 2] {
    dev_song::sample(frame % dev_song::FRAMES).map(|value| f32::from(value) / 32_768.0)
}

fn source_44100(frame: u32) -> [f32; 2] {
    let phase = (frame % 200) as i32;
    let triangle = if phase < 100 {
        phase * 2 - 100
    } else {
        300 - phase * 2
    };
    let step = if (frame / 2_205).is_multiple_of(2) {
        64
    } else {
        -64
    };
    let value = (triangle + step) as f32 / 512.0;
    [value, -value]
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [case, destination] = args.as_slice() else {
        return Err("usage: native-vorbis-fixture CASE NEW_WAV".into());
    };
    let case = case.to_str().ok_or("CASE must be UTF-8")?;
    let (rate, frames, signal, sample): (u32, u32, &str, StereoSample) = match case {
        "short-1" => (48_000, 1, "nonzero stereo control", |_| [0.125, -0.25]),
        "short-1024" => (48_000, 1_024, "nonzero stereo control", |_| [0.125, -0.25]),
        "short-1025" => (48_000, 1_025, "nonzero stereo control", |_| [0.125, -0.25]),
        "original-48000" => (
            dev_song::SAMPLE_RATE,
            48_000,
            "original song prefix",
            original,
        ),
        "original-64s" => (
            dev_song::SAMPLE_RATE,
            dev_song::FRAMES,
            "original 64-second song",
            original,
        ),
        "original-600s" => (
            dev_song::SAMPLE_RATE,
            28_800_000,
            "original song repeated every 3072000 frames",
            original,
        ),
        "head-tail-impulses" => (
            48_000,
            48_000,
            "distinct stereo head and tail impulses",
            |frame| match frame {
                0 => [0.75, -0.5],
                47_999 => [-0.5, 0.75],
                _ => [0.0, 0.0],
            },
        ),
        "silence-48000" => (48_000, 48_000, "silence", |_| [0.0, 0.0]),
        "domain-edge" => (48_000, 1_025, "inclusive domain edges", |_| [4.0, -4.0]),
        "positive-next-up" => (
            48_000,
            1_025,
            "first f32 above positive domain edge",
            |_| [4.0_f32.next_up(), 0.0],
        ),
        "negative-next-up" => (
            48_000,
            1_025,
            "first f32 below negative domain edge",
            |_| [-4.0_f32.next_up(), 0.0],
        ),
        "infinity" => (48_000, 1_025, "positive infinity control", |_| {
            [f32::INFINITY, 0.0]
        }),
        "nan" => (48_000, 1_025, "NaN control", |_| [f32::NAN, 0.0]),
        "source-44100" => (
            44_100,
            44_100,
            "integer triangle plus step, anti-phase stereo",
            source_44100,
        ),
        _ => return Err(format!("Unknown fixture case: {case}").into()),
    };
    let data_bytes = frames * 8;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut writer = BufWriter::with_capacity(64 * 1_024, file);
    writer.write_all(b"RIFF")?;
    writer.write_all(&(36 + data_bytes).to_le_bytes())?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&16_u32.to_le_bytes())?;
    writer.write_all(&3_u16.to_le_bytes())?;
    writer.write_all(&2_u16.to_le_bytes())?;
    writer.write_all(&rate.to_le_bytes())?;
    writer.write_all(&(rate * 8).to_le_bytes())?;
    writer.write_all(&8_u16.to_le_bytes())?;
    writer.write_all(&32_u16.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&data_bytes.to_le_bytes())?;
    for frame in 0..frames {
        for value in sample(frame) {
            writer.write_all(&value.to_le_bytes())?;
        }
    }
    writer.flush()?;
    let generator = if case.starts_with("original-") {
        "runtime/dev_song.rs"
    } else {
        "QA integer control"
    };
    println!(
        "{{\"case\":\"{case}\",\"source_rate\":{rate},\"source_frames\":{frames},\"signal\":\"{signal}\",\"license\":\"CC0-1.0\",\"generator\":\"{generator}\"}}"
    );
    Ok(())
}

type StereoSample = fn(u32) -> [f32; 2];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
