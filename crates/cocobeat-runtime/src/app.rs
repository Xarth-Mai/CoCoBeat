use crate::{
    audio::AudioOutput,
    brand_intro::{
        self, BrandImpact, BrandIntroControl, BrandIntroPhase, BrandIntroStatus, BrandIntroSystems,
    },
    clock::MonotonicTime,
    dev_song,
    display::{self, DisplayState, DisplaySystems, PresentationCamera},
    i18n::{Locale, Message},
    input::{self, Control, InputState, SettingsAction},
    session::{CONTENT_ID, RULES_ID, Session},
    settings::{DisplaySettings, QualityPreset, QualitySettings, Settings},
    settings_menu::SettingsMenu,
    ui_assets,
    view::{self, SettingsScroll, VisualState},
};
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    camera::{ImageRenderTarget, RenderTarget},
    prelude::*,
    render::{
        render_resource::{TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::{ExitCondition, WindowCloseRequested},
    winit::WinitPlugin,
};
use cocobeat_replay::Replay;
use cocobeat_schema::{DuoEvent, DuoRules, SessionEpoch};
use kira::sound::PlaybackState;
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Ready,
    Starting,
    Running,
    Pausing,
    Paused,
    Finished,
    Fault,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Smoke {
    Scene,
    Startup,
    Settings,
    Locale(Locale),
    Languages(Locale),
    Menu(Locale),
    Quality(QualitySettings),
    Graphics(Locale),
    Pacing(Locale),
}

#[derive(Clone, Copy, Debug)]
struct SmokeViewport {
    size: [u32; 2],
    scale: f32,
    selection: Option<usize>,
}

impl Default for SmokeViewport {
    fn default() -> Self {
        Self {
            size: [1280, 800],
            scale: 1.0,
            selection: None,
        }
    }
}

impl SmokeViewport {
    fn parse(width: &str, height: &str, scale: &str, selection: &str) -> Result<Self, String> {
        let size = [width, height].map(|axis| axis.parse::<u32>());
        let [Ok(width), Ok(height)] = size else {
            return Err("Smoke dimensions must be integer physical pixels".into());
        };
        if !(1..=8192).contains(&width)
            || !(1..=8192).contains(&height)
            || u64::from(width) * u64::from(height) > 16_777_216
        {
            return Err("Smoke dimensions must be 1..8192 pixels and at most 16 megapixels".into());
        }
        let scale = scale
            .parse::<f32>()
            .map_err(|_| "Invalid smoke scale factor")?;
        if !scale.is_finite() || !(0.25..=8.0).contains(&scale) {
            return Err("Smoke scale factor must be finite and within 0.25..8".into());
        }
        let selection = selection
            .parse::<usize>()
            .map_err(|_| "Invalid smoke row index")?;
        Ok(Self {
            size: [width, height],
            scale,
            selection: Some(selection),
        })
    }
}

#[derive(Resource)]
struct Game {
    session: Session,
    phase: Phase,
    notice: Message,
    saved_facts: usize,
    transition_started: std::time::Instant,
}

impl Game {
    fn new() -> Result<Self, String> {
        Ok(Self {
            session: Session::new(SessionEpoch(0))?,
            phase: Phase::Ready,
            notice: Message::new("game.welcome"),
            saved_facts: 0,
            transition_started: std::time::Instant::now(),
        })
    }

    fn save(&mut self) -> Result<(), String> {
        if self.session.replay.facts().is_empty() {
            self.notice = Message::new("game.nothing_to_save");
        } else if self.saved_facts != self.session.replay.facts().len() {
            let path = self.session.save(Path::new("replays"))?;
            self.saved_facts = self.session.replay.facts().len();
            self.notice = Message::with("game.saved", [("path", path.display().to_string())]);
        }
        Ok(())
    }

    fn start(&mut self, audio: &mut AudioOutput) -> Result<(), String> {
        self.save()?;
        let epoch = self
            .session
            .epoch()
            .0
            .checked_add(1)
            .ok_or("Session epoch overflow")?;
        let session = Session::new(SessionEpoch(epoch))?;
        audio.start()?;
        self.session = session;
        self.saved_facts = 0;
        self.phase = Phase::Starting;
        self.transition_started = std::time::Instant::now();
        self.notice = Message::new("game.waiting_audio");
        Ok(())
    }

    fn main_menu(&mut self) -> Result<(), String> {
        self.save()?;
        // Keep the epoch until the next explicit Start advances it
        self.session = Session::new(self.session.epoch())?;
        self.saved_facts = 0;
        self.phase = Phase::Ready;
        self.notice = Message::new("game.ready");
        Ok(())
    }

    fn fault(&mut self, audio: &mut AudioOutput, error: String, message: &'static str) {
        audio.stop();
        self.phase = Phase::Fault;
        let saved = self.save();
        eprintln!("Session stopped: {error}");
        self.notice = Message::new(message);
        if let Err(save_error) = saved {
            eprintln!("Replay save failed: {save_error}");
            self.notice = Message::new("game.replay_failed");
        }
    }

    fn observe_playback(
        &mut self,
        position: f64,
        state: PlaybackState,
        observed: MonotonicTime,
    ) -> Result<bool, String> {
        if self.phase == Phase::Pausing && state == PlaybackState::Paused {
            self.session.observe_audio(position, observed)?;
            self.session.update_position(observed)?;
            self.session
                .clock
                .pause(observed)
                .map_err(|error| format!("Pause clock: {error:?}"))?;
            self.phase = Phase::Paused;
        } else if matches!(self.phase, Phase::Starting | Phase::Running)
            && state == PlaybackState::Playing
            // A newly queued Kira handle already says Playing before its first callback
            && (self.phase == Phase::Running || position > 0.0)
        {
            self.session.observe_audio(position, observed)?;
            self.session.update_position(observed)?;
            if self.phase == Phase::Starting {
                self.phase = Phase::Running;
                self.notice = Message::new("game.listening");
                return Ok(true);
            }
        }
        Ok(false)
    }
}

pub fn run() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [] => run_game(),
        [flag] if flag == "--help" || flag == "-h" => {
            println!(
                "CoCoBeat: 64-second local duet\n  --replay FILE          validate a saved development-song replay\n  --visual-smoke PNG     render a deterministic scene without audio or gameplay\n  --startup-smoke PNG    render the native intro and Ready eye loop without audio\n  --settings-smoke PNG   render settings with a simulated low-resolution fullscreen scene\n  --locale-smoke CODE PNG    render localized settings on a simulated surface\n  --language-smoke CODE PNG  render the native-name language selector\n  --menu-smoke CODE PNG      render the localized Ready menu without audio\n  --quality-smoke PRESET PNG  render low/medium/high/off graphics on a simulated surface\n  --settings-page-smoke PAGE CODE PNG  render graphics/pacing settings\n  --viewport-smoke PAGE CODE WIDTH HEIGHT SCALE ROW PNG  render main/graphics/pacing/languages/ready at physical pixels and DPI; ROW starts at 0\nEnter / controller South: menu confirmation; Esc / Start: pause\nReplays are saved locally in ./replays/"
            );
            Ok(())
        }
        [flag, path] if flag == "--replay" => validate_replay(path),
        [flag, path] if flag == "--visual-smoke" => visual_smoke(PathBuf::from(path), Smoke::Scene),
        [flag, path] if flag == "--startup-smoke" => {
            visual_smoke(PathBuf::from(path), Smoke::Startup)
        }
        [flag, path] if flag == "--settings-smoke" => {
            visual_smoke(PathBuf::from(path), Smoke::Settings)
        }
        [flag, code, path]
            if flag == "--locale-smoke" || flag == "--language-smoke" || flag == "--menu-smoke" =>
        {
            if let Some(locale) = Locale::ALL.into_iter().find(|locale| locale.code() == code) {
                visual_smoke(
                    PathBuf::from(path),
                    match flag.as_str() {
                        "--locale-smoke" => Smoke::Locale(locale),
                        "--language-smoke" => Smoke::Languages(locale),
                        _ => Smoke::Menu(locale),
                    },
                )
            } else {
                Err(format!("Unsupported locale: {code}"))
            }
        }
        [flag, preset, path] if flag == "--quality-smoke" => {
            let mut quality = QualitySettings::default();
            match preset.as_str() {
                "low" => quality.set_preset(QualityPreset::Low),
                "medium" => quality.set_preset(QualityPreset::Medium),
                "high" => quality.set_preset(QualityPreset::High),
                "off" => {
                    quality.set_preset(QualityPreset::Low);
                    quality.preset = QualityPreset::Custom;
                    quality.rain = crate::settings::RainAmount::Off;
                    quality.fog = false;
                }
                _ => {
                    return {
                        eprintln!("Unsupported graphics preset: {preset}");
                        ExitCode::FAILURE
                    };
                }
            }
            visual_smoke(PathBuf::from(path), Smoke::Quality(quality))
        }
        [flag, page, code, path] if flag == "--settings-page-smoke" => {
            match (
                page.as_str(),
                Locale::ALL.into_iter().find(|locale| locale.code() == code),
            ) {
                ("graphics", Some(locale)) => {
                    visual_smoke(PathBuf::from(path), Smoke::Graphics(locale))
                }
                ("pacing", Some(locale)) => {
                    visual_smoke(PathBuf::from(path), Smoke::Pacing(locale))
                }
                _ => Err(format!(
                    "Unsupported settings page or locale: {page} / {code}"
                )),
            }
        }
        [flag, page, code, width, height, scale, selection, path] if flag == "--viewport-smoke" => {
            (|| {
                let locale = Locale::ALL
                    .into_iter()
                    .find(|locale| locale.code() == code)
                    .ok_or_else(|| format!("Unsupported locale: {code}"))?;
                let mode = match page.as_str() {
                    "main" => Smoke::Locale(locale),
                    "graphics" => Smoke::Graphics(locale),
                    "pacing" => Smoke::Pacing(locale),
                    "languages" => Smoke::Languages(locale),
                    "ready" => Smoke::Menu(locale),
                    _ => return Err(format!("Unsupported settings page: {page}")),
                };
                let viewport = SmokeViewport::parse(width, height, scale, selection)?;
                if matches!(mode, Smoke::Menu(_)) && viewport.selection != Some(0) {
                    return Err("Ready preview has no settings rows; use row 0".into());
                }
                visual_smoke_at(PathBuf::from(path), mode, viewport)
            })()
        }
        _ => Err("Unknown arguments; run cocobeat-game --help".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn base_app() -> Result<App, String> {
    let settings = SettingsMenu::load();
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        close_when_requested: false,
        primary_window: Some(Window {
            title: "CoCoBeat — One Beat".into(),
            name: Some("cocobeat".into()),
            resolution: (1280, 800).into(),
            ..default()
        }),
        ..default()
    }));
    ui_assets::install(&mut app)?;
    view::install(&mut app);
    {
        let mut visual = app.world_mut().resource_mut::<VisualState>();
        visual.locale = settings.values.locale;
        visual.quality = settings.values.quality;
    }
    display::install(&mut app, settings.values.display);
    app.insert_resource(settings);
    Ok(app)
}

fn run_game() -> Result<(), String> {
    let audio = AudioOutput::new()?;
    let mut app = base_app()?;
    let pacing = app.world().resource::<SettingsMenu>().values.pacing;
    display::install_frame_pacing(&mut app, pacing);
    input::install(&mut app);
    brand_intro::install(&mut app);
    app.world_mut()
        .resource_mut::<InputState>()
        .set_controls_enabled(false);
    install_window_icon(&mut app)?;
    app.insert_non_send(audio)
        .insert_resource(Game::new()?)
        .add_systems(Update, suspend_intro.before(BrandIntroSystems::Advance))
        .add_systems(
            Update,
            update_game
                .after(BrandIntroSystems::Advance)
                .after(DisplaySystems::Sync),
        )
        .add_systems(Update, reconcile_audio.after(update_game));
    match app.run() {
        AppExit::Success => Ok(()),
        error => Err(format!("Application exited: {error:?}")),
    }
}

fn install_window_icon(app: &mut App) -> Result<(), String> {
    use bevy::{ecs::system::NonSendMarker, image::*};
    let image = Image::from_buffer(
        include_bytes!("../../../assets/brand/icons/symbol-256.png"),
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        default(),
    )
    .map_err(|error| format!("Window icon: {error}"))?
    .try_into_dynamic()
    .map_err(|error| format!("Window icon pixels: {error}"))?
    .to_rgba8();
    let icon = winit::window::Icon::from_rgba(image.to_vec(), image.width(), image.height())
        .map_err(|error| format!("Window icon: {error}"))?;
    app.add_systems(
        Update,
        move |mut created: MessageReader<bevy::window::WindowCreated>, _main: NonSendMarker| {
            for event in created.read() {
                bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
                    if let Some(window) = windows.get_window(event.window) {
                        window.set_window_icon(Some(icon.clone()));
                    }
                });
            }
        },
    );
    Ok(())
}

fn suspend_intro(
    game: Res<Game>,
    input: Res<InputState>,
    mut control: ResMut<BrandIntroControl>,
    audio: Option<NonSendMut<AudioOutput>>,
) {
    control.idle_enabled = game.phase == Phase::Ready && input.menu_open;
    let suspended = !input.is_focused();
    if control.suspended != suspended {
        control.suspended = suspended;
        // The explicit visual preview shares this control path without an audio backend
        if let Some(mut audio) = audio {
            if suspended {
                audio.pause_brand();
            } else {
                audio.resume_brand();
            }
        }
    }
}

fn reconcile_audio(mut audio: NonSendMut<AudioOutput>) {
    audio.reconcile_playback();
}

fn close_game(game: &mut Game, audio: &mut AudioOutput, exit: &mut MessageWriter<AppExit>) {
    let saved = game.save();
    audio.stop();
    match saved {
        Ok(()) => {
            exit.write(AppExit::Success);
        }
        Err(error) => {
            eprintln!("Replay save failed during exit: {error}");
            exit.write(AppExit::Error(std::num::NonZeroU8::new(1).unwrap()));
        }
    }
}

fn validate_replay(path: &str) -> Result<(), String> {
    let replay = Replay::load(path).map_err(|error| error.to_string())?;
    let engine = replay
        .replay(
            CONTENT_ID,
            RULES_ID,
            dev_song::anchors(),
            DuoRules::default(),
        )
        .map_err(|error| error.to_string())?;
    println!(
        "Replay OK: {} facts, {} rule events, epoch {}",
        replay.facts().len(),
        engine.events().len(),
        replay.epoch().0
    );
    Ok(())
}

fn feedback(
    events: Vec<DuoEvent>,
    audio: &mut AudioOutput,
    visual: &mut VisualState,
) -> Result<(), String> {
    for event in events {
        if matches!(event, DuoEvent::FreeSync(_) | DuoEvent::AnchorSync(_)) {
            visual.sync_pulse = 1.0;
            audio.sync()?;
        }
    }
    Ok(())
}

fn update_game(
    mut game: ResMut<Game>,
    mut input: ResMut<InputState>,
    mut audio: NonSendMut<AudioOutput>,
    mut visual: ResMut<VisualState>,
    (time, mut settings, mut display, mut settings_scroll): (
        Res<Time>,
        ResMut<SettingsMenu>,
        ResMut<DisplayState>,
        ResMut<SettingsScroll>,
    ),
    (mut exit, mut close_requests): (MessageWriter<AppExit>, MessageReader<WindowCloseRequested>),
    (mut brand, mut impacts): (ResMut<BrandIntroStatus>, MessageReader<BrandImpact>),
) {
    let at = || {
        MonotonicTime::from_nanos(
            u64::try_from(input.origin.elapsed().as_nanos()).unwrap_or(u64::MAX),
        )
    };
    let observed = at();
    if close_requests.read().next().is_some() {
        close_game(&mut game, &mut audio, &mut exit);
        input.queued.clear();
        return;
    }
    let settings_now = input.origin.elapsed().as_secs_f64();
    settings.sync_pacing(&display);
    if settings.tick(settings_now, &mut display) {
        settings_scroll.reset();
    }
    visual.quality = settings.values.quality;
    visual.locale = settings.values.locale;
    if !input.controls_enabled() {
        let error = if brand.phase == BrandIntroPhase::Failed {
            impacts.clear();
            None
        } else {
            audio.take_error().or_else(|| {
                impacts
                    .read()
                    .find_map(|&impact| audio.play_brand(impact).err())
            })
        };
        if let Some(error) = error {
            brand.phase = BrandIntroPhase::Failed;
            brand.error = Some(error);
            brand.reveal_progress = 0.0;
        }
        input.queued.clear();
        if brand.phase == BrandIntroPhase::Failed {
            if game.phase != Phase::Fault {
                audio.stop();
                game.phase = Phase::Fault;
                game.notice = Message::new("game.startup_failed");
                eprintln!(
                    "Startup failed: {}",
                    brand.error.as_deref().unwrap_or("unknown error")
                );
            }
            brand.reveal_progress = 0.0;
            visual.status = format!(
                "{}\n{}",
                game.notice.render(visual.locale),
                visual.locale.text("game.close_window")
            );
        } else if brand.is_complete() {
            input.set_controls_enabled(true);
            input.set_menu_open(true);
            game.phase = Phase::Ready;
            visual.status = visual.locale.text("game.ready").into();
        }
        // Completion only unlocks the next fresh press, never a queued startup control
        return;
    }
    let delta = time.delta_secs().min(1.0);
    for pulse in &mut visual.hit_pulses {
        *pulse = (*pulse - delta * 4.0).max(0.0);
    }
    visual.sync_pulse = (visual.sync_pulse - delta * 1.8).max(0.0);

    let mut fault_message = "game.stopped";
    let result = (|| -> Result<(), String> {
        if let Some(error) = audio.take_error() {
            let _ = game.session.clock.device_lost(observed);
            fault_message = "game.audio_failed";
            return Err(error);
        }
        if matches!(
            game.phase,
            Phase::Starting | Phase::Running | Phase::Pausing
        ) && let (Some(position), Some(state)) = (audio.position(), audio.state())
            && game
                .observe_playback(position, state, observed)
                .inspect_err(|_| {
                    fault_message = "game.clock_failed";
                })?
        {
            input.set_menu_open(false);
        }
        if matches!(game.phase, Phase::Starting | Phase::Pausing)
            && audio.state() != Some(PlaybackState::Stopped)
            && game.transition_started.elapsed() > std::time::Duration::from_secs(2)
        {
            fault_message = "game.audio_failed";
            return Err(
                "Audio callback did not acknowledge the state transition within 2 seconds".into(),
            );
        }

        for event in std::mem::take(&mut input.queued) {
            if let Control::Settings(action) = event.control {
                if action == SettingsAction::Open {
                    settings_scroll.reset();
                    if input.menu_open && !matches!(game.phase, Phase::Running | Phase::Starting) {
                        settings.begin(&display);
                        input.set_settings_open(true);
                    } else {
                        input.set_settings_open(false);
                    }
                } else if !settings_scroll.handle(action) {
                    if settings.handle(action, settings_now, &mut display) {
                        input.set_settings_open(false);
                    }
                    settings_scroll.reset();
                }
                // A second device cannot confirm a new preview in this capture batch
                break;
            }
            if settings.is_open() && event.control != Control::FocusLost {
                continue;
            }
            match event.control {
                Control::Hit(player) if game.phase == Phase::Running => {
                    let consumed_ns =
                        u64::try_from(input.origin.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    let events = game.session.hit(player, event.monotonic_ns, consumed_ns)?;
                    audio.hit(player)?;
                    visual.hit_pulses[player.index()] = 1.0;
                    feedback(events, &mut audio, &mut visual)?;
                }
                Control::Start | Control::TogglePause if game.phase == Phase::Paused => {
                    game.session
                        .clock
                        .resume(observed)
                        .map_err(|error| format!("Resume clock: {error:?}"))?;
                    audio.resume();
                    game.phase = Phase::Starting;
                    game.transition_started = std::time::Instant::now();
                }
                Control::Start
                    if matches!(game.phase, Phase::Ready | Phase::Finished | Phase::Fault) =>
                {
                    game.start(&mut audio)?;
                }
                Control::Restart => {
                    game.start(&mut audio)?;
                    input.set_menu_open(true);
                }
                Control::MainMenu => {
                    game.main_menu()?;
                    audio.stop();
                    input.open_main_menu();
                    visual.hit_pulses = [0.0; 2];
                    visual.sync_pulse = 0.0;
                    // Return to Ready without consuming a second confirmation from this batch
                    break;
                }
                Control::TogglePause | Control::FocusLost
                    if matches!(game.phase, Phase::Running | Phase::Starting) =>
                {
                    audio.pause();
                    game.session
                        .clock
                        .invalidate_calibration(observed)
                        .map_err(|error| format!("Pause transition: {error:?}"))?;
                    game.phase = Phase::Pausing;
                    game.transition_started = std::time::Instant::now();
                    input.set_menu_open(true);
                }
                Control::SaveReplay => game.save().inspect_err(|_| {
                    fault_message = "game.replay_failed";
                })?,
                Control::Quit => {
                    close_game(&mut game, &mut audio, &mut exit);
                    return Ok(());
                }
                _ => {}
            }
        }
        if matches!(
            game.phase,
            Phase::Starting | Phase::Running | Phase::Pausing | Phase::Paused
        ) {
            if audio.state() == Some(PlaybackState::Stopped) {
                feedback(game.session.finish()?, &mut audio, &mut visual)?;
                game.phase = Phase::Finished;
                input.set_menu_open(true);
                game.save()?;
            } else if game.phase == Phase::Running {
                feedback(game.session.advance()?, &mut audio, &mut visual)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = game.session.clock.invalidate_calibration(observed);
        game.fault(&mut audio, error, fault_message);
        input.set_menu_open(true);
    }
    visual.song_seconds = game.session.current.as_seconds_f64();
    visual.resonance = f32::from(game.session.engine.resonance().level_per_mille) / 1_000.0;
    visual.running = game.phase == Phase::Running;
    visual.quality = settings.values.quality;
    let locale = settings.values.locale;
    visual.locale = locale;
    visual.settings = settings.presentation(settings_now, &display, locale);
    if settings.is_open() {
        return;
    }
    visual.status = game_status(&game, &input, &settings);
}

fn game_status(game: &Game, input: &InputState, settings: &SettingsMenu) -> String {
    let locale = settings.values.locale;
    let song_seconds = game.session.current.as_seconds_f64();
    let next = dev_song::ANCHOR_FRAMES
        .iter()
        .find(|&&frame| frame > game.session.current.frames())
        .map(|&frame| {
            Message::with(
                "hud.next_anchor",
                [(
                    "seconds",
                    format!("{:.1}", frame as f64 / 48_000.0 - song_seconds),
                )],
            )
            .render(locale)
        })
        .unwrap_or_else(|| locale.text("hud.final_release").into());
    format!(
        "{} | {next}\n{}\n{}\n{}\n{}\n{}\n{}",
        locale.text(match game.phase {
            Phase::Ready => "phase.ready",
            Phase::Starting => "phase.starting",
            Phase::Running => "phase.running",
            Phase::Pausing => "phase.pausing",
            Phase::Paused => "phase.paused",
            Phase::Finished => "phase.finished",
            Phase::Fault => "phase.fault",
        }),
        input.menu_text(locale),
        input.bindings_text(locale),
        input.status.render(locale),
        game.notice.render(locale),
        settings.notice.render(locale),
        Message::with(
            "hud.timing",
            [(
                "milliseconds",
                format!("{:.1}", game.session.uncertainty_frames as f64 / 48.0)
            )]
        )
        .render(locale)
    )
}

fn visual_smoke(path: PathBuf, mode: Smoke) -> Result<(), String> {
    visual_smoke_at(path, mode, SmokeViewport::default())
}

fn smoke_layout_metrics(
    viewport: SmokeViewport,
    camera: &Camera,
    (panel, panel_transform): (&ComputedNode, &UiGlobalTransform),
    selected: Option<(usize, &ComputedNode, &UiGlobalTransform)>,
) -> Result<serde_json::Value, String> {
    let expected = Vec2::new(viewport.size[0] as f32, viewport.size[1] as f32) / viewport.scale;
    let logical = camera
        .logical_viewport_size()
        .ok_or("Missing logical viewport")?;
    if !logical.is_finite() || logical.distance(expected) > 0.01 {
        return Err(format!(
            "Wrong logical viewport: {logical:?}; expected {expected:?}"
        ));
    }
    let panel_min = panel_transform.translation - panel.size * 0.5
        + panel.padding.min_inset
        + panel.border.min_inset;
    let panel_max = panel_transform.translation + panel.size * 0.5
        - panel.padding.max_inset
        - panel.border.max_inset;
    let mut metrics = serde_json::json!({
        "physical_size": viewport.size,
        "scale_factor": viewport.scale,
        "logical_size": [logical.x, logical.y],
        "panel_physical": [panel_min.x, panel_min.y, panel_max.x, panel_max.y],
        "panel_inside_viewport": panel_min.cmpge(Vec2::ZERO).all()
            && panel_max.cmple(Vec2::new(viewport.size[0] as f32, viewport.size[1] as f32)).all(),
        "selected_row": null,
    });
    let Some((selected, row, row_transform)) = selected else {
        return Ok(metrics);
    };
    let row_min = row_transform.translation - row.size * 0.5;
    let row_max = row_transform.translation + row.size * 0.5;
    let oversized = row.size.y > panel_max.y - panel_min.y;
    if !panel_min.is_finite()
        || !panel_max.is_finite()
        || !row_min.is_finite()
        || !row_max.is_finite()
        || panel_max.cmple(panel_min).any()
        || panel_min.cmplt(Vec2::splat(-1.0)).any()
        || panel_max
            .cmpgt(Vec2::new(viewport.size[0] as f32, viewport.size[1] as f32) + Vec2::ONE)
            .any()
        || row.size.cmple(Vec2::ZERO).any()
        || row_min.x < panel_min.x - 1.0
        || row_max.x > panel_max.x + 1.0
        || row_max.y <= panel_min.y
        || row_min.y >= panel_max.y
        || (!oversized && (row_min.y < panel_min.y - 1.0 || row_max.y > panel_max.y + 1.0))
    {
        return Err(format!(
            "Selected settings row {selected} is not accessible: panel={panel_min:?}..{panel_max:?}, row={row_min:?}..{row_max:?}"
        ));
    }
    metrics["selected_row"] = serde_json::json!(selected);
    metrics["row_physical"] = serde_json::json!([row_min.x, row_min.y, row_max.x, row_max.y]);
    metrics["oversized_row"] = serde_json::json!(oversized);
    Ok(metrics)
}

fn visual_smoke_at(path: PathBuf, mode: Smoke, viewport: SmokeViewport) -> Result<(), String> {
    let startup = !matches!(mode, Smoke::Scene | Smoke::Quality(_));
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::render::RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(
        std::time::Duration::from_secs_f64(1.0 / 60.0),
    ))
    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(
            if matches!(
                mode,
                Smoke::Locale(_)
                    | Smoke::Languages(_)
                    | Smoke::Menu(_)
                    | Smoke::Graphics(_)
                    | Smoke::Pacing(_)
            ) {
                0.1
            } else {
                1.0 / 60.0
            },
        ),
    ));
    ui_assets::install(&mut app)?;
    view::install(&mut app);
    display::install(&mut app, default());
    app.world_mut()
        .resource_mut::<DisplayState>()
        .set_headless_surface(viewport.size);
    if matches!(
        mode,
        Smoke::Settings
            | Smoke::Locale(_)
            | Smoke::Languages(_)
            | Smoke::Graphics(_)
            | Smoke::Pacing(_)
    ) {
        let selected = DisplaySettings {
            fullscreen: true,
            fullscreen_size: [640, 480],
            ..default()
        };
        app.world_mut()
            .resource_mut::<DisplayState>()
            .request(selected);
        let mut menu = SettingsMenu::default();
        menu.values = Settings {
            display: selected,
            locale: match mode {
                Smoke::Locale(locale)
                | Smoke::Languages(locale)
                | Smoke::Graphics(locale)
                | Smoke::Pacing(locale) => locale,
                _ => Locale::EnUs,
            },
            ..default()
        };
        menu.sync_pacing(app.world().resource::<DisplayState>());
        menu.begin(app.world().resource::<DisplayState>());
        if matches!(
            mode,
            Smoke::Languages(_) | Smoke::Graphics(_) | Smoke::Pacing(_)
        ) {
            let mut display = app.world_mut().resource_mut::<DisplayState>();
            // Enter each real settings page through the production menu controls
            let row = match mode {
                Smoke::Graphics(_) => 5,
                Smoke::Pacing(_) => 6,
                _ => 7,
            };
            for _ in 0..row {
                menu.handle(SettingsAction::Down, 0.0, &mut display);
            }
            menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        }
        if let Some(selection) = viewport.selection {
            let mut display = app.world_mut().resource_mut::<DisplayState>();
            let presentation = menu
                .presentation(0.0, &display, menu.values.locale)
                .unwrap();
            let count = presentation.rows.len();
            if selection >= count {
                return Err(format!("Smoke row index {selection} is outside 0..{count}"));
            }
            let current = presentation
                .rows
                .iter()
                .position(|row| row.selected)
                .unwrap();
            for _ in 0..(selection + count - current) % count {
                menu.handle(SettingsAction::Down, 0.0, &mut display);
            }
        }
        app.insert_resource(menu).add_systems(
            Update,
            (|settings: Res<SettingsMenu>,
              display: Res<DisplayState>,
              mut visual: ResMut<VisualState>| {
                visual.locale = settings.values.locale;
                visual.settings = settings.presentation(0.0, &display, visual.locale);
            })
            .after(DisplaySystems::Sync),
        );
    }
    if let Smoke::Menu(locale) = mode {
        let mut settings = SettingsMenu::default();
        settings.values.locale = locale;
        app.insert_resource(settings).add_systems(
            Update,
            |game: Res<Game>,
             input: Res<InputState>,
             settings: Res<SettingsMenu>,
             mut visual: ResMut<VisualState>| {
                visual.locale = settings.values.locale;
                visual.status = game_status(&game, &input, &settings);
            },
        );
    }
    if startup {
        brand_intro::install(&mut app);
        app.init_resource::<InputState>()
            .insert_resource(Game::new()?)
            .add_systems(Update, suspend_intro.before(BrandIntroSystems::Advance));
    }
    let mut image = Image::new_target_texture(
        viewport.size[0],
        viewport.size[1],
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera_target = target.clone();
    app.add_systems(
        PostStartup,
        (move |mut commands: Commands, cameras: Query<Entity, With<PresentationCamera>>| {
            for camera in &cameras {
                commands
                    .entity(camera)
                    .insert(RenderTarget::Image(ImageRenderTarget {
                        handle: camera_target.clone(),
                        scale_factor: viewport.scale,
                    }));
            }
        })
        .after(DisplaySystems::Setup)
        .before(bevy::camera::CameraUpdateSystems),
    );
    *app.world_mut().resource_mut::<VisualState>() = if startup {
        VisualState {
            status:
                "Ready | native startup and menu eye loop\nAudio and physical input acceptance NOT RUN"
                    .into(),
            ..default()
        }
    } else {
        VisualState {
            song_seconds: 32.0,
            hit_pulses: [0.8, 0.6],
            sync_pulse: 0.8,
            resonance: 0.7,
            status:
                "VISUAL SMOKE | deterministic preview\nAudio, input and hardware acceptance NOT RUN"
                    .into(),
            running: false,
            ..default()
        }
    };
    if let Smoke::Quality(quality) = mode {
        app.world_mut().resource_mut::<VisualState>().quality = quality;
        app.world_mut()
            .resource_mut::<DisplayState>()
            .request(DisplaySettings {
                fullscreen: true,
                fullscreen_size: [640, 480],
                ..default()
            });
    }
    app.add_systems(
        Update,
        (move |mut commands: Commands,
               mut frame: Local<u32>,
               mut requested: Local<bool>,
               intro: Option<Res<BrandIntroStatus>>,
               visual: Res<VisualState>,
               cameras: Query<&Camera, With<PresentationCamera>>,
               panels: Query<(&ComputedNode, &UiGlobalTransform), With<view::StatusPanel>>,
               rows: Query<(&view::SettingsRowNode, &ComputedNode, &UiGlobalTransform)>,
               mut exit: MessageWriter<AppExit>| {
            *frame += 1;
            if *frame > 1_200
                || intro
                    .as_ref()
                    .is_some_and(|status| status.phase == BrandIntroPhase::Failed)
            {
                eprintln!("Startup preview failed or exceeded frame limit: {intro:?}");
                exit.write(AppExit::Error(std::num::NonZeroU8::new(1).unwrap()));
                return;
            }
            let ready = if startup {
                intro
                    .as_ref()
                    .is_some_and(|status| status.is_complete() && status.idle_seconds >= 6.1)
            } else {
                *frame == 30
            };
            if !*requested && ready {
                if visual.settings.is_some() || matches!(mode, Smoke::Menu(_)) {
                    let metrics = (|| {
                        let selected = if let Some(settings) = &visual.settings {
                            let selected = settings
                                .rows
                                .iter()
                                .position(|row| row.selected)
                                .ok_or("Missing selected settings row")?;
                            let (_, row, transform) = rows
                                .iter()
                                .find(|(row, _, _)| row.0 == selected)
                                .ok_or("Selected settings row was not laid out")?;
                            Some((selected, row, transform))
                        } else {
                            None
                        };
                        smoke_layout_metrics(
                            viewport,
                            cameras.single().map_err(|error| error.to_string())?,
                            panels.single().map_err(|error| error.to_string())?,
                            selected,
                        )
                    })();
                    match metrics {
                        Ok(metrics) => eprintln!("VIEWPORT_GEOMETRY {metrics}"),
                        Err(error) => {
                            eprintln!("Settings layout failed: {error}");
                            exit.write(AppExit::Error(std::num::NonZeroU8::new(1).unwrap()));
                            return;
                        }
                    }
                }
                *requested = true;
                let output = path.clone();
                commands
                    .spawn(Screenshot(RenderTarget::Image(ImageRenderTarget {
                        handle: target.clone(),
                        scale_factor: viewport.scale,
                    })))
                    .observe(
                        move |capture: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                            let result = capture
                                .image
                                .clone()
                                .try_into_dynamic()
                                .map_err(|error| error.to_string())
                                .and_then(|image| {
                                    image
                                        .to_rgb8()
                                        .save(&output)
                                        .map_err(|error| error.to_string())
                                });
                            match result {
                                Ok(()) => {
                                    exit.write(AppExit::Success);
                                }
                                Err(error) => {
                                    eprintln!("Screenshot failed: {error}");
                                    exit.write(AppExit::Error(
                                        std::num::NonZeroU8::new(1).unwrap(),
                                    ));
                                }
                            }
                        },
                    );
            }
        })
        .after(BrandIntroSystems::Advance),
    );
    match app.run() {
        AppExit::Success => Ok(()),
        error => Err(format!("Visual preview exited: {error:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::{
            gamepad::{GamepadButtonStateChangedEvent, GamepadConnectionEvent},
            keyboard::KeyboardInput,
        },
        window::WindowFocused,
    };
    use cocobeat_schema::SongTime;
    use kira::{
        AudioManager, AudioManagerSettings, Frame,
        backend::mock::{MockBackend, MockBackendSettings},
        sound::static_sound::StaticSoundData,
    };

    #[test]
    fn viewport_smoke_parsing_bounds_physical_sizes_scales_and_row_indices() {
        for (width, height, scale) in [
            (1280, 800, 2.0),
            (640, 360, 1.0),
            (320, 240, 1.0),
            (180, 120, 1.0),
        ] {
            let viewport = SmokeViewport::parse(
                &width.to_string(),
                &height.to_string(),
                &scale.to_string(),
                "12",
            )
            .unwrap();
            assert_eq!(viewport.size, [width, height]);
            assert_eq!(viewport.scale, scale);
            assert_eq!(viewport.selection, Some(12));
        }
        for (width, height, scale, row) in [
            ("0", "800", "1", "0"),
            ("8193", "800", "1", "0"),
            ("8192", "8192", "1", "0"),
            ("1280", "-1", "1", "0"),
            ("1280", "800", "NaN", "0"),
            ("1280", "800", "inf", "0"),
            ("1280", "800", "0", "0"),
            ("1280", "800", "-1", "0"),
            ("1280", "800", "9", "0"),
            ("1280", "800", "1", "-1"),
        ] {
            assert!(SmokeViewport::parse(width, height, scale, row).is_err());
        }
    }

    #[test]
    fn captured_history_survives_playback_observation_and_consumption_cadences() {
        use cocobeat_core::DuoEngine;
        use cocobeat_schema::{DuoInput, Hit, PlayerId};

        let captures = [
            (PlayerId::P1, 111_000_000, 1_205_328),
            (PlayerId::P2, 130_000_000, 1_206_240),
            (PlayerId::P1, 1_001_000_000, 1_248_048),
            (PlayerId::P2, 1_018_000_000, 1_248_864),
            (PlayerId::P1, 5_002_000_000, 1_440_096),
            (PlayerId::P2, 5_011_000_000, 1_440_528),
        ];
        let expected_hits: Vec<_> = captures
            .iter()
            .enumerate()
            .map(|(index, &(player, _, frame))| Hit {
                epoch: SessionEpoch(0),
                player,
                seq: (index / 2) as u64,
                song_time: SongTime::from_frames(frame),
            })
            .collect();
        let mut expected =
            DuoEngine::new(SessionEpoch(0), dev_song::anchors(), DuoRules::default()).unwrap();
        for &hit in &expected_hits {
            expected.ingest(DuoInput::Hit(hit)).unwrap();
        }
        for player in [PlayerId::P1, PlayerId::P2] {
            expected
                .ingest(DuoInput::Watermark {
                    epoch: SessionEpoch(0),
                    player,
                    through: SongTime::from_frames(3_092_881),
                })
                .unwrap();
        }
        assert_eq!(
            expected
                .events()
                .iter()
                .filter(|event| matches!(event, DuoEvent::FreeSync(_)))
                .count(),
            1
        );
        assert_eq!(
            expected
                .events()
                .iter()
                .filter(|event| matches!(event, DuoEvent::AnchorSync(_)))
                .count(),
            2
        );

        // Millisecond-aligned observations avoid fractional-frame cursor quantization
        // These cycles average 60/144/48 Hz; the last case consumes every 500 ms
        let mut cadence_144 = [7_u64; 18];
        cadence_144[17] = 6;
        let mut consumption_times = Vec::new();
        for cadence in [
            &[16_u64, 17, 17][..],
            &cadence_144,
            &[20, 21, 21, 21, 21, 21],
            &[500],
        ] {
            let mut game = Game::new().unwrap();
            game.phase = Phase::Starting;
            assert!(
                game.observe_playback(25.0, PlaybackState::Playing, MonotonicTime::from_nanos(0))
                    .unwrap()
            );
            let (mut elapsed_ms, mut tick, mut next) = (0_u64, 0, 0);
            while elapsed_ms < 6_000 {
                elapsed_ms = (elapsed_ms + cadence[tick % cadence.len()]).min(6_000);
                tick += 1;
                let consumed_ns = elapsed_ms * 1_000_000;
                assert!(
                    !game
                        .observe_playback(
                            25.0 + elapsed_ms as f64 / 1_000.0,
                            PlaybackState::Playing,
                            MonotonicTime::from_nanos(consumed_ns),
                        )
                        .unwrap()
                );
                // Even in the 500 ms case, each capture is within its anchor's
                // 250 ms validity; a fresh observation cannot certify an expired gap
                while next < captures.len() && captures[next].1 <= consumed_ns {
                    let (player, observed_ns, _) = captures[next];
                    game.session.hit(player, observed_ns, consumed_ns).unwrap();
                    next += 1;
                }
                game.session.advance().unwrap();
            }
            assert_eq!(next, captures.len());
            assert_eq!(game.session.current, SongTime::from_frames(1_488_000));
            let recorded_hits: Vec<_> = game
                .session
                .replay
                .facts()
                .iter()
                .filter_map(|fact| match *fact {
                    DuoInput::Hit(hit) => Some(hit),
                    _ => None,
                })
                .collect();
            assert_eq!(recorded_hits, expected_hits, "cadence={cadence:?}");
            consumption_times.push(
                game.session
                    .diagnostics
                    .iter()
                    .map(|input| input.consumed_ns)
                    .collect::<Vec<_>>(),
            );
            game.session.finish().unwrap();
            let replay = Replay::decode(game.session.replay.encode().unwrap().as_slice()).unwrap();
            let restored = replay
                .replay(
                    CONTENT_ID,
                    RULES_ID,
                    dev_song::anchors(),
                    DuoRules::default(),
                )
                .unwrap();
            for engine in [&game.session.engine, &restored] {
                assert_eq!(engine.events(), expected.events(), "cadence={cadence:?}");
                assert_eq!(
                    engine.resonance(),
                    expected.resonance(),
                    "cadence={cadence:?}"
                );
            }
        }
        for first in 0..consumption_times.len() {
            for second in first + 1..consumption_times.len() {
                assert_ne!(consumption_times[first], consumption_times[second]);
            }
        }
    }

    #[test]
    fn menu_brand_control_follows_game_and_captured_focus() {
        let mut app = App::new();
        app.add_message::<WindowFocused>()
            .add_message::<KeyboardInput>()
            .add_message::<GamepadConnectionEvent>()
            .add_message::<GamepadButtonStateChangedEvent>()
            .insert_resource(Game::new().unwrap())
            .init_resource::<BrandIntroControl>()
            .add_systems(Update, suspend_intro);
        input::install(&mut app);
        // The presentation control must not unlock startup input or start a song
        app.world_mut()
            .resource_mut::<InputState>()
            .set_controls_enabled(false);
        app.update();
        assert!(app.world().resource::<BrandIntroControl>().idle_enabled);
        assert!(!app.world().resource::<InputState>().controls_enabled());
        assert_eq!(app.world().resource::<Game>().phase, Phase::Ready);

        let window = app.world_mut().spawn_empty().id();
        for focused in [false, true] {
            app.world_mut()
                .write_message(WindowFocused { window, focused });
            app.update();
            let control = app.world().resource::<BrandIntroControl>();
            assert_eq!(control.suspended, !focused);
            assert!(control.idle_enabled);
        }
        app.world_mut()
            .resource_mut::<InputState>()
            .set_menu_open(false);
        app.update();
        assert!(!app.world().resource::<BrandIntroControl>().idle_enabled);
        app.world_mut()
            .resource_mut::<InputState>()
            .set_menu_open(true);
        for phase in [
            Phase::Starting,
            Phase::Running,
            Phase::Pausing,
            Phase::Paused,
            Phase::Finished,
            Phase::Fault,
        ] {
            app.world_mut().resource_mut::<Game>().phase = phase;
            app.update();
            assert!(!app.world().resource::<BrandIntroControl>().idle_enabled);
        }

        {
            let mut game = app.world_mut().resource_mut::<Game>();
            game.session = Session::new(SessionEpoch(9)).unwrap();
            game.session.current = SongTime::from_frames(96_000);
            game.main_menu().unwrap();
            assert_eq!(game.phase, Phase::Ready);
            assert_eq!(game.session.epoch(), SessionEpoch(9));
            assert_eq!(game.session.current, SongTime::ZERO);
            assert!(game.session.clock.last_observation().is_none());
            assert_eq!(game.saved_facts, 0);
        }
        app.world_mut()
            .resource_mut::<InputState>()
            .open_main_menu();
        app.update();
        assert!(app.world().resource::<BrandIntroControl>().idle_enabled);
        assert!(!app.world().resource::<BrandIntroControl>().suspended);
    }

    #[test]
    fn initial_playing_state_waits_for_actual_callback_progress() {
        let mut audio = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: 48_000,
            },
            ..default()
        })
        .unwrap();
        let handle = audio
            .play(StaticSoundData {
                sample_rate: 48_000,
                frames: vec![Frame::ZERO; 48_000].into(),
                settings: default(),
                slice: None,
            })
            .unwrap();
        let mut game = Game::new().unwrap();
        game.phase = Phase::Starting;
        assert_eq!(handle.state(), PlaybackState::Playing);
        assert_eq!(handle.position(), 0.0);
        assert!(
            !game
                .observe_playback(
                    handle.position(),
                    handle.state(),
                    MonotonicTime::from_nanos(0)
                )
                .unwrap()
        );
        assert_eq!(game.phase, Phase::Starting);
        assert!(game.session.clock.last_observation().is_none());

        audio.backend_mut().on_start_processing();
        audio.backend_mut().process();
        audio.backend_mut().on_start_processing();
        assert!(handle.position() > 0.0);
        assert!(
            game.observe_playback(
                handle.position(),
                handle.state(),
                MonotonicTime::from_nanos(10_000_000),
            )
            .unwrap()
        );
        assert_eq!(game.phase, Phase::Running);
        assert!(game.session.clock.last_observation().is_some());
    }
}
