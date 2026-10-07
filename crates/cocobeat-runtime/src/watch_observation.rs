//! Explicit native watcher QA records synthetic input and acknowledged source cursors

use super::*;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
    },
    window::{PrimaryWindow, WindowFocused},
};
use std::{collections::BTreeMap, fs::File, io::Write, time::Instant};

#[derive(Resource)]
struct Observation {
    directory: PathBuf,
    frames: File,
    started: Instant,
    frame: u32,
    step: u8,
    step_at: Instant,
    original: Vec<u8>,
    epoch: SessionEpoch,
    captures: BTreeMap<&'static str, Option<Result<[u32; 2], String>>>,
    finished: Option<serde_json::Value>,
    paused: Option<serde_json::Value>,
    expected_events: Vec<DuoEvent>,
}

pub(super) fn install_if_requested(app: &mut App) -> Result<(), String> {
    let Some(directory) = std::env::var_os("COCOBEAT_WATCH_OBSERVATION_DIR") else {
        return Ok(());
    };
    let game = app.world().resource::<Game>();
    if game.playback.is_none() {
        return Err("Watch observation requires --package DIR --watch-replay FILE".into());
    }
    if !(96_000..=4_800_000).contains(&game.content.end.frames()) {
        return Err("Native watch observation requires a 2-to-100-second QA song".into());
    }
    let original = game
        .session
        .replay
        .encode()
        .map_err(|error| error.to_string())?;
    let epoch = game.session.epoch();
    let expected_events = game
        .session
        .replay
        .replay(
            &game.content.content_id,
            RULES_ID,
            game.content.anchors.clone(),
            DuoRules::default(),
        )
        .map_err(|error| error.to_string())?
        .events()
        .to_vec();
    let directory = PathBuf::from(directory);
    std::fs::create_dir(&directory)
        .map_err(|error| format!("Watch observation directory: {error}"))?;
    let mut metadata =
        File::create_new(directory.join("metadata.json")).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut metadata, &serde_json::json!({
        "process_id": std::process::id(), "build_id": env!("COCOBEAT_BUILD_ID"),
        "entrypoint": "--package DIR --watch-replay FILE, native DefaultPlugins and AudioOutput",
        "controls": "synthetic KeyboardInput and WindowFocused through existing input capture",
        "audio": "actual acknowledged Kira source cursor; no device output measurement",
        "epoch": epoch.0, "stage_version": game.session.replay.identity().stage_compiler_version,
        "canonical_frames": game.content.end.frames(),
        "not_run": ["physical keyboard", "physical gamepad", "speaker timing", "historical arrival timing", "human experience"]
    })).map_err(|error| error.to_string())?;
    metadata.sync_all().map_err(|error| error.to_string())?;
    let mut frames =
        File::create_new(directory.join("frames.csv")).map_err(|error| error.to_string())?;
    writeln!(frames, "frame,phase,brand_seconds,controls_enabled,audio_state,audio_position,acknowledged_frames,displayed_frames,consumed,total,events,epoch,step")
        .map_err(|error| error.to_string())?;
    if let Some(size) = std::env::var_os("COCOBEAT_WATCH_OBSERVATION_SIZE") {
        let size = match size.to_str() {
            Some("1280x800") => [1280, 800],
            Some("640x480") => [640, 480],
            _ => return Err("Watch observation size must be 1280x800 or 640x480".into()),
        };
        let settings = DisplaySettings {
            fullscreen: false,
            window_size: size,
            ..app.world().resource::<DisplayState>().actual()
        };
        app.world_mut()
            .resource_mut::<DisplayState>()
            .request(settings);
        app.world_mut()
            .resource_mut::<SettingsMenu>()
            .values
            .display = settings;
        for mut window in app
            .world_mut()
            .query::<&mut Window>()
            .iter_mut(app.world_mut())
        {
            window.resolution.set_scale_factor_override(Some(1.0));
            window.resolution.set_physical_resolution(size[0], size[1]);
        }
    }
    let now = Instant::now();
    app.insert_resource(Observation {
        directory,
        frames,
        started: now,
        frame: 0,
        step: 0,
        step_at: now,
        original,
        epoch,
        captures: BTreeMap::new(),
        finished: None,
        paused: None,
        expected_events,
    })
    .add_systems(Update, observe.after(update_game));
    Ok(())
}

fn key(keys: &mut MessageWriter<KeyboardInput>, window: Entity, key_code: KeyCode) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        keys.write(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        });
    }
}

fn snapshot(
    observation: &mut Observation,
    name: &'static str,
    commands: &mut Commands,
) -> Result<(), String> {
    if observation.captures.contains_key(name) {
        return Ok(());
    }
    let path = observation.directory.join(format!("{name}.png"));
    File::create_new(&path).map_err(|error| error.to_string())?;
    observation.captures.insert(name, None);
    commands.spawn(Screenshot::primary_window()).observe(
        move |capture: On<ScreenshotCaptured>, mut observation: ResMut<Observation>| {
            let result = capture
                .image
                .clone()
                .try_into_dynamic()
                .map_err(|error| error.to_string())
                .and_then(|image| {
                    let size = [image.width(), image.height()];
                    image.save(&path).map_err(|error| error.to_string())?;
                    Ok(size)
                });
            observation.captures.insert(name, Some(result));
        },
    );
    Ok(())
}

fn record(observation: &Observation, game: &Game, error: Option<&str>) -> Result<(), String> {
    let mut file = File::create_new(observation.directory.join("result.json"))
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, &serde_json::json!({
        "status": if error.is_none() { "PASS" } else { "FAIL" }, "error": error,
        "phase": format!("{:?}", game.phase), "process_id": std::process::id(),
        "frames": observation.frame, "steps": observation.step,
        "epoch": game.session.epoch().0, "original_epoch": observation.epoch.0,
        "facts_unchanged": game.session.replay.encode().map_err(|error| error.to_string())? == observation.original,
        "capture": observation.captures, "finished": observation.finished, "paused": observation.paused,
    })).map_err(|error| error.to_string())?;
    observation
        .frames
        .sync_all()
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

fn observe(
    mut observation: ResMut<Observation>,
    (mut game, input, brand): (ResMut<Game>, Res<InputState>, Res<BrandIntroStatus>),
    (audio, online): (NonSend<AudioOutput>, NonSend<OnlineRound>),
    windows: Query<Entity, With<PrimaryWindow>>,
    (mut commands, mut keys, mut focus): (
        Commands,
        MessageWriter<KeyboardInput>,
        MessageWriter<WindowFocused>,
    ),
) {
    if game.closing {
        return;
    }
    let result = (|| -> Result<(), String> {
        observation.frame += 1;
        if observation.frame > 40_000 || observation.started.elapsed().as_secs() > 150 {
            return Err("Native watch observation exceeded its bounded deadline".into());
        }
        if game.phase == Phase::Fault {
            return Err(game
                .fault_details
                .clone()
                .unwrap_or_else(|| game.notice.key.into()));
        }
        if game.session.epoch() != observation.epoch
            || online.enabled()
            || !game.session.diagnostics.is_empty()
        {
            return Err("Watcher changed epoch or captured live/network input".into());
        }
        let playback = game.playback.as_ref().ok_or("Watcher disappeared")?;
        let ack = game
            .session
            .clock
            .last_observation()
            .map(|v| v.song_time.frames());
        if matches!(game.phase, Phase::Running | Phase::Paused)
            && ack != Some(game.session.current.frames())
        {
            return Err(
                "Watcher displayed an extrapolated cursor instead of acknowledged source position"
                    .into(),
            );
        }
        let frame = observation.frame;
        let step = observation.step;
        writeln!(
            observation.frames,
            "{frame},{:?},{},{},{:?},{},{},{},{},{},{},{},{step}",
            game.phase,
            brand.elapsed_seconds,
            input.controls_enabled(),
            audio.state(),
            audio.position().map(|v| v.to_string()).unwrap_or_default(),
            ack.map(|v| v.to_string()).unwrap_or_default(),
            game.session.current.frames(),
            playback.consumed(),
            game.session.replay.facts().len(),
            game.session.engine.events().len(),
            game.session.epoch().0,
        )
        .map_err(|error| error.to_string())?;
        if let Some(error) = observation
            .captures
            .values()
            .find_map(|v| v.as_ref().and_then(|v| v.as_ref().err()))
        {
            return Err(format!("Watch screenshot failed: {error}"));
        }
        let window = windows.single().map_err(|error| error.to_string())?;
        // Focus and controls are synthetic software observations, not physical device evidence
        if observation.frame == 1 {
            focus.write(WindowFocused {
                window,
                focused: true,
            });
        }
        let advance = match observation.step {
            0 if brand.is_complete() && input.controls_enabled() && game.phase == Phase::Ready => {
                snapshot(&mut observation, "ready", &mut commands)?;
                key(&mut keys, window, KeyCode::Enter); // Claim menu ownership
                true
            }
            1 if observation.step_at.elapsed().as_millis() > 150 => {
                key(&mut keys, window, KeyCode::Enter); // Fresh confirmation
                true
            }
            2 if game.phase == Phase::Running
                && game.session.current.frames() >= game.content.end.frames() / 2 =>
            {
                // Live game keys and Save are intentionally harmless in read-only watch mode
                key(&mut keys, window, KeyCode::KeyF);
                key(&mut keys, window, KeyCode::KeyJ);
                key(&mut keys, window, KeyCode::F6);
                key(&mut keys, window, KeyCode::Escape);
                true
            }
            3 if game.phase == Phase::Paused => {
                observation.paused = Some(serde_json::json!({
                    "song_frame": game.session.current.frames(),
                    "acknowledged_frame": ack,
                    "consumed": playback.consumed(),
                    "stage_sample": format!("{:?}", game.content.stage.as_ref().and_then(|stage| stage.sample(game.session.current))),
                }));
                snapshot(&mut observation, "paused", &mut commands)?;
                true
            }
            4 if observation.step_at.elapsed().as_millis() > 350
                && observation.captures["paused"].is_some() =>
            {
                key(&mut keys, window, KeyCode::Enter);
                true
            }
            5 if game.phase == Phase::Finished => {
                if playback.consumed() != game.session.replay.facts().len()
                    || game.session.engine.events() != observation.expected_events
                {
                    return Err("Watcher EOF differs from original full core history".into());
                }
                let stage = game
                    .content
                    .stage
                    .as_ref()
                    .ok_or("Missing watcher StagePlan")?;
                observation.finished = Some(serde_json::json!({
                    "consumed": playback.consumed(), "total": game.session.replay.facts().len(),
                    "events": game.session.engine.events().len(), "source_end": game.session.current.frames(),
                    "summary": format!("{:?}", game.summary()), "compiler_version": stage.compiler_version(),
                }));
                snapshot(&mut observation, "finished", &mut commands)?;
                true
            }
            6 if observation.captures["finished"].is_some()
                && observation.step_at.elapsed().as_millis() > 150 =>
            {
                key(&mut keys, window, KeyCode::F5);
                true
            }
            7 if game.phase == Phase::Running && game.session.current.frames() >= 24_000 => {
                snapshot(&mut observation, "restarted", &mut commands)?;
                key(&mut keys, window, KeyCode::Escape);
                true
            }
            8 if game.phase == Phase::Paused => {
                key(&mut keys, window, KeyCode::ArrowDown);
                true
            }
            9 if observation.step_at.elapsed().as_millis() > 100 => {
                // Pause actions: Resume, Settings, Restart, MainMenu, Quit
                key(&mut keys, window, KeyCode::ArrowDown);
                true
            }
            10 if observation.step_at.elapsed().as_millis() > 100 => {
                key(&mut keys, window, KeyCode::ArrowDown);
                true
            }
            11 if observation.step_at.elapsed().as_millis() > 100 => {
                key(&mut keys, window, KeyCode::Enter);
                true
            }
            12 if game.phase == Phase::Ready => {
                if playback.consumed() != 0 || !game.session.engine.events().is_empty() {
                    return Err("Returning to menu did not reset recorded playback".into());
                }
                snapshot(&mut observation, "returned", &mut commands)?;
                true
            }
            13 if observation.captures.values().all(Option::is_some) => {
                if game
                    .session
                    .replay
                    .encode()
                    .map_err(|error| error.to_string())?
                    != observation.original
                {
                    return Err("Read-only watcher mutated original facts".into());
                }
                record(&observation, &game, None)?;
                game.closing = true;
                return Ok(());
            }
            _ => false,
        };
        if advance {
            observation.step += 1;
            observation.step_at = Instant::now();
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Native watch observation failed: {error}");
        let _ = record(&observation, &game, Some(&error))
            .inspect_err(|error| eprintln!("Watch result write failed: {error}"));
        game.closing_error = true;
        game.closing = true;
    }
}
