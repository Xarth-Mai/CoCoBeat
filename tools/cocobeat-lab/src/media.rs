use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

pub enum Operation {
    Decode,
    Resample,
    Readback(u64),
}

pub fn decode(input: &Path, output: &Path, operation: Operation) -> Result<(), String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| format!("Create {}: {error}", output.display()))?;
    let result = (|| {
        let mut writer = BufWriter::new(file);
        let mut consume = |frames: &[[f32; 2]]| {
            for frame in frames {
                for sample in frame {
                    writer
                        .write_all(&sample.to_le_bytes())
                        .map_err(|error| format!("Write {}: {error}", output.display()))?;
                }
            }
            Ok(())
        };
        let summary = match operation {
            Operation::Resample => {
                let decoded = cocobeat_media::resample_source(input, &mut consume)?;
                format!(
                    "Resampled: {} Hz / {} source frames -> 48000 Hz, stereo F32LE, {} frames",
                    decoded.source_sample_rate, decoded.source_frames, decoded.output_frames
                )
            }
            Operation::Decode => {
                let decoded = cocobeat_media::decode_source(input, |_, frames| consume(frames))?;
                format!(
                    "Source decoded: {} Hz, stereo F32LE, {} frames",
                    decoded.sample_rate, decoded.source_frames
                )
            }
            Operation::Readback(expected) => {
                let frames = cocobeat_media::decode_canonical(input, expected, &mut consume)?;
                format!("Canonical readback: Ogg Vorbis, 48000 Hz, stereo F32LE, {frames} frames")
            }
        };
        writer
            .flush()
            .map_err(|error| format!("Flush {}: {error}", output.display()))?;
        Ok::<_, String>(summary)
    })();
    match result {
        Ok(summary) => {
            println!("{summary}");
            println!("PCM only; encoder admission and SongPackage Ready are separate gates.");
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
