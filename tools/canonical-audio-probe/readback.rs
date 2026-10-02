use std::{
    error::Error,
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};
use symphonia::core::{
    audio::sample::Sample,
    codecs::audio::AudioDecoderOptions,
    formats::{FormatOptions, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: readback INPUT_OGG OUTPUT_F32LE".into());
    }
    let input = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);
    let mss = MediaSourceStream::new(Box::new(File::open(input)?), Default::default());
    let mut format = symphonia::default::get_probe().probe(
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
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())?;
    let sample_rate = params.sample_rate.ok_or("no sample rate")?;
    let channels = params.channels.as_ref().ok_or("no channel layout")?.count();
    let id = track.id;
    let mut frames = 0usize;
    let mut samples = Vec::<f32>::new();
    let mut out = BufWriter::new(File::create(output)?);
    while let Some(packet) = format.next_packet()? {
        if packet.track_id != id {
            continue;
        }
        let buf = decoder.decode(&packet)?;
        if buf.spec().rate() != sample_rate || buf.spec().channels().count() != channels {
            return Err("decoded format changed".into());
        }
        samples.resize(buf.samples_interleaved(), f32::MID);
        buf.copy_to_slice_interleaved(&mut samples);
        frames += buf.frames();
        for v in &samples {
            out.write_all(&v.to_le_bytes())?;
        }
    }
    out.flush()?;
    println!("{{\"sample_rate\":{sample_rate},\"channels\":{channels},\"frames\":{frames}}}");
    Ok(())
}
