//! Bounded source decoding and strict readback of the final canonical file

use cocobeat_schema::CANONICAL_SAMPLE_RATE;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use symphonia::core::{
    checksum::Crc32,
    codecs::audio::{AudioCodecId, AudioDecoderOptions, well_known::*},
    common::Limit,
    formats::{FormatId, FormatOptions, probe::Hint, well_known::*},
    io::{MediaSourceStream, Monitor, ReadOnlySource},
    meta::MetadataOptions,
};

pub const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_SOURCE_SAMPLE_RATE: u32 = 192_000;
pub const MAX_SOURCE_SECONDS: u64 = 600;
// FLAC allows at most 65,535 frames in one block
const MAX_BLOCK_FRAMES: u64 = 65_536;
const MAX_PACKET_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedSource {
    pub sample_rate: u32,
    pub source_frames: u64,
}

/// Delivers finite stereo blocks at the original sample rate, duplicating mono
/// A callback error stops decoding immediately; earlier blocks remain provisional
/// until this function succeeds, so consumers must discard partial imports on error
pub fn decode_source(
    path: impl AsRef<Path>,
    consume: impl FnMut(u32, &[[f32; 2]]) -> Result<(), String>,
) -> Result<DecodedSource, String> {
    decode(path.as_ref(), None, consume)
}

/// Fully reads the final Ogg Vorbis file on its original 48 kHz stereo timeline
/// Expected frames come from the input to the encoder, not from this file's header
/// Blocks remain provisional until success; no remixing, resampling or clipping occurs
pub fn decode_canonical(
    path: impl AsRef<Path>,
    expected_frames: u64,
    mut consume: impl FnMut(&[[f32; 2]]) -> Result<(), String>,
) -> Result<u64, String> {
    if !(1..=u64::from(CANONICAL_SAMPLE_RATE) * MAX_SOURCE_SECONDS).contains(&expected_frames) {
        return Err(
            "Expected canonical frames must cover more than zero and at most ten minutes".into(),
        );
    }
    decode(path.as_ref(), Some(expected_frames), |_, frames| {
        consume(frames)
    })
    .map(|decoded| decoded.source_frames)
}

fn decode(
    path: &Path,
    expected_canonical_frames: Option<u64>,
    mut consume: impl FnMut(u32, &[[f32; 2]]) -> Result<(), String>,
) -> Result<DecodedSource, String> {
    if !path.is_file() {
        return Err("Audio source must be a regular file".into());
    }
    let file = File::open(path).map_err(|error| format!("Cannot open audio source: {error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE_BYTES {
        return Err("Audio source must be a regular file no larger than 512 MiB".into());
    }
    let mut validation_file = file.try_clone().map_err(|error| error.to_string())?;
    // Sequential import avoids MP3's bitrate-based duration estimate and tail trimming
    let source = ReadOnlySource::new(file.take(metadata.len()));
    // This import stage does not consume tags or artwork
    let metadata_options = MetadataOptions::default()
        .limit_tag_bytes(Limit::Maximum(0))
        .limit_visual_bytes(Limit::Maximum(0));
    let mut format = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            MediaSourceStream::new(Box::new(source), Default::default()),
            FormatOptions::default(),
            metadata_options,
        )
        .map_err(|error| format!("Unsupported or invalid audio source: {error}"))?;
    let ogg_frames = if format.format_info().format == FORMAT_ID_OGG {
        // Cloned File handles share the cursor; restore it behind the demuxer's buffer
        let position = validation_file
            .stream_position()
            .map_err(|error| error.to_string())?;
        let frames = verify_ogg(&mut validation_file, metadata.len())?;
        validation_file
            .seek(SeekFrom::Start(position))
            .map_err(|error| error.to_string())?;
        Some(frames)
    } else {
        None
    };
    let [track] = format.tracks() else {
        return Err("Audio source must contain exactly one track".into());
    };
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or("Audio source has no supported audio track")?;
    if !supported(format.format_info().format, params.codec) {
        return Err("Unsupported format: use WAV/PCM, FLAC, MP3, or Ogg Vorbis".into());
    }
    let sample_rate = params
        .sample_rate
        .ok_or("Audio source has no sample rate")?;
    let channel_layout = params
        .channels
        .as_ref()
        .ok_or("Audio source has no channels")?
        .clone();
    let channels = channel_layout.count();
    if expected_canonical_frames.is_some()
        && (format.format_info().format != FORMAT_ID_OGG
            || params.codec != CODEC_ID_VORBIS
            || sample_rate != CANONICAL_SAMPLE_RATE
            || channels != 2)
    {
        return Err(
            "Canonical audio must be Ogg Vorbis at 48000 Hz with exactly two channels".into(),
        );
    }
    if !(1..=MAX_SOURCE_SAMPLE_RATE).contains(&sample_rate) {
        return Err("Audio source sample rate must be within 1..=192000 Hz".into());
    }
    if !(1..=2).contains(&channels) {
        return Err("Audio source must be mono or stereo".into());
    }
    if !track.time_base.is_some_and(|time_base| {
        u64::from(time_base.numer.get()) * u64::from(sample_rate)
            == u64::from(time_base.denom.get())
    }) {
        return Err("Audio source timestamps must count source sample frames".into());
    }
    if params
        .max_frames_per_packet
        .is_some_and(|n| n > MAX_BLOCK_FRAMES)
        || params
            .extra_data
            .as_ref()
            .is_some_and(|data| data.len() > MAX_PACKET_BYTES)
    {
        return Err("Audio source codec block exceeds the import resource limit".into());
    }
    let declared_frames = ogg_frames.or(track.num_frames);
    if expected_canonical_frames.is_some_and(|expected| declared_frames != Some(expected)) {
        return Err(
            "Canonical audio frame count does not match the expected encoder input length".into(),
        );
    }
    if let Some(frames) = declared_frames {
        checked_frame_count(0, frames, sample_rate)?;
    }
    let track_id = track.id;
    let mp3_padding =
        (params.codec == CODEC_ID_MP3).then_some(u64::from(track.padding.unwrap_or(0)));
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default().verify(true))
        .map_err(|error| format!("Cannot decode audio source: {error}"))?;
    let mut interleaved = Vec::<f32>::new();
    let mut stereo = Vec::<[f32; 2]>::new();
    let mut source_frames = 0;
    while let Some(packet) = format
        .next_packet()
        .map_err(|error| format!("Cannot read audio packet: {error}"))?
    {
        if format.tracks().len() != 1 || packet.track_id != track_id {
            return Err("Audio source changed tracks during import".into());
        }
        if packet.data.len() > MAX_PACKET_BYTES {
            return Err("Audio packet exceeds the import resource limit".into());
        }
        if mp3_padding.is_some_and(|padding| packet.trim_end.get() > padding) {
            return Err("MP3 packets extend past the declared audio and encoder padding".into());
        }
        let audio = decoder
            .decode(&packet)
            .map_err(|error| format!("Cannot decode audio packet: {error}"))?;
        if audio.spec().rate() != sample_rate
            || audio.spec().channels().count() != channels
            || (channels == 2 && audio.spec().channels() != &channel_layout)
        {
            return Err("Audio source changed sample rate or channels during import".into());
        }
        if audio.frames() as u64 > MAX_BLOCK_FRAMES {
            return Err("Decoded audio block exceeds the import resource limit".into());
        }
        if audio.frames() == 0 {
            continue;
        }
        let first_frame = packet
            .pts
            .checked_add(packet.trim_start)
            .ok_or("Audio packet timestamp overflow")?
            .get();
        if first_frame != source_frames as i64 {
            return Err("Audio source contains missing or overlapping frames".into());
        }
        source_frames = checked_frame_count(source_frames, audio.frames() as u64, sample_rate)?;
        if expected_canonical_frames.is_some_and(|expected| source_frames > expected) {
            return Err("Canonical audio decoded beyond the expected frame count".into());
        }
        interleaved.resize(audio.samples_interleaved(), 0.0);
        audio.copy_to_slice_interleaved(&mut interleaved);
        if interleaved.iter().any(|sample| !sample.is_finite()) {
            return Err("Audio source contains non-finite samples".into());
        }
        stereo.clear();
        stereo.extend(
            interleaved
                .chunks_exact(channels)
                .map(|frame| [frame[0], frame[channels - 1]]),
        );
        consume(sample_rate, &stereo)?;
    }
    if source_frames == 0 {
        return Err("Audio source contains no audio frames".into());
    }
    if declared_frames.is_some_and(|frames| frames != source_frames) {
        return Err("Audio source frame count does not match its declared length".into());
    }
    if decoder.finalize().verify_ok == Some(false) {
        return Err("Audio source checksum verification failed".into());
    }
    Ok(DecodedSource {
        sample_rate,
        source_frames,
    })
}

fn supported(format: FormatId, codec: AudioCodecId) -> bool {
    match format {
        FORMAT_ID_FLAC => codec == CODEC_ID_FLAC,
        FORMAT_ID_MP3 => codec == CODEC_ID_MP3,
        FORMAT_ID_OGG => codec == CODEC_ID_VORBIS,
        FORMAT_ID_WAVE => matches!(
            codec,
            CODEC_ID_PCM_U8
                | CODEC_ID_PCM_S8
                | CODEC_ID_PCM_U16LE
                | CODEC_ID_PCM_U16BE
                | CODEC_ID_PCM_S16LE
                | CODEC_ID_PCM_S16BE
                | CODEC_ID_PCM_U24LE
                | CODEC_ID_PCM_U24BE
                | CODEC_ID_PCM_S24LE
                | CODEC_ID_PCM_S24BE
                | CODEC_ID_PCM_U32LE
                | CODEC_ID_PCM_U32BE
                | CODEC_ID_PCM_S32LE
                | CODEC_ID_PCM_S32BE
                | CODEC_ID_PCM_F32LE
                | CODEC_ID_PCM_F32BE
                | CODEC_ID_PCM_F64LE
                | CODEC_ID_PCM_F64BE
        ),
        _ => false,
    }
}

fn checked_frame_count(total: u64, added: u64, rate: u32) -> Result<u64, String> {
    total
        .checked_add(added)
        .filter(|&frames| frames <= u64::from(rate) * MAX_SOURCE_SECONDS)
        .ok_or_else(|| "Audio source exceeds the ten-minute duration limit".into())
}

// Symphonia can recover past damaged Ogg pages; imports must reject that time loss
fn verify_ogg(file: &mut File, length: u64) -> Result<u64, String> {
    file.rewind().map_err(|error| error.to_string())?;
    let mut position = 0;
    let mut serial = None;
    let mut sequence = 0;
    let mut continuation = false;
    let mut granule = 0;
    let mut ended = false;
    let mut body = Vec::new();
    while position < length {
        if ended || length - position < 27 {
            return Err("Ogg contains trailing or truncated page data".into());
        }
        let mut header = [0; 27];
        file.read_exact(&mut header)
            .map_err(|error| error.to_string())?;
        let flags = header[5];
        let page_serial = u32::from_le_bytes(header[14..18].try_into().unwrap());
        let page_sequence = u32::from_le_bytes(header[18..22].try_into().unwrap());
        if &header[..4] != b"OggS"
            || header[4] != 0
            || flags & !7 != 0
            || (flags & 2 != 0) != serial.is_none()
            || (flags & 1 != 0) != continuation
            || serial.is_some_and(|serial| serial != page_serial)
            || page_sequence != sequence
        {
            return Err("Ogg contains invalid, missing, chained, or multiplexed pages".into());
        }
        serial = Some(page_serial);
        sequence += 1;
        let mut lacing = [0; 255];
        let lacing = &mut lacing[..usize::from(header[26])];
        if position + 27 + lacing.len() as u64 > length {
            return Err("Ogg page lacing is truncated".into());
        }
        file.read_exact(lacing).map_err(|error| error.to_string())?;
        let body_len = lacing.iter().map(|&size| usize::from(size)).sum::<usize>();
        position += 27 + lacing.len() as u64 + body_len as u64;
        if position > length {
            return Err("Ogg page body is truncated".into());
        }
        // At most 255 segments of 255 bytes each, independent of file declarations
        body.resize(body_len, 0);
        file.read_exact(&mut body)
            .map_err(|error| error.to_string())?;
        let expected_crc = u32::from_le_bytes(header[22..26].try_into().unwrap());
        header[22..26].fill(0);
        let mut crc = Crc32::new(0);
        crc.process_buf_bytes(&header);
        crc.process_buf_bytes(lacing);
        crc.process_buf_bytes(&body);
        if crc.crc() != expected_crc {
            return Err("Ogg page checksum verification failed".into());
        }
        if let Some(&last) = lacing.last() {
            continuation = last == 255;
        }
        let page_granule = u64::from_le_bytes(header[6..14].try_into().unwrap());
        if page_granule != u64::MAX {
            if page_granule < granule {
                return Err("Ogg granule positions moved backwards".into());
            }
            granule = page_granule;
        }
        ended = flags & 4 != 0;
        if ended && (continuation || page_granule == u64::MAX) {
            return Err("Ogg final page has an incomplete packet or unknown frame count".into());
        }
    }
    if !ended {
        return Err("Ogg source has no end-of-stream page".into());
    }
    Ok(granule)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    struct TestSource(PathBuf);

    impl TestSource {
        fn new(bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "cocobeat-decode-{}-{}.wav",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }

    impl Drop for TestSource {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn wave(rate: u32, channels: u16, samples: &[f32]) -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((36 + samples.len() as u32 * 4).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(3u16.to_le_bytes());
        bytes.extend(channels.to_le_bytes());
        bytes.extend(rate.to_le_bytes());
        bytes.extend((rate * u32::from(channels) * 4).to_le_bytes());
        bytes.extend((channels * 4).to_le_bytes());
        bytes.extend(32u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend((samples.len() as u32 * 4).to_le_bytes());
        bytes.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
        bytes
    }

    #[test]
    fn original_frames_silence_and_level_survive_mono_and_stereo_decode() {
        for channels in [1, 2] {
            let samples = if channels == 1 {
                vec![0.0, 1.5, -0.25, 0.0]
            } else {
                vec![0.0, 0.0, 1.5, -0.5, -0.25, 0.75, 0.0, 0.0]
            };
            let source = TestSource::new(&wave(44_100, channels, &samples));
            let mut actual = Vec::new();
            let info = decode_source(&source.0, |rate, block| {
                assert_eq!(rate, 44_100);
                actual.extend_from_slice(block);
                Ok(())
            })
            .unwrap();
            assert_eq!(
                info,
                DecodedSource {
                    sample_rate: 44_100,
                    source_frames: 4
                }
            );
            let expected = if channels == 1 {
                vec![[0.0, 0.0], [1.5, 1.5], [-0.25, -0.25], [0.0, 0.0]]
            } else {
                vec![[0.0, 0.0], [1.5, -0.5], [-0.25, 0.75], [0.0, 0.0]]
            };
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn invalid_sources_and_consumer_errors_never_report_success() {
        for bytes in [
            wave(192_001, 1, &[0.0]),
            wave(48_000, 3, &[0.0; 3]),
            wave(48_000, 1, &[]),
            wave(48_000, 1, &[f32::NAN]),
            wave(48_000, 1, &[f32::INFINITY]),
            b"not audio".to_vec(),
            wave(1, 1, &vec![0.0; 601]),
        ] {
            let source = TestSource::new(&bytes);
            assert!(decode_source(&source.0, |_, _| Ok(())).is_err());
        }
        let source = TestSource::new(&wave(48_000, 1, &[0.0]));
        assert_eq!(
            decode_source(&source.0, |_, _| Err("cancelled".into())),
            Err("cancelled".into())
        );
        File::options()
            .write(true)
            .open(&source.0)
            .unwrap()
            .set_len(MAX_SOURCE_BYTES + 1)
            .unwrap();
        assert!(decode_source(&source.0, |_, _| panic!("oversize source was decoded")).is_err());
        assert_eq!(checked_frame_count(28_799_999, 1, 48_000), Ok(28_800_000));
        assert!(checked_frame_count(28_800_000, 1, 48_000).is_err());
        assert!(checked_frame_count(u64::MAX, 1, 48_000).is_err());
        assert!(!supported(FORMAT_ID_OGG, CODEC_ID_FLAC));
        assert!(!supported(FORMAT_ID_WAVE, CODEC_ID_ADPCM_IMA_WAV));
    }

    #[test]
    fn codec_padding_and_ogg_gaps_preserve_or_reject_the_original_timeline() {
        let mp3 = include_bytes!("../../../testdata/synthetic/media-import/mono.mp3");
        let ogg = include_bytes!("../../../testdata/synthetic/media-import/mono.ogg");
        for (bytes, rate, frames) in [
            (mp3.as_slice(), 44_100, 8_820),
            (ogg.as_slice(), 48_000, 100_800),
        ] {
            let source = TestSource::new(bytes);
            let mut delivered = 0;
            let actual = decode_source(&source.0, |actual_rate, block| {
                assert_eq!(actual_rate, rate);
                assert!(block.iter().all(|frame| frame[0] == frame[1]));
                delivered += block.len() as u64;
                Ok(())
            })
            .unwrap();
            assert_eq!(actual.source_frames, frames);
            assert_eq!(delivered, frames);
        }

        // Under-reporting Xing length must not silently crop real trailing packets
        let xing = mp3.windows(4).position(|bytes| bytes == b"Xing").unwrap();
        for frames in [4u32, 8, 10] {
            let mut corrupted = mp3.to_vec();
            corrupted[xing + 8..xing + 12].copy_from_slice(&frames.to_be_bytes());
            let source = TestSource::new(&corrupted);
            assert!(decode_source(&source.0, |_, _| Ok(())).is_err());
        }

        // Mutate only the middle audio page; the original headers and EOS survive
        let mut pages = Vec::new();
        let mut start = 0;
        while start < ogg.len() {
            assert_eq!(&ogg[start..start + 4], b"OggS");
            let header_end = start + 27 + usize::from(ogg[start + 26]);
            let end = header_end
                + ogg[start + 27..header_end]
                    .iter()
                    .map(|&length| usize::from(length))
                    .sum::<usize>();
            pages.push(start..end);
            start = end;
        }
        assert_eq!(pages.len(), 5);
        let middle = pages[3].clone();
        let mut corrupted = ogg.to_vec();
        corrupted[middle.start + 22] ^= 1;
        let mut missing = ogg.to_vec();
        missing.drain(middle);
        for bytes in [corrupted, missing] {
            let source = TestSource::new(&bytes);
            let error = decode_source(&source.0, |_, _| Ok(())).unwrap_err();
            assert!(error.contains("Ogg"), "{error}");
        }
    }

    #[test]
    fn canonical_readback_preserves_stereo_and_stops_on_consumer_error() {
        let source = TestSource::new(include_bytes!(
            "../../../testdata/synthetic/media-import/stereo-canonical.ogg"
        ));
        let mut frames = Vec::new();
        assert_eq!(
            decode_canonical(&source.0, 4_800, |block| {
                frames.extend_from_slice(block);
                Ok(())
            }),
            Ok(4_800)
        );
        assert_eq!(frames.len(), 4_800);
        assert!(frames.iter().flatten().all(|sample| sample.is_finite()));
        // Independent 440/880 Hz channels must not be silently duplicated or swapped
        for channel in 0..2 {
            let frequency = if channel == 0 { 440.0 } else { 880.0 };
            let amplitude = if channel == 0 { 0.1 } else { 0.2 };
            let error = frames[480..4_320]
                .iter()
                .enumerate()
                .map(|(offset, frame)| {
                    let phase = std::f32::consts::TAU * frequency * (offset + 480) as f32
                        / CANONICAL_SAMPLE_RATE as f32;
                    (frame[channel] - amplitude * phase.sin()).powi(2)
                })
                .sum::<f32>()
                / 3_840.0;
            assert!(error.sqrt() < 0.01, "channel {channel}: {error}");
        }
        let mut callbacks = 0;
        assert_eq!(
            decode_canonical(&source.0, 4_800, |_| {
                callbacks += 1;
                Err("cancelled".into())
            }),
            Err("cancelled".into())
        );
        assert_eq!(callbacks, 1);
    }

    #[test]
    fn canonical_readback_rejects_wrong_contracts_and_corrupted_files() {
        fn repair_crc(page: &mut [u8]) {
            page[22..26].fill(0);
            let mut crc = Crc32::new(0);
            crc.process_buf_bytes(page);
            page[22..26].copy_from_slice(&crc.crc().to_le_bytes());
        }

        let ogg = include_bytes!("../../../testdata/synthetic/media-import/stereo-canonical.ogg");
        let source = TestSource::new(ogg);
        for expected in [0, 4_799, 4_801, 28_800_001, u64::MAX] {
            assert!(
                decode_canonical(&source.0, expected, |_| panic!(
                    "invalid length delivered PCM"
                ))
                .is_err()
            );
        }
        let mut wrong_rate = ogg.to_vec();
        let first_page_end = 27
            + usize::from(ogg[26])
            + ogg[27..27 + usize::from(ogg[26])]
                .iter()
                .map(|&length| usize::from(length))
                .sum::<usize>();
        let identification = ogg
            .windows(7)
            .position(|bytes| bytes == b"\x01vorbis")
            .unwrap();
        wrong_rate[identification + 12..identification + 16]
            .copy_from_slice(&44_100u32.to_le_bytes());
        repair_crc(&mut wrong_rate[..first_page_end]);
        let mut corrupt = ogg.to_vec();
        *corrupt.last_mut().unwrap() ^= 1;
        for (bytes, expected) in [
            (wave(48_000, 2, &[0.1, 0.2]), 1),
            (
                include_bytes!("../../../testdata/synthetic/media-import/mono.ogg").to_vec(),
                100_800,
            ),
            (wrong_rate, 4_800),
            (corrupt, 4_800),
            (ogg[..ogg.len() - 1].to_vec(), 4_800),
        ] {
            let source = TestSource::new(&bytes);
            assert!(
                decode_canonical(&source.0, expected, |_| panic!(
                    "invalid file delivered PCM"
                ))
                .is_err()
            );
        }
        // A matching EOS declaration alone cannot establish a continuous frame-zero timeline
        let mut false_length = ogg.to_vec();
        let last_page = ogg.windows(4).rposition(|bytes| bytes == b"OggS").unwrap();
        assert_eq!(false_length[last_page + 5] & 4, 4);
        false_length[last_page + 6..last_page + 14].copy_from_slice(&96_000u64.to_le_bytes());
        repair_crc(&mut false_length[last_page..]);
        let source = TestSource::new(&false_length);
        let mut delivered = 0;
        let error = decode_canonical(&source.0, 96_000, |block| {
            delivered += block.len();
            Ok(())
        })
        .unwrap_err();
        assert_eq!(delivered, 0);
        assert!(error.contains("missing or overlapping frames"), "{error}");
    }
}
