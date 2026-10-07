//! Opt-in native observations use synthetic controls, never physical input evidence

use super::*;
use std::{fs::File, io::Write, time::Instant};

#[derive(Resource)]
struct Observation {
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

pub(super) fn install(app: &mut App, directory: &Path) -> Result<(), String> {
    std::fs::create_dir(directory).map_err(|error| format!("Observation directory: {error}"))?;
    let file = |name| File::create_new(directory.join(name)).map_err(|error| error.to_string());
    let mut metadata = file("metadata.json")?;
    serde_json::to_writer_pretty(&mut metadata, &serde_json::json!({
        "build_id": env!("COCOBEAT_BUILD_ID"),
        "entrypoint": "native DefaultPlugins, AudioOutput, update_game",
        "controls": "synthetic CapturedControl at software-observed monotonic timestamps",
        "audio": "actual Kira source cursor, no speaker latency measurement",
        "output_info": app.world().non_send::<AudioOutput>().output_info(),
        "not_run": ["physical keyboard", "physical gamepad", "speaker synchronization", "two machines", "human experience", "native-language review"]
    })).map_err(|error| error.to_string())?;
    metadata.sync_all().map_err(|error| error.to_string())?;
    file("running.png")?;
    file("screenshot.png")?;
    let mut frames = file("frames.csv")?;
    writeln!(frames, "frame,monotonic_ns,phase,controls_enabled,brand_complete,cursor_seconds,source_song_frames,estimated_song_frames,epoch,local_player,facts,events,synthetic_hits,network_started")
        .map_err(|error| error.to_string())?;
    app.insert_resource(Observation {
        directory: directory.to_path_buf(),
        frames,
        result: file("result.json")?,
        started: Instant::now(),
        frame: 0,
        start_sent: false,
        hits_sent: 0,
        requested: [false; 2],
        captures: [None, None],
        terminal_written: false,
    })
    .add_systems(
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
    if observation.terminal_written {
        return Ok(());
    }
    observation.terminal_written = true;
    let diagnostics: Vec<_> = game.session.diagnostics.iter().map(|capture| serde_json::json!({
        "player": format!("{:?}", capture.player), "seq": capture.seq,
        "observed_ns": capture.observed_ns, "consumed_ns": capture.consumed_ns,
        "song_frames": capture.song_frames, "uncertainty_frames": capture.uncertainty_frames,
    })).collect();
    let result = serde_json::json!({
        "status": if game.phase == Phase::Finished && error.is_none() { "COMPLETE" } else { "FAILED" },
        "phase": format!("{:?}", game.phase), "error": error.or(game.fault_details.as_deref()),
        "epoch": game.session.epoch().0, "content_id": game.content.content_id,
        "canonical_frames": game.content.end.frames(), "frames_observed": observation.frame,
        "local_player": online.player.map(|player| format!("{player:?}")),
        "network_started": online.started, "local_ended": online.local_ended,
        "facts": game.session.replay.facts().len(), "events": game.session.engine.events().len(),
        "synthetic_hits_requested": observation.hits_sent, "capture_diagnostics": diagnostics,
        "running_capture": observation.captures[0], "terminal_capture": observation.captures[1],
    });
    serde_json::to_writer_pretty(&mut observation.result, &result)
        .map_err(|error| error.to_string())?;
    observation
        .frames
        .sync_all()
        .and_then(|()| observation.result.sync_all())
        .map_err(|error| error.to_string())
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
        if observation.started.elapsed().as_secs() >= 120 {
            return Err("Native observation exceeded its 120-second deadline".into());
        }
        observation.frame += 1;
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
        let frame = observation.frame;
        let hits_sent = observation.hits_sent;
        writeln!(
            observation.frames,
            "{},{now},{:?},{},{},{cursor},{source},{},{},{},{},{},{},{}",
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
            online.started
        )
        .map_err(|error| error.to_string())?;
        if !observation.start_sent
            && brand.is_complete()
            && input.controls_enabled()
            && game.phase == Phase::Ready
        {
            input.queued.push(input::CapturedControl {
                control: Control::Start,
                monotonic_ns: now,
            });
            observation.start_sent = true;
        }
        if game.phase == Phase::Running && observation.hits_sent < 3 {
            let threshold = game.content.end.frames() * (observation.hits_sent as i64 + 1) / 4;
            if game.session.current.frames() >= threshold {
                let player = online.player.unwrap_or(PlayerId::P1);
                input.queued.push(input::CapturedControl {
                    control: Control::Hit(player),
                    monotonic_ns: now,
                });
                observation.hits_sent += 1;
            }
        }
        if let Some(error) = observation
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
                matches!(game.phase, Phase::Finished | Phase::Fault)
            };
            if !eligible || observation.requested[index] {
                continue;
            }
            observation.requested[index] = true;
            let path = observation.directory.join(if index == 0 {
                "running.png"
            } else {
                "screenshot.png"
            });
            commands.spawn(Screenshot::primary_window()).observe(
                move |capture: On<ScreenshotCaptured>, mut observation: ResMut<Observation>| {
                    observation.captures[index] = Some(
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
        if observation.captures[1].is_some()
            && (!observation.requested[0] || observation.captures[0].is_some())
        {
            record_result(&mut observation, &game, &online, None)?;
            game.closing_error = game.phase == Phase::Fault;
            input.queued.push(input::CapturedControl {
                control: Control::Quit,
                monotonic_ns: now,
            });
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Native observation failed: {error}");
        let _ = record_result(&mut observation, &game, &online, Some(&error))
            .inspect_err(|write| eprintln!("Observation result write failed: {write}"));
        game.fault(&mut audio, error, "game.stopped");
        game.closing_error = true;
        game.closing = true;
        online.stop();
        input.queued.clear();
    }
}
