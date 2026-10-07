//! Opt-in native observations use synthetic controls, never physical input evidence

use super::*;
use std::{fs::File, io::Write, time::Instant};

#[derive(Resource)]
struct Observation {
    directory: PathBuf,
    round_count: usize,
    rounds: Vec<serde_json::Value>,
    current: RoundObservation,
    restart_sent: bool,
    scenario: &'static str,
    cancel_host: bool,
    fault_injected: bool,
    same_epoch: Option<SameEpochObservation>,
}

#[derive(Default)]
struct SameEpochObservation {
    request_sent: bool,
    seen: bool,
    ready: bool,
    identity: Option<(u64, u64, SessionEpoch)>,
    prefix: Vec<DuoInput>,
    recovering_hits: Option<usize>,
    probe_requested_ns: Option<u64>,
    probe_cleared_observed_ns: Option<u64>,
    probe_dropped_before_ready: bool,
    last_stage: Option<String>,
    records: Vec<serde_json::Value>,
}

struct RoundObservation {
    number: usize,
    directory: PathBuf,
    frames: File,
    result: File,
    started: Instant,
    frame: u64,
    start_sent: bool,
    hits_sent: usize,
    requested: [bool; 2],
    captures: [Option<Result<[u32; 2], String>>; 2],
    terminal_written: bool,
}

impl RoundObservation {
    fn new(
        directory: PathBuf,
        number: usize,
        output_info: &str,
        scenario: &str,
    ) -> Result<Self, String> {
        let file = |name| File::create_new(directory.join(name)).map_err(|error| error.to_string());
        let mut metadata = file("metadata.json")?;
        serde_json::to_writer_pretty(&mut metadata, &serde_json::json!({
            "build_id": env!("COCOBEAT_BUILD_ID"),
            "entrypoint": "native DefaultPlugins, AudioOutput, update_game",
            "process_id": std::process::id(),
            "scenario": scenario,
            "controls": "synthetic CapturedControl at software-observed monotonic timestamps",
            "audio": "actual Kira source cursor, no speaker latency measurement",
            "output_info": output_info,
            "not_run": ["physical keyboard", "physical gamepad", "speaker synchronization", "two machines", "human experience", "native-language review"]
        })).map_err(|error| error.to_string())?;
        metadata.sync_all().map_err(|error| error.to_string())?;
        file("running.png")?;
        file("screenshot.png")?;
        let mut frames = file("frames.csv")?;
        writeln!(frames, "frame,monotonic_ns,phase,controls_enabled,brand_complete,cursor_seconds,source_song_frames,estimated_song_frames,epoch,local_player,facts,events,synthetic_hits,network_started,online_recovering,source_generation,source_id,source_sequence,audio_state")
            .map_err(|error| error.to_string())?;
        Ok(Self {
            number,
            frames,
            result: file("result.json")?,
            directory,
            started: Instant::now(),
            frame: 0,
            start_sent: false,
            hits_sent: 0,
            requested: [false; 2],
            captures: [None, None],
            terminal_written: false,
        })
    }
}

impl Observation {
    fn new(directory: &Path, round_count: usize, output_info: &str) -> Result<Self, String> {
        let selected = std::env::var("COCOBEAT_LIVE_OBSERVATION_SCENARIO")
            .unwrap_or_else(|_| "complete".into());
        let (scenario, cancel_host) = match selected.as_str() {
            "complete" => ("complete", false),
            "reenter-before-ready-host" => ("reenter-before-ready", true),
            "reenter-before-ready-guest" => ("reenter-before-ready", false),
            "reenter-after-hit-host" => ("reenter-after-hit", true),
            "reenter-after-hit-guest" => ("reenter-after-hit", false),
            "same-epoch-active-host" => ("same-epoch-active", true),
            "same-epoch-active-guest" => ("same-epoch-active", false),
            _ => return Err("Unknown live observation scenario".into()),
        };
        if scenario == "same-epoch-active" && round_count != 1 {
            return Err("Same epoch observation requires one original round".into());
        }
        if scenario.starts_with("reenter-") && round_count != 2 {
            return Err("Recovery observation requires exactly two declared rounds".into());
        }
        std::fs::create_dir(directory)
            .map_err(|error| format!("Observation directory: {error}"))?;
        let first = if round_count > 1 {
            let path = directory.join("round-1");
            std::fs::create_dir(&path).map_err(|error| error.to_string())?;
            path
        } else {
            directory.to_path_buf()
        };
        Ok(Self {
            directory: directory.to_path_buf(),
            round_count,
            rounds: Vec::new(),
            current: RoundObservation::new(first, 1, output_info, scenario)?,
            restart_sent: false,
            scenario,
            cancel_host,
            fault_injected: false,
            same_epoch: (scenario == "same-epoch-active").then(SameEpochObservation::default),
        })
    }

    fn observe_transition(
        &mut self,
        phase: Phase,
        remaining_rounds: usize,
        output_info: &str,
    ) -> Result<(), String> {
        if self.restart_sent
            && phase == Phase::Connecting
            && remaining_rounds + self.current.number + 1 == self.round_count
        {
            let number = self.current.number + 1;
            let directory = self.directory.join(format!("round-{number}"));
            std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
            self.current = RoundObservation::new(directory, number, output_info, self.scenario)?;
            self.restart_sent = false;
        }
        Ok(())
    }

    fn write_summary(&self, error: Option<&str>) -> Result<(), String> {
        if self.round_count == 1 {
            return Ok(());
        }
        let complete = error.is_none()
            && self.rounds.len() == self.round_count
            && self.rounds.iter().enumerate().all(|(index, round)| {
                round["status"]
                    == if self.scenario != "complete" && index == 0 {
                        "FAILED"
                    } else {
                        "COMPLETE"
                    }
            });
        let mut file = File::create_new(self.directory.join("summary.json"))
            .map_err(|error| error.to_string())?;
        serde_json::to_writer_pretty(
            &mut file,
            &serde_json::json!({
                "round_count": self.round_count,
                "status": if !complete { "FAILED" } else if self.scenario == "complete" { "COMPLETE" } else { "RECOVERED" },
                "scenario": self.scenario,
                "process_id": std::process::id(),
                "error": error,
                "rounds": self.rounds,
            }),
        )
        .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())
    }
}

pub(super) fn install(app: &mut App, directory: &Path) -> Result<(), String> {
    let round_count = app.world().non_send::<OnlineRound>().remaining_rounds() + 1;
    let observation = Observation::new(
        directory,
        round_count,
        app.world().non_send::<AudioOutput>().output_info(),
    )?;
    app.insert_resource(observation).add_systems(
        Update,
        observe
            .before(update_game)
            .after(BrandIntroSystems::Advance),
    );
    Ok(())
}

fn record_result(
    observation: &mut Observation,
    game: &Game,
    online: &OnlineRound,
    error: Option<&str>,
) -> Result<(), String> {
    if observation.current.terminal_written {
        return Ok(());
    }
    observation.current.terminal_written = true;
    let diagnostics: Vec<_> = game.session.diagnostics.iter().map(|capture| serde_json::json!({
        "player": format!("{:?}", capture.player), "seq": capture.seq,
        "observed_ns": capture.observed_ns, "consumed_ns": capture.consumed_ns,
        "song_frames": capture.song_frames, "uncertainty_frames": capture.uncertainty_frames,
    })).collect();
    let result = serde_json::json!({
        "status": if game.phase == Phase::Finished && error.is_none() { "COMPLETE" } else { "FAILED" },
        "phase": format!("{:?}", game.phase), "error": error.or(game.fault_details.as_deref()),
        "process_id": std::process::id(), "scenario": observation.scenario,
        "local_worker_cancel_requested": observation.fault_injected && observation.current.number == 1,
        "same_epoch": observation.same_epoch.as_ref().map(|recovery| serde_json::json!({
            "request_sent": recovery.request_sent, "recovering_seen": recovery.seen,
            "ready_after_gate": recovery.ready, "records": recovery.records,
            "probe_requested_ns": recovery.probe_requested_ns,
            "probe_cleared_observed_ns": recovery.probe_cleared_observed_ns,
            "probe_dropped_before_ready": recovery.probe_dropped_before_ready,
            "cause": "explicit active QUIC maintenance request, not packet loss or speaker evidence",
        })),
        "core_events": format!("{:?}", game.session.engine.events()),
        "owned_workers_finished": online.is_finished(),
        "epoch": game.session.epoch().0, "content_id": game.content.content_id,
        "canonical_frames": game.content.end.frames(), "frames_observed": observation.current.frame,
        "local_player": online.player.map(|player| format!("{player:?}")),
        "network_started": online.started, "local_ended": online.local_ended,
        "facts": game.session.replay.facts().len(), "events": game.session.engine.events().len(),
        "synthetic_hits_requested": observation.current.hits_sent, "capture_diagnostics": diagnostics,
        "running_capture": observation.current.captures[0], "terminal_capture": observation.current.captures[1],
    });
    serde_json::to_writer_pretty(&mut observation.current.result, &result)
        .map_err(|error| error.to_string())?;
    observation
        .current
        .frames
        .sync_all()
        .and_then(|()| observation.current.result.sync_all())
        .map_err(|error| error.to_string())?;
    observation.rounds.push(serde_json::json!({
        "round": observation.current.number,
        "epoch": result["epoch"],
        "status": result["status"],
    }));
    Ok(())
}

fn observe(
    mut observation: ResMut<Observation>,
    mut game: ResMut<Game>,
    mut input: ResMut<InputState>,
    brand: Res<BrandIntroStatus>,
    mut audio: NonSendMut<AudioOutput>,
    mut online: NonSendMut<OnlineRound>,
    mut commands: Commands,
) {
    if game.closing {
        return;
    }
    let now = u64::try_from(input.origin.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let result = (|| -> Result<(), String> {
        observation.observe_transition(
            game.phase,
            online.remaining_rounds(),
            audio.output_info(),
        )?;
        if observation.current.started.elapsed().as_secs() >= 120 {
            return Err("Native observation exceeded its 120-second deadline".into());
        }
        observation.current.frame += 1;
        let source = game
            .session
            .clock
            .last_observation()
            .map(|last| last.song_time.frames().to_string())
            .unwrap_or_default();
        let cursor = audio
            .position()
            .map(|position| position.to_string())
            .unwrap_or_default();
        let actual_source = audio.source_observation();
        let source_read_after = Instant::now();
        let frame = observation.current.frame;
        let hits_sent = observation.current.hits_sent;
        writeln!(
            observation.current.frames,
            "{},{now},{:?},{},{},{cursor},{source},{},{},{},{},{},{},{},{},{},{},{},{:?}",
            frame,
            game.phase,
            input.controls_enabled(),
            brand.is_complete(),
            game.session.current.frames(),
            game.session.epoch().0,
            online
                .player
                .map(|player| format!("{player:?}"))
                .unwrap_or_default(),
            game.session.replay.facts().len(),
            game.session.engine.events().len(),
            hits_sent,
            online.started,
            online.recovering(),
            actual_source
                .map(|source| source.generation.to_string())
                .unwrap_or_default(),
            actual_source
                .map(|source| source.source_id.to_string())
                .unwrap_or_default(),
            actual_source
                .map(|source| source.sequence.to_string())
                .unwrap_or_default(),
            audio.state(),
        )
        .map_err(|error| error.to_string())?;
        if let Some(recovery) = &mut observation.same_epoch {
            if let Some(requested) = recovery.probe_requested_ns
                && now > requested
                && recovery.probe_cleared_observed_ns.is_none()
                && !input
                    .queued
                    .iter()
                    .any(|event| matches!(event.control, Control::Hit(_)))
            {
                recovery.probe_cleared_observed_ns = Some(now);
            }
            if game.phase == Phase::Recovering && !recovery.seen {
                recovery.seen = true;
                recovery.recovering_hits = Some(game.session.diagnostics.len());
                if recovery.identity.is_none() {
                    let source =
                        actual_source.ok_or("Recovery observation lacks the original source")?;
                    recovery.identity =
                        Some((source.generation, source.source_id, game.session.epoch()));
                    recovery.prefix = game.session.replay.facts().to_vec();
                }
            }
            if game.phase == Phase::Recovering {
                if recovery.recovering_hits != Some(game.session.diagnostics.len()) {
                    return Err("Recovery accepted a performing Hit while input was gated".into());
                }
                if online.recovering() && recovery.probe_requested_ns.is_none() {
                    input.queued.push(input::CapturedControl {
                        input_kind: cocobeat_replay::timing::InputKind::Internal,
                        control: Control::Hit(
                            online.player.ok_or("Recovery probe lacks a player")?,
                        ),
                        monotonic_ns: now,
                    });
                    recovery.probe_requested_ns = Some(now);
                }
            } else if recovery.seen && game.phase == Phase::Running && !online.recovering() {
                if !recovery.ready {
                    if recovery.probe_requested_ns.is_none()
                        || recovery.probe_cleared_observed_ns.is_none()
                        || recovery.recovering_hits != Some(game.session.diagnostics.len())
                        || game
                            .session
                            .replay
                            .facts()
                            .iter()
                            .filter(|fact| matches!(fact, DuoInput::Hit(hit) if Some(hit.player) == online.player))
                            .count()
                            != 2
                        || input
                            .queued
                            .iter()
                            .any(|event| matches!(event.control, Control::Hit(_)))
                    {
                        return Err("Recovery probe survived the real input gate".into());
                    }
                    recovery.probe_dropped_before_ready = true;
                }
                recovery.ready = true;
            }
            if let Some(identity) = recovery.identity {
                if game.session.epoch() != identity.2
                    || !game.session.replay.facts().starts_with(&recovery.prefix)
                {
                    return Err(
                        "Recovery replaced the original epoch or ordered GUI history prefix".into(),
                    );
                }
                if matches!(game.phase, Phase::Recovering | Phase::Running)
                    && let Some(source) = actual_source
                    && (source.generation, source.source_id) != (identity.0, identity.1)
                {
                    return Err("Recovery replaced the original Kira source instance".into());
                }
            }
            let stage = format!(
                "{:?}:{:?}:{}",
                game.phase,
                audio.state(),
                online.recovering()
            );
            if recovery.last_stage.as_ref() != Some(&stage) {
                if recovery.records.len() >= 32 {
                    return Err("Recovery stage observation exceeded its bound".into());
                }
                recovery.records.push(serde_json::json!({
                    "stage": stage, "phase": format!("{:?}", game.phase),
                    "epoch": game.session.epoch().0, "content_id": game.content.content_id,
                    "facts": game.session.replay.encode().map_err(|error| error.to_string())?,
                    "core_events": format!("{:?}", game.session.engine.events()),
                    "controls_enabled": input.controls_enabled(), "online_recovering": online.recovering(),
                    "local_hits": game.session.diagnostics.len(), "ready_after_gate": recovery.ready,
                    "source": actual_source.map(|source| serde_json::json!({
                        "generation": source.generation, "source_id": source.source_id,
                        "sequence": source.sequence, "position_seconds": source.position_seconds,
                        "publication_width_ns": source.published_between[1].checked_duration_since(source.published_between[0]).map(|duration| duration.as_nanos()),
                        "publication_age_lower_ns": source_read_after.checked_duration_since(source.published_between[1]).map(|duration| duration.as_nanos()),
                        "publication_age_upper_ns": source_read_after.checked_duration_since(source.published_between[0]).map(|duration| duration.as_nanos()),
                    })),
                }));
                recovery.last_stage = Some(stage);
            }
        }
        if !observation.current.start_sent
            && brand.is_complete()
            && input.controls_enabled()
            && game.phase == Phase::Ready
        {
            input.queued.push(input::CapturedControl {
                input_kind: cocobeat_replay::timing::InputKind::Internal,
                control: Control::Start,
                monotonic_ns: now,
            });
            observation.current.start_sent = true;
        }
        if game.phase == Phase::Running && observation.current.hits_sent < 3 {
            let threshold = if let Some(recovery) = &observation.same_epoch {
                if observation.current.hits_sent == 2 && !recovery.ready {
                    i64::MAX
                } else {
                    [48_000, 72_000, 192_000][observation.current.hits_sent]
                }
            } else {
                game.content.end.frames() * (observation.current.hits_sent as i64 + 1) / 4
            };
            if game.session.current.frames() >= threshold {
                let player = online.player.unwrap_or(PlayerId::P1);
                input.queued.push(input::CapturedControl {
                    input_kind: cocobeat_replay::timing::InputKind::Internal,
                    control: Control::Hit(player),
                    monotonic_ns: now,
                });
                observation.current.hits_sent += 1;
            }
        }
        if observation.cancel_host
            && game.phase == Phase::Running
            && game.session.current.frames() >= 96_000
            && actual_source.is_some_and(|source| source.position_seconds >= 2.0)
            && let Some(recovery) = &mut observation.same_epoch
            && !recovery.request_sent
            && [PlayerId::P1, PlayerId::P2].into_iter().all(|player| {
                game.session
                    .replay
                    .facts()
                    .iter()
                    .filter(|fact| matches!(fact, DuoInput::Hit(hit) if hit.player == player))
                    .count()
                    >= 2
            })
        {
            let source = actual_source.ok_or("Recovery request lacks actual source publication")?;
            recovery.identity = Some((source.generation, source.source_id, game.session.epoch()));
            recovery.prefix = game.session.replay.facts().to_vec();
            online.send(LiveCommand::RequestRecovery {
                epoch: game.session.epoch(),
            })?;
            recovery.request_sent = true;
        }
        if observation.cancel_host && !observation.fault_injected && observation.current.number == 1
        {
            let cancel = match observation.scenario {
                "reenter-before-ready" => {
                    game.phase == Phase::Connecting && game.notice.key == "network.listening"
                }
                "reenter-after-hit" => {
                    game.phase == Phase::Running
                        && observation.current.captures[0].is_some()
                        && !game.session.diagnostics.is_empty()
                        && online.player.is_some_and(|local| {
                            game.session.replay.facts().iter().any(
                                |fact| matches!(fact, DuoInput::Hit(hit) if hit.player != local),
                            )
                        })
                }
                _ => false,
            };
            if cancel {
                online.cancel_worker();
                observation.fault_injected = true;
            }
        }
        if let Some(error) = observation
            .current
            .captures
            .iter()
            .find_map(|capture| capture.as_ref().and_then(|result| result.as_ref().err()))
        {
            return Err(format!("Native screenshot: {error}"));
        }
        for index in 0..2 {
            let eligible = if index == 0 {
                game.phase == Phase::Running && !game.session.diagnostics.is_empty()
            } else {
                game.phase == Phase::Fault
                    || (game.phase == Phase::Finished
                        && (online.remaining_rounds() == 0
                            || (online.can_start_next() && input.can_start_next_round())))
            };
            if !eligible || observation.current.requested[index] {
                continue;
            }
            observation.current.requested[index] = true;
            let path = observation.current.directory.join(if index == 0 {
                "running.png"
            } else {
                "screenshot.png"
            });
            let number = observation.current.number;
            commands.spawn(Screenshot::primary_window()).observe(
                move |capture: On<ScreenshotCaptured>, mut observation: ResMut<Observation>| {
                    if observation.current.number != number {
                        return;
                    }
                    observation.current.captures[index] = Some(
                        capture
                            .image
                            .clone()
                            .try_into_dynamic()
                            .map_err(|error| error.to_string())
                            .and_then(|image| {
                                let dimensions = [image.width(), image.height()];
                                image.save(&path).map_err(|error| error.to_string())?;
                                Ok(dimensions)
                            }),
                    );
                },
            );
        }
        if observation.current.captures[1].is_some()
            && (!observation.current.requested[0] || observation.current.captures[0].is_some())
            && online.is_finished()
        {
            if observation
                .same_epoch
                .as_ref()
                .is_some_and(|recovery| !recovery.seen || !recovery.ready)
                && game.phase == Phase::Finished
            {
                return Err(
                    "Same epoch observation finished without a real gated continuation".into(),
                );
            }
            record_result(&mut observation, &game, &online, None)?;
            let expected_fault = observation.scenario.starts_with("reenter-")
                && observation.current.number == 1
                && game.phase == Phase::Fault;
            if (game.phase == Phase::Finished || expected_fault) && online.remaining_rounds() > 0 {
                if !observation.restart_sent
                    && online.can_start_next()
                    && input.can_start_next_round()
                {
                    input.queued.push(input::CapturedControl {
                        input_kind: cocobeat_replay::timing::InputKind::Internal,
                        control: Control::Restart,
                        monotonic_ns: now,
                    });
                    observation.restart_sent = true;
                }
            } else {
                observation.write_summary(game.fault_details.as_deref())?;
                game.closing_error = game.phase == Phase::Fault;
                input.queued.push(input::CapturedControl {
                    input_kind: cocobeat_replay::timing::InputKind::Internal,
                    control: Control::Quit,
                    monotonic_ns: now,
                });
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Native observation failed: {error}");
        let _ = record_result(&mut observation, &game, &online, Some(&error))
            .inspect_err(|write| eprintln!("Observation result write failed: {write}"));
        let _ = observation
            .write_summary(Some(&error))
            .inspect_err(|write| eprintln!("Observation summary write failed: {write}"));
        game.fault(&mut audio, error, "game.stopped");
        game.closing_error = true;
        game.closing = true;
        online.stop();
        input.queued.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_keep_single_round_paths_and_rotate_only_after_a_new_connection() {
        let directory = std::env::temp_dir().join(format!(
            "cocobeat-observation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let single = directory.join("single");
        let observation = Observation::new(&single, 1, "test output").unwrap();
        assert_eq!(observation.current.directory, single);
        let files = [
            "metadata.json",
            "frames.csv",
            "result.json",
            "running.png",
            "screenshot.png",
        ];
        assert!(files.iter().all(|file| single.join(file).is_file()));
        observation.write_summary(None).unwrap();
        assert!(!single.join("summary.json").exists());
        drop(observation);

        let multiple = directory.join("multiple");
        let mut observation = Observation::new(&multiple, 2, "test output").unwrap();
        observation.current.frame = 7;
        observation.current.start_sent = true;
        observation.current.hits_sent = 3;
        observation.current.requested = [true; 2];
        observation.current.captures = [Some(Ok([1280, 800])), Some(Ok([1280, 800]))];
        let mut game = Game::new().unwrap();
        game.phase = Phase::Finished;
        record_result(&mut observation, &game, &OnlineRound::default(), None).unwrap();
        let first_files =
            files.map(|file| std::fs::read(multiple.join("round-1").join(file)).unwrap());
        observation.restart_sent = true;
        observation
            .observe_transition(Phase::Finished, 1, "test output")
            .unwrap();
        observation
            .observe_transition(Phase::Connecting, 1, "test output")
            .unwrap();
        assert_eq!(observation.current.number, 1);
        assert!(!multiple.join("round-2").exists());

        observation
            .observe_transition(Phase::Connecting, 0, "test output")
            .unwrap();
        assert_eq!(observation.current.number, 2);
        assert_eq!(observation.current.directory, multiple.join("round-2"));
        assert_eq!(observation.current.frame, 0);
        assert_eq!(observation.current.hits_sent, 0);
        assert!(!observation.current.start_sent);
        assert!(!observation.current.terminal_written);
        assert_eq!(observation.current.requested, [false; 2]);
        assert_eq!(observation.current.captures, [None, None]);
        assert!(!observation.restart_sent);
        observation.current.frame = 1;
        observation
            .observe_transition(Phase::Connecting, 0, "test output")
            .unwrap();
        assert_eq!(observation.current.frame, 1);
        game.session = Session::for_content(SessionEpoch(42), &game.content).unwrap();
        record_result(&mut observation, &game, &OnlineRound::default(), None).unwrap();
        observation.write_summary(None).unwrap();
        let summary: serde_json::Value =
            serde_json::from_slice(&std::fs::read(multiple.join("summary.json")).unwrap()).unwrap();
        assert_eq!(summary["status"], "COMPLETE");
        assert_eq!(summary["round_count"], 2);
        assert_eq!(
            summary["rounds"],
            serde_json::json!([
                {"round": 1, "epoch": 0, "status": "COMPLETE"},
                {"round": 2, "epoch": 42, "status": "COMPLETE"},
            ])
        );
        for (file, original) in files.iter().zip(first_files) {
            assert_eq!(
                std::fs::read(multiple.join("round-1").join(file)).unwrap(),
                original
            );
        }
        drop(observation);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
