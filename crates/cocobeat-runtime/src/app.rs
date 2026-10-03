use crate::{
    audio::AudioOutput,
    brand_intro::{
        self, BrandImpact, BrandIntroControl, BrandIntroPhase, BrandIntroStatus, BrandIntroSystems,
    },
    clock::MonotonicTime,
    content::{self, RULES_ID, SongContent},
    dev_song,
    display::{self, DisplayState, DisplaySystems, PresentationCamera},
    i18n::{Locale, Message},
    input::{
        self, Control, InputSource, InputState, MenuPhase, MenuPresentation, MenuRowRole,
        MenuScroll, SettingsAction,
    },
    session::{Session, SessionResults},
    settings::{DisplaySettings, QualityPreset, QualitySettings, Settings},
    settings_menu::SettingsMenu,
    ui_assets,
    view::{self, VisualState},
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
use cocobeat_schema::{AnchorGrade, DuoEvent, DuoInput, DuoRules, SessionEpoch, SongTime};
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
    Section(Locale, SongTime, QualitySettings),
    Feedback(FeedbackSmoke, Option<QualitySettings>),
    FeedbackMotion,
    Startup,
    Settings,
    SettingsFault(Locale),
    Locale(Locale),
    Languages(Locale),
    Menu(Locale, Phase),
    Players(Locale),
    Quality(QualitySettings),
    Graphics(Locale),
    Pacing(Locale),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FeedbackSmoke {
    Local,
    Free,
    Anchor,
    AnchorGood,
    Miss,
    Approach,
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
    content: SongContent,
    session: Session,
    phase: Phase,
    notice: Message,
    saved_facts: usize,
    replay_status: Message,
    fault_details: Option<String>,
    results: Option<SessionResults>,
    transition_started: std::time::Instant,
}

impl Game {
    #[cfg(test)]
    fn new() -> Result<Self, String> {
        Self::with_content(SongContent::development())
    }

    fn with_content(content: SongContent) -> Result<Self, String> {
        Ok(Self {
            session: Session::for_content(SessionEpoch(0), &content)?,
            content,
            phase: Phase::Ready,
            notice: Message::new("game.welcome"),
            saved_facts: 0,
            replay_status: Message::default(),
            fault_details: None,
            results: None,
            transition_started: std::time::Instant::now(),
        })
    }

    fn save(&mut self) -> Result<(), String> {
        self.save_to(Path::new("replays"))
    }

    fn save_to(&mut self, directory: &Path) -> Result<(), String> {
        if self.session.replay.facts().is_empty() {
            if self.phase != Phase::Fault {
                self.notice = Message::new("game.nothing_to_save");
            }
        } else if self.saved_facts != self.session.replay.facts().len() {
            let path = self.session.save(directory).inspect_err(|error| {
                self.replay_status =
                    Message::with("results.replay_failed", [("error", error.clone())]);
            })?;
            self.saved_facts = self.session.replay.facts().len();
            self.replay_status = Message::with(
                "results.replay_saved",
                [("path", path.display().to_string())],
            );
            if self.phase != Phase::Fault {
                self.notice = Message::with("game.saved", [("path", path.display().to_string())]);
            }
        }
        Ok(())
    }

    fn save_before_terminal_transition(
        &mut self,
        control: Control,
        directory: &Path,
    ) -> Result<(), String> {
        if matches!(self.phase, Phase::Finished | Phase::Fault)
            && matches!(control, Control::Restart | Control::MainMenu)
        {
            self.save_to(directory)?;
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
        let session = Session::for_content(SessionEpoch(epoch), &self.content)?;
        audio.start()?;
        self.session = session;
        self.saved_facts = 0;
        self.replay_status = Message::default();
        self.fault_details = None;
        self.results = None;
        self.phase = Phase::Starting;
        self.transition_started = std::time::Instant::now();
        self.notice = Message::new("game.waiting_audio");
        Ok(())
    }

    fn main_menu(&mut self) -> Result<(), String> {
        self.save()?;
        // Keep the epoch until the next explicit Start advances it
        self.session = Session::for_content(self.session.epoch(), &self.content)?;
        self.saved_facts = 0;
        self.replay_status = Message::default();
        self.fault_details = None;
        self.results = None;
        self.phase = Phase::Ready;
        self.notice = Message::new("game.ready");
        Ok(())
    }

    fn next_anchor(&self) -> Option<SongTime> {
        self.content
            .anchors
            .iter()
            .find(|anchor| anchor.song_time >= self.session.current)
            .map(|anchor| anchor.song_time)
    }

    fn fault(&mut self, audio: &mut AudioOutput, error: String, message: &'static str) {
        audio.stop();
        self.phase = Phase::Fault;
        self.fault_details = Some(error.clone());
        self.results = Some(self.session.summary());
        let saved = self.save();
        eprintln!("Session stopped: {error}");
        self.notice = Message::new(message);
        if let Err(save_error) = saved {
            eprintln!("Replay save failed: {save_error}");
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

    fn request_pause(
        &mut self,
        input: &mut InputState,
        source: Option<InputSource>,
        observed: MonotonicTime,
    ) -> Result<bool, String> {
        if !matches!(self.phase, Phase::Running | Phase::Starting) {
            return Ok(false);
        }
        self.session
            .clock
            .invalidate_calibration(observed)
            .map_err(|error| format!("Pause transition: {error:?}"))?;
        self.phase = Phase::Pausing;
        self.transition_started = std::time::Instant::now();
        input.set_menu_open(true);
        if let Some(source) = source {
            input.claim_menu(source);
        }
        Ok(true)
    }

    fn filter_transition_controls(&self, input: &mut InputState) {
        // Captured controls must be gated before reading this frame's audio acknowledgment
        if self.phase == Phase::Pausing {
            input
                .queued
                .retain(|event| event.control == Control::FocusLost);
        } else if self.phase == Phase::Starting {
            input.queued.retain(|event| {
                matches!(event.control, Control::TogglePause(_) | Control::FocusLost)
            });
        }
    }
}

pub fn run() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [] => run_game(None),
        [flag, directory] if flag == "--package" => run_game(Some(Path::new(directory))),
        [flag, directory, replay, path] if flag == "--package" && replay == "--replay" => {
            content::load_package(Path::new(directory))
                .and_then(|(content, _sound)| validate_replay(path, &content))
        }
        [flag, directory, smoke, path] if flag == "--package" && smoke == "--visual-smoke" => {
            content::load_package(Path::new(directory)).and_then(|(content, _sound)| {
                visual_smoke_for_content(
                    PathBuf::from(path),
                    Smoke::Scene,
                    SmokeViewport::default(),
                    content,
                )
            })
        }
        [
            flag,
            directory,
            smoke,
            frame,
            code,
            preset,
            width,
            height,
            scale,
            path,
        ] if flag == "--package" && smoke == "--section-smoke" => (|| {
            let (content, _sound) = content::load_package(Path::new(directory))?;
            let frame = frame
                .parse::<i64>()
                .map_err(|_| "Invalid section preview frame")?;
            if !(0..=content.end.frames()).contains(&frame) {
                return Err("Section preview frame must lie within the song timeline".into());
            }
            let locale = Locale::ALL
                .into_iter()
                .find(|locale| locale.code() == code)
                .ok_or("Unsupported section preview locale")?;
            let mode = Smoke::Section(locale, SongTime::from_frames(frame), smoke_quality(preset)?);
            let mut viewport = SmokeViewport::parse(width, height, scale, "0")?;
            viewport.selection = None;
            visual_smoke_for_content(PathBuf::from(path), mode, viewport, content)
        })(),
        [flag] if flag == "--help" || flag == "-h" => {
            println!(
                "CoCoBeat: local duet\n  --package DIR         play a validated authored song package; default is the 64-second development song\n  --package DIR --replay FILE  validate a replay against the full package identity\n  --package DIR --visual-smoke PNG  preview the loaded duration and Anchors without audio\n  --package DIR --section-smoke FRAME CODE PRESET WIDTH HEIGHT SCALE PNG  preview authored cues at an integer song frame\n  --replay FILE          validate a saved development-song replay\n  --visual-smoke PNG     render a deterministic scene without audio or gameplay\n  --feedback-smoke EFFECT [PRESET [WIDTH HEIGHT SCALE]] PNG  render local/free/anchor/anchor-good/miss/approach; optional low/medium/high/off at reduced 3D resolution\n  --feedback-motion-smoke DIR  render 240 ordered GPU frames with real rule feedback\n  --startup-smoke PNG    render the native intro and Ready eye loop without audio\n  --settings-smoke PNG   render settings with a simulated low-resolution fullscreen scene\n  --locale-smoke CODE PNG    render localized settings on a simulated surface\n  --language-smoke CODE PNG  render the native-name language selector\n  --menu-smoke CODE PNG      render the localized Ready menu without audio\n  --quality-smoke PRESET PNG  render low/medium/high/off graphics on a simulated surface\n  --settings-page-smoke PAGE CODE PNG  render graphics/pacing settings\n  --viewport-smoke PAGE CODE WIDTH HEIGHT SCALE ROW PNG  render main/graphics/pacing/languages/ready/players/starting/pausing/paused/finished/fault/settings-fault at physical pixels and DPI; ROW starts at 0\nEnter / controller Start: claim or take menu control (first press only)\nEnter / controller South: menu confirmation; Esc / Start: pause\nReplays are saved locally in ./replays/"
            );
            Ok(())
        }
        [flag, path] if flag == "--replay" => validate_replay(path, &SongContent::development()),
        [flag, path] if flag == "--visual-smoke" => visual_smoke(PathBuf::from(path), Smoke::Scene),
        [flag, path] if flag == "--feedback-motion-smoke" => {
            visual_smoke(PathBuf::from(path), Smoke::FeedbackMotion)
        }
        [flag, effect, path] if flag == "--feedback-smoke" => feedback_smoke_mode(effect, None)
            .and_then(|mode| visual_smoke(PathBuf::from(path), mode)),
        [flag, effect, preset, path] if flag == "--feedback-smoke" => {
            feedback_smoke_mode(effect, Some(preset))
                .and_then(|mode| visual_smoke(PathBuf::from(path), mode))
        }
        [flag, effect, preset, width, height, scale, path] if flag == "--feedback-smoke" => {
            (|| {
                let mode = feedback_smoke_mode(effect, Some(preset))?;
                let mut viewport = SmokeViewport::parse(width, height, scale, "0")?;
                viewport.selection = None;
                visual_smoke_at(PathBuf::from(path), mode, viewport)
            })()
        }
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
                        _ => Smoke::Menu(locale, Phase::Ready),
                    },
                )
            } else {
                Err(format!("Unsupported locale: {code}"))
            }
        }
        [flag, preset, path] if flag == "--quality-smoke" => smoke_quality(preset)
            .and_then(|quality| visual_smoke(PathBuf::from(path), Smoke::Quality(quality))),
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
                    "ready" => Smoke::Menu(locale, Phase::Ready),
                    "players" => Smoke::Players(locale),
                    "starting" => Smoke::Menu(locale, Phase::Starting),
                    "pausing" => Smoke::Menu(locale, Phase::Pausing),
                    "paused" => Smoke::Menu(locale, Phase::Paused),
                    "finished" => Smoke::Menu(locale, Phase::Finished),
                    "fault" => Smoke::Menu(locale, Phase::Fault),
                    "settings-fault" => Smoke::SettingsFault(locale),
                    _ => return Err(format!("Unsupported settings page: {page}")),
                };
                let viewport = SmokeViewport::parse(width, height, scale, selection)?;
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

fn run_game(package: Option<&Path>) -> Result<(), String> {
    let (content, sound) = if let Some(path) = package {
        content::load_package(path)?
    } else {
        (SongContent::development(), content::development_sound())
    };
    let game = Game::with_content(content)?;
    let audio = AudioOutput::new(Some(sound))?;
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
        .insert_resource(game)
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

fn validate_replay(path: &str, content: &SongContent) -> Result<(), String> {
    let replay = Replay::load(path).map_err(|error| error.to_string())?;
    if replay.facts().iter().any(|fact| {
        matches!(fact, DuoInput::Hit(hit) if hit.song_time < SongTime::ZERO || hit.song_time >= content.end)
    }) {
        return Err("Replay Hit lies outside the song timeline".into());
    }
    let engine = replay
        .replay(
            &content.content_id,
            RULES_ID,
            content.anchors.clone(),
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
        if visual_feedback(event, visual) {
            audio.sync()?;
        }
    }
    Ok(())
}

// Only confirmed rule events produce shared feedback; local Hit remains immediate
fn visual_feedback(event: DuoEvent, visual: &mut VisualState) -> bool {
    match event {
        DuoEvent::FreeSync(_) => visual.free_sync_pulse = 1.0,
        DuoEvent::AnchorSync(event) => {
            visual.anchor_sync_pulse = 1.0;
            visual.anchor_sync_precise =
                event.p1.grade == AnchorGrade::Precise && event.p2.grade == AnchorGrade::Precise;
        }
        DuoEvent::AnchorJudged(judgement) => {
            if judgement.grade == AnchorGrade::Miss {
                visual.miss_pulses[judgement.player.index()] = 1.0;
            }
            return false;
        }
    }
    true
}

fn reset_feedback(visual: &mut VisualState) {
    visual.hit_pulses = [0.0; 2];
    visual.free_sync_pulse = 0.0;
    visual.anchor_sync_pulse = 0.0;
    visual.anchor_sync_precise = false;
    visual.miss_pulses = [0.0; 2];
}

fn decay_feedback(visual: &mut VisualState, delta: f32) {
    for pulse in &mut visual.hit_pulses {
        *pulse = (*pulse - delta * 4.0).max(0.0);
    }
    for pulse in &mut visual.miss_pulses {
        *pulse = (*pulse - delta * 2.0).max(0.0);
    }
    visual.free_sync_pulse = (visual.free_sync_pulse - delta * 1.8).max(0.0);
    visual.anchor_sync_pulse = (visual.anchor_sync_pulse - delta * 1.4).max(0.0);
}

fn update_game(
    mut game: ResMut<Game>,
    mut input: ResMut<InputState>,
    mut audio: NonSendMut<AudioOutput>,
    mut visual: ResMut<VisualState>,
    (time, mut settings, mut display, mut menu_scroll): (
        Res<Time>,
        ResMut<SettingsMenu>,
        ResMut<DisplayState>,
        ResMut<MenuScroll>,
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
    settings.sync_display(&display);
    if settings.tick(settings_now, &mut display) {
        menu_scroll.reset();
    }
    visual.quality = settings.values.quality;
    visual.locale = settings.values.locale;
    visual.duration_seconds = game.content.end.as_seconds_f64();
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
            menu_scroll.reset();
            visual.status = visual.locale.text("game.ready").into();
        }
        // Completion only unlocks the next fresh press, never a queued startup control
        return;
    }
    let delta = time.delta_secs().min(1.0);
    decay_feedback(&mut visual, delta);

    let mut fault_message = "game.stopped";
    let result = (|| -> Result<(), String> {
        if let Some(error) = audio.take_error() {
            let _ = game.session.clock.device_lost(observed);
            fault_message = "game.audio_failed";
            return Err(error);
        }
        game.filter_transition_controls(&mut input);
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
            menu_scroll.reset();
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
            if matches!(game.phase, Phase::Starting | Phase::Pausing)
                && !matches!(event.control, Control::TogglePause(_) | Control::FocusLost)
            {
                continue;
            }
            if let Control::Settings(action) = event.control {
                if action == SettingsAction::Open {
                    menu_scroll.reset();
                    if input.menu_open && !matches!(game.phase, Phase::Running | Phase::Starting) {
                        if !settings.is_open() {
                            settings.begin(&display);
                        }
                        input.set_settings_open(true);
                    } else {
                        input.set_settings_open(false);
                    }
                } else if !menu_scroll.handle(action) {
                    if settings.handle(action, settings_now, &mut display) {
                        input.set_settings_open(false);
                    }
                    menu_scroll.reset();
                }
                // A second device cannot confirm a new preview in this capture batch
                break;
            }
            if settings.is_open() && event.control != Control::FocusLost {
                continue;
            }
            if let Err(error) =
                game.save_before_terminal_transition(event.control, Path::new("replays"))
            {
                eprintln!("Replay save failed before leaving session: {error}");
                break;
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
                Control::Start | Control::TogglePause(_) if game.phase == Phase::Paused => {
                    game.session
                        .clock
                        .resume(observed)
                        .map_err(|error| format!("Resume clock: {error:?}"))?;
                    audio.resume();
                    game.phase = Phase::Starting;
                    game.transition_started = std::time::Instant::now();
                    menu_scroll.reset();
                }
                Control::Start if game.phase == Phase::Ready => {
                    game.start(&mut audio)?;
                    reset_feedback(&mut visual);
                    menu_scroll.reset();
                }
                Control::Restart => {
                    game.start(&mut audio)?;
                    reset_feedback(&mut visual);
                    input.set_menu_open(true);
                    menu_scroll.reset();
                }
                Control::MainMenu => {
                    game.main_menu()?;
                    audio.stop();
                    input.open_main_menu();
                    menu_scroll.reset();
                    reset_feedback(&mut visual);
                    // Return to Ready without consuming a second confirmation from this batch
                    break;
                }
                Control::TogglePause(_) | Control::FocusLost => {
                    let source = match event.control {
                        Control::TogglePause(source) => Some(source),
                        _ => None,
                    };
                    if game.request_pause(&mut input, source, observed)? {
                        audio.pause();
                        menu_scroll.reset();
                        break;
                    }
                }
                Control::SaveReplay => {
                    if let Err(error) = game.save() {
                        if matches!(game.phase, Phase::Finished | Phase::Fault) {
                            eprintln!("Replay save failed: {error}");
                        } else {
                            fault_message = "game.replay_failed";
                            return Err(error);
                        }
                    }
                }
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
                let events = game.session.finish()?;
                game.results = Some(game.session.summary());
                feedback(events, &mut audio, &mut visual)?;
                game.phase = Phase::Finished;
                input.set_menu_open(true);
                menu_scroll.reset();
                if let Err(error) = game.save() {
                    eprintln!("Replay save failed after song end: {error}");
                    game.notice = Message::new("game.replay_failed");
                }
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
        menu_scroll.reset();
    }
    visual.transitioning = matches!(game.phase, Phase::Starting | Phase::Pausing);
    input.set_menu_transitioning(visual.transitioning);
    visual.song_seconds = game.session.current.as_seconds_f64();
    visual.next_anchor_seconds = game.next_anchor().map(SongTime::as_seconds_f64);
    visual.resonance = f32::from(game.session.engine.resonance().level_per_mille) / 1_000.0;
    visual.running = game.phase == Phase::Running;
    visual.quality = settings.values.quality;
    let locale = settings.values.locale;
    visual.locale = locale;
    input.set_menu_phase(
        menu_phase(game.phase),
        game.saved_facts != game.session.replay.facts().len(),
    );
    visual.menu = runtime_menu(&game, &mut input, &settings, settings_now, &display);
    update_section_visuals(
        &game.content,
        game.session.current,
        matches!(game.phase, Phase::Running | Phase::Pausing | Phase::Paused),
        &mut visual,
    );
    if visual.menu.is_none() {
        visual.status = game_status(&game, &input, &settings);
    }
}

fn update_section_visuals(
    content: &SongContent,
    time: SongTime,
    active: bool,
    visual: &mut VisualState,
) {
    let (latest, next) = if active {
        content.section_cues(time)
    } else {
        (None, None)
    };
    visual.next_section_seconds = next.map(|cue| cue.time.as_seconds_f64());
    visual.section_hint = if visual.menu.is_some() {
        None
    } else {
        next.map(|cue| ("hud.section_next", cue))
            .or_else(|| latest.map(|cue| ("hud.section_recent", cue)))
            .map(|(key, cue)| {
                let label: String = cue
                    .label
                    .chars()
                    .map(|character| {
                        if character.is_control() {
                            ' '
                        } else {
                            character
                        }
                    })
                    .collect();
                let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
                let label = if label.is_empty() {
                    format!("#{}", cue.id)
                } else {
                    format!("#{} · {label}", cue.id)
                };
                Message::with(key, [("label", label)]).render(visual.locale)
            })
    };
}

fn next_anchor_label(game: &Game, locale: Locale) -> String {
    let song_seconds = game.session.current.as_seconds_f64();
    game.next_anchor()
        .map(|time| {
            Message::with(
                "hud.next_anchor",
                [(
                    "seconds",
                    format!("{:.1}", time.as_seconds_f64() - song_seconds),
                )],
            )
            .render(locale)
        })
        .unwrap_or_else(|| locale.text("hud.final_release").into())
}

fn game_text(game: &Game, settings: &SettingsMenu) -> (String, Vec<String>) {
    let locale = settings.values.locale;
    let mut title = locale
        .text(match game.phase {
            Phase::Ready => "phase.ready",
            Phase::Starting => "phase.starting",
            Phase::Running => "phase.running",
            Phase::Pausing => "phase.pausing",
            Phase::Paused => "phase.paused",
            Phase::Finished => "phase.finished",
            Phase::Fault => "phase.fault",
        })
        .to_string();
    if game.phase == Phase::Fault {
        title.push('\n');
        title.push_str(&game.notice.render(locale));
    }
    let mut details = Vec::new();
    if let Some(results) = &game.results {
        let pair = |key, counts: [u64; 2]| {
            Message::with(
                key,
                [
                    ("first", counts[0].to_string()),
                    ("second", counts[1].to_string()),
                ],
            )
            .render(locale)
        };
        details.push(pair("results.hits", results.hits));
        for (grade, key) in [
            "results.precise",
            "results.good",
            "results.late_early",
            "results.miss",
        ]
        .into_iter()
        .enumerate()
        {
            details.push(pair(
                key,
                [results.anchors[0][grade], results.anchors[1][grade]],
            ));
        }
        let together = Message::with(
            "results.sync",
            [
                ("free", results.free_sync.to_string()),
                ("anchor", results.anchor_sync.to_string()),
            ],
        )
        .render(locale);
        if game.phase == Phase::Finished {
            title.push('\n');
            title.push_str(&together);
            title.push('\n');
            title.push_str(
                locale.text(if game.replay_status.key == "results.replay_failed" {
                    "results.replay_failed_short"
                } else if game.saved_facts == game.session.replay.facts().len() {
                    "results.replay_saved_short"
                } else {
                    "results.replay_pending"
                }),
            );
        }
        details.push(together);
    } else {
        details.push(next_anchor_label(game, locale));
    }
    if !game.session.replay.facts().is_empty() {
        details.push(
            if game.replay_status.key == "results.replay_failed"
                || game.saved_facts == game.session.replay.facts().len()
            {
                game.replay_status.render(locale)
            } else {
                locale.text("results.replay_pending").into()
            },
        );
    }
    if let Some(error) = &game.fault_details {
        details
            .push(Message::with("game.error_details", [("error", error.clone())]).render(locale));
    }
    details.extend(
        [
            if game.results.is_some() && game.notice.key == "game.saved" {
                String::new()
            } else {
                game.notice.render(locale)
            },
            settings.notice.render(locale),
            Message::with(
                "hud.timing",
                [(
                    "milliseconds",
                    format!("{:.1}", game.session.uncertainty_frames as f64 / 48.0),
                )],
            )
            .render(locale),
        ]
        .into_iter()
        .filter(|line| !line.is_empty()),
    );
    (title, details)
}

fn menu_phase(phase: Phase) -> MenuPhase {
    match phase {
        Phase::Ready => MenuPhase::Ready,
        Phase::Running | Phase::Paused => MenuPhase::Paused,
        Phase::Finished => MenuPhase::Finished,
        Phase::Fault => MenuPhase::Fault,
        Phase::Starting | Phase::Pausing => MenuPhase::Transition,
    }
}

fn runtime_menu(
    game: &Game,
    input: &mut InputState,
    settings: &SettingsMenu,
    now: f64,
    display: &DisplayState,
) -> Option<MenuPresentation> {
    settings
        .presentation(now, display, settings.values.locale)
        .map(|mut menu| {
            let locale = settings.values.locale;
            menu.owner_hint = Some(input.menu_owner_hint(locale));
            if game.phase == Phase::Fault {
                menu.title = format!(
                    "{}\n{}\n{}",
                    locale.text("phase.fault"),
                    game.notice.render(locale),
                    menu.title
                );
                if let Some(row) = menu
                    .rows
                    .iter_mut()
                    .find(|row| row.role == MenuRowRole::Information)
                {
                    let mut details = vec![row.text.clone()];
                    if let Some(error) = &game.fault_details {
                        details.push(
                            Message::with("game.error_details", [("error", error.clone())])
                                .render(locale),
                        );
                    }
                    if !game.replay_status.key.is_empty() {
                        details.push(game.replay_status.render(locale));
                    }
                    row.text = details.join("\n");
                }
            }
            menu
        })
        .or_else(|| game_menu(game, input, settings))
}

fn game_menu(
    game: &Game,
    input: &mut InputState,
    settings: &SettingsMenu,
) -> Option<MenuPresentation> {
    if !input.menu_open {
        return None;
    }
    input.set_menu_phase(
        menu_phase(game.phase),
        game.saved_facts != game.session.replay.facts().len(),
    );
    let (title, information) = game_text(game, settings);
    input.menu_presentation(settings.values.locale, title, information)
}

fn game_status(game: &Game, input: &InputState, settings: &SettingsMenu) -> String {
    // Binding details and timing diagnostics remain available in the pause menu
    let locale = settings.values.locale;
    if game.phase == Phase::Fault {
        return game.notice.render(locale);
    }
    let mut lines = vec![next_anchor_label(game, locale)];
    if input.status.key == "input.controller_disconnected" {
        lines.push(input.status.render(locale));
    }
    if matches!(game.notice.key, "game.saved" | "game.nothing_to_save") {
        lines.push(game.notice.render(locale));
    }
    lines.join("\n")
}

fn smoke_quality(preset: &str) -> Result<QualitySettings, String> {
    let mut quality = QualitySettings::default();
    match preset {
        "low" => quality.set_preset(QualityPreset::Low),
        "medium" => quality.set_preset(QualityPreset::Medium),
        "high" => quality.set_preset(QualityPreset::High),
        "off" => {
            quality.set_preset(QualityPreset::Low);
            quality.preset = QualityPreset::Custom;
            quality.rain = crate::settings::RainAmount::Off;
            quality.fog = false;
        }
        _ => return Err(format!("Unsupported graphics preset: {preset}")),
    }
    Ok(quality)
}

fn feedback_smoke_mode(effect: &str, preset: Option<&str>) -> Result<Smoke, String> {
    let effect = match effect {
        "local" => FeedbackSmoke::Local,
        "free" => FeedbackSmoke::Free,
        "anchor" => FeedbackSmoke::Anchor,
        "anchor-good" => FeedbackSmoke::AnchorGood,
        "miss" => FeedbackSmoke::Miss,
        "approach" => FeedbackSmoke::Approach,
        _ => return Err(format!("Unsupported feedback sample: {effect}")),
    };
    Ok(Smoke::Feedback(
        effect,
        preset.map(smoke_quality).transpose()?,
    ))
}

fn apply_smoke_visuals(visual: &mut VisualState, mode: Smoke) {
    if let Smoke::Feedback(effect, _) = mode {
        reset_feedback(visual);
        visual.next_anchor_seconds = None;
        match effect {
            FeedbackSmoke::Local => visual.hit_pulses = [0.8, 0.6],
            FeedbackSmoke::Free => visual.free_sync_pulse = 0.7,
            FeedbackSmoke::Anchor | FeedbackSmoke::AnchorGood => {
                visual.anchor_sync_pulse = 0.7;
                visual.anchor_sync_precise = effect == FeedbackSmoke::Anchor;
            }
            FeedbackSmoke::Miss => visual.miss_pulses = [0.8, 0.0],
            FeedbackSmoke::Approach => visual.next_anchor_seconds = Some(34.0),
        }
    }
    if let Smoke::Quality(quality) | Smoke::Feedback(_, Some(quality)) = mode {
        visual.quality = quality;
    }
}

fn visual_smoke(path: PathBuf, mode: Smoke) -> Result<(), String> {
    visual_smoke_at(path, mode, SmokeViewport::default())
}

fn smoke_layout_metrics(
    viewport: SmokeViewport,
    camera: &Camera,
    (panel, panel_transform): (&ComputedNode, &UiGlobalTransform),
    (selected, row, row_transform): (usize, &ComputedNode, &UiGlobalTransform),
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
            "Selected menu row {selected} is not accessible: panel={panel_min:?}..{panel_max:?}, row={row_min:?}..{row_max:?}"
        ));
    }
    Ok(serde_json::json!({
        "physical_size": viewport.size,
        "scale_factor": viewport.scale,
        "logical_size": [logical.x, logical.y],
        "panel_physical": [panel_min.x, panel_min.y, panel_max.x, panel_max.y],
        "panel_inside_viewport": panel_min.cmpge(Vec2::ZERO).all()
            && panel_max.cmple(Vec2::new(viewport.size[0] as f32, viewport.size[1] as f32)).all(),
        "selected_row": selected,
        "row_physical": [row_min.x, row_min.y, row_max.x, row_max.y],
        "oversized_row": oversized,
    }))
}

const FEEDBACK_MOTION_FRAMES: u32 = 240;
const FEEDBACK_MOTION_ANCHORS: [u32; 4] = [34, 35, 36, 40];

fn feedback_motion_engine() -> Result<cocobeat_core::DuoEngine, String> {
    use cocobeat_schema::{Anchor, SongTime};

    cocobeat_core::DuoEngine::new(
        SessionEpoch(1),
        FEEDBACK_MOTION_ANCHORS
            .into_iter()
            .enumerate()
            .map(|(id, seconds)| Anchor {
                id: id as u64,
                song_time: SongTime::from_frames(i64::from(seconds) * 48_000),
            })
            .collect(),
        DuoRules::default(),
    )
    .map_err(|error| error.to_string())
}

// Synthetic captured inputs exercise the production rules and feedback at 30 Hz
fn advance_feedback_motion(
    frame: u32,
    engine: &mut cocobeat_core::DuoEngine,
    visual: &mut VisualState,
) -> Result<Vec<DuoEvent>, String> {
    use cocobeat_schema::{DuoInput, Hit, PlayerId, SongTime};

    if frame == 0 {
        reset_feedback(visual);
    }
    decay_feedback(visual, 1.0 / 30.0);
    let song_time = SongTime::from_frames(32 * 48_000 + i64::from(frame) * 1_600);
    for (player, hit) in [
        (PlayerId::P1, matches!(frame, 9 | 30 | 62 | 90)),
        (PlayerId::P2, matches!(frame, 31 | 62 | 90)),
    ] {
        if hit {
            engine
                .ingest(DuoInput::Hit(Hit {
                    epoch: SessionEpoch(1),
                    player,
                    seq: u64::from(frame),
                    song_time,
                }))
                .map_err(|error| error.to_string())?;
            visual.hit_pulses[player.index()] = 1.0;
            eprintln!(
                "FEEDBACK_INPUT {}",
                serde_json::json!({"frame": frame, "player": player.index(), "song_frames": song_time.frames()})
            );
        }
    }
    let mut events = Vec::new();
    for player in [PlayerId::P1, PlayerId::P2] {
        events.extend(
            engine
                .ingest(DuoInput::Watermark {
                    epoch: SessionEpoch(1),
                    player,
                    through: song_time,
                })
                .map_err(|error| error.to_string())?,
        );
    }
    for event in &events {
        visual_feedback(*event, visual);
    }
    visual.song_seconds = song_time.as_seconds_f64();
    visual.duration_seconds = f64::from(dev_song::FRAMES) / f64::from(dev_song::SAMPLE_RATE);
    visual.next_anchor_seconds = FEEDBACK_MOTION_ANCHORS
        .into_iter()
        .map(f64::from)
        .find(|at| *at >= visual.song_seconds);
    visual.resonance = f32::from(engine.resonance().level_per_mille) / 1_000.0;
    visual.status = "FEEDBACK MOTION | captured inputs, confirmed core events\nAudio, physical input and frame-rate acceptance NOT RUN".into();
    Ok(events)
}

fn visual_smoke_at(path: PathBuf, mode: Smoke, viewport: SmokeViewport) -> Result<(), String> {
    visual_smoke_for_content(path, mode, viewport, SongContent::development())
}

fn visual_smoke_for_content(
    path: PathBuf,
    mode: Smoke,
    viewport: SmokeViewport,
    content: SongContent,
) -> Result<(), String> {
    #[derive(Resource, Default)]
    struct SavedFrames(u32);

    let motion = mode == Smoke::FeedbackMotion;
    let mut motion_engine = if motion {
        std::fs::create_dir(&path)
            .map_err(|error| format!("Motion output must be a new directory: {error}"))?;
        Some(feedback_motion_engine()?)
    } else {
        None
    };
    let startup = !matches!(
        mode,
        Smoke::Scene
            | Smoke::Section(..)
            | Smoke::Feedback(..)
            | Smoke::FeedbackMotion
            | Smoke::Quality(_)
    );
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
                    | Smoke::Menu(_, _)
                    | Smoke::Players(_)
                    | Smoke::Graphics(_)
                    | Smoke::Pacing(_)
                    | Smoke::SettingsFault(_)
            ) {
                0.1
            } else if motion {
                1.0 / 30.0
            } else {
                1.0 / 60.0
            },
        ),
    ));
    ui_assets::install(&mut app)?;
    view::install(&mut app);
    app.init_resource::<InputState>()
        .init_resource::<SavedFrames>();
    display::install(&mut app, default());
    app.world_mut()
        .resource_mut::<DisplayState>()
        .set_headless_surface(viewport.size);
    if matches!(
        mode,
        Smoke::Settings
            | Smoke::SettingsFault(_)
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
                | Smoke::SettingsFault(locale)
                | Smoke::Languages(locale)
                | Smoke::Graphics(locale)
                | Smoke::Pacing(locale) => locale,
                _ => Locale::EnUs,
            },
            ..default()
        };
        menu.sync_pacing(app.world().resource::<DisplayState>());
        menu.begin(app.world().resource::<DisplayState>());
        if matches!(mode, Smoke::SettingsFault(_)) {
            let mut display = app.world_mut().resource_mut::<DisplayState>();
            menu.handle(SettingsAction::Down, 0.0, &mut display);
            menu.handle(SettingsAction::Confirm, 0.0, &mut display);
            menu.handle(SettingsAction::Down, 0.0, &mut display);
            menu.handle(SettingsAction::Confirm, 1.0, &mut display);
        }
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
        app.world_mut()
            .resource_mut::<InputState>()
            .claim_menu(InputSource::Keyboard);
        app.insert_resource(menu).add_systems(
            Update,
            (|settings: Res<SettingsMenu>,
              display: Res<DisplayState>,
              mut input: ResMut<InputState>,
              game: Res<Game>,
              mut visual: ResMut<VisualState>| {
                visual.locale = settings.values.locale;
                visual.menu = runtime_menu(&game, &mut input, &settings, 1.0, &display);
            })
            .after(DisplaySystems::Sync),
        );
    }
    if startup {
        brand_intro::install(&mut app);
        let mut game = Game::with_content(content.clone())?;
        if matches!(mode, Smoke::SettingsFault(_)) {
            game.phase = Phase::Fault;
            game.notice = Message::new("game.audio_failed");
            game.fault_details = Some(
                "Audio backend disconnected during display preview\nThe original playback error remains available while display settings are restored".into(),
            );
            game.replay_status = Message::with(
                "results.replay_failed",
                [(
                    "error",
                    "Permission denied: replay directory is not writable".into(),
                )],
            );
        }
        if let Smoke::Menu(_, phase) = mode {
            game.phase = phase;
            game.notice = Message::new(match phase {
                Phase::Starting => "game.waiting_audio",
                Phase::Pausing | Phase::Paused => "game.listening",
                Phase::Finished => "",
                Phase::Fault => "game.audio_failed",
                _ => "game.welcome",
            });
            if phase == Phase::Finished {
                game.session.finish()?;
                game.results = Some(game.session.summary());
            }
            if phase == Phase::Fault {
                game.fault_details = Some("Smoke fixture: audio output became unavailable".into());
            }
            game.session.current = cocobeat_schema::SongTime::from_frames(match phase {
                Phase::Pausing | Phase::Paused | Phase::Fault => content.end.frames() / 2,
                Phase::Finished => content.end.frames(),
                _ => 0,
            });
        }
        app.init_resource::<InputState>()
            .insert_resource(game)
            .add_systems(Update, suspend_intro.before(BrandIntroSystems::Advance));
    }
    if let Smoke::Menu(locale, _) | Smoke::Players(locale) = mode {
        let mut settings = SettingsMenu::default();
        settings.values.locale = locale;
        let mut input = InputState::default();
        let mut scroll = MenuScroll::default();
        if matches!(mode, Smoke::Players(_)) {
            input.claim_menu(InputSource::Keyboard);
            input.navigate_menu(SettingsAction::Down, &mut scroll);
            input.activate(None, 0, &mut scroll);
        }
        let presentation =
            game_menu(app.world().resource::<Game>(), &mut input, &settings).unwrap();
        if let Some(selection) = viewport.selection {
            if selection >= presentation.rows.len() {
                return Err(format!(
                    "Smoke row index {selection} is outside 0..{}",
                    presentation.rows.len()
                ));
            }
            for _ in 0..selection {
                input.navigate_menu(SettingsAction::Down, &mut scroll);
            }
        }
        app.insert_resource(settings)
            .insert_resource(input)
            .insert_resource(scroll)
            .add_systems(
                Update,
                |game: Res<Game>,
                 mut input: ResMut<InputState>,
                 settings: Res<SettingsMenu>,
                 mut visual: ResMut<VisualState>| {
                    visual.locale = settings.values.locale;
                    visual.song_seconds = game.session.current.as_seconds_f64();
                    visual.duration_seconds = game.content.end.as_seconds_f64();
                    visual.transitioning = matches!(game.phase, Phase::Starting | Phase::Pausing);
                    visual.menu = game_menu(&game, &mut input, &settings);
                },
            );
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
            duration_seconds: content.end.as_seconds_f64(),
            status:
                "Ready | native startup and menu eye loop\nAudio and physical input acceptance NOT RUN"
                    .into(),
            ..default()
        }
    } else {
        VisualState {
            song_seconds: SongTime::from_frames(content.end.frames() / 2).as_seconds_f64(),
            duration_seconds: content.end.as_seconds_f64(),
            hit_pulses: [0.8, 0.6],
            anchor_sync_pulse: 0.8,
            anchor_sync_precise: true,
            next_anchor_seconds: content
                .anchors
                .iter()
                .find(|anchor| anchor.song_time.frames() >= content.end.frames() / 2)
                .map(|anchor| anchor.song_time.as_seconds_f64()),
            resonance: 0.7,
            status:
                "VISUAL SMOKE | deterministic preview\nAudio, input and hardware acceptance NOT RUN"
                    .into(),
            running: false,
            ..default()
        }
    };
    apply_smoke_visuals(&mut app.world_mut().resource_mut::<VisualState>(), mode);
    if matches!(mode, Smoke::Scene | Smoke::Section(..)) {
        let mut visual = app.world_mut().resource_mut::<VisualState>();
        let time = if let Smoke::Section(locale, time, quality) = mode {
            visual.locale = locale;
            visual.quality = quality;
            visual.song_seconds = time.as_seconds_f64();
            visual.next_anchor_seconds = content
                .anchors
                .iter()
                .find(|anchor| anchor.song_time >= time)
                .map(|anchor| anchor.song_time.as_seconds_f64());
            reset_feedback(&mut visual);
            time
        } else {
            SongTime::from_frames(content.end.frames() / 2)
        };
        update_section_visuals(&content, time, true, &mut visual);
    }
    if matches!(mode, Smoke::Quality(_) | Smoke::Feedback(_, Some(_))) {
        app.world_mut()
            .resource_mut::<DisplayState>()
            .request(DisplaySettings {
                fullscreen: true,
                fullscreen_size: if viewport.size[0] < 640 || viewport.size[1] < 480 {
                    [320, 240]
                } else {
                    [640, 480]
                },
                ..default()
            });
    }
    app.add_systems(
        Update,
        (move |mut commands: Commands,
               mut frame: Local<u32>,
               mut completed_frames: Local<u32>,
               mut requested: Local<bool>,
               intro: Option<Res<BrandIntroStatus>>,
               mut visual: ResMut<VisualState>,
               cameras: Query<&Camera, With<PresentationCamera>>,
               game_cameras: Query<&Camera, With<display::GameCamera>>,
               panels: Query<(&ComputedNode, &UiGlobalTransform), With<view::StatusPanel>>,
               rows: Query<(&view::MenuRowNode, &ComputedNode, &UiGlobalTransform)>,
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
                let complete = intro.as_ref().is_some_and(|status| status.is_complete());
                if complete {
                    *completed_frames += 1;
                } else {
                    *completed_frames = 0;
                }
                if matches!(mode, Smoke::SettingsFault(_))
                    || matches!(mode, Smoke::Menu(_, phase) if phase != Phase::Ready)
                {
                    *completed_frames >= 3
                } else {
                    complete
                        && intro
                            .as_ref()
                            .is_some_and(|status| status.idle_seconds >= 6.1)
                }
            } else if motion {
                (30..30 + FEEDBACK_MOTION_FRAMES).contains(&*frame)
            } else {
                *frame == 30
            };
            if !*requested && ready {
                if matches!(mode, Smoke::Scene | Smoke::Section(..)) {
                    eprintln!(
                        "CONTENT_SAMPLE {}",
                        serde_json::json!({
                            "content_id": content.content_id,
                            "duration_frames": content.end.frames(),
                            "duration_seconds": visual.duration_seconds,
                            "song_seconds": visual.song_seconds,
                            "next_anchor_seconds": visual.next_anchor_seconds,
                            "next_section_seconds": visual.next_section_seconds,
                            "section_hint": visual.section_hint,
                            "locale": visual.locale.code(),
                            "quality": visual.quality,
                        })
                    );
                }
                if let Smoke::Feedback(effect, _) = mode {
                    eprintln!(
                        "FEEDBACK_SAMPLE {}",
                        serde_json::json!({
                            "effect": format!("{effect:?}"),
                            "quality": visual.quality,
                            "game_physical_size": game_cameras.single().ok()
                                .and_then(Camera::physical_viewport_size).map(|size| size.to_array()),
                            "ui_physical_size": cameras.single().ok()
                                .and_then(Camera::physical_viewport_size).map(|size| size.to_array()),
                            "scale_factor": viewport.scale,
                            "song_seconds": visual.song_seconds,
                            "hit_pulses": visual.hit_pulses,
                            "free_sync_pulse": visual.free_sync_pulse,
                            "anchor_sync_pulse": visual.anchor_sync_pulse,
                            "anchor_sync_precise": visual.anchor_sync_precise,
                            "miss_pulses": visual.miss_pulses,
                            "next_anchor_seconds": visual.next_anchor_seconds,
                        })
                    );
                }
                if let Some(engine) = &mut motion_engine {
                    match advance_feedback_motion(*frame - 30, engine, &mut visual) {
                        Ok(events) => eprintln!(
                            "FEEDBACK_FRAME {}",
                            serde_json::json!({
                                "frame": *frame - 30,
                                "song_seconds": visual.song_seconds,
                                "watermark_frames": 32 * 48_000 + u64::from(*frame - 30) * 1_600,
                                "events": events.iter().map(|event| format!("{event:?}")).collect::<Vec<_>>(),
                                "hit_pulses": visual.hit_pulses,
                                "free_sync_pulse": visual.free_sync_pulse,
                                "anchor_sync_pulse": visual.anchor_sync_pulse,
                                "anchor_sync_precise": visual.anchor_sync_precise,
                                "miss_pulses": visual.miss_pulses,
                                "next_anchor_seconds": visual.next_anchor_seconds,
                                "resonance": visual.resonance,
                            })
                        ),
                        Err(error) => {
                            eprintln!("Motion feedback failed: {error}");
                            exit.write(AppExit::error());
                            return;
                        }
                    }
                }
                if !matches!(
                    mode,
                    Smoke::Scene
                        | Smoke::Section(..)
                        | Smoke::Feedback(..)
                        | Smoke::FeedbackMotion
                        | Smoke::Startup
                        | Smoke::Quality(_)
                ) && visual.menu.is_none()
                {
                    eprintln!("Expected menu presentation is missing");
                    exit.write(AppExit::Error(std::num::NonZeroU8::new(1).unwrap()));
                    return;
                }
                if let Some(menu) = &visual.menu {
                    let metrics = (|| {
                        let selected = menu
                            .rows
                            .iter()
                            .position(|row| row.selected)
                            .ok_or("Missing selected menu row")?;
                        let (_, row, transform) = rows
                            .iter()
                            .find(|(row, _, _)| row.0 == selected)
                            .ok_or("Selected menu row was not laid out")?;
                        let mut metrics = smoke_layout_metrics(
                            viewport,
                            cameras.single().map_err(|error| error.to_string())?,
                            panels.single().map_err(|error| error.to_string())?,
                            (selected, row, transform),
                        )?;
                        metrics["menu_row_count"] = serde_json::json!(menu.rows.len());
                        metrics["menu_title"] = serde_json::json!(menu.title);
                        metrics["menu_rows"] = serde_json::json!(
                            menu.rows.iter().map(|row| &row.text).collect::<Vec<_>>()
                        );
                        Ok::<_, String>(metrics)
                    })();
                    match metrics {
                        Ok(metrics) => eprintln!("VIEWPORT_GEOMETRY {metrics}"),
                        Err(error) => {
                            eprintln!("Menu layout failed: {error}");
                            exit.write(AppExit::Error(std::num::NonZeroU8::new(1).unwrap()));
                            return;
                        }
                    }
                }
                *requested = !motion;
                let output = if motion {
                    path.join(format!("frame_{:04}.png", *frame - 30))
                } else {
                    path.clone()
                };
                commands
                    .spawn(Screenshot(RenderTarget::Image(ImageRenderTarget {
                        handle: target.clone(),
                        scale_factor: viewport.scale,
                    })))
                    .observe(
                        move |capture: On<ScreenshotCaptured>,
                              mut saved: ResMut<SavedFrames>,
                              mut exit: MessageWriter<AppExit>| {
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
                                    saved.0 += 1;
                                    if !motion || saved.0 == FEEDBACK_MOTION_FRAMES {
                                        exit.write(AppExit::Success);
                                    }
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
    use crate::content::CONTENT_ID;
    use bevy::{
        input::{
            gamepad::{GamepadConnectionEvent, GamepadEvent},
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
    fn section_presentation_follows_the_song_and_clears_on_menus_or_inactive_phases() {
        let mut content = SongContent::development();
        content.sections = [
            (4, 48_000, "\n 雨\t夜 {label}\r\u{0} "),
            (7, 96_000, " \t\n "),
        ]
        .into_iter()
        .map(|(id, frame, label)| cocobeat_schema::SectionCue {
            id,
            time: SongTime::from_frames(frame),
            label: label.into(),
        })
        .collect();
        let original = content.sections.clone();
        let mut visual = VisualState {
            locale: Locale::EnUs,
            ..default()
        };
        update_section_visuals(&content, SongTime::ZERO, true, &mut visual);
        assert!(
            visual
                .section_hint
                .as_ref()
                .unwrap()
                .contains("#4 · 雨 夜 {label}")
        );
        assert!(
            !visual
                .section_hint
                .as_ref()
                .unwrap()
                .chars()
                .any(char::is_control)
        );
        assert_eq!(visual.next_section_seconds, Some(1.0));

        // A pause menu hides auxiliary text while the cue geometry keeps the same song position
        let mut game = Game::with_content(content.clone()).unwrap();
        game.phase = Phase::Paused;
        let mut input = InputState::default();
        input.claim_menu(InputSource::Keyboard);
        visual.menu = game_menu(&game, &mut input, &SettingsMenu::default());
        assert!(visual.menu.is_some());
        update_section_visuals(&content, SongTime::ZERO, true, &mut visual);
        assert!(visual.section_hint.is_none());
        assert_eq!(visual.next_section_seconds, Some(1.0));
        visual.menu = None;
        update_section_visuals(&content, SongTime::from_frames(48_000), true, &mut visual);
        assert!(visual.section_hint.as_ref().unwrap().ends_with("#7"));
        assert_eq!(visual.next_section_seconds, Some(2.0));
        update_section_visuals(&content, SongTime::from_frames(96_000), true, &mut visual);
        assert_eq!(
            visual.section_hint,
            Some(
                Message::with("hud.section_recent", [("label", "#7".into())]).render(Locale::EnUs)
            )
        );
        assert_eq!(visual.next_section_seconds, None);
        for (time, active) in [(content.end, true), (SongTime::ZERO, false)] {
            update_section_visuals(&content, time, active, &mut visual);
            assert!(visual.section_hint.is_none());
            assert_eq!(visual.next_section_seconds, None);
        }
        update_section_visuals(&content, SongTime::ZERO, true, &mut visual);
        assert_eq!(visual.next_section_seconds, Some(1.0));
        assert_eq!(content.sections, original);
    }

    #[test]
    fn returning_to_ready_keeps_the_selected_content_and_anchor_hints() {
        for frames in [4_800, 3_120_017] {
            let content = SongContent {
                content_id: format!("test-package-{frames}"),
                end: SongTime::from_frames(frames),
                anchors: vec![cocobeat_schema::Anchor {
                    id: 71,
                    song_time: SongTime::from_frames(frames - 1),
                }],
                sections: vec![],
            };
            let mut game = Game::with_content(content.clone()).unwrap();
            assert_eq!(game.phase, Phase::Ready);
            assert_eq!(game.next_anchor(), Some(content.anchors[0].song_time));
            game.session = Session::for_content(SessionEpoch(9), &content).unwrap();
            game.session.current = content.end;
            game.phase = Phase::Finished;
            assert_eq!(game.next_anchor(), None);
            game.main_menu().unwrap();
            assert_eq!(game.phase, Phase::Ready);
            assert_eq!(game.session.epoch(), SessionEpoch(9));
            assert_eq!(game.session.current, SongTime::ZERO);
            assert_eq!(game.next_anchor(), Some(content.anchors[0].song_time));
            game.session.finish().unwrap();
            assert_eq!(game.session.current, content.end);
            assert_eq!(game.session.engine.events().len(), 2);
            let replayed = game
                .session
                .replay
                .replay(
                    &content.content_id,
                    RULES_ID,
                    content.anchors,
                    DuoRules::default(),
                )
                .unwrap();
            assert_eq!(replayed.events(), game.session.engine.events());
        }
    }

    #[test]
    fn motion_preview_uses_real_confirmations_and_equal_grade_lifetimes() {
        let mut engine = feedback_motion_engine().unwrap();
        let mut visual = VisualState::default();
        let mut confirmed = Vec::new();
        let mut samples = Vec::new();
        for frame in 0..FEEDBACK_MOTION_FRAMES {
            let events = advance_feedback_motion(frame, &mut engine, &mut visual).unwrap();
            if !events.is_empty() {
                confirmed.push((frame, events));
            }
            samples.push((
                visual.hit_pulses,
                visual.anchor_sync_pulse,
                visual.anchor_sync_precise,
            ));
        }
        assert_eq!(
            confirmed
                .iter()
                .map(|(frame, _)| *frame)
                .collect::<Vec<_>>(),
            [44, 66, 96, 126]
        );
        assert!(matches!(confirmed[0].1.as_slice(), [DuoEvent::FreeSync(_)]));
        for (index, grade) in [(1, AnchorGrade::Good), (2, AnchorGrade::Precise)] {
            let [
                DuoEvent::AnchorJudged(p1),
                DuoEvent::AnchorJudged(p2),
                DuoEvent::AnchorSync(sync),
            ] = confirmed[index].1.as_slice()
            else {
                panic!("Both individual judgements must precede confirmed AnchorSync");
            };
            assert_eq!((p1.grade, p2.grade), (grade, grade));
            assert_eq!((sync.p1.grade, sync.p2.grade), (grade, grade));
        }
        assert!(confirmed[3].1.iter().all(|event| matches!(
            event,
            DuoEvent::AnchorJudged(judgement) if judgement.grade == AnchorGrade::Miss
        )));
        assert_eq!(samples[9].0, [1.0, 0.0]);
        assert_eq!(samples[62].0, [1.0; 2]);
        assert_eq!(samples[90].0, [1.0; 2]);
        assert!(!samples[66].2);
        assert!(samples[96].2);
        assert_eq!(samples[66].1, 1.0);
        for elapsed in 0..=22 {
            assert_eq!(samples[66 + elapsed].1, samples[96 + elapsed].1);
        }
        assert_eq!(samples[118].1, 0.0);
        assert_eq!(engine.events().len(), 9);
        assert_eq!(visual.next_anchor_seconds, Some(40.0));
        assert_eq!(visual.song_seconds, 32.0 + 239.0 / 30.0);
    }

    #[test]
    fn feedback_smoke_keeps_default_quality_and_accepts_explicit_presets() {
        use crate::settings::{AntiAliasing, RainAmount};

        let feedback = |visual: &VisualState| {
            (
                visual.hit_pulses,
                visual.free_sync_pulse,
                visual.anchor_sync_pulse,
                visual.anchor_sync_precise,
                visual.miss_pulses,
                visual.next_anchor_seconds,
                visual.song_seconds,
                visual.resonance,
            )
        };
        for effect in ["local", "free", "anchor", "anchor-good", "miss", "approach"] {
            let default_mode = feedback_smoke_mode(effect, None).unwrap();
            assert!(matches!(default_mode, Smoke::Feedback(_, None)));
            let mut visual = VisualState {
                song_seconds: 32.0,
                resonance: 0.7,
                ..default()
            };
            apply_smoke_visuals(&mut visual, default_mode);
            let expected = feedback(&visual);
            assert_eq!(visual.quality, QualitySettings::default());
            for preset in ["low", "medium", "high", "off"] {
                visual.hit_pulses = [1.0; 2];
                visual.free_sync_pulse = 1.0;
                visual.anchor_sync_pulse = 1.0;
                visual.miss_pulses = [1.0; 2];
                let mode = feedback_smoke_mode(effect, Some(preset)).unwrap();
                apply_smoke_visuals(&mut visual, mode);
                assert_eq!(visual.quality, smoke_quality(preset).unwrap());
                assert_eq!(feedback(&visual), expected);
            }
        }
        let Smoke::Feedback(FeedbackSmoke::AnchorGood, Some(off)) =
            feedback_smoke_mode("anchor-good", Some("off")).unwrap()
        else {
            panic!("The selected effect must survive the quality override");
        };
        assert_eq!(off.antialiasing, AntiAliasing::Off);
        assert_eq!(off.rain, RainAmount::Off);
        assert!(!off.bloom && !off.fog && !off.shadows);
        assert!(feedback_smoke_mode("unknown", Some("off")).is_err());
        assert!(feedback_smoke_mode("free", Some("unknown")).is_err());
        assert!(smoke_quality("unknown").is_err());
    }

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
    fn game_menu_preserves_phase_notices_and_focuses_information_without_game_controls() {
        let mut settings = SettingsMenu::default();
        settings.values.locale = Locale::EnUs;
        settings.notice = Message::new("settings_notice.save_failed");
        let mut game = Game::new().unwrap();
        game.notice = Message::new("game.audio_failed");
        let mut titles = std::collections::HashSet::new();
        for phase in [Phase::Ready, Phase::Paused, Phase::Finished, Phase::Fault] {
            game.phase = phase;
            let mut input = InputState::default();
            let mut scroll = MenuScroll::default();
            let menu = game_menu(&game, &mut input, &settings).unwrap();
            if phase == Phase::Fault {
                assert!(menu.title.contains(&game.notice.render(Locale::EnUs)));
            }
            assert!(titles.insert(menu.title));
            for notice in [
                game.notice.render(Locale::EnUs),
                settings.notice.render(Locale::EnUs),
            ] {
                assert!(menu.rows.iter().any(|row| row.text == notice));
            }
            let last = menu.rows.len() - 1;
            for _ in 0..last {
                input.navigate_menu(SettingsAction::Down, &mut scroll);
            }
            let menu = game_menu(&game, &mut input, &settings).unwrap();
            assert!(menu.rows[last].selected);
            assert!(menu.rows.iter().any(|row| row.text.contains("ms")));
            assert!(input.queued.is_empty());
            input.set_settings_open(true);
            assert!(game_menu(&game, &mut input, &settings).is_none());
            input.set_settings_open(false);
            input.set_menu_open(false);
            assert!(game_menu(&game, &mut input, &settings).is_none());
            let status = game_status(&game, &input, &settings);
            assert!(!status.contains("ms"));
            if phase == Phase::Fault {
                assert_eq!(status, game.notice.render(Locale::EnUs));
            } else {
                assert_eq!(status, next_anchor_label(&game, Locale::EnUs));
            }
            if phase == Phase::Ready {
                input.status =
                    Message::with("input.controller_disconnected", [("player", "P1".into())]);
                game.notice = Message::with("game.saved", [("path", "replays/test.json".into())]);
                let status = game_status(&game, &input, &settings);
                assert!(status.contains(&input.status.render(Locale::EnUs)));
                assert!(status.contains("replays/test.json"));
                assert!(!status.contains("ms"));
                game.notice = Message::new("game.audio_failed");
            }
        }
    }

    #[test]
    fn terminal_transition_waits_for_replay_storage_without_replacing_results_or_fault() {
        for phase in [Phase::Finished, Phase::Fault] {
            for control in [Control::Restart, Control::MainMenu] {
                let mut game = Game::new().unwrap();
                game.session.finish().unwrap();
                game.results = Some(game.session.summary());
                game.phase = phase;
                game.notice = Message::new("game.audio_failed");
                game.fault_details = Some("original backend failure".into());
                let facts = game.session.replay.facts().to_vec();
                let summary = game.session.summary();
                let directory = std::env::temp_dir().join(format!(
                    "cocobeat-terminal-save-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                std::fs::write(&directory, b"blocks replay directory").unwrap();
                assert!(
                    game.save_before_terminal_transition(control, &directory)
                        .is_err()
                );
                assert_eq!(game.phase, phase);
                assert_eq!(game.session.replay.facts(), facts);
                assert_eq!(game.results.as_ref(), Some(&summary));
                assert_eq!(game.notice.key, "game.audio_failed");
                assert_eq!(
                    game.fault_details.as_deref(),
                    Some("original backend failure")
                );
                assert_eq!(game.replay_status.key, "results.replay_failed");
                assert_eq!(game.saved_facts, 0);
                std::fs::remove_file(&directory).unwrap();
                game.save_before_terminal_transition(control, &directory)
                    .unwrap();
                assert_eq!(game.saved_facts, facts.len());
                assert_eq!(game.phase, phase);
                assert_eq!(game.session.replay.facts(), facts);
                assert_eq!(game.results.as_ref(), Some(&summary));
                if phase == Phase::Fault {
                    assert_eq!(game.notice.key, "game.audio_failed");
                    assert_eq!(
                        game.fault_details.as_deref(),
                        Some("original backend failure")
                    );
                }
                // The real MainMenu transition now sees saved history and makes no second write
                if control == Control::MainMenu {
                    game.main_menu().unwrap();
                    assert_eq!(game.phase, Phase::Ready);
                    assert!(game.results.is_none());
                    assert!(game.session.replay.facts().is_empty());
                }
                std::fs::remove_dir_all(directory).unwrap();
            }
        }
    }

    #[test]
    fn replay_retry_preserves_stopped_cause_on_failure_success_and_empty_history() {
        let mut game = Game::new().unwrap();
        game.phase = Phase::Fault;
        game.notice = Message::new("game.audio_failed");
        game.fault_details = Some("original audio backend failure".into());
        let directory = std::env::temp_dir().join(format!(
            "cocobeat-results-retry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        game.save_to(&directory).unwrap();
        assert!(!directory.exists());
        assert_eq!(game.notice.key, "game.audio_failed");
        game.session.finish().unwrap();
        std::fs::write(&directory, b"blocks replay directory").unwrap();
        assert!(game.save_to(&directory).is_err());
        assert_eq!(game.replay_status.key, "results.replay_failed");
        assert_eq!(game.saved_facts, 0);
        assert_eq!(game.phase, Phase::Fault);
        assert_eq!(game.notice.key, "game.audio_failed");
        assert_eq!(
            game.fault_details.as_deref(),
            Some("original audio backend failure")
        );
        std::fs::remove_file(&directory).unwrap();
        game.save_to(&directory).unwrap();
        assert_eq!(game.saved_facts, game.session.replay.facts().len());
        assert_eq!(game.replay_status.key, "results.replay_saved");
        assert_eq!(game.notice.key, "game.audio_failed");
        assert_eq!(
            game.fault_details.as_deref(),
            Some("original audio backend failure")
        );
        let json = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .unwrap();
        let saved = Replay::decode(std::fs::read(json).unwrap().as_slice()).unwrap();
        assert_eq!(saved.facts(), game.session.replay.facts());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn finished_results_and_settings_fault_keep_truth_and_display_rollback() {
        let mut game = Game::new().unwrap();
        game.session.finish().unwrap();
        game.results = Some(game.session.summary());
        game.phase = Phase::Finished;
        let mut settings = SettingsMenu::default();
        settings.values.locale = Locale::EnUs;
        let mut input = InputState::default();
        let menu = game_menu(&game, &mut input, &settings).unwrap();
        assert_eq!(menu.rows[0].text, "Restart song");
        assert!(menu.rows.iter().any(|row| row.text == "Miss · P1 7 / P2 7"));
        assert!(
            menu.rows
                .iter()
                .any(|row| row.text == "Recorded Hits · P1 0 / P2 0")
        );
        assert!(
            menu.rows
                .iter()
                .any(|row| row.text == "Replay has unsaved history")
        );
        game.saved_facts = game.session.replay.facts().len();
        game.replay_status = Message::with(
            "results.replay_saved",
            [("path", "replays/completed.json".into())],
        );
        assert!(
            !game_menu(&game, &mut input, &settings)
                .unwrap()
                .rows
                .iter()
                .any(|row| row.text == "Save replay")
        );

        let mut display = DisplayState::new(DisplaySettings::default());
        display.set_headless_surface([1920, 1080]);
        let original = display.actual();
        settings.begin(&display);
        settings.handle(SettingsAction::Down, 0.0, &mut display);
        settings.handle(SettingsAction::Confirm, 0.0, &mut display);
        settings.handle(SettingsAction::Down, 0.0, &mut display);
        settings.handle(SettingsAction::Confirm, 1.0, &mut display);
        assert_ne!(display.actual(), original);
        let preview = settings.presentation(1.0, &display, Locale::EnUs).unwrap();
        assert!(preview.title.contains("KEEP DISPLAY"));
        input.set_settings_open(true);
        game.phase = Phase::Fault;
        game.notice = Message::new("game.audio_failed");
        game.fault_details = Some("backend stopped\nlong diagnostic is preserved".into());
        game.replay_status = Message::with(
            "results.replay_failed",
            [("error", "permission denied".into())],
        );
        let fault = runtime_menu(&game, &mut input, &settings, 1.0, &display).unwrap();
        assert!(fault.title.starts_with("Stopped\nAudio failed"));
        assert!(fault.title.contains(&preview.title));
        assert_eq!(fault.rows.len(), preview.rows.len());
        assert!(
            fault
                .rows
                .iter()
                .any(|row| row.text.contains("long diagnostic is preserved")
                    && row.text.contains("permission denied"))
        );
        assert!(settings.is_open());
        assert!(!settings.tick(15.0, &mut display));
        assert!(settings.tick(16.0, &mut display));
        assert_eq!(display.actual(), original);
        assert!(settings.is_open());
        assert_eq!(game.phase, Phase::Fault);
    }

    #[test]
    fn real_anchor_grades_share_pulse_age_but_only_two_precise_hits_get_the_precise_style() {
        use cocobeat_core::DuoEngine;
        use cocobeat_schema::{Anchor, DuoInput, Hit, PlayerId};
        for offset in [0, 3_000] {
            let epoch = SessionEpoch(1);
            let mut engine = DuoEngine::new(
                epoch,
                vec![Anchor {
                    id: 1,
                    song_time: SongTime::from_frames(48_000),
                }],
                DuoRules::default(),
            )
            .unwrap();
            for player in [PlayerId::P1, PlayerId::P2] {
                engine
                    .ingest(DuoInput::Hit(Hit {
                        epoch,
                        player,
                        seq: 0,
                        song_time: SongTime::from_frames(48_000 + offset),
                    }))
                    .unwrap();
            }
            for player in [PlayerId::P1, PlayerId::P2] {
                engine
                    .ingest(DuoInput::Watermark {
                        epoch,
                        player,
                        through: SongTime::from_frames(96_000),
                    })
                    .unwrap();
            }
            let mut visual = VisualState::default();
            let sync = *engine
                .events()
                .iter()
                .find(|event| matches!(event, DuoEvent::AnchorSync(_)))
                .unwrap();
            assert!(visual_feedback(sync, &mut visual));
            assert_eq!(visual.anchor_sync_pulse, 1.0);
            assert_eq!(visual.anchor_sync_precise, offset == 0);
            reset_feedback(&mut visual);
            assert!(!visual.anchor_sync_precise);
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
        assert!(expected.events().is_empty());
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
                let mut visual = VisualState::default();
                let shared = engine
                    .events()
                    .iter()
                    .filter(|&&event| visual_feedback(event, &mut visual))
                    .count();
                assert_eq!(shared, 3);
                assert_eq!(visual.free_sync_pulse, 1.0);
                assert_eq!(visual.anchor_sync_pulse, 1.0);
                assert_eq!(visual.miss_pulses, [1.0; 2]);
                assert_eq!(visual.hit_pulses, [0.0; 2]);
                reset_feedback(&mut visual);
                assert_eq!(visual.free_sync_pulse, 0.0);
                assert_eq!(visual.anchor_sync_pulse, 0.0);
                assert_eq!(visual.miss_pulses, [0.0; 2]);
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
            .add_message::<GamepadEvent>()
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
    fn playback_callbacks_gate_start_and_the_accepted_pause_owns_the_menu() {
        let mut audio = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: 48_000,
            },
            ..default()
        })
        .unwrap();
        let mut handle = audio
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

        let pad = World::new().spawn_empty().id();
        let mut input = InputState::default();
        input.claim_menu(InputSource::Keyboard);
        input.set_menu_open(false);
        assert!(
            game.request_pause(
                &mut input,
                Some(InputSource::Pad(pad)),
                MonotonicTime::from_nanos(20_000_000),
            )
            .unwrap()
        );
        let accepted_owner = input.menu_owner_hint(Locale::EnUs);
        assert!(input.menu_open);
        assert_eq!(game.phase, Phase::Pausing);
        // A second device in this batch cannot replace the accepted pausing device
        assert!(
            !game
                .request_pause(
                    &mut input,
                    Some(InputSource::Keyboard),
                    MonotonicTime::from_nanos(20_000_000),
                )
                .unwrap()
        );
        assert_eq!(input.menu_owner_hint(Locale::EnUs), accepted_owner);
        game.observe_playback(
            handle.position(),
            handle.state(),
            MonotonicTime::from_nanos(21_000_000),
        )
        .unwrap();
        assert_eq!(game.phase, Phase::Pausing);
        input.queued = [
            Control::Start,
            Control::TogglePause(InputSource::Keyboard),
            Control::Restart,
            Control::Settings(SettingsAction::Confirm),
            Control::FocusLost,
        ]
        .map(|control| input::CapturedControl {
            control,
            monotonic_ns: 21_000_000,
        })
        .into();
        game.filter_transition_controls(&mut input);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::FocusLost);
        handle.pause(kira::Tween {
            duration: std::time::Duration::ZERO,
            ..default()
        });
        audio.backend_mut().on_start_processing();
        audio.backend_mut().process();
        audio.backend_mut().on_start_processing();
        assert_eq!(handle.state(), PlaybackState::Paused);
        game.observe_playback(
            handle.position(),
            handle.state(),
            MonotonicTime::from_nanos(30_000_000),
        )
        .unwrap();
        assert_eq!(game.phase, Phase::Paused);
        assert_eq!(input.menu_owner_hint(Locale::EnUs), accepted_owner);

        // A resume callback cannot give stale menu confirmations a second meaning
        game.phase = Phase::Starting;
        input.queued = [
            Control::Start,
            Control::Restart,
            Control::MainMenu,
            Control::TogglePause(InputSource::Keyboard),
        ]
        .map(|control| input::CapturedControl {
            control,
            monotonic_ns: 40_000_000,
        })
        .into();
        game.filter_transition_controls(&mut input);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(
            input.queued[0].control,
            Control::TogglePause(InputSource::Keyboard)
        );
    }
}
