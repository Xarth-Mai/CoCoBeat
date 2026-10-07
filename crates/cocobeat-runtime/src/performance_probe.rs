//! Opt-in monotonic native cadence observations with production rules and synthetic controls

use super::*;
use crate::clock::{ClockConfig, ClockObservation};
use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    render::renderer::{RenderAdapterInfo, RenderDevice},
    time::{TimeSystems, TimeUpdateStrategy},
    window::PrimaryWindow,
};
use serde::Serialize;
use std::{
    fs::File,
    io::{BufWriter, Write},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const DEADLINE_SECONDS: u64 = 150;
const MAX_ROWS: usize = 200_000;

struct Probe {
    directory: PathBuf,
    origin: Instant,
    boundary_ns: u64,
    rows: Vec<Row>,
    ready_at: Option<u64>,
    started: bool,
    schedule: Vec<(i64, PlayerId)>,
    sent: usize,
    requested_captures: Vec<(u64, PlayerId)>,
    finished_at: Option<u64>,
    adapter: Option<serde_json::Value>,
    done: bool,
    producer: Producer,
    last_publication: Option<u64>,
    producer_complete: bool,
    producer_error: Option<String>,
    producer_captures: Vec<Capture>,
}

#[derive(Debug)]
struct Capture {
    player: PlayerId,
    target_frame: i64,
    observed_ns: u64,
    observed_at: Instant,
    deadline_ns: u64,
    anchor_ns: u64,
    anchor_frame: i64,
}

struct Producer {
    publication: Option<SyncSender<ClockObservation>>,
    captures: Receiver<Capture>,
    worker: Option<JoinHandle<Result<(), String>>>,
}

impl Producer {
    fn new(schedule: Vec<(i64, PlayerId)>, origin: Instant) -> Result<Self, String> {
        let (publication, anchors) = mpsc::sync_channel(1);
        let (captured, captures) = mpsc::sync_channel(schedule.len() + 1);
        let worker = thread::Builder::new()
            .name("cocobeat-perf-input".into())
            .spawn(move || capture_worker(schedule, origin, anchors, captured))
            .map_err(|error| format!("Performance capture worker: {error}"))?;
        Ok(Self {
            publication: Some(publication),
            captures,
            worker: Some(worker),
        })
    }

    fn stop(&mut self) -> Result<(), String> {
        self.publication.take();
        match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| "Performance capture worker panicked".to_string())?,
            None => Ok(()),
        }
    }

    fn drain(&mut self) -> (Vec<Capture>, Option<Result<(), String>>) {
        let complete = self.worker.as_ref().is_some_and(JoinHandle::is_finished);
        let result = complete.then(|| self.stop());
        (self.captures.try_iter().collect(), result)
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            bevy::log::error!("Performance capture cleanup: {error}");
        }
    }
}

fn relative_ns(origin: Instant, at: Instant) -> Result<u64, String> {
    u64::try_from(
        at.checked_duration_since(origin)
            .ok_or("Capture Instant precedes input origin")?
            .as_nanos(),
    )
    .map_err(|_| "Capture Instant exceeds u64 nanoseconds".into())
}

fn target_deadline(
    origin: Instant,
    anchor: ClockObservation,
    target: i64,
) -> Result<(Instant, u64), String> {
    if target <= 0 {
        return Err("Performance capture target must be positive".into());
    }
    let numerator = (i128::from(target) - i128::from(anchor.song_time.frames())) * 1_000_000_000;
    let delta = numerator.div_euclid(48_000) + i128::from(numerator.rem_euclid(48_000) != 0);
    let nanos = u64::try_from(i128::from(anchor.monotonic.nanos()) + delta)
        .map_err(|_| "Performance capture deadline nanoseconds overflow".to_string())?;
    let deadline = origin
        .checked_add(Duration::from_nanos(nanos))
        .ok_or("Performance capture Instant deadline overflow")?;
    Ok((deadline, nanos))
}

fn capture_worker(
    schedule: Vec<(i64, PlayerId)>,
    origin: Instant,
    anchors: Receiver<ClockObservation>,
    captures: SyncSender<Capture>,
) -> Result<(), String> {
    let mut anchor = match anchors.recv() {
        Ok(anchor) => anchor,
        Err(_) => return Ok(()),
    };
    let max_age = ClockConfig::default().max_extrapolation_ns;
    for (target, player) in schedule {
        loop {
            match anchors.try_recv() {
                Ok(new) => {
                    if new.epoch != anchor.epoch
                        || new.monotonic <= anchor.monotonic
                        || new.song_time < anchor.song_time
                    {
                        return Err("Invalid audio capture publication".into());
                    }
                    anchor = new;
                    continue;
                }
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                Err(mpsc::TryRecvError::Empty) => {}
            }
            let now = Instant::now();
            let observed_ns = relative_ns(origin, now)?;
            let age = observed_ns
                .checked_sub(anchor.monotonic.nanos())
                .ok_or("Audio capture publication is in the future")?;
            if age >= max_age {
                return Err("Stale audio capture publication".into());
            }
            let (deadline, deadline_ns) = target_deadline(origin, anchor, target)?;
            if now >= deadline {
                captures
                    .try_send(Capture {
                        player,
                        target_frame: target,
                        observed_ns,
                        observed_at: now,
                        deadline_ns,
                        anchor_ns: anchor.monotonic.nanos(),
                        anchor_frame: anchor.song_time.frames(),
                    })
                    .map_err(|error| format!("Performance capture queue: {error}"))?;
                break;
            }
            let wait = deadline
                .duration_since(now)
                .min(Duration::from_nanos(max_age - age));
            match anchors.recv_timeout(wait) {
                Ok(new) => {
                    if new.epoch != anchor.epoch
                        || new.monotonic <= anchor.monotonic
                        || new.song_time < anchor.song_time
                    {
                        return Err("Invalid audio capture publication".into());
                    }
                    anchor = new;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct Row {
    frame: usize,
    monotonic_ns: u64,
    phase: String,
    brand_complete: bool,
    focused: bool,
    controls_enabled: bool,
    display_pending: bool,
    physical_width: u32,
    physical_height: u32,
    scale_factor: f32,
    render_width: u32,
    render_height: u32,
    window_requested_present_mode: String,
    frame_limit: String,
    vsync: bool,
    quality: QualitySettings,
    song_frame: i64,
    stage_kind: Option<String>,
    section_cue_id: Option<u64>,
    cursor_seconds: Option<f64>,
    facts: usize,
    events: usize,
    synthetic_hits: usize,
    time_real_ms: f64,
    diagnostic_frame_ms: Option<f64>,
}

pub(super) fn install_if_requested(app: &mut App) -> Result<(), String> {
    let Some(directory) = std::env::var_os("COCOBEAT_PERFORMANCE_DIR") else {
        return Ok(());
    };
    if app.world().contains_resource::<TimeUpdateStrategy>()
        && !matches!(
            app.world().resource::<TimeUpdateStrategy>(),
            TimeUpdateStrategy::Automatic
        )
    {
        return Err("Performance probe requires automatic real Time".into());
    }
    let size =
        std::env::var("COCOBEAT_PERFORMANCE_SIZE").map_err(|_| "Missing performance size")?;
    let (width, height) = size
        .split_once('x')
        .ok_or("Performance size must be WIDTHxHEIGHT")?;
    let viewport = SmokeViewport::parse(width, height, "1", "0")?;
    let quality = smoke_quality(
        &std::env::var("COCOBEAT_PERFORMANCE_QUALITY")
            .map_err(|_| "Missing performance quality")?,
    )?;
    let pacing = match std::env::var("COCOBEAT_PERFORMANCE_PACING").as_deref() {
        Ok("unlimited") => crate::settings::PacingSettings {
            frame_limit: crate::settings::FrameLimit::Unlimited,
            vsync: false,
        },
        Ok("limited60") => crate::settings::PacingSettings {
            frame_limit: crate::settings::FrameLimit::Limited(60_000),
            vsync: false,
        },
        Ok("vsync") => crate::settings::PacingSettings {
            frame_limit: crate::settings::FrameLimit::Unlimited,
            vsync: true,
        },
        _ => return Err("Performance pacing must be unlimited/limited60/vsync".into()),
    };
    let game = app.world().resource::<Game>();
    if game.content.end.frames() < 62 * 48_000 || game.content.stage.is_none() {
        return Err(
            "Performance probe requires a compiled-stage package lasting at least 62 seconds"
                .into(),
        );
    }
    if game.playback.is_some() || app.world().non_send::<OnlineRound>().enabled() {
        return Err("Performance workload requires a fresh local session, not Replay watching or networking".into());
    }
    let schedule = hit_schedule(&game.content);
    if schedule
        .iter()
        .any(|(at, _)| !(1..game.content.end.frames()).contains(at))
    {
        return Err("Performance schedule targets must all lie within the package timeline".into());
    }
    let metadata = serde_json::json!({
        "build_id": env!("COCOBEAT_BUILD_ID"), "release": !cfg!(debug_assertions),
        "process_id": std::process::id(), "time_strategy": "Automatic",
        "origin_scope": "probe installation after package decode, AudioOutput creation and base_app setup; excludes earlier process startup",
        "boundary": "First after DisplaySystems::Pace and before TimeSystems; labels observed in PostUpdate",
        "content_id": game.content.content_id, "end_frames": game.content.end.frames(),
        "stage_compiler_version": game.content.stage.as_ref().map(|stage| stage.compiler_version()),
        "requested_physical_size": viewport.size, "requested_scale": 1,
        "stage_segments": game.content.stage.as_ref().map(|stage| stage.segments().iter().map(|segment| serde_json::json!({"start_frame": segment.start.frames(), "end_frame": segment.end.frames(), "kind": format!("{:?}", segment.kind)})).collect::<Vec<_>>()),
        "section_cues": game.content.sections.iter().map(|cue| serde_json::json!({"id": cue.id, "song_frame": cue.time.frames(), "label": cue.label})).collect::<Vec<_>>(),
        "quality": quality, "pacing": pacing, "schedule": schedule.iter().map(|(frame, player)| serde_json::json!({"song_frame": frame, "player": format!("{player:?}")})).collect::<Vec<_>>(),
        "controls": "synthetic CapturedControl, independent worker actual Instant relative to input.origin",
        "capture_producer": "independent_instant_v2", "publication_max_age_ns": ClockConfig::default().max_extrapolation_ns,
        "ready_warmup_seconds": 5, "ready_sample_seconds": 10,
        "running_warmup_seconds": 10, "running_sample_seconds": 50,
        "deadline_seconds": DEADLINE_SECONDS, "audio_output": app.world().non_send::<AudioOutput>().output_info(),
        "not_measured": ["GPU elapsed", "draw calls", "VRAM", "audio underruns", "displayed or dropped monitor frames", "physical input", "speaker latency", "four-platform native graphics", "human acceptance"]
    });
    let directory = PathBuf::from(directory);
    std::fs::create_dir(&directory)
        .map_err(|error| format!("Performance output directory: {error}"))?;
    write_json(&directory.join("metadata.json"), &metadata)?;
    {
        let mut settings = app.world_mut().resource_mut::<SettingsMenu>();
        settings.values.quality = quality;
        settings.values.pacing = pacing;
        settings.values.display = DisplaySettings {
            fullscreen: false,
            window_size: viewport.size,
            fullscreen_size: viewport.size,
        };
    }
    let settings = app.world().resource::<SettingsMenu>().values.display;
    app.world_mut()
        .resource_mut::<DisplayState>()
        .request(settings);
    app.world_mut().resource_mut::<VisualState>().quality = quality;
    let world = app.world_mut();
    let mut query = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
    if let Ok(mut window) = query.single_mut(world) {
        window.resolution.set_scale_factor_override(Some(1.0));
        window
            .resolution
            .set_physical_resolution(viewport.size[0], viewport.size[1]);
    }
    let producer = Producer::new(
        schedule.clone(),
        app.world().resource::<InputState>().origin,
    )?;
    app.add_plugins(FrameTimeDiagnosticsPlugin::default())
        .insert_non_send(Probe {
            directory,
            origin: Instant::now(),
            boundary_ns: 0,
            rows: Vec::with_capacity(65_536),
            ready_at: None,
            started: false,
            schedule,
            sent: 0,
            requested_captures: Vec::new(),
            finished_at: None,
            adapter: None,
            done: false,
            producer,
            last_publication: None,
            producer_complete: false,
            producer_error: None,
            producer_captures: Vec::new(),
        })
        .add_systems(
            First,
            boundary.after(DisplaySystems::Pace).before(TimeSystems),
        )
        .add_systems(
            Update,
            drive.before(update_game).after(BrandIntroSystems::Advance),
        )
        .add_systems(PostUpdate, record);
    Ok(())
}

fn hit_schedule(content: &SongContent) -> Vec<(i64, PlayerId)> {
    let mut schedule = vec![
        (12 * 48_000, PlayerId::P1),
        (12 * 48_000 + 800, PlayerId::P2),
        (18 * 48_000, PlayerId::P1),
    ];
    for (index, anchor) in content.anchors.iter().enumerate() {
        if index % 3 == 2 {
            continue;
        }
        let at = anchor.song_time.frames() - if index % 3 == 0 { 3_360 } else { 0 };
        if at > 0 {
            schedule.extend([(at, PlayerId::P1), (at + 480, PlayerId::P2)]);
        }
    }
    schedule.sort_by_key(|entry| entry.0);
    schedule
}

fn boundary(mut probe: NonSendMut<Probe>) {
    probe.boundary_ns = probe.origin.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
}

fn drive(
    mut probe: NonSendMut<Probe>,
    game: Res<Game>,
    brand: Res<BrandIntroStatus>,
    display: Res<DisplayState>,
    mut input: ResMut<InputState>,
) {
    if probe.done {
        return;
    }
    if brand.is_complete()
        && !display.pending
        && input.controls_enabled()
        && game.phase == Phase::Ready
    {
        let now = probe.boundary_ns;
        let ready_at = *probe.ready_at.get_or_insert(now);
        if !probe.started && now.saturating_sub(ready_at) >= 15_000_000_000 {
            let monotonic_ns = input.origin.elapsed().as_nanos() as u64;
            input.queued.push(input::CapturedControl {
                control: Control::Start,
                monotonic_ns,
            });
            probe.started = true;
        }
    }
    let (captures, completion) = probe.producer.drain();
    for capture in captures {
        if relative_ns(input.origin, capture.observed_at) != Ok(capture.observed_ns) {
            probe
                .producer_error
                .get_or_insert("Capture original Instant identity changed".into());
        } else if game.phase != Phase::Running {
            probe
                .producer_error
                .get_or_insert("Capture arrived outside Running".into());
        } else {
            input.queued.push(input::CapturedControl {
                control: Control::Hit(capture.player),
                monotonic_ns: capture.observed_ns,
            });
            probe
                .requested_captures
                .push((capture.observed_ns, capture.player));
            probe.sent += 1;
        }
        probe.producer_captures.push(capture);
    }
    if let Some(result) = completion {
        match result {
            Ok(()) if probe.sent == probe.schedule.len() => probe.producer_complete = true,
            Ok(()) => {
                probe
                    .producer_error
                    .get_or_insert("Capture producer stopped with missing inputs".into());
            }
            Err(error) => {
                probe.producer_error.get_or_insert(error);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn record(
    mut probe: NonSendMut<Probe>,
    game: Res<Game>,
    audio: NonSend<AudioOutput>,
    input: Res<InputState>,
    brand: Res<BrandIntroStatus>,
    display: Res<DisplayState>,
    settings: Res<SettingsMenu>,
    time: Res<Time<Real>>,
    diagnostics: Res<DiagnosticsStore>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<&RenderTarget, With<display::GameCamera>>,
    images: Res<Assets<Image>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    device: Option<Res<RenderDevice>>,
    mut exit: MessageWriter<AppExit>,
) {
    if probe.done {
        return;
    }
    if game.phase == Phase::Running
        && let Some(anchor) = game.session.clock.last_observation()
        && probe.last_publication != Some(anchor.monotonic.nanos())
        && let Some(sender) = probe.producer.publication.as_ref()
    {
        match sender.try_send(anchor) {
            Ok(()) => probe.last_publication = Some(anchor.monotonic.nanos()),
            Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => {}
        }
    }
    if probe.adapter.is_none()
        && let Some(adapter) = adapter
    {
        probe.adapter = Some(
            serde_json::json!({"adapter": format!("{:?}", adapter.0), "enabled_device_features": device.map(|device| format!("{:?}", device.features()))}),
        );
    }
    let window = windows.single().ok();
    let render = cameras
        .single()
        .ok()
        .and_then(|target| match target {
            RenderTarget::Image(target) => images
                .get(&target.handle)
                .map(|image| [image.width(), image.height()]),
            _ => None,
        })
        .unwrap_or([0, 0]);
    let frame = probe.rows.len();
    let at = probe.boundary_ns;
    let sent = probe.sent;
    probe.rows.push(Row {
        frame,
        monotonic_ns: at,
        phase: format!("{:?}", game.phase),
        brand_complete: brand.is_complete(),
        focused: input.is_focused(),
        controls_enabled: input.controls_enabled(),
        display_pending: display.pending,
        physical_width: window.map_or(0, Window::physical_width),
        physical_height: window.map_or(0, Window::physical_height),
        scale_factor: window.map_or(0.0, Window::scale_factor),
        render_width: render[0],
        render_height: render[1],
        window_requested_present_mode: window
            .map(|window| format!("{:?}", window.present_mode))
            .unwrap_or_default(),
        frame_limit: format!("{:?}", settings.values.pacing.frame_limit),
        vsync: settings.values.pacing.vsync,
        quality: settings.values.quality,
        song_frame: game.session.current.frames(),
        stage_kind: game
            .content
            .stage
            .as_ref()
            .and_then(|stage| stage.sample(game.session.current))
            .map(|sample| format!("{:?}", sample.kind)),
        section_cue_id: game
            .content
            .section_cues(game.session.current)
            .0
            .map(|cue| cue.id),
        cursor_seconds: audio.position(),
        facts: game.session.replay.facts().len(),
        events: game.session.engine.events().len(),
        synthetic_hits: sent,
        time_real_ms: time.delta_secs_f64() * 1000.0,
        diagnostic_frame_ms: diagnostics
            .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
            .and_then(|value| value.value()),
    });
    let terminal = game.phase == Phase::Finished;
    if terminal {
        probe.finished_at.get_or_insert(at);
    }
    let mut error = if let Some(error) = &probe.producer_error {
        Some(error.clone())
    } else if game.phase == Phase::Fault {
        Some(
            game.fault_details
                .as_deref()
                .unwrap_or("Game entered Fault")
                .to_owned(),
        )
    } else if at >= DEADLINE_SECONDS * 1_000_000_000 {
        Some("Performance deadline exceeded".to_owned())
    } else if probe.rows.len() >= MAX_ROWS {
        Some("Performance row limit exceeded".to_owned())
    } else if matches!(game.phase, Phase::Paused | Phase::Pausing) {
        Some("Performance workload was interrupted by pause or focus loss".to_owned())
    } else {
        None
    };
    if error.is_some()
        || probe
            .finished_at
            .is_some_and(|finished| at.saturating_sub(finished) >= 1_000_000_000)
    {
        if let Err(worker_error) = probe.producer.stop() {
            probe.producer_error = Some(worker_error.clone());
            error.get_or_insert(worker_error);
        }
        let remaining: Vec<_> = probe.producer.captures.try_iter().collect();
        probe.producer_captures.extend(remaining);
        if !probe.producer_complete && error.is_none() {
            error = Some("Capture producer incomplete at EOF".into());
        }
        probe.done = true;
        let result = finish(&probe, &game, input.origin, error.as_deref());
        if let Err(error) = &result {
            bevy::log::error!("Performance output: {error}");
        }
        exit.write(if result.is_ok() && error.is_none() {
            AppExit::Success
        } else {
            AppExit::error()
        });
    }
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let mut file = File::create_new(path).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n")
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

fn finish(
    probe: &Probe,
    game: &Game,
    input_origin: Instant,
    error: Option<&str>,
) -> Result<(), String> {
    let mut file = BufWriter::new(
        File::create_new(probe.directory.join("frames.jsonl"))
            .map_err(|error| error.to_string())?,
    );
    for row in &probe.rows {
        serde_json::to_writer(&mut file, row).map_err(|error| error.to_string())?;
        file.write_all(b"\n").map_err(|error| error.to_string())?;
    }
    file.flush()
        .and_then(|()| file.get_ref().sync_all())
        .map_err(|error| error.to_string())?;
    game.session
        .replay
        .save(probe.directory.join("workload.replay"))
        .map_err(|error| error.to_string())?;
    write_json(
        &probe.directory.join("result.json"),
        &serde_json::json!({"status": if error.is_none() { "COMPLETE" } else { "FAILED" }, "error": error, "adapter": probe.adapter, "row_count": probe.rows.len(), "ready_at_ns": probe.ready_at, "synthetic_hits": probe.sent, "expected_hits": probe.schedule.len(), "capture_producer": {"completed": probe.producer_complete, "error": probe.producer_error, "captures": probe.producer_captures.iter().map(|capture| serde_json::json!({"player": format!("{:?}", capture.player), "target_frame": capture.target_frame, "observed_ns": capture.observed_ns, "moment_ns": relative_ns(input_origin, capture.observed_at).ok(), "deadline_ns": capture.deadline_ns, "anchor_ns": capture.anchor_ns, "anchor_frame": capture.anchor_frame})).collect::<Vec<_>>() }, "requested_captures": probe.requested_captures.iter().map(|(at, player)| serde_json::json!({"observed_ns": at, "player": format!("{player:?}")})).collect::<Vec<_>>(), "events": game.session.engine.events().iter().map(|event| format!("{event:?}")).collect::<Vec<_>>(), "capture_diagnostics": game.session.diagnostics.iter().map(|capture| serde_json::json!({"player": format!("{:?}", capture.player), "seq": capture.seq, "observed_ns": capture.observed_ns, "consumed_ns": capture.consumed_ns, "song_frames": capture.song_frames, "uncertainty_frames": capture.uncertainty_frames})).collect::<Vec<_>>(), "summary": {"hits": game.session.summary().hits, "anchors": game.session.summary().anchors, "free_sync": game.session.summary().free_sync, "anchor_sync": game.session.summary().anchor_sync}}),
    )
}

#[cfg(test)]
mod producer_tests {
    use super::*;
    use crate::clock::MonotonicTime;
    use cocobeat_schema::{SessionEpoch, SongTime};

    fn anchor(frame: i64, ns: u64) -> ClockObservation {
        ClockObservation {
            epoch: SessionEpoch(1),
            monotonic: MonotonicTime::from_nanos(ns),
            song_time: SongTime::from_frames(frame),
            uncertainty_frames: 2_400,
        }
    }

    #[test]
    fn checked_deadline_keeps_targets_and_rejects_invalid_instants() {
        let origin = Instant::now();
        let (deadline, ns) = target_deadline(origin, anchor(0, 1_000_000), 480).unwrap();
        assert_eq!(ns, 11_000_000);
        assert_eq!(target_deadline(origin, anchor(0, 0), 1).unwrap().1, 20_834);
        assert_eq!(
            target_deadline(origin, anchor(2, 1_000_000), 1).unwrap().1,
            979_167
        );
        assert_eq!(
            deadline,
            origin.checked_add(Duration::from_nanos(ns)).unwrap()
        );
        assert!(target_deadline(origin, anchor(0, 0), 0).is_err());
        assert!(target_deadline(origin, anchor(0, u64::MAX), 1).is_err());
        assert!(target_deadline(origin, anchor(i64::MIN, 0), i64::MAX).is_err());
        assert!(relative_ns(origin.checked_add(Duration::from_secs(1)).unwrap(), origin).is_err());
    }

    #[test]
    fn real_capture_occurs_without_render_draining_and_retains_its_instant() {
        let origin = Instant::now();
        let (publication, anchors) = mpsc::sync_channel(1);
        let (captured, captures) = mpsc::sync_channel(3);
        let worker = thread::spawn(move || {
            capture_worker(
                vec![(480, PlayerId::P1), (960, PlayerId::P2)],
                origin,
                anchors,
                captured,
            )
        });
        publication.send(anchor(0, 0)).unwrap();
        thread::sleep(Duration::from_millis(50));
        worker.join().unwrap().unwrap();
        let consumed_ns = relative_ns(origin, Instant::now()).unwrap();
        let receipts: Vec<_> = captures.try_iter().collect();
        assert_eq!(receipts.len(), 2);
        assert_eq!(
            (receipts[0].target_frame, receipts[1].target_frame),
            (480, 960)
        );
        assert_eq!(
            (receipts[0].player, receipts[1].player),
            (PlayerId::P1, PlayerId::P2)
        );
        for receipt in &receipts {
            let moment_ns = relative_ns(origin, receipt.observed_at).unwrap();
            assert_eq!(moment_ns, receipt.observed_ns);
            assert_eq!(receipt.anchor_frame, 0);
            println!(
                "target={} deadline_ns={} captured_ns={} original_moment_ns={} consumed_ns={}",
                receipt.target_frame,
                receipt.deadline_ns,
                receipt.observed_ns,
                moment_ns,
                consumed_ns
            );
            assert!(receipt.observed_ns >= receipt.deadline_ns);
            assert!(receipt.observed_ns < consumed_ns);
            assert!(
                receipt.observed_ns - receipt.anchor_ns
                    < ClockConfig::default().max_extrapolation_ns
            );
        }
        assert!(receipts[0].observed_ns < receipts[1].observed_ns);
    }

    #[test]
    fn stale_publication_and_future_publication_stop_without_fake_captures() {
        for (origin, publication_at, expected) in [
            (
                Instant::now()
                    .checked_sub(Duration::from_millis(300))
                    .unwrap(),
                0,
                "Stale",
            ),
            (Instant::now(), 1_000_000_000, "future"),
        ] {
            let (publication, anchors) = mpsc::sync_channel(1);
            let (captured, captures) = mpsc::sync_channel(2);
            publication.send(anchor(0, publication_at)).unwrap();
            let error = capture_worker(vec![(48_000, PlayerId::P1)], origin, anchors, captured)
                .unwrap_err();
            assert!(error.contains(expected));
            assert!(captures.try_recv().is_err());
        }
    }

    #[test]
    fn closing_sender_cancels_a_future_capture_and_join() {
        let origin = Instant::now();
        let (publication, anchors) = mpsc::sync_channel(1);
        let (captured, captures) = mpsc::sync_channel(2);
        let worker = thread::spawn(move || {
            capture_worker(vec![(60 * 48_000, PlayerId::P1)], origin, anchors, captured)
        });
        publication.send(anchor(0, 0)).unwrap();
        thread::sleep(Duration::from_millis(5));
        drop(publication);
        worker.join().unwrap().unwrap();
        assert!(captures.try_recv().is_err());
    }
}
