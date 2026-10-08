use cocobeat_schema::SongTime;
use std::path::Path;
use std::process::ExitCode;

mod anchors;
mod content;
mod editor;
mod label_adoption;
mod labels;
mod labels_cli;
mod media;
mod music_truth;
mod native_beat_compare;
mod native_tempo;
mod package;
mod replay;
mod stage;
mod structure_features;
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
        [command, input, authoring, channel, output]
            if command == "import-experimental-beat"
                || command == "import-experimental-analysis" =>
        {
            report(package::import_experimental_beat(
                Path::new(input),
                Path::new(authoring),
                channel,
                Path::new(output),
                command == "import-experimental-analysis",
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
        [command, source, channel, output] if command == "inspect-native-tempo" => report(
            native_tempo::inspect(Path::new(source), channel, Path::new(output)),
        ),
        [command, source, channel, output] if command == "inspect-structure-features" => report(
            structure_features::inspect(Path::new(source), channel, Path::new(output)),
        ),
        [command, source, channel, output] if command == "compile-structure-candidate" => report(
            structure_features::compile(Path::new(source), channel, Path::new(output)),
        ),
        [command, input] if command == "verify-package" => {
            report(package::verify(Path::new(input)))
        }
        [command, source] if command == "music-truth-source" => {
            report(music_truth::inspect_source(Path::new(source)))
        }
        [command, source, reviewer, channel, output] if command == "music-truth-template" => {
            report(music_truth::template(
                Path::new(source),
                reviewer,
                channel,
                Path::new(output),
            ))
        }
        [command, source, input, output] if command == "import-music-truth" => report(
            music_truth::import(Path::new(source), Path::new(input), Path::new(output)),
        ),
        [command, source, left, right, output] if command == "compare-music-truth" => {
            report(music_truth::compare(
                Path::new(source),
                Path::new(left),
                Path::new(right),
                Path::new(output),
            ))
        }
        [command, source, evidence, truth, tolerance, output]
            if command == "compare-native-beats" =>
        {
            report(
                tolerance
                    .parse::<u64>()
                    .map_err(|_| "Tolerance frames must be an unsigned integer".to_string())
                    .and_then(|frames| {
                        native_beat_compare::compare_native_beats(
                            Path::new(source),
                            Path::new(evidence),
                            Path::new(truth),
                            frames,
                            Path::new(output),
                        )
                    }),
            )
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
        [command, source, labels, selection, output] if command == "adopt-labeled-anchors" => {
            report(label_adoption::adopt(
                Path::new(source),
                Path::new(labels),
                Path::new(selection),
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
        [command, input] if command == "inspect-stage-plan" => {
            report(stage::inspect_plan(Path::new(input)))
        }
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
        [command, input, flag, options @ ..]
            if command == "workbench-candidates" && flag == "--structure" =>
        {
            report(
                structure_workbench_options(options).and_then(|(channel, locale)| {
                    workbench::run(
                        Path::new(input),
                        workbench::Mode::StructureFeatures(channel),
                        locale,
                    )
                }),
            )
        }
        [command, input, target, options @ ..]
            if command == "workbench"
                || command == "workbench-replay"
                || command == "workbench-candidates"
                || command == "workbench-beats"
                || command == "workbench-labels" =>
        {
            report(workbench_locale(options).and_then(|locale| {
                let mode = if command == "workbench-labels" {
                    workbench::Mode::Labels(Path::new(target))
                } else if command == "workbench-candidates" {
                    workbench::Mode::Candidates(Path::new(target))
                } else if command == "workbench-beats" {
                    workbench::Mode::NativeBeats(Path::new(target))
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
                "Usage: cocobeat-lab time-smoke | timing-sim [output-dir] | generate-dev [output-dir] | audio-probe <30|64|300|600> <output-dir> | decode-audio <input> <new-output.f32le> | resample-audio <input> <new-output.f32le> | readback-canonical <input.ogg> <expected-frames> <new-output.f32le> | prepare-audio <final.ogg> <expected-frames> <new-staging-dir> | import-authored-package <source-audio> <authoring.json> <new-package-dir> | import-experimental-beat <source-audio> <authoring.json> <left|right> <new-output-dir> | import-experimental-analysis <source-audio> <authoring.json> <left|right> <new-output-dir> | build-authored-package <final.ogg> <expected-frames> <authoring.json> <new-package-dir> | inspect-native-tempo <package-dir> <left|right> <new-report.json> | inspect-structure-features <package-dir> <left|right> <new-report.json> | compile-structure-candidate <package-dir> <left|right> <new-package-dir> | verify-package <package-dir> | music-truth-source <package-dir> | music-truth-template <package-dir> <reviewer> <stereo|left|right> <new-json> | import-music-truth <package-dir> <manual.json> <new-json> | compare-music-truth <package-dir> <reviewer-a.json> <reviewer-b.json> <new-report.json> | compare-native-beats <package-dir> <evidence-dir> <music-truth.json> <tolerance-frames> <new-report.json> | label-source <package-dir> | import-labels <package-dir> <manual-labels.json> <new-labels.json> | compare-labels <package-dir> <reviewer-a.json> <reviewer-b.json> <new-comparison.json> | adopt-labeled-anchors <package-dir> <labels.json> <selection.json> <new-package-dir> | inspect-stage-plan <package-dir> | inspect-stage <package-dir> <frame> | inspect-replay-stage <package-dir> <replay.json> <frame> | edit-anchors <package-dir> <patch.json> <new-package-dir> | propose-anchors <package-dir> <min-confidence> <min-gap-frames> <new-report.json> | adopt-anchor-proposal <package-dir> <report.json> <selection.json> <new-package-dir> | inspect-replay <package-dir> <replay.json> <new-report.jsonl> [--timing SIDECAR] | net-host <package-dir> <local-replay.json> <IP:port> <new-invite.json> <new-output-dir> | net-join <package-dir> <local-replay.json> <invite.json> <new-output-dir> | net-receive <new-package-dir> <local-replay.json> <invite.json> <new-output-dir> | workbench-labels <package-dir> <new-labels.json> [--locale CODE] | workbench <package-dir> <new-package-dir> [--locale CODE] | workbench-replay <package-dir> <replay.json> [--timing SIDECAR] [--locale CODE] | workbench-candidates <package-dir> <proposal.json> [--locale CODE] | workbench-candidates <package-dir> --structure <left|right> [--locale CODE] | workbench-beats <package-dir> <evidence-dir> [--locale CODE]"
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

fn structure_workbench_options(
    options: &[String],
) -> Result<(usize, cocobeat_runtime::Locale), String> {
    let Some((channel, options)) = options.split_first() else {
        return Err("Structure workbench requires an explicit left or right channel".into());
    };
    let channel = match channel.as_str() {
        "left" => 0,
        "right" => 1,
        _ => return Err("Structure workbench requires an explicit left or right channel".into()),
    };
    workbench_locale(options).map(|locale| (channel, locale))
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
    fn structure_workbench_requires_channel_and_only_known_locale_options() {
        let options = |args: &[&str]| args.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
        for (channel, expected) in [("left", 0), ("right", 1)] {
            let parsed =
                structure_workbench_options(&options(&[channel, "--locale", "fr"])).unwrap();
            assert_eq!(parsed, (expected, cocobeat_runtime::Locale::Fr));
        }
        for args in [
            vec![],
            vec!["stereo"],
            vec!["--locale", "fr"],
            vec!["left", "--locale"],
            vec!["left", "--locale", "unknown"],
            vec!["right", "extra"],
            vec!["left", "--locale", "fr", "--locale", "fr"],
        ] {
            assert!(
                structure_workbench_options(&options(&args)).is_err(),
                "{args:?}"
            );
        }
    }

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
