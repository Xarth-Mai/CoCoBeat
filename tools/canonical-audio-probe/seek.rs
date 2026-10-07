use std::{
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
};
use symphonia::core::{
    audio::sample::Sample,
    codecs::audio::AudioDecoderOptions,
    formats::{FormatOptions, SeekMode, SeekTo, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
    units::Timestamp,
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 6 {
        return Err(
            "usage: seek INPUT_OGG OUTPUT_DIR WINDOW_FRAMES PREROLL_FRAMES TARGET_FRAME...".into(),
        );
    }
    let output = PathBuf::from(&args[2]);
    let window: usize = args[3].parse()?;
    let preroll: i64 = args[4].parse()?;
    if ![0, 1024].contains(&preroll) {
        return Err("this experiment accepts preroll 0 or 1024".into());
    }
    if !(1..=48_000).contains(&window) {
        return Err("window must be 1..=48000 frames".into());
    }
    let open = || -> Result<_, Box<dyn Error>> {
        let mss = MediaSourceStream::new(Box::new(File::open(&args[1])?), Default::default());
        let format = symphonia::default::get_probe().probe(
            &Hint::new(),
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or("no audio track")?;
        let params = track
            .codec_params
            .as_ref()
            .ok_or("no codec")?
            .audio()
            .ok_or("not audio")?;
        let time_base = track.time_base.ok_or("no time base")?;
        let track_start = track.start_ts.get();
        let delay = track.delay.unwrap_or(0);
        let total = track.num_frames.ok_or("missing valid frame count")?;
        if preroll != 0 && delay > 1024 {
            return Err("extra preroll experiment requires observed delay<=1024".into());
        }
        if params.sample_rate != Some(48_000)
            || params.channels.as_ref().map(|c| c.count()) != Some(2)
            || time_base.numer.get() != 1
            || time_base.denom.get() != 48_000
            || track_start.checked_add(i64::from(delay)) != Some(0)
        {
            return Err(format!("experiment requires 48k stereo, playable start=0, frame time base=1/48000; got rate={:?}, channels={:?}, time_base={time_base:?}, start={}", params.sample_rate, params.channels.as_ref().map(|c| c.count()), track.start_ts.get()).into());
        }
        let id = track.id;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())?;
        Ok((format, decoder, id, track_start, delay, total))
    };
    let (mut format, mut decoder, id, track_start, delay, total) = open()?;
    fs::create_dir_all(&output)?;
    println!(
        "index,target,request,required_ts,actual_ts,first_packet_pts,first_packet_frames,first_output_frame,discarded_frames,packets,output_frames,track_start,delay,method,status"
    );
    let mut samples = Vec::<f32>::new();
    let mut failed = false;
    for (index, argument) in args[5..].iter().enumerate() {
        let target: i64 = argument.parse()?;
        if target < 0 {
            return Err("target must be nonnegative".into());
        }
        if target as u64 > total {
            return Err("target exceeds declared valid frame count".into());
        }
        let request = (target - preroll).max(0);
        let from_start = preroll == 1024 && target <= 1024;
        let method = if from_start { "from_start" } else { "seek" };
        let request_csv = if from_start {
            String::new()
        } else {
            request.to_string()
        };
        let attempt = (|| -> Result<String, Box<dyn Error>> {
            let (required_ts, actual_ts) = if from_start {
                // A clamped preroll starts with a fresh stream, before any seek call
                (format, decoder, _, _, _, _) = open()?;
                (String::new(), String::new())
            } else {
                let seeked = format.seek(
                    SeekMode::Accurate,
                    SeekTo::Timestamp {
                        ts: Timestamp::new(request),
                        track_id: id,
                    },
                )?;
                decoder.reset();
                if seeked.track_id != id
                    || seeked.required_ts.get() != request
                    || seeked.actual_ts.get() > request
                {
                    return Err("accurate seek contract violated".into());
                }
                (
                    seeked.required_ts.get().to_string(),
                    seeked.actual_ts.get().to_string(),
                )
            };
            let mut out = BufWriter::new(File::create(
                output.join(format!("seek-{index}-{target}.f32le")),
            )?);
            let (mut written, mut discarded, mut packets) = (0usize, 0usize, 0usize);
            let (mut first_packet_pts, mut first_packet_frames, mut first_output_frame) =
                (0i64, 0usize, -1i64);
            while written < window {
                let Some(packet) = format.next_packet()? else {
                    break;
                };
                if packet.track_id != id {
                    continue;
                }
                let buf = decoder.decode(&packet)?;
                if buf.spec().rate() != 48_000 || buf.spec().channels().count() != 2 {
                    return Err("decoded format changed".into());
                }
                let frames = buf.frames();
                if packets == 0 {
                    first_packet_pts = packet.pts.get();
                    first_packet_frames = frames;
                }
                packets += 1;
                // Vorbis gapless reset consumes the first overlap packet without output
                if frames == 0 {
                    continue;
                }
                if frames as u64 != packet.dur.get() {
                    return Err("decoded frame count differs from valid packet duration".into());
                }
                let start = packet
                    .pts
                    .checked_add(packet.trim_start)
                    .ok_or("timestamp overflow")?
                    .get();
                let skip = usize::try_from(target.saturating_sub(start))
                    .unwrap_or(0)
                    .min(frames);
                discarded += skip;
                if skip == frames {
                    continue;
                }
                let first = start.checked_add(skip as i64).ok_or("timestamp overflow")?;
                if first != target + written as i64 {
                    return Err(format!("missing or overlapping frames after seek preroll: requested={target}, method={method}, actual_seek={actual_ts}, first_packet_pts={first_packet_pts}, first_packet_frames={first_packet_frames}, next_output={first}, expected={}", target + written as i64).into());
                }
                if written == 0 {
                    first_output_frame = first;
                }
                let take = (window - written).min(frames - skip);
                samples.resize(buf.samples_interleaved(), f32::MID);
                buf.copy_to_slice_interleaved(&mut samples);
                for value in &samples[skip * 2..(skip + take) * 2] {
                    out.write_all(&value.to_le_bytes())?;
                }
                written += take;
            }
            out.flush()?;
            Ok(format!(
                "{index},{target},{request_csv},{required_ts},{actual_ts},{first_packet_pts},{first_packet_frames},{first_output_frame},{discarded},{packets},{written},{track_start},{delay},{method},PASS"
            ))
        })();
        match attempt {
            Ok(row) => println!("{row}"),
            Err(error) => {
                failed = true;
                eprintln!("FAIL index={index} target={target}: {error}");
                println!(
                    "{index},{target},{request_csv},,,,,,,,,{track_start},{delay},{method},FAIL"
                );
            }
        }
    }
    if failed {
        return Err("one or more seek attempts failed".into());
    }
    Ok(())
}
