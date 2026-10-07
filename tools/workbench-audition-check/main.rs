#![allow(dead_code)]

mod anchors;
mod replay;
mod workbench;

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, package, output] if command == "make-replay" => {
            use cocobeat_schema::{DuoInput, PlayerId, SessionEpoch, SongTime};
            let package = cocobeat_media::validate_package(package).unwrap();
            let mut replay = cocobeat_replay::Replay::new(
                cocobeat_replay::ReplayIdentity {
                    content_id: format!(
                        "package-blake3:{}",
                        package
                            .manifest
                            .package_hash
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<String>()
                    ),
                    rules_id: package.chart.ruleset_id.clone(),
                    build_id: "workbench-real-audio-software-qa".into(),
                    stage_compiler_version: Some(2),
                },
                SessionEpoch(123),
            )
            .unwrap();
            for player in [PlayerId::P1, PlayerId::P2] {
                replay
                    .record(DuoInput::Watermark {
                        epoch: SessionEpoch(123),
                        player,
                        through: SongTime::from_frames(-123),
                    })
                    .unwrap();
            }
            use std::io::Write;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)
                .and_then(|mut file| file.write_all(&replay.encode()?))
                .map_err(|error| error.to_string())
        }
        [package, recording] if std::env::var_os("QA_REAL_AUDIO").is_some() => workbench::run(
            std::path::Path::new(package),
            workbench::Mode::Replay(std::path::Path::new(recording)),
            cocobeat_runtime::Locale::ZhCn,
        ),
        [package, destination] => workbench::run(
            std::path::Path::new(package),
            workbench::Mode::Edit(std::path::Path::new(destination)),
            cocobeat_runtime::Locale::ZhCn,
        ),
        _ => Err("Expected PACKAGE NEW_DESTINATION".into()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
