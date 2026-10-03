use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

pub fn decode(input: &Path, output: &Path) -> Result<(), String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("Create {}: {error}", output.display()))?;
    let result = (|| {
        let mut writer = BufWriter::new(file);
        let decoded = cocobeat_media::decode_source(input, |_, frames| {
            for frame in frames {
                for sample in frame {
                    writer
                        .write_all(&sample.to_le_bytes())
                        .map_err(|error| format!("Write {}: {error}", output.display()))?;
                }
            }
            Ok(())
        })?;
        writer
            .flush()
            .map_err(|error| format!("Flush {}: {error}", output.display()))?;
        Ok::<_, String>(decoded)
    })();
    match result {
        Ok(decoded) => {
            println!(
                "Source decoded: {} Hz, stereo F32LE, {} frames",
                decoded.sample_rate, decoded.source_frames
            );
            println!("Source PCM only; canonical encoding and SongPackage validation are pending.");
            Ok(())
        }
        Err(error) => {
            fs::remove_file(output).map_err(|cleanup| {
                format!(
                    "{error}; remove partial output {}: {cleanup}",
                    output.display()
                )
            })?;
            Err(error)
        }
    }
}
