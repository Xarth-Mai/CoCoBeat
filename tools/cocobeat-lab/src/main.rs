use cocobeat_schema::SongTime;
use std::path::Path;
use std::process::ExitCode;

mod content;
mod media;
mod timing;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command] if command == "time-smoke" => {
            let time = SongTime::from_seconds(64).expect("64 seconds fits in SongTime");
            println!("64 s = {} canonical frames", time.frames());
            println!("Integer timeline smoke check only; no hardware latency measurement.");
            ExitCode::SUCCESS
        }
        [command, rest @ ..] if command == "timing-sim" && rest.len() <= 1 => {
            let path = rest
                .first()
                .map(String::as_str)
                .unwrap_or("target/timing-sim");
            report(timing::run(Path::new(path)))
        }
        [command, rest @ ..] if command == "generate-dev" && rest.len() <= 1 => {
            let path = rest
                .first()
                .map(String::as_str)
                .unwrap_or("target/dev-assets");
            report(content::generate(Path::new(path)))
        }
        [command, seconds, output] if command == "audio-probe" => report(
            probe_duration(seconds)
                .map_err(str::to_owned)
                .and_then(|duration| {
                    cocobeat_runtime::probe::audio_probe(duration, Path::new(output))
                }),
        ),
        [command, input, output] if command == "decode-audio" => {
            report(media::decode(Path::new(input), Path::new(output)))
        }
        _ => {
            eprintln!(
                "Usage: cocobeat-lab time-smoke | timing-sim [output-dir] | generate-dev [output-dir] | audio-probe <30|64|300|600> <output-dir> | decode-audio <input> <new-output.f32le>"
            );
            if args.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

fn probe_duration(value: &str) -> Result<u32, &'static str> {
    match value.parse() {
        Ok(seconds @ (30 | 64 | 300 | 600)) => Ok(seconds),
        _ => Err("audio-probe duration must be 30, 64, 300 or 600 seconds"),
    }
}

fn report<E: std::fmt::Display>(result: Result<(), E>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_probe_requires_an_explicit_supported_duration() {
        for seconds in [30, 64, 300, 600] {
            assert_eq!(probe_duration(&seconds.to_string()), Ok(seconds));
        }
        for value in ["", "0", "31", "-30", "1.0", "30s", "4294967296"] {
            assert!(probe_duration(value).is_err());
        }
    }
}
