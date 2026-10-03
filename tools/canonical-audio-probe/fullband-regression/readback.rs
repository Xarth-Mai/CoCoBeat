use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: fullband-strict-readback <canonical.ogg> <expected-frames> <new-output>".into(),
        );
    }
    let expected = args[1]
        .to_str()
        .ok_or("non-UTF8 frame count")?
        .parse::<u64>()
        .map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])
        .map_err(|e| e.to_string())?;
    let mut output = BufWriter::new(file);
    let frames = cocobeat_media::decode_canonical(&args[0], expected, |block| {
        for frame in block {
            for sample in frame {
                output
                    .write_all(&sample.to_le_bytes())
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })?;
    output.flush().map_err(|e| e.to_string())?;
    println!("{{\"frames\":{frames},\"sample_rate\":48000,\"channels\":2}}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
