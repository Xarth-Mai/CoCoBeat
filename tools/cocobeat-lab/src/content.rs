use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use cocobeat_runtime::dev_song::{FRAMES, SAMPLE_RATE, sample};
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn generate(output: &Path) -> io::Result<()> {
    fs::create_dir_all(output)?;
    let path = output.join("cocobeat-64.wav");
    let mut writer = BufWriter::new(File::create(&path)?);
    let checksum = write_wav(&mut writer)?;
    writer.flush()?;
    fs::write(
        output.join("cocobeat-64.txt"),
        format!(
            "CoCoBeat original development audio v1\nlicense=CC0-1.0\n\
             format=PCM s16le stereo\nsample_rate=48000\nframes=3072000\n\
             bpm=120\nmeter=4/4\nbytes=12288044\nfnv1a64={checksum:016x}\n\
             source=crates/cocobeat-runtime/src/dev_song.rs\n\
             annotations=assets/dev/vertical_slice/event_frames.csv\n\
             DEVELOPMENT SOURCE ONLY; canonical Ogg encode/readback NOT RUN\n"
        ),
    )?;
    println!("{}: 64 s / 48 kHz / stereo / PCM16", path.display());
    println!("FNV-1a64 {checksum:016x} (reproducibility checksum, not a security hash)");
    Ok(())
}

fn write_wav(writer: &mut impl Write) -> io::Result<u64> {
    let data_bytes = FRAMES * 4;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&1_u16.to_le_bytes());
    header.extend_from_slice(&2_u16.to_le_bytes());
    header.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    header.extend_from_slice(&4_u16.to_le_bytes());
    header.extend_from_slice(&16_u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    let mut hash = FNV_OFFSET;
    let mut emit = |bytes: &[u8]| -> io::Result<()> {
        writer.write_all(bytes)?;
        for byte in bytes {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
        }
        Ok(())
    };
    emit(&header)?;
    for frame in 0..FRAMES {
        let [left, right] = sample(frame);
        emit(&left.to_le_bytes())?;
        emit(&right.to_le_bytes())?;
    }
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_content_has_exact_length_silence_and_stable_bytes() {
        let mut bytes = Vec::new();
        let checksum = write_wav(&mut bytes).unwrap();
        assert_eq!(bytes.len(), 12_288_044);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[24..28], &48_000_u32.to_le_bytes());
        assert!(
            bytes[44 + 1_920_000 * 4..44 + 2_304_000 * 4]
                .iter()
                .all(|b| *b == 0)
        );
        assert_eq!(checksum, 0xc7b1_f9a2_5ff6_458a);
        assert!(bytes[44..384_000 * 4].iter().any(|b| *b != 0));
        assert_eq!(sample(3_071_999), [0, 0]);
        let anchors = cocobeat_runtime::dev_song::anchors();
        let mut anchor_csv = String::from("anchor_id,song_frame\n");
        for anchor in &anchors {
            anchor_csv.push_str(&format!("{},{}\n", anchor.id, anchor.song_time.frames()));
        }
        assert_eq!(
            anchor_csv,
            include_str!("../../../assets/dev/vertical_slice/anchors.csv")
        );
        let labeled_frames: Vec<i64> =
            include_str!("../../../assets/dev/vertical_slice/event_frames.csv")
                .lines()
                .filter(|line| line.starts_with("anchor_"))
                .map(|line| line.split(',').nth(2).unwrap().parse().unwrap())
                .collect();
        assert_eq!(
            anchors
                .iter()
                .map(|anchor| anchor.song_time.frames())
                .collect::<Vec<_>>(),
            labeled_frames
        );
    }
}
