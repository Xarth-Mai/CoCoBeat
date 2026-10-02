use crate::{
    audio::AudioOutput,
    brand_intro::{
        self, BrandImpact, BrandIntroControl, BrandIntroPhase, BrandIntroStatus, BrandIntroSystems,
    },
    clock::MonotonicTime,
    dev_song,
    display::{self, DisplayState, DisplaySystems, PresentationCamera},
    input::{self, Control, InputState, SettingsAction},
    session::{CONTENT_ID, RULES_ID, Session},
    settings::{DisplaySettings, Settings},
    settings_menu::SettingsMenu,
    view::{self, VisualState},
};
use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    camera::RenderTarget,
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
}

#[derive(Resource)]
struct Game {
    session: Session,
    phase: Phase,
    notice: String,
    saved_facts: usize,
    transition_started: std::time::Instant,
}

impl Game {
    fn new() -> Result<Self, String> {
        Ok(Self {
            session: Session::new(SessionEpoch(0))?,
            phase: Phase::Ready,
            notice: "Play together; silence is welcome".into(),
            saved_facts: 0,
            transition_started: std::time::Instant::now(),
        })
    }

    fn save(&mut self) -> Result<(), String> {
        if self.session.replay.facts().is_empty() {
            self.notice = "No input history to save yet".into();
        } else if self.saved_facts != self.session.replay.facts().len() {
            let path = self.session.save(Path::new("replays"))?;
            self.saved_facts = self.session.replay.facts().len();
            self.notice = format!("Saved {}", path.display());
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
        self.notice = "Waiting for audio playback".into();
        Ok(())
    }

    fn main_menu(&mut self) -> Result<(), String> {
        self.save()?;
        // Keep the epoch until the next explicit Start advances it
        self.session = Session::new(self.session.epoch())?;
        self.saved_facts = 0;
        self.phase = Phase::Ready;
        self.notice = "Ready — confirm Start to play together".into();
        Ok(())
    }

    fn fault(&mut self, audio: &mut AudioOutput, error: String) {
        audio.stop();
        self.phase = Phase::Fault;
        let saved = self.save();
        self.notice = format!("Stopped: {error}. Restart explicitly to try again");
        if let Err(save_error) = saved {
            self.notice
                .push_str(&format!("; Replay save failed: {save_error}"));
        }
        eprintln!("{}", self.notice);
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
                self.notice = "Hear your partner; shared feedback follows confirmed history".into();
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
                "CoCoBeat: 64-second local duet\n  --replay FILE          validate a saved development-song replay\n  --visual-smoke PNG     render a deterministic scene without audio or gameplay\n  --startup-smoke PNG    render the native intro and Ready eye loop without audio\n  --settings-smoke PNG   render settings with a simulated low-resolution fullscreen scene\nEnter / controller South: menu confirmation; Esc / Start: pause\nReplays are saved locally in ./replays/"
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

fn base_app() -> App {
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
    view::install(&mut app);
    display::install(&mut app, settings.values.display);
    app.insert_resource(settings);
    app
}

fn run_game() -> Result<(), String> {
    let audio = AudioOutput::new()?;
    let mut app = base_app();
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
    (time, mut settings, mut display): (Res<Time>, ResMut<SettingsMenu>, ResMut<DisplayState>),
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
    settings.tick(settings_now, &mut display);
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
                game.notice = format!(
                    "Startup failed: {}",
                    brand.error.as_deref().unwrap_or("unknown error")
                );
                eprintln!("{}", game.notice);
            }
            brand.reveal_progress = 0.0;
            visual.status = format!("{}\nClose window to exit", game.notice);
        } else if brand.is_complete() {
            input.set_controls_enabled(true);
            input.set_menu_open(true);
            game.phase = Phase::Ready;
            visual.status = "Ready — confirm Start to play together".into();
        }
        // Completion only unlocks the next fresh press, never a queued startup control
        return;
    }
    let delta = time.delta_secs().min(1.0);
    for pulse in &mut visual.hit_pulses {
        *pulse = (*pulse - delta * 4.0).max(0.0);
    }
    visual.sync_pulse = (visual.sync_pulse - delta * 1.8).max(0.0);

    let result = (|| -> Result<(), String> {
        if let Some(error) = audio.take_error() {
            let _ = game.session.clock.device_lost(observed);
            return Err(error);
        }
        if matches!(
            game.phase,
            Phase::Starting | Phase::Running | Phase::Pausing
        ) && let (Some(position), Some(state)) = (audio.position(), audio.state())
            && game.observe_playback(position, state, observed)?
        {
            input.set_menu_open(false);
        }
        if matches!(game.phase, Phase::Starting | Phase::Pausing)
            && audio.state() != Some(PlaybackState::Stopped)
            && game.transition_started.elapsed() > std::time::Duration::from_secs(2)
        {
            return Err(
                "Audio callback did not acknowledge the state transition within 2 seconds".into(),
            );
        }

        for event in std::mem::take(&mut input.queued) {
            if let Control::Settings(action) = event.control {
                if action == SettingsAction::Open {
                    if input.menu_open && !matches!(game.phase, Phase::Running | Phase::Starting) {
                        settings.begin(&display);
                        input.set_settings_open(true);
                    } else {
                        input.set_settings_open(false);
                    }
                } else if settings.handle(action, settings_now, &mut display) {
                    input.set_settings_open(false);
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
                Control::SaveReplay => game.save()?,
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
        game.fault(&mut audio, error);
        input.set_menu_open(true);
    }
    visual.song_seconds = game.session.current.as_seconds_f64();
    visual.resonance = f32::from(game.session.engine.resonance().level_per_mille) / 1_000.0;
    visual.running = game.phase == Phase::Running;
    visual.settings_open = settings.is_open();
    if settings.is_open() {
        visual.status = settings.text(settings_now, &display);
        return;
    }
    let next = dev_song::ANCHOR_FRAMES
        .iter()
        .find(|&&frame| frame > game.session.current.frames())
        .map(|&frame| {
            format!(
                "next Anchor in {:.1}s",
                frame as f64 / 48_000.0 - visual.song_seconds
            )
        })
        .unwrap_or_else(|| "final release".into());
    visual.status = format!(
        "{:?} | {next}\n{}\n{}\n{}\n{}\n{}\nSoftware cursor estimate +/- {:.1}ms; hardware latency NOT MEASURED",
        game.phase,
        input.menu_text(),
        input.bindings_text(),
        input.status,
        game.notice,
        settings.notice,
        game.session.uncertainty_frames as f64 / 48.0
    );
}

fn visual_smoke(path: PathBuf, mode: Smoke) -> Result<(), String> {
    let startup = mode != Smoke::Scene;
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
        std::time::Duration::from_secs_f64(1.0 / 60.0),
    ));
    view::install(&mut app);
    display::install(&mut app, default());
    app.world_mut()
        .resource_mut::<DisplayState>()
        .set_headless_surface([1280, 800]);
    if mode == Smoke::Settings {
        let selected = DisplaySettings {
            fullscreen: true,
            fullscreen_size: [640, 480],
            ..default()
        };
        app.world_mut()
            .resource_mut::<DisplayState>()
            .request(selected);
        let mut menu = SettingsMenu::default();
        menu.values = Settings { display: selected };
        menu.begin(app.world().resource::<DisplayState>());
        app.insert_resource(menu).add_systems(
            Update,
            (|settings: Res<SettingsMenu>,
              display: Res<DisplayState>,
              mut visual: ResMut<VisualState>| {
                visual.settings_open = true;
                visual.status = settings.text(0.0, &display);
            })
            .after(DisplaySystems::Sync),
        );
    }
    if startup {
        brand_intro::install(&mut app);
        app.init_resource::<InputState>()
            .insert_resource(Game::new()?)
            .add_systems(Update, suspend_intro.before(BrandIntroSystems::Advance));
    }
    let mut image = Image::new_target_texture(1280, 800, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera_target = target.clone();
    app.add_systems(
        PostStartup,
        (move |mut commands: Commands, cameras: Query<Entity, With<PresentationCamera>>| {
            for camera in &cameras {
                commands
                    .entity(camera)
                    .insert(RenderTarget::Image(camera_target.clone().into()));
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
            settings_open: false,
        }
    };
    app.add_systems(
        Update,
        (move |mut commands: Commands,
               mut frame: Local<u32>,
               mut requested: Local<bool>,
               intro: Option<Res<BrandIntroStatus>>,
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
                *requested = true;
                let output = path.clone();
                commands.spawn(Screenshot::image(target.clone())).observe(
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
                                exit.write(AppExit::Error(std::num::NonZeroU8::new(1).unwrap()));
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
