//! Explicit authored-library QA uses production input capture, never direct menu actions

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
    automatic_frames: Option<File>,
    started: Instant,
    step_at: Instant,
    frame: u32,
    step: u8,
    navigation_step: u8,
    navigation: BTreeMap<&'static str, serde_json::Value>,
    scenario: String,
    source_records: Vec<serde_json::Value>,
    source_not_run: Vec<&'static str>,
    captures: BTreeMap<&'static str, Option<Result<[u32; 2], String>>>,
    snapshots: BTreeMap<&'static str, serde_json::Value>,
    rejections: Vec<serde_json::Value>,
    before: Option<serde_json::Value>,
    cancel: Option<serde_json::Value>,
    close_requested_while_owned: bool,
    worker_unfinished_at_close: bool,
    recorded: bool,
}

pub(super) fn install_if_requested(app: &mut App) -> Result<(), String> {
    let Some(directory) = std::env::var_os("COCOBEAT_LIBRARY_OBSERVATION_DIR") else {
        return Ok(());
    };
    if app.world().resource::<Game>().playback.is_some()
        || app.world().non_send::<OnlineRound>().enabled()
        || app.world().non_send::<LibraryBrowser>().loader.is_none()
    {
        return Err("Library observation requires a normal local song library".into());
    }
    let size = match std::env::var("COCOBEAT_LIBRARY_OBSERVATION_SIZE").as_deref() {
        Ok("1280x800") => [1280, 800],
        Ok("640x480") => [640, 480],
        _ => return Err("Library observation size must be 1280x800 or 640x480".into()),
    };
    let locale = match std::env::var("COCOBEAT_LIBRARY_OBSERVATION_LOCALE").as_deref() {
        Ok("zh-CN") => Locale::ZhCn,
        Ok("en-US") => Locale::EnUs,
        _ => return Err("Library observation locale must be zh-CN or en-US".into()),
    };
    let scenario = std::env::var("COCOBEAT_LIBRARY_OBSERVATION_SCENARIO")
        .map_err(|error| error.to_string())?;
    if !matches!(
        scenario.as_str(),
        "complete"
            | "close-scan"
            | "close-load"
            | "source-complete"
            | "source-back"
            | "source-focus"
            | "source-bad-json"
            | "source-unknown-rules"
            | "source-background-error"
            | "source-close"
            | "timing-short"
            | "automatic-structure"
    ) {
        return Err("Unknown library observation scenario".into());
    }
    let directory = PathBuf::from(directory);
    if !directory.is_absolute() {
        return Err("Library observation directory must be absolute".into());
    }
    std::fs::create_dir(&directory)
        .map_err(|error| format!("Library observation directory: {error}"))?;
    let mut metadata =
        File::create_new(directory.join("metadata.json")).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut metadata, &serde_json::json!({
        "process_id": std::process::id(), "build_id": env!("COCOBEAT_BUILD_ID"),
        "entrypoint": if scenario.starts_with("source-") { "normal local Ready runtime with existing data-root library and native DefaultPlugins / AudioOutput" } else if matches!(scenario.as_str(), "timing-short" | "automatic-structure") { "--package PACKAGE with optional explicit --timing-diagnostics, native DefaultPlugins and original input capture" } else { "--library DIR with native DefaultPlugins and AudioOutput" },
        "scenario": scenario, "locale": locale.code(), "size": size,
        "input": "synthetic KeyboardInput and WindowFocused through the production capture system",
        "audio": "acknowledged Kira source position, not speaker output",
        "disable_gamescope_wsi": std::env::var("DISABLE_GAMESCOPE_WSI").ok(),
        "audio_observations": "Independent historical main-mix and source publication snapshots; not a paired callback, CPAL entry or device latency",
        "not_run": ["physical keyboard", "physical gamepads", "physical mixed input", "speaker timing", "human acceptance"]
    })).map_err(|error| error.to_string())?;
    metadata.sync_all().map_err(|error| error.to_string())?;
    let display = DisplaySettings {
        fullscreen: false,
        window_size: size,
        ..app.world().resource::<DisplayState>().actual()
    };
    app.world_mut()
        .resource_mut::<DisplayState>()
        .request(display);
    let mut settings = app.world_mut().resource_mut::<SettingsMenu>();
    settings.values.display = display;
    settings.values.locale = locale;
    for mut window in app
        .world_mut()
        .query::<&mut Window>()
        .iter_mut(app.world_mut())
    {
        window.resolution.set_scale_factor_override(Some(1.0));
        window.resolution.set_physical_resolution(size[0], size[1]);
    }
    let mut frames =
        File::create_new(directory.join("frames.csv")).map_err(|error| error.to_string())?;
    writeln!(frames, "frame,phase,library_open,busy,worker_finished,content_id,canonical_frames,epoch,facts,events,controls_enabled,audio_state,audio_position,brand_seconds,step")
        .map_err(|error| error.to_string())?;
    let automatic = scenario == "automatic-structure";
    let automatic_frames = if automatic {
        Some(
            File::create_new(directory.join("structure-frames.jsonl"))
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let now = Instant::now();
    app.insert_resource(Observation {
        directory,
        frames,
        automatic_frames,
        started: now,
        step_at: now,
        frame: 0,
        step: 0,
        navigation_step: 0,
        navigation: BTreeMap::new(),
        scenario,
        source_records: vec![],
        source_not_run: vec![],
        captures: BTreeMap::new(),
        snapshots: BTreeMap::new(),
        rejections: vec![],
        before: None,
        cancel: None,
        close_requested_while_owned: false,
        worker_unfinished_at_close: false,
        recorded: false,
    });
    if automatic {
        app.add_systems(
            PostUpdate,
            observe
                .after(crate::scene::animate)
                .after(bevy::ui::UiSystems::Layout),
        );
    } else {
        app.add_systems(Update, observe.after(update_game));
    }
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

fn select(keys: &mut MessageWriter<KeyboardInput>, window: Entity, index: usize) {
    for _ in 0..index {
        key(keys, window, KeyCode::ArrowDown);
    }
    key(keys, window, KeyCode::Enter);
}

fn state(game: &Game, audio: &AudioOutput) -> Result<serde_json::Value, String> {
    let callback = audio.callback_observation();
    let source = audio.source_observation();
    let read_after = Instant::now();
    let callback = callback.map(|observed| serde_json::json!({
        "generation": observed.generation,
        "sequence": observed.sequence,
        "output_sample_rate": observed.output_sample_rate,
        "previous_frames": observed.previous_frames,
        "previous_callback_interval_ns": observed.previous_observed_at.and_then(|previous| observed.observed_at.checked_duration_since(previous)).map(|duration| duration.as_nanos()),
        "observation_age_ns": read_after.checked_duration_since(observed.observed_at).map(|duration| duration.as_nanos()),
    }));
    let source = source.map(|observed| serde_json::json!({
        "generation": observed.generation,
        "source_id": observed.source_id,
        "sequence": observed.sequence,
        "position_seconds": observed.position_seconds,
        "publication_interval_width_ns": observed.published_between[1].checked_duration_since(observed.published_between[0]).map(|duration| duration.as_nanos()),
        "publication_age_lower_ns": read_after.checked_duration_since(observed.published_between[1]).map(|duration| duration.as_nanos()),
        "publication_age_upper_ns": read_after.checked_duration_since(observed.published_between[0]).map(|duration| duration.as_nanos()),
    }));
    Ok(serde_json::json!({
        "content_id": game.content.content_id, "song_id": game.song_id, "package_path": game.package_path,
        "canonical_frames": game.content.end.frames(), "epoch": game.session.epoch().0,
        "facts": game.session.replay.facts().len(), "events": game.session.engine.events().len(),
        "phase": format!("{:?}", game.phase), "results": format!("{:?}", game.results),
        "audio_started": audio.state().is_some_and(|state| state != PlaybackState::Stopped),
        "source_position_seconds": audio.position(),
        "callback_observation": callback, "source_observation": source,
        "acknowledged_frame": game.session.clock.last_observation().map(|observed| observed.song_time.frames()),
        "replay_bytes": game.session.replay.encode().map_err(|error| error.to_string())?,
    }))
}

fn same_song(before: &serde_json::Value, after: &serde_json::Value) -> bool {
    before.as_object().is_some_and(|fields| {
        fields
            .iter()
            .filter(|(key, _)| {
                !matches!(key.as_str(), "callback_observation" | "source_observation")
            })
            .all(|(key, value)| after.get(key) == Some(value))
    })
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

fn record(
    observation: &Observation,
    game: &Game,
    browser: &LibraryBrowser,
    online: &OnlineRound,
    error: Option<&str>,
) -> Result<(), String> {
    let mut file = File::create_new(observation.directory.join("result.json"))
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, &serde_json::json!({
        "status": if error.is_none() { "PASS" } else { "FAIL" }, "error": error,
        "scenario": observation.scenario, "process_id": std::process::id(), "steps": observation.step,
        "phase": format!("{:?}", game.phase), "frames": observation.frame,
        "owned_workers_finished": !browser.busy() && browser.is_finished() && online.is_finished(),
        "close_requested_while_owned": observation.close_requested_while_owned,
        "worker_unfinished_at_close": observation.worker_unfinished_at_close,
        "snapshots": observation.snapshots, "rejections": observation.rejections,
        "cancel": observation.cancel, "capture": observation.captures,
        "navigation": observation.navigation,
        "source_records": observation.source_records, "source_not_run": observation.source_not_run,
    })).map_err(|error| error.to_string())?;
    observation
        .frames
        .sync_all()
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

fn observe(
    mut observation: ResMut<Observation>,
    (game, input, brand, visual): (
        Res<Game>,
        Res<InputState>,
        Res<BrandIntroStatus>,
        Res<VisualState>,
    ),
    (audio, browser, online): (
        NonSend<AudioOutput>,
        NonSend<LibraryBrowser>,
        NonSend<OnlineRound>,
    ),
    windows: Query<Entity, With<PrimaryWindow>>,
    (scene, ground, ground_meshes, meshes): AutomaticScene,
    (mut commands, mut keys, mut focus, mut close): (
        Commands,
        MessageWriter<KeyboardInput>,
        MessageWriter<WindowFocused>,
        MessageWriter<WindowCloseRequested>,
    ),
) {
    if observation.recorded {
        return;
    }
    let result = (|| -> Result<(), String> {
        observation.frame += 1;
        if observation.frame > 40_000 || observation.started.elapsed().as_secs() > 165 {
            return Err("Library observation exceeded its bounded deadline".into());
        }
        if game.phase == Phase::Fault {
            return Err(game
                .fault_details
                .clone()
                .unwrap_or_else(|| game.notice.key.into()));
        }
        if online.enabled() || game.playback.is_some() {
            return Err("Library observer entered online or Replay mode".into());
        }
        if let Some(error) = observation
            .captures
            .values()
            .find_map(|capture| capture.as_ref().and_then(|capture| capture.as_ref().err()))
        {
            return Err(format!("Library screenshot failed: {error}"));
        }
        let frame = observation.frame;
        let step = observation.step;
        writeln!(
            observation.frames,
            "{frame},{:?},{},{},{},{},{},{},{},{},{},{:?},{},{},{step}",
            game.phase,
            input.library_open(),
            browser.busy(),
            browser.is_finished(),
            game.content.content_id,
            game.content.end.frames(),
            game.session.epoch().0,
            game.session.replay.facts().len(),
            game.session.engine.events().len(),
            input.controls_enabled(),
            audio.state(),
            audio
                .position()
                .map(|position| position.to_string())
                .unwrap_or_default(),
            brand.elapsed_seconds
        )
        .map_err(|error| error.to_string())?;
        let window = windows.single().map_err(|error| error.to_string())?;
        if frame == 1 {
            focus.write(WindowFocused {
                window,
                focused: true,
            });
        }
        if observation.scenario == "automatic-structure" {
            if observe_automatic(
                &mut observation,
                &game,
                &input,
                &brand,
                &visual,
                &audio,
                &browser,
                &online,
                scene.as_deref(),
                ground.as_deref(),
                &ground_meshes,
                meshes
                    .as_deref()
                    .ok_or("Automatic scene has no Mesh assets resource")?,
                &mut commands,
                &mut keys,
                &mut close,
                window,
            )? {
                observation.step += 1;
                observation.step_at = Instant::now();
            }
            return Ok(());
        }
        if observation.scenario == "timing-short" {
            if game.closing {
                if observation.step != 9 {
                    return Err("Timing observation closed before its actual control checks".into());
                }
                if !browser.busy() && browser.is_finished() && online.is_finished() {
                    record(&observation, &game, &browser, &online, None)?;
                    observation.recorded = true;
                }
                return Ok(());
            }
            let settled = observation.step_at.elapsed().as_millis() > 250;
            let target = if game.session.timing.is_some() {
                game.content
                    .anchors
                    .first()
                    .ok_or("Timing fixture has no authored Anchor")?
                    .song_time
                    .as_seconds_f64()
            } else {
                0.5
            };
            let advance = match step {
                0 if brand.is_complete()
                    && input.controls_enabled()
                    && game.phase == Phase::Ready =>
                {
                    observation
                        .snapshots
                        .insert("timing_ready", state(&game, &audio)?);
                    observation.snapshots.insert("timing_configuration", serde_json::json!({
                        "enabled": game.session.timing.is_some(), "target_song_seconds": target,
                        "capture_players": game.session.timing.as_ref().map(|timing| &timing.players),
                        "scope": "KeyboardInput message observation; no physical-versus-injected classification",
                    }));
                    key(&mut keys, window, KeyCode::Enter);
                    true
                }
                1 if settled => {
                    activate_label(&visual, visual.locale.text("menu.start"), &mut keys, window)?;
                    true
                }
                2 if game.phase == Phase::Running
                    && audio.position().is_some_and(|position| position >= target) =>
                {
                    observation
                        .snapshots
                        .insert("timing_emit_hits", state(&game, &audio)?);
                    observation.snapshots.insert("timing_emission", serde_json::json!({
                        "emit_input_relative_ns": input.origin.elapsed().as_nanos(),
                        "target_song_seconds": target,
                        "basis": "Observer emission time only; original capture timestamps come from Session diagnostics",
                    }));
                    key(&mut keys, window, KeyCode::KeyF);
                    key(&mut keys, window, KeyCode::KeyJ);
                    true
                }
                3 if game.phase == Phase::Running
                    && game.session.diagnostics.len() == 2
                    && audio
                        .position()
                        .is_some_and(|position| position >= target + 0.75) =>
                {
                    observation
                        .snapshots
                        .insert("timing_before_pause", state(&game, &audio)?);
                    observation.snapshots.insert("timing_capture_diagnostics", serde_json::json!(
                        game.session.diagnostics.iter().map(|capture| serde_json::json!({
                            "player": capture.player.index() + 1, "seq": capture.seq,
                            "observed_ns": capture.observed_ns, "consumed_ns": capture.consumed_ns,
                            "song_frames": capture.song_frames, "uncertainty_frames": capture.uncertainty_frames,
                        })).collect::<Vec<_>>()
                    ));
                    key(&mut keys, window, KeyCode::Escape);
                    true
                }
                4 if game.phase == Phase::Paused && settled => {
                    observation
                        .snapshots
                        .insert("timing_paused", state(&game, &audio)?);
                    snapshot(&mut observation, "timing-paused", &mut commands)?;
                    observation.captures["timing-paused"].is_some()
                }
                5 if settled => {
                    activate_label(
                        &visual,
                        visual.locale.text("menu.resume"),
                        &mut keys,
                        window,
                    )?;
                    true
                }
                6 if game.phase == Phase::Running
                    && settled
                    && audio.position().is_some_and(|position| {
                        position
                            > observation.snapshots["timing_paused"]["source_position_seconds"]
                                .as_f64()
                                .unwrap_or(f64::INFINITY)
                                + 0.25
                    }) =>
                {
                    observation
                        .snapshots
                        .insert("timing_resumed", state(&game, &audio)?);
                    key(&mut keys, window, KeyCode::F6);
                    true
                }
                7 if settled => {
                    if game.replay_status.key != "results.replay_saved" {
                        return Err(
                            "Explicit timing run save did not use the original Replay save path"
                                .into(),
                        );
                    }
                    observation.snapshots.insert("timing_explicit_saved", serde_json::json!({
                        "status_key": game.replay_status.key, "status_args": game.replay_status.args,
                        "actual_song": state(&game, &audio)?,
                    }));
                    true
                }
                8 => {
                    observation
                        .snapshots
                        .insert("timing_before_close", state(&game, &audio)?);
                    close.write(WindowCloseRequested { window });
                    true
                }
                _ => false,
            };
            if advance {
                observation.step += 1;
                observation.step_at = Instant::now();
            }
            return Ok(());
        }
        if observation.scenario.starts_with("source-") {
            let settled = observation.step_at.elapsed().as_millis() > 150;
            if observe_source(
                &mut observation,
                &game,
                &input,
                &brand,
                &visual,
                &audio,
                &browser,
                &online,
                &mut commands,
                &mut keys,
                &mut focus,
                &mut close,
                window,
                settled,
            )? {
                observation.step += 1;
                observation.step_at = Instant::now();
            }
            return Ok(());
        }
        if game.closing && !browser.busy() && browser.is_finished() && online.is_finished() {
            if (observation.scenario == "complete" && step != 22)
                || (observation.scenario != "complete" && !observation.close_requested_while_owned)
            {
                return Err(
                    "Library window closed before the requested observation completed".into(),
                );
            }
            record(&observation, &game, &browser, &online, None)?;
            observation.recorded = true;
            return Ok(());
        }
        if game.closing {
            return Ok(());
        }
        let settled = observation.step_at.elapsed().as_millis() > 150;
        if step == 2 && observation.scenario == "complete" && observation.navigation_step < 6 {
            let advanced = match observation.navigation_step {
                0 if input.library_open() && !browser.busy() && settled => {
                    snapshot(&mut observation, "library", &mut commands)?;
                    if observation.captures["library"].is_some() {
                        for _ in 0..browser.entries.len() + 2 {
                            key(&mut keys, window, KeyCode::ArrowDown);
                        }
                        true
                    } else {
                        false
                    }
                }
                navigation @ 1..=3 if input.library_open() && !browser.busy() && settled => {
                    let (name, index) = match navigation {
                        1 => ("library-refresh", browser.entries.len() + 2),
                        2 => ("library-back", browser.entries.len() + 3),
                        _ => ("library-information", browser.entries.len() + 4),
                    };
                    let row = visual
                        .menu
                        .as_ref()
                        .and_then(|menu| menu.rows.get(index))
                        .filter(|row| row.selected)
                        .ok_or("Library navigation did not focus the expected real menu row")?;
                    if navigation == 3 && row.role != MenuRowRole::Information {
                        return Err("Library navigation did not reach its information row".into());
                    }
                    observation.navigation.insert(
                        name,
                        serde_json::json!({
                            "index": index, "selected": row.selected,
                            "role": format!("{:?}", row.role), "text": row.text,
                            "candidate_count": browser.entries.len(),
                        }),
                    );
                    snapshot(&mut observation, name, &mut commands)?;
                    if observation.captures[name].is_some() {
                        key(
                            &mut keys,
                            window,
                            if navigation == 3 {
                                KeyCode::Escape
                            } else {
                                KeyCode::ArrowDown
                            },
                        );
                        true
                    } else {
                        false
                    }
                }
                4 if !input.library_open() && game.phase == Phase::Ready && settled => {
                    select(&mut keys, window, 3);
                    true
                }
                5 if input.library_open() && !browser.busy() && settled => true,
                _ => false,
            };
            if advanced {
                observation.navigation_step += 1;
                observation.step_at = Instant::now();
            }
            return Ok(());
        }
        let advance = match step {
            0 if brand.is_complete() && input.controls_enabled() && game.phase == Phase::Ready => {
                key(&mut keys, window, KeyCode::Enter);
                true
            }
            1 if settled => {
                select(&mut keys, window, 3);
                true
            }
            2 if input.library_open() && browser.busy() && observation.scenario == "close-scan" => {
                observation.close_requested_while_owned = true;
                observation.worker_unfinished_at_close = !browser.is_finished();
                close.write(WindowCloseRequested { window });
                false
            }
            2 if input.library_open() && !browser.busy() => {
                let names: Vec<_> = browser
                    .entries
                    .iter()
                    .map(|entry| {
                        entry
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect();
                if names
                    != [
                        "00-valid-a",
                        "01-valid-b",
                        "02-bad-audio",
                        "03-extra-file",
                        "04-unknown-rules",
                    ]
                {
                    return Err(format!("Unexpected library discovery: {names:?}"));
                }
                snapshot(&mut observation, "library", &mut commands)?;
                select(&mut keys, window, 1);
                true
            }
            3 if input.library_open() && browser.busy() => {
                snapshot(&mut observation, "loading", &mut commands)?;
                if observation.scenario == "close-load" {
                    observation.close_requested_while_owned = true;
                    observation.worker_unfinished_at_close = !browser.is_finished();
                    close.write(WindowCloseRequested { window });
                    false
                } else {
                    true
                }
            }
            4 if !input.library_open() && !browser.busy() && game.phase == Phase::Ready => {
                let value = state(&game, &audio)?;
                if value["audio_started"] != false || game.song_id.is_none() {
                    return Err("Selected package did not stop at fresh Ready".into());
                }
                observation.snapshots.insert("ready_a", value);
                snapshot(&mut observation, "ready-a", &mut commands)?;
                true
            }
            5 if settled && observation.captures["ready-a"].is_some() => {
                key(&mut keys, window, KeyCode::Enter);
                true
            }
            6 if game.phase == Phase::Running
                && audio.position().is_some_and(|position| position >= 0.5) =>
            {
                observation
                    .snapshots
                    .insert("running_a", state(&game, &audio)?);
                snapshot(&mut observation, "running-a", &mut commands)?;
                key(&mut keys, window, KeyCode::KeyF);
                key(&mut keys, window, KeyCode::KeyJ);
                key(&mut keys, window, KeyCode::Escape);
                true
            }
            7 if game.phase == Phase::Paused && settled => {
                if game.session.diagnostics.len() != 2 {
                    return Err(
                        "Library playback did not capture both original software Hits".into(),
                    );
                }
                observation
                    .snapshots
                    .insert("paused_a", state(&game, &audio)?);
                let index = if game.saved_facts != game.session.replay.facts().len() {
                    5
                } else {
                    4
                };
                select(&mut keys, window, index);
                true
            }
            8 if game.phase == Phase::Ready && settled => {
                select(&mut keys, window, 3);
                true
            }
            9 if input.library_open() && !browser.busy() => {
                select(&mut keys, window, 2);
                true
            }
            10 if input.library_open() && browser.busy() => true,
            11 if !input.library_open() && !browser.busy() && game.phase == Phase::Ready => {
                observation
                    .snapshots
                    .insert("ready_b", state(&game, &audio)?);
                snapshot(&mut observation, "ready-b", &mut commands)?;
                true
            }
            12 if settled => {
                select(&mut keys, window, 3);
                true
            }
            13 if input.library_open() && !browser.busy() => {
                observation.before = Some(state(&game, &audio)?);
                select(&mut keys, window, 3 + observation.rejections.len());
                true
            }
            14 if input.library_open() && browser.busy() => true,
            15 if input.library_open() && !browser.busy() => {
                if browser.notice.key != "library.failed" {
                    return Err(
                        "Invalid package was not rejected by full production validation".into(),
                    );
                }
                let before = observation
                    .before
                    .take()
                    .ok_or("Missing rejection baseline")?;
                let after = state(&game, &audio)?;
                if !same_song(&before, &after) {
                    return Err("Rejected package changed the old song or history".into());
                }
                let name =
                    ["bad-audio", "extra-file", "unknown-rules"][observation.rejections.len()];
                observation.rejections.push(serde_json::json!({ "name": name, "before": before, "after": after, "error": browser.notice.args }));
                if observation.rejections.len() == 3 {
                    snapshot(&mut observation, "rejected", &mut commands)?;
                }
                key(&mut keys, window, KeyCode::Escape);
                true
            }
            16 if !input.library_open() && settled => {
                if observation.rejections.len() < 3 {
                    observation.step = 11;
                }
                true
            }
            17 if settled => {
                select(&mut keys, window, 3);
                true
            }
            18 if input.library_open() && !browser.busy() => {
                observation.before = Some(state(&game, &audio)?);
                select(&mut keys, window, 1);
                true
            }
            19 if input.library_open() && browser.busy() => {
                key(&mut keys, window, KeyCode::Escape);
                true
            }
            20 if !input.library_open() && !browser.busy() => {
                let before = observation
                    .before
                    .take()
                    .ok_or("Missing cancellation baseline")?;
                let after = state(&game, &audio)?;
                if !same_song(&before, &after) {
                    return Err("Cancelled load changed the old song or history".into());
                }
                observation.cancel = Some(
                    serde_json::json!({ "before": before, "after": after, "worker_finished": browser.is_finished() }),
                );
                snapshot(&mut observation, "cancelled", &mut commands)?;
                true
            }
            21 if observation.captures.values().all(Option::is_some) => {
                close.write(WindowCloseRequested { window });
                true
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
        eprintln!("Native library observation failed: {error}");
        let _ = record(&observation, &game, &browser, &online, Some(&error))
            .inspect_err(|error| eprintln!("Library result write failed: {error}"));
        observation.recorded = true;
        if let Ok(window) = windows.single() {
            close.write(WindowCloseRequested { window });
        }
    }
}

fn source_probe(
    observation: &mut Observation,
    label: &'static str,
    game: &Game,
    input: &InputState,
    browser: &LibraryBrowser,
) {
    observation.source_records.push(serde_json::json!({
        "label": label, "emit_or_observe_input_relative_ns": input.origin.elapsed().as_nanos(),
        "step": observation.step, "phase": format!("{:?}", game.phase),
        "library_open": input.library_open(), "focused": input.is_focused(),
        "busy": browser.busy(), "importing": browser.importing(),
        "worker_finished": browser.is_finished(), "page": format!("{:?}", browser.page),
        "pending_destination": browser.pending_source.as_ref().map(|(_, path)| path),
        "notice_key": browser.notice.key, "notice_args": browser.notice.args,
        "content_id": game.content.content_id, "facts": game.session.replay.facts().len(),
        "events": game.session.engine.events().len(),
    }));
}

fn activate_label(
    visual: &VisualState,
    label: &str,
    keys: &mut MessageWriter<KeyboardInput>,
    window: Entity,
) -> Result<(), String> {
    let menu = visual.menu.as_ref().ok_or("Missing actual menu")?;
    let target = menu
        .rows
        .iter()
        .position(|row| row.text.lines().next() == Some(label))
        .ok_or_else(|| format!("Actual menu has no action {label:?}"))?;
    let selected = menu
        .rows
        .iter()
        .position(|row| row.selected)
        .ok_or("Missing actual focus")?;
    for _ in 0..(target + menu.rows.len() - selected) % menu.rows.len() {
        key(keys, window, KeyCode::ArrowDown);
    }
    key(keys, window, KeyCode::Enter);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn observe_source(
    observation: &mut Observation,
    game: &Game,
    input: &InputState,
    brand: &BrandIntroStatus,
    visual: &VisualState,
    audio: &AudioOutput,
    browser: &LibraryBrowser,
    online: &OnlineRound,
    commands: &mut Commands,
    keys: &mut MessageWriter<KeyboardInput>,
    focus: &mut MessageWriter<WindowFocused>,
    close: &mut MessageWriter<WindowCloseRequested>,
    window: Entity,
    settled: bool,
) -> Result<bool, String> {
    let scenario = observation.scenario.clone();
    if game.closing {
        if !browser.busy() && browser.is_finished() && online.is_finished() {
            if observation.step != 15 && scenario != "source-close" {
                return Err("Source observation closed before its completed checks".into());
            }
            source_probe(
                observation,
                "owned-workers-finished-at-close",
                game,
                input,
                browser,
            );
            record(observation, game, browser, online, None)?;
            observation.recorded = true;
        }
        return Ok(false);
    }
    let preserved = |observation: &Observation| -> Result<(), String> {
        let before = observation
            .snapshots
            .get("before_import")
            .ok_or("Missing original song baseline")?;
        if !same_song(before, &state(game, audio)?) {
            return Err("Source import changed the original song or history after selection was abandoned or rejected".into());
        }
        Ok(())
    };
    let advance = match observation.step {
        0 if brand.is_complete() && input.controls_enabled() && game.phase == Phase::Ready => {
            observation
                .snapshots
                .insert("before_import", state(game, audio)?);
            key(keys, window, KeyCode::Enter);
            true
        }
        1 if settled => {
            activate_label(visual, visual.locale.text("library.open"), keys, window)?;
            true
        }
        2 if input.library_open() && !browser.busy() && settled => {
            activate_label(visual, visual.locale.text("import.open"), keys, window)?;
            true
        }
        3 if input.library_open()
            && browser.page == LibraryMenu::Sources
            && !browser.busy()
            && settled =>
        {
            if browser.sources.len() != 1
                || browser.sources[0].source.file_name().unwrap_or_default() != "00-source.wav"
            {
                return Err(
                    "Source fixture did not discover exactly its one paired candidate".into(),
                );
            }
            snapshot(observation, "source-list", commands)?;
            if observation.captures["source-list"].is_some() {
                activate_label(visual, "00-source.wav", keys, window)?;
                true
            } else {
                false
            }
        }
        4 if browser.page == LibraryMenu::Confirm && !browser.busy() && settled => {
            let (source, destination) = browser
                .pending_source
                .as_ref()
                .ok_or("Missing actual source confirmation")?;
            observation.source_records.push(serde_json::json!({
                "label": "actual-source-confirmation", "source": source.source,
                "authoring": source.authoring, "destination": destination,
                "destination_exists_before_confirmation": destination.exists(),
            }));
            if destination.exists() {
                return Err("Confirmation target was already created".into());
            }
            snapshot(observation, "source-confirm", commands)?;
            observation.snapshots.insert("confirm", state(game, audio)?);
            observation.captures["source-confirm"].is_some()
        }
        5 if settled => {
            source_probe(observation, "emit-import-confirm", game, input, browser);
            activate_label(visual, visual.locale.text("import.confirm"), keys, window)?;
            true
        }
        6 if browser.importing() => {
            source_probe(
                observation,
                "first-owned-source-worker",
                game,
                input,
                browser,
            );
            if browser.is_finished() {
                observation
                    .source_not_run
                    .push("import worker still executing at first observation");
            }
            match scenario.as_str() {
                "source-back" | "source-background-error" => key(keys, window, KeyCode::Escape),
                "source-focus" => {
                    focus.write(WindowFocused {
                        window,
                        focused: false,
                    });
                }
                "source-close" => {
                    observation.close_requested_while_owned = true;
                    observation.worker_unfinished_at_close = !browser.is_finished();
                    if browser.is_finished() {
                        observation
                            .source_not_run
                            .push("close during unfinished source encoding");
                    }
                    close.write(WindowCloseRequested { window });
                }
                _ => {}
            }
            true
        }
        6 if !browser.busy() && settled => {
            // Never manufacture an in-flight control window if the real worker has already completed
            observation
                .source_not_run
                .push("source worker ownership / cancellation / close control window");
            if matches!(
                scenario.as_str(),
                "source-back" | "source-focus" | "source-background-error" | "source-close"
            ) {
                observation.step = 13;
            }
            true
        }
        7 if scenario == "source-complete"
            && !input.library_open()
            && !browser.busy()
            && game.phase == Phase::Ready =>
        {
            let current = state(game, audio)?;
            if current["audio_started"] != false
                || current["content_id"] == observation.snapshots["before_import"]["content_id"]
                || game.package_path.is_none()
            {
                return Err("Imported song did not reach distinct validated Ready without automatic playback".into());
            }
            observation.snapshots.insert("imported_ready", current);
            snapshot(observation, "imported-ready", commands)?;
            true
        }
        8 if scenario == "source-complete"
            && settled
            && observation.captures["imported-ready"].is_some() =>
        {
            source_probe(observation, "emit-fresh-start", game, input, browser);
            activate_label(visual, visual.locale.text("menu.start"), keys, window)?;
            true
        }
        9 if scenario == "source-complete"
            && game.phase == Phase::Running
            && audio.position().is_some_and(|position| position >= 0.5) =>
        {
            observation
                .snapshots
                .insert("imported_running", state(game, audio)?);
            source_probe(observation, "emit-two-hits-and-pause", game, input, browser);
            snapshot(observation, "imported-running", commands)?;
            key(keys, window, KeyCode::KeyF);
            key(keys, window, KeyCode::KeyJ);
            key(keys, window, KeyCode::Escape);
            true
        }
        10 if scenario == "source-complete" && game.phase == Phase::Paused && settled => {
            if game.session.diagnostics.len() != 2 {
                return Err("Imported source did not capture both actual software Hits".into());
            }
            observation
                .snapshots
                .insert("imported_paused", state(game, audio)?);
            observation.source_records.push(serde_json::json!({ "label": "actual-hit-diagnostics", "captures": game.session.diagnostics.iter().map(|capture| serde_json::json!({
                "player": format!("{:?}", capture.player), "seq": capture.seq,
                "observed_ns": capture.observed_ns, "consumed_ns": capture.consumed_ns,
                "song_frames": capture.song_frames, "uncertainty_frames": capture.uncertainty_frames,
            })).collect::<Vec<_>>() }));
            observation.step = 13;
            true
        }
        7 if matches!(
            scenario.as_str(),
            "source-bad-json" | "source-unknown-rules"
        ) && !browser.busy() =>
        {
            preserved(observation)?;
            if browser.notice.key != "library.failed" {
                return Err("Source rejection lost its actual importer / loader error".into());
            }
            observation
                .snapshots
                .insert("rejected_source", state(game, audio)?);
            source_probe(observation, "actual-source-rejection", game, input, browser);
            snapshot(observation, "source-rejected", commands)?;
            observation.step = 13;
            true
        }
        7 if matches!(
            scenario.as_str(),
            "source-back" | "source-focus" | "source-background-error"
        ) && !input.library_open() =>
        {
            preserved(observation)?;
            if scenario == "source-background-error" && browser.busy() {
                return Ok(false);
            }
            source_probe(
                observation,
                "selection-abandoned-before-reopen",
                game,
                input,
                browser,
            );
            if scenario == "source-background-error" {
                if browser.notice.key != "library.failed" {
                    return Err("Background importer error was discarded or overwritten".into());
                }
                snapshot(observation, "background-error", commands)?;
            }
            if !input.is_focused() {
                focus.write(WindowFocused {
                    window,
                    focused: true,
                });
            }
            true
        }
        8 if matches!(
            scenario.as_str(),
            "source-back" | "source-focus" | "source-background-error"
        ) && input.is_focused()
            && settled =>
        {
            if browser.importing() && !browser.is_finished() {
                if visual.menu.as_ref().is_some_and(|menu| {
                    menu.rows.iter().any(|row| {
                        row.text.lines().next() == Some(visual.locale.text("menu.start"))
                    })
                }) {
                    return Err(
                        "Ready exposed Start while the source worker was still owned".into(),
                    );
                }
                source_probe(
                    observation,
                    "emit-ready-confirm-while-import-busy",
                    game,
                    input,
                    browser,
                );
                key(keys, window, KeyCode::Enter);
            } else {
                observation
                    .source_not_run
                    .push("Ready confirmation while import worker still executing");
            }
            true
        }
        9 if matches!(
            scenario.as_str(),
            "source-back" | "source-focus" | "source-background-error"
        ) && settled =>
        {
            preserved(observation)?;
            source_probe(
                observation,
                "busy-confirm-did-not-start-song",
                game,
                input,
                browser,
            );
            key(keys, window, KeyCode::Escape);
            true
        }
        10 if matches!(
            scenario.as_str(),
            "source-back" | "source-focus" | "source-background-error"
        ) && settled =>
        {
            source_probe(observation, "emit-reopen-library", game, input, browser);
            activate_label(visual, visual.locale.text("library.open"), keys, window)?;
            true
        }
        11 if matches!(
            scenario.as_str(),
            "source-back" | "source-focus" | "source-background-error"
        ) && input.library_open() =>
        {
            if !observation
                .source_records
                .iter()
                .any(|record| record["label"] == "actual-reopened-library")
            {
                source_probe(observation, "actual-reopened-library", game, input, browser);
            }
            if browser.busy() || !settled {
                return Ok(false);
            }
            preserved(observation)?;
            source_probe(
                observation,
                "reopened-library-never-selected-abandoned-result",
                game,
                input,
                browser,
            );
            observation
                .snapshots
                .insert("after_background", state(game, audio)?);
            snapshot(observation, "background-preserved", commands)?;
            observation.step = 13;
            true
        }
        14 if observation.captures.values().all(Option::is_some) => {
            source_probe(observation, "emit-normal-close", game, input, browser);
            close.write(WindowCloseRequested { window });
            true
        }
        _ => false,
    };
    Ok(advance)
}

type AutomaticScene<'w, 's> = (
    Option<Res<'w, crate::scene::StageScene>>,
    Option<Res<'w, crate::scene::StageGround>>,
    AutomaticGround<'w, 's>,
    Option<Res<'w, Assets<Mesh>>>,
);

type AutomaticGround<'w, 's> = Query<
    'w,
    's,
    &'static Mesh3d,
    (
        With<crate::scene::SceneEntity>,
        With<bevy::camera::visibility::NoFrustumCulling>,
    ),
>;

fn automatic_sample(
    game: &Game,
    stage: Option<&crate::scene::StageScene>,
) -> Result<serde_json::Value, String> {
    let stage = stage.ok_or("Automatic structure has no actual StageScene")?;
    if stage.0.content_id() != game.content.content_id || stage.0.compiler_version() != 2 {
        return Err("Automatic scene differs from the loaded Stage2 identity".into());
    }
    let current = game.session.current.clamp(SongTime::ZERO, game.content.end);
    let sample = stage
        .0
        .sample(current)
        .ok_or("Automatic stage has no actual sample")?;
    Ok(serde_json::json!({
        "song_frame": game.session.current.frames(),
        "stage_kind": format!("{:?}", sample.kind),
        "distance_mm": sample.distance_mm,
        "half_width_mm": sample.half_width_mm,
        "lateral_mm": sample.lateral_mm,
        "elevation_mm": sample.elevation_mm,
        "slope_x_ppm": sample.slope_x_ppm,
        "slope_y_ppm": sample.slope_y_ppm,
        "section_cue_id": game.content.section_cues(game.session.current).0.map(|cue| cue.id),
    }))
}

fn automatic_geometry(
    ground: Option<&crate::scene::StageGround>,
    handles: &AutomaticGround,
    meshes: &Assets<Mesh>,
    visual: &VisualState,
) -> Result<serde_json::Value, String> {
    use std::hash::Hasher;
    if ground.is_none() {
        return Err("Automatic renderer has no actual StageGround".into());
    }
    let mut values = Vec::new();
    for handle in handles {
        let mesh = meshes
            .get(&handle.0)
            .ok_or("Automatic ground Mesh asset is missing")?;
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Err("Automatic ground has no Float32x3 positions".into());
        };
        if vertices.len() != 514 {
            return Err("Automatic NoFrustumCulling ground changed its 257-row shape".into());
        }
        let mut minimum = [f32::INFINITY; 3];
        let mut maximum = [f32::NEG_INFINITY; 3];
        let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
        for vertex in vertices {
            for axis in 0..3 {
                if !vertex[axis].is_finite() {
                    return Err("Automatic renderer contains non-finite ground positions".into());
                }
                minimum[axis] = minimum[axis].min(vertex[axis]);
                maximum[axis] = maximum[axis].max(vertex[axis]);
                fingerprint.write_u32(vertex[axis].to_bits());
            }
        }
        values.push(serde_json::json!({
            "asset_id": format!("{:?}", handle.0.id()), "vertices": vertices.len(),
            "minimum": minimum, "maximum": maximum,
            "position_binary32_fingerprint": format!("{:016x}", fingerprint.finish()),
        }));
    }
    if values.len() != 9 {
        return Err(
            "Automatic renderer did not expose exactly nine tagged ground Mesh assets".into(),
        );
    }
    Ok(serde_json::json!({
        "read_schedule": "PostUpdate after scene::animate and UI Layout",
        "renderer_song_frame": visual.song_time.frames(), "actual_mesh_count": values.len(),
        "fingerprint_basis": "std DefaultHasher of all position binary32 bits, not a cryptographic identity",
        "meshes": values,
    }))
}

#[allow(clippy::too_many_arguments)]
fn automatic_snapshot(
    observation: &mut Observation,
    name: &'static str,
    game: &Game,
    audio: &AudioOutput,
    scene: Option<&crate::scene::StageScene>,
    ground: Option<&crate::scene::StageGround>,
    handles: &AutomaticGround,
    meshes: &Assets<Mesh>,
    visual: &VisualState,
    commands: &mut Commands,
) -> Result<(), String> {
    if observation.snapshots.contains_key(name) {
        return Ok(());
    }
    observation.snapshots.insert(name, serde_json::json!({
        "observed_at_ns": observation.started.elapsed().as_nanos(),
        "state": state(game, audio)?, "stage": automatic_sample(game, scene)?,
        "geometry": automatic_geometry(ground, handles, meshes, visual)?,
        "capture_basis": "Native Screenshot asynchronous readback requested after actual scene/UI update",
    }));
    snapshot(observation, name, commands)
}

#[allow(clippy::too_many_arguments)]
fn observe_automatic(
    observation: &mut Observation,
    game: &Game,
    input: &InputState,
    brand: &BrandIntroStatus,
    visual: &VisualState,
    audio: &AudioOutput,
    browser: &LibraryBrowser,
    online: &OnlineRound,
    scene: Option<&crate::scene::StageScene>,
    ground: Option<&crate::scene::StageGround>,
    handles: &AutomaticGround,
    meshes: &Assets<Mesh>,
    commands: &mut Commands,
    keys: &mut MessageWriter<KeyboardInput>,
    close: &mut MessageWriter<WindowCloseRequested>,
    window: Entity,
) -> Result<bool, String> {
    if observation.started.elapsed().as_secs() >= 85 {
        return Err("Automatic structure exceeded its 85-second observer deadline".into());
    }
    let row = serde_json::json!({
        "frame": observation.frame, "observed_at_ns": observation.started.elapsed().as_nanos(),
        "step": observation.step, "phase": format!("{:?}", game.phase),
        "brand_complete": brand.is_complete(), "brand_seconds": brand.elapsed_seconds,
        "controls_enabled": input.controls_enabled(), "focused": input.is_focused(),
        "source_position_seconds": audio.position(), "facts": game.session.replay.facts().len(),
        "stage": automatic_sample(game, scene)?,
    });
    let rows = observation
        .automatic_frames
        .as_mut()
        .ok_or("Missing automatic frames file")?;
    serde_json::to_writer(&mut *rows, &row).map_err(|error| error.to_string())?;
    writeln!(rows).map_err(|error| error.to_string())?;
    if game.closing {
        if observation.step != 8 {
            return Err("Automatic window closed before its requested actual observations".into());
        }
        if !browser.busy() && browser.is_finished() && online.is_finished() {
            rows.sync_all().map_err(|error| error.to_string())?;
            record(observation, game, browser, online, None)?;
            observation.recorded = true;
        }
        return Ok(false);
    }
    let stage = scene.ok_or("Missing actual automatic StageScene")?;
    let long = stage
        .0
        .segments()
        .iter()
        .any(|segment| segment.kind == cocobeat_stage::SegmentKind::Curve);
    let settled = observation.step_at.elapsed().as_millis() >= 250;
    if game.phase == Phase::Finished && observation.step < 5 {
        return Err("Automatic song finished before requested cue/geometry observations".into());
    }
    let advance = match observation.step {
        0 if brand.is_complete() && input.controls_enabled() && game.phase == Phase::Ready => {
            if game.content.sections.len() != 2 || stage.0.compiler_version() != 2 {
                return Err(
                    "Automatic fixture must contain two real section cues and Stage2".into(),
                );
            }
            observation.snapshots.insert("automatic_metadata", serde_json::json!({
                "process_id": std::process::id(), "build_id": env!("COCOBEAT_BUILD_ID"),
                "package_path": game.package_path, "content_id": game.content.content_id,
                "end_frames": game.content.end.frames(), "stage_compiler_version": stage.0.compiler_version(),
                "stage_segments": stage.0.segments().iter().map(|segment| serde_json::json!({
                    "start_frame": segment.start.frames(), "end_frame": segment.end.frames(), "kind": format!("{:?}", segment.kind),
                })).collect::<Vec<_>>(),
                "section_cues": game.content.sections.iter().map(|cue| serde_json::json!({
                    "id": cue.id, "song_frame": cue.time.frames(), "label": cue.label,
                })).collect::<Vec<_>>(),
                "controls": "Fresh KeyboardInput message through unchanged production menu/capture",
            }));
            automatic_snapshot(
                observation,
                "automatic-ready",
                game,
                audio,
                scene,
                ground,
                handles,
                meshes,
                visual,
                commands,
            )?;
            true
        }
        1 if observation.step_at.elapsed().as_secs_f64() >= 1.0
            && observation.captures["automatic-ready"].is_some() =>
        {
            let before = &observation.snapshots["automatic-ready"]["state"];
            if game.phase != Phase::Ready
                || game.session.current != SongTime::ZERO
                || state(game, audio)?["audio_started"] != false
                || state(game, audio)?["facts"] != before["facts"]
                || audio.position() != before["source_position_seconds"].as_f64()
            {
                return Err(
                    "Automatic Ready started or advanced without fresh confirmation".into(),
                );
            }
            observation.snapshots.insert("automatic_ready_wait", serde_json::json!({
                "actual_wait_ns": observation.step_at.elapsed().as_nanos(), "state": state(game, audio)?,
                "menu_owner_hint": input.menu_owner_hint(visual.locale),
            }));
            let keyboard_owner = Message::with(
                "menu.owner",
                [("device", visual.locale.text("menu.keyboard").to_string())],
            )
            .render(visual.locale);
            if !input.menu_open || input.menu_owner_hint(visual.locale) != keyboard_owner {
                key(keys, window, KeyCode::Enter);
            }
            true
        }
        2 if settled && game.phase == Phase::Ready && input.menu_open && visual.menu.is_some() => {
            observation.snapshots.insert("automatic_confirm", serde_json::json!({
                "emit_input_relative_ns": input.origin.elapsed().as_nanos(), "state": state(game, audio)?,
                "menu_owner_hint": input.menu_owner_hint(visual.locale),
            }));
            activate_label(visual, visual.locale.text("menu.start"), keys, window)?;
            true
        }
        3 if game.phase == Phase::Running && audio.position().is_some_and(|value| value > 0.05) => {
            observation
                .snapshots
                .insert("automatic_running", state(game, audio)?);
            true
        }
        4 if game.phase == Phase::Running => {
            let target = if long {
                let segment = stage
                    .0
                    .segments()
                    .iter()
                    .find(|v| v.kind == cocobeat_stage::SegmentKind::Curve)
                    .ok_or("Missing Curve")?;
                segment.start.frames() + (segment.end.frames() - segment.start.frames()) / 2
            } else {
                game.content.sections[1].time.frames() + 48_000
            };
            if game.session.current.frames() < target {
                return Ok(false);
            }
            if long
                && stage
                    .0
                    .sample(game.session.current)
                    .is_none_or(|v| v.kind != cocobeat_stage::SegmentKind::Curve)
            {
                return Err("Automatic observer missed the actual Curve capture interval".into());
            }
            if game
                .content
                .section_cues(game.session.current)
                .0
                .map(|cue| cue.id)
                != Some(game.content.sections[1].id)
            {
                return Err(
                    "Automatic runtime did not select the actual second generated cue".into(),
                );
            }
            automatic_snapshot(
                observation,
                if long {
                    "automatic-curve"
                } else {
                    "automatic-second-cue"
                },
                game,
                audio,
                scene,
                ground,
                handles,
                meshes,
                visual,
                commands,
            )?;
            true
        }
        5 if long && game.phase == Phase::Running => {
            let segment = stage
                .0
                .segments()
                .iter()
                .find(|v| v.kind == cocobeat_stage::SegmentKind::Bridge)
                .ok_or("Missing Bridge")?;
            let target =
                segment.start.frames() + (segment.end.frames() - segment.start.frames()) / 2;
            if game.session.current.frames() < target {
                return Ok(false);
            }
            if stage
                .0
                .sample(game.session.current)
                .is_none_or(|v| v.kind != cocobeat_stage::SegmentKind::Bridge)
            {
                return Err("Automatic observer missed the actual Bridge capture interval".into());
            }
            automatic_snapshot(
                observation,
                "automatic-bridge",
                game,
                audio,
                scene,
                ground,
                handles,
                meshes,
                visual,
                commands,
            )?;
            true
        }
        5 if !long && game.phase == Phase::Finished => {
            automatic_snapshot(
                observation,
                "automatic-finished",
                game,
                audio,
                scene,
                ground,
                handles,
                meshes,
                visual,
                commands,
            )?;
            true
        }
        6 if game.phase == Phase::Finished => {
            if game.session.current != game.content.end {
                return Err("Automatic terminal did not reach original EOF".into());
            }
            observation
                .snapshots
                .insert("automatic_finished", state(game, audio)?);
            true
        }
        7 if observation.captures.values().all(Option::is_some) => {
            close.write(WindowCloseRequested { window });
            true
        }
        _ => false,
    };
    Ok(advance)
}
