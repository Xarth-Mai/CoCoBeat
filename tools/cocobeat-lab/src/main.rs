use cocobeat_schema::SongTime;
use std::path::Path;
use std::process::ExitCode;

mod anchors;
mod content;
mod editor;
mod labels;
mod labels_cli;
mod media;
mod package;
mod replay;
mod stage;
mod timing;
mod workbench;

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
        [command, input, output] if command == "decode-audio" || command == "resample-audio" => {
            report(media::decode(
                Path::new(input),
                Path::new(output),
                if command == "resample-audio" {
                    media::Operation::Resample
                } else {
                    media::Operation::Decode
                },
            ))
        }
        [command, input, frames, output]
            if command == "readback-canonical" || command == "prepare-audio" =>
        {
            report(
                frames
                    .parse::<u64>()
                    .map_err(|_| {
                        "Expected canonical frames must be an unsigned integer".to_string()
                    })
                    .and_then(|expected| {
                        if command == "prepare-audio" {
                            media::prepare(Path::new(input), expected, Path::new(output))
                        } else {
                            media::decode(
                                Path::new(input),
                                Path::new(output),
                                media::Operation::Readback(expected),
                            )
                        }
                    }),
            )
        }
        [command, input, authoring, channel, output] if command == "import-experimental-beat" => {
            report(package::import_experimental_beat(
                Path::new(input),
                Path::new(authoring),
                channel,
                Path::new(output),
            ))
        }
        [command, input, authoring, output] if command == "import-authored-package" => report(
            package::import_authored(Path::new(input), Path::new(authoring), Path::new(output)),
        ),
        [command, input, frames, authoring, output] if command == "build-authored-package" => {
            report(
                frames
                    .parse::<u64>()
                    .map_err(|_| {
                        "Expected canonical frames must be an unsigned integer".to_string()
                    })
                    .and_then(|frames| {
                        package::build(
                            Path::new(input),
                            frames,
                            Path::new(authoring),
                            Path::new(output),
                        )
                    }),
            )
        }
        [command, input] if command == "verify-package" => {
            report(package::verify(Path::new(input)))
        }
        [command, source] if command == "label-source" => {
            report(labels_cli::inspect_source(Path::new(source)))
        }
        [command, source, input, output] if command == "import-labels" => report(
            labels_cli::import(Path::new(source), Path::new(input), Path::new(output)),
        ),
        [command, source, left, right, output] if command == "compare-labels" => {
            report(labels_cli::compare(
                Path::new(source),
                Path::new(left),
                Path::new(right),
                Path::new(output),
            ))
        }
        [command, input, patch, output] if command == "edit-anchors" => report(editor::edit(
            Path::new(input),
            Path::new(patch),
            Path::new(output),
        )),
        [command, input, confidence, gap, output] if command == "propose-anchors" => report(
            anchors::propose(Path::new(input), confidence, gap, Path::new(output)),
        ),
        [command, input, proposal, selection, output] if command == "adopt-anchor-proposal" => {
            report(anchors::adopt(
                Path::new(input),
                Path::new(proposal),
                Path::new(selection),
                Path::new(output),
            ))
        }
        [command, input, recording, output, flag, timing]
            if command == "inspect-replay" && flag == "--timing" =>
        {
            report(replay::inspect_with_timing(
                Path::new(input),
                Path::new(recording),
                Path::new(output),
                Some(Path::new(timing)),
            ))
        }
        [command, input, recording, output] if command == "inspect-replay" => report(
            replay::inspect(Path::new(input), Path::new(recording), Path::new(output)),
        ),
        [command, input, recording, frame] if command == "inspect-replay-stage" => report(
            stage::inspect_replay(Path::new(input), Path::new(recording), frame),
        ),
        [command, input, frame] if command == "inspect-stage" => {
            report(stage::inspect(Path::new(input), frame))
        }
        [command, input, recording, flag, timing, options @ ..]
            if command == "workbench-replay" && flag == "--timing" =>
        {
            report(workbench_locale(options).and_then(|locale| {
                workbench::run(
                    Path::new(input),
                    workbench::Mode::Replay(Path::new(recording), Some(Path::new(timing))),
                    locale,
                )
            }))
        }
        [command, input, target, options @ ..]
            if command == "workbench"
                || command == "workbench-replay"
                || command == "workbench-candidates" =>
        {
            report(workbench_locale(options).and_then(|locale| {
                let mode = if command == "workbench-candidates" {
                    workbench::Mode::Candidates(Path::new(target))
                } else if command == "workbench-replay" {
                    workbench::Mode::Replay(Path::new(target), None)
                } else {
                    workbench::Mode::Edit(Path::new(target))
                };
                workbench::run(Path::new(input), mode, locale)
            }))
        }
        [command, package, replay, bind, invite, output] if command == "net-host" => report(
            bind.parse()
                .map_err(|_| "net-host requires an explicit IP:port socket address".to_string())
                .and_then(|bind| {
                    cocobeat_net::host(
                        Path::new(package),
                        Path::new(replay),
                        bind,
                        Path::new(invite),
                        Path::new(output),
                    )
                })
                .and_then(print_session),
        ),
        [command, package, replay, invite, output] if command == "net-join" => report(
            cocobeat_net::join(
                Path::new(package),
                Path::new(replay),
                Path::new(invite),
                Path::new(output),
            )
            .and_then(print_session),
        ),
        [command, package, replay, invite, output] if command == "net-receive" => report(
            cocobeat_net::join_receive(
                Path::new(package),
                Path::new(replay),
                Path::new(invite),
                Path::new(output),
            )
            .and_then(print_session),
        ),
        _ => {
            eprintln!(
                "Usage: cocobeat-lab time-smoke | timing-sim [output-dir] | generate-dev [output-dir] | audio-probe <30|64|300|600> <output-dir> | decode-audio <input> <new-output.f32le> | resample-audio <input> <new-output.f32le> | readback-canonical <input.ogg> <expected-frames> <new-output.f32le> | prepare-audio <final.ogg> <expected-frames> <new-staging-dir> | import-authored-package <source-audio> <authoring.json> <new-package-dir> | import-experimental-beat <source-audio> <authoring.json> <left|right> <new-output-dir> | build-authored-package <final.ogg> <expected-frames> <authoring.json> <new-package-dir> | verify-package <package-dir> | label-source <package-dir> | import-labels <package-dir> <manual-labels.json> <new-labels.json> | compare-labels <package-dir> <reviewer-a.json> <reviewer-b.json> <new-comparison.json> | inspect-stage <package-dir> <frame> | inspect-replay-stage <package-dir> <replay.json> <frame> | edit-anchors <package-dir> <patch.json> <new-package-dir> | propose-anchors <package-dir> <min-confidence> <min-gap-frames> <new-report.json> | adopt-anchor-proposal <package-dir> <report.json> <selection.json> <new-package-dir> | inspect-replay <package-dir> <replay.json> <new-report.jsonl> [--timing SIDECAR] | net-host <package-dir> <local-replay.json> <IP:port> <new-invite.json> <new-output-dir> | net-join <package-dir> <local-replay.json> <invite.json> <new-output-dir> | net-receive <new-package-dir> <local-replay.json> <invite.json> <new-output-dir> | workbench <package-dir> <new-package-dir> [--locale CODE] | workbench-replay <package-dir> <replay.json> [--timing SIDECAR] [--locale CODE] | workbench-candidates <package-dir> <proposal.json> [--locale CODE]"
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

fn print_session(summary: cocobeat_net::SessionSummary) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string(&summary).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn workbench_locale(options: &[String]) -> Result<cocobeat_runtime::Locale, String> {
    use cocobeat_runtime::Locale;
    match options {
        [] => Ok(
            cocobeat_runtime::configured_locale().unwrap_or_else(|error| {
                eprintln!("{error}; using the system language");
                Locale::system_default()
            }),
        ),
        [flag, code] if flag == "--locale" => Locale::ALL
            .into_iter()
            .find(|locale| locale.code() == code)
            .ok_or_else(|| format!("Unsupported locale: {code}")),
        _ => Err("workbench accepts only an optional --locale CODE".into()),
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
