use std::time::{Duration, Instant};

use bevy::{
    camera::{CameraUpdateSystems, RenderTarget},
    ecs::system::NonSendMarker,
    image::ImageSampler,
    prelude::*,
    render::{render_resource::TextureFormat, renderer::RenderDevice},
    window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode},
    winit::{WINIT_WINDOWS, WinitSettings},
};

use crate::{
    display_area::{AreaInfo, AreaReader, AreaSource, PhysicalRect},
    i18n::Message,
    settings::{DisplaySettings, FrameLimit, PacingSettings},
    settings_menu::SettingsMenu,
};

const PRESETS: [[u32; 2]; 6] = [
    [1280, 720],
    [1280, 800],
    [1600, 900],
    [1920, 1080],
    [2560, 1440],
    [3840, 2160],
];
const READBACK_WAIT: Duration = Duration::from_secs(2);

#[derive(Component)]
pub(crate) struct GameCamera;

#[derive(Component)]
pub(crate) struct PresentationCamera;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum DisplaySystems {
    Setup,
    Sync,
    Pace,
}

struct Pending {
    target: DisplaySettings,
    started: Duration,
    size_sent: bool,
    mode_change: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct WindowEnvironment {
    monitor: PhysicalRect,
    work_area: Option<PhysicalRect>,
    scale_factor: f64,
}

#[derive(Resource)]
pub(crate) struct DisplayState {
    pub physical_size: [u32; 2],
    pub native_size: Option<[u32; 2]>,
    pub window_managed: bool,
    pub pending: bool,
    pub notice: Message,
    actual: DisplaySettings,
    request: Option<DisplaySettings>,
    in_flight: Option<Pending>,
    window_limit: Option<[u32; 2]>,
    texture_limit: u32,
    headless_surface: Option<[u32; 2]>,
    refresh_limit: Option<u32>,
    has_requested: bool,
    environment: Option<WindowEnvironment>,
    fit_environment: bool,
    position_request: Option<[i32; 2]>,
}

impl DisplayState {
    pub(crate) fn new(initial: DisplaySettings) -> Self {
        Self {
            physical_size: [1280, 800],
            native_size: None,
            window_managed: false,
            pending: true,
            notice: Message::default(),
            actual: initial,
            request: Some(initial),
            in_flight: None,
            window_limit: None,
            texture_limit: 16384,
            headless_surface: None,
            refresh_limit: None,
            has_requested: false,
            environment: None,
            fit_environment: false,
            position_request: None,
        }
    }

    pub(crate) fn actual(&self) -> DisplaySettings {
        self.actual
    }

    pub(crate) fn request(&mut self, settings: DisplaySettings) {
        self.request = Some(settings);
        self.pending = true;
        self.notice = Message::default();
        self.apply_headless();
    }

    pub(crate) fn resolution_options(&self, fullscreen: bool) -> Vec<[u32; 2]> {
        let limit = if fullscreen {
            self.native_size
        } else {
            self.window_limit
        };
        let mut options = PRESETS.to_vec();
        options.extend(self.native_size);
        options.retain(|size| {
            size.iter().all(|&axis| axis <= self.texture_limit)
                && limit.is_none_or(|limit| fits(*size, limit))
        });
        options.sort_unstable();
        options.dedup();
        options
    }

    pub(crate) fn frame_rates(&self) -> Vec<FrameLimit> {
        let maximum = self.refresh_limit.unwrap_or(60_000);
        let mut rates: Vec<_> = (1..=maximum / 60_000)
            .map(|step| FrameLimit::Limited(step * 60_000))
            .collect();
        if rates.last() != Some(&FrameLimit::Limited(maximum)) {
            rates.push(FrameLimit::Limited(maximum));
        }
        rates.push(FrameLimit::Unlimited);
        rates
    }

    pub(crate) fn normalize_pacing(&self, pacing: &mut PacingSettings) {
        if let Some(maximum) = self.refresh_limit {
            pacing.frame_limit = match pacing.frame_limit {
                FrameLimit::Display => FrameLimit::Limited(maximum),
                FrameLimit::Limited(rate) => FrameLimit::Limited(rate.min(maximum)),
                FrameLimit::Unlimited => FrameLimit::Unlimited,
            };
        }
    }

    // Explicitly simulated output for the existing offscreen smoke, never native acceptance
    pub(crate) fn set_headless_surface(&mut self, size: [u32; 2]) {
        self.headless_surface = Some(size.map(|axis| axis.max(1)));
        self.refresh_limit = Some(60_000);
        self.apply_headless();
    }

    fn apply_headless(&mut self) {
        if let Some(size) = self.headless_surface {
            self.physical_size = size;
            self.native_size = Some(size);
            self.window_limit = Some(size);
            if let Some(request) = self.request.take() {
                self.actual = self.constrain(request);
                self.in_flight = None;
                self.pending = false;
                self.notice = Message::default();
            }
        }
    }

    fn constrain(&self, mut target: DisplaySettings) -> DisplaySettings {
        let gpu_limit = [self.texture_limit; 2];
        target.window_size = contain(target.window_size, self.window_limit.unwrap_or(gpu_limit));
        target.fullscreen_size = contain(
            target.fullscreen_size,
            self.native_size
                .unwrap_or(gpu_limit)
                .map(|axis| axis.min(self.texture_limit)),
        );
        target
    }

    fn observe(&mut self, observed: Observation, now: Duration) {
        let environment = observed.area.map(|area| WindowEnvironment {
            monitor: area.monitor,
            work_area: area.work_area,
            scale_factor: observed.scale_factor,
        });
        if environment != self.environment {
            self.environment = environment;
            self.fit_environment = environment.is_some();
        }
        let area_notice = match observed.area {
            Some(AreaInfo {
                work_area: Some(_),
                source: AreaSource::Windows,
                ..
            }) => "display.work_area_windows",
            Some(AreaInfo {
                work_area: Some(_),
                source: AreaSource::X11,
                ..
            }) => "display.work_area_x11",
            _ => "display.work_area_unavailable",
        };
        if matches!(
            self.notice.key,
            "" | "display.work_area_windows"
                | "display.work_area_x11"
                | "display.work_area_unavailable"
                | "display.managed"
        ) {
            self.notice = Message::new(area_notice);
        }
        self.physical_size = observed.size;
        self.native_size = observed.native;
        self.window_limit = observed.window_limit;
        self.refresh_limit = Some(observed.refresh_limit);
        self.window_managed = !observed.fullscreen && observed.managed;
        self.actual.fullscreen_size = self.constrain(self.actual).fullscreen_size;
        if let Some(pending) = &mut self.in_flight {
            // X11 can report the desired mode before the WM applies its geometry
            pending.mode_change |= observed.fullscreen != self.actual.fullscreen;
            if !pending.target.fullscreen
                && !observed.fullscreen
                && observed
                    .native
                    .is_some_and(|native| observed.size != native)
            {
                // Non-fullscreen geometry corroborates exit even if the WM adjusts its size
                self.actual.fullscreen = false;
                self.actual.window_size = observed.size;
                pending.mode_change = false;
            }
            let matched = observed.fullscreen == pending.target.fullscreen
                && if pending.target.fullscreen {
                    observed.native == Some(observed.size)
                } else {
                    pending.size_sent && observed.size == pending.target.window_size
                };
            let elapsed = now.saturating_sub(pending.started) >= READBACK_WAIT;
            let resize_declined = elapsed
                && !pending.mode_change
                && !pending.target.fullscreen
                && !observed.fullscreen
                && pending.size_sent;
            if matched || resize_declined {
                self.actual.fullscreen = observed.fullscreen;
                if !observed.fullscreen {
                    self.actual.window_size = observed.size;
                }
                self.notice = if matched {
                    Message::new(area_notice)
                } else {
                    Message::new("display.size_unconfirmed")
                };
                // Accepting an observed resize must leave no deferred adapter action
                self.in_flight = None;
                self.pending = false;
            } else {
                self.pending = true;
                if elapsed {
                    self.notice = Message::new("display.mode_pending");
                }
            }
        } else if !observed.fullscreen || observed.native == Some(observed.size) {
            self.actual.fullscreen = observed.fullscreen;
            if !observed.fullscreen {
                self.actual.window_size = observed.size;
            }
        }
        if self.window_managed {
            self.notice = Message::new("display.managed");
        }
        self.fit_changed_environment(&observed);
    }

    fn fit_changed_environment(&mut self, observed: &Observation) {
        if !self.fit_environment
            || observed.fullscreen
            || self.window_managed
            || self.in_flight.is_some()
            || self.request.is_some_and(|request| request.fullscreen)
        {
            return;
        }
        let Some(area) = observed.area else {
            return;
        };
        let target = self.constrain(self.request.unwrap_or(self.actual));
        if self.request.is_none() && target.window_size != observed.size {
            self.request = Some(target);
            self.pending = true;
        }
        if let Some(position) = observed.outer_position {
            let outer_size = [
                target.window_size[0].saturating_add(observed.decoration[0]),
                target.window_size[1].saturating_add(observed.decoration[1]),
            ];
            let contained =
                contain_position(position, outer_size, area.work_area.unwrap_or(area.monitor));
            if contained != position {
                self.position_request = Some(contained);
            }
        }
        // One attempt per monitor/work-area/DPI change, even if the WM declines it
        self.fit_environment = false;
    }

    fn window_size_request(
        &mut self,
        now: Duration,
        reported_fullscreen: bool,
    ) -> Option<[u32; 2]> {
        let pending = self.in_flight.as_mut()?;
        if pending.target.fullscreen || reported_fullscreen || pending.size_sent {
            return None;
        }
        // Wait for the native fullscreen exit before requesting the restored client size
        pending.size_sent = true;
        pending.started = now;
        self.pending = true;
        Some(pending.target.window_size)
    }
}

struct Observation {
    size: [u32; 2],
    native: Option<[u32; 2]>,
    window_limit: Option<[u32; 2]>,
    fullscreen: bool,
    managed: bool,
    refresh_limit: u32,
    area: Option<AreaInfo>,
    scale_factor: f64,
    outer_position: Option<[i32; 2]>,
    decoration: [u32; 2],
}

fn contain_position(position: [i32; 2], size: [u32; 2], area: PhysicalRect) -> [i32; 2] {
    let origins = [area.x, area.y];
    let extents = [area.width, area.height];
    std::array::from_fn(|axis| {
        let minimum = i64::from(origins[axis]);
        let maximum = minimum + i64::from(extents[axis].saturating_sub(size[axis]));
        i64::from(position[axis]).clamp(minimum, maximum) as i32
    })
}

fn refresh_limit(modes: impl IntoIterator<Item = u32>, current: Option<u32>) -> u32 {
    modes
        .into_iter()
        .filter(|&rate| rate >= 1_000)
        .max()
        .or(current.filter(|&rate| rate >= 1_000))
        .unwrap_or(60_000)
}

#[derive(Resource)]
struct FramePacing {
    limit: FrameLimit,
    last_start: Option<Instant>,
}

impl FramePacing {
    fn remaining(&self, now: Instant) -> Duration {
        let rate = match self.limit {
            FrameLimit::Display => 60_000,
            FrameLimit::Limited(rate) => rate,
            FrameLimit::Unlimited => return Duration::ZERO,
        };
        let period =
            Duration::from_nanos(1_000_000_000_000_u64.div_ceil(u64::from(rate.max(1_000))));
        self.last_start.map_or(Duration::ZERO, |last| {
            period.saturating_sub(now.saturating_duration_since(last))
        })
    }
}

// Production only: explicit offscreen smoke time is independent of wall-clock pacing
pub(crate) fn install_frame_pacing(app: &mut App, initial: PacingSettings) {
    app.insert_resource(FramePacing {
        limit: initial.frame_limit,
        last_start: None,
    })
    .insert_resource(WinitSettings::continuous())
    .add_systems(
        First,
        gate_frame
            .in_set(DisplaySystems::Pace)
            .before(bevy::time::TimeSystems),
    )
    .add_systems(PostUpdate, apply_pacing);
}

fn gate_frame(mut pacing: ResMut<FramePacing>) {
    let remaining = pacing.remaining(Instant::now());
    if !remaining.is_zero() {
        std::thread::sleep(remaining);
    }
    // Start anew after a slow frame; never catch up with a burst of shorter updates
    pacing.last_start = Some(Instant::now());
}

fn apply_pacing(
    settings: Res<SettingsMenu>,
    mut pacing: ResMut<FramePacing>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    pacing.limit = settings.values.pacing.frame_limit;
    let present_mode = if settings.values.pacing.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };
    for mut window in &mut windows {
        if window.present_mode != present_mode {
            window.present_mode = present_mode;
        }
    }
}

pub(crate) fn install(app: &mut App, initial: DisplaySettings) {
    app.insert_resource(DisplayState::new(initial))
        .add_systems(
            PostStartup,
            setup
                .in_set(DisplaySystems::Setup)
                .before(CameraUpdateSystems),
        )
        .add_systems(Update, sync.in_set(DisplaySystems::Sync))
        .add_systems(
            PostUpdate,
            update_render
                .before(CameraUpdateSystems)
                .before(bevy::ui::UiSystems::Prepare),
        );
}

fn sync(
    mut state: ResMut<DisplayState>,
    time: Res<Time<Real>>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    device: Option<Res<RenderDevice>>,
    mut areas: Local<AreaReader>,
    _main_thread: NonSendMarker,
) {
    if let Some(device) = device {
        state.texture_limit = device.limits().max_texture_dimension_2d;
    }
    let Ok((entity, mut window)) = windows.single_mut() else {
        state.apply_headless();
        return;
    };
    let now = time.elapsed();
    let observed = WINIT_WINDOWS.with_borrow(|windows| {
        let native = windows.get_window(entity)?;
        let inner = native.inner_size();
        // Zero extent while minimized is not a persistent resolution choice
        if inner.width == 0 || inner.height == 0 {
            return None;
        }
        let size = [inner.width, inner.height];
        let monitor = native.current_monitor().filter(|monitor| {
            let size = monitor.size();
            size.width > 0 && size.height > 0
        });
        let native_size = monitor.as_ref().map(|monitor| {
            let size = monitor.size();
            [size.width, size.height]
        });
        let outer = native.outer_size();
        let border = [
            outer.width.saturating_sub(inner.width),
            outer.height.saturating_sub(inner.height),
        ];
        let area = monitor
            .as_ref()
            .map(|monitor| areas.observe(native, monitor, now));
        Some(Observation {
            size,
            native: native_size,
            // Unknown work areas retain only the monitor/decorations upper bound
            window_limit: area.map(|area| {
                let available = area.work_area.unwrap_or(area.monitor);
                [
                    available.width.saturating_sub(border[0]).max(1),
                    available.height.saturating_sub(border[1]).max(1),
                ]
            }),
            fullscreen: native.fullscreen().is_some(),
            managed: native.is_maximized()
                || area.is_some_and(|area| area.resize_allowed == Some(false)),
            refresh_limit: monitor.as_ref().map_or(60_000, |monitor| {
                refresh_limit(
                    monitor
                        .video_modes()
                        .map(|mode| mode.refresh_rate_millihertz()),
                    monitor.refresh_rate_millihertz(),
                )
            }),
            area,
            scale_factor: native.scale_factor(),
            outer_position: native
                .outer_position()
                .ok()
                .map(|position| [position.x, position.y]),
            decoration: border,
        })
    });
    let Some(observed) = observed else {
        return;
    };
    let reported_fullscreen = observed.fullscreen;
    state.observe(observed, now);
    if let Some(request) = state.request.take() {
        let mut target = state.constrain(request);
        if target.fullscreen && !state.actual.fullscreen && state.has_requested {
            target.window_size = state.actual.window_size;
        }
        state.has_requested = true;
        if !target.fullscreen && state.window_managed {
            target.window_size = state.actual.window_size;
        }
        let mode_change = target.fullscreen != state.actual.fullscreen
            || state
                .in_flight
                .as_ref()
                .is_some_and(|pending| pending.mode_change);
        state.actual.fullscreen_size = target.fullscreen_size;
        if target.fullscreen {
            state.actual.window_size = target.window_size;
        }
        window.mode = if target.fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
        state.in_flight = Some(Pending {
            target,
            started: now,
            size_sent: false,
            mode_change,
        });
        state.pending = true;
    }
    if let Some([width, height]) = state.window_size_request(now, reported_fullscreen) {
        window.resolution.set_physical_resolution(width, height);
    }
    if let Some([x, y]) = state.position_request.take() {
        window.position.set(IVec2::new(x, y));
    }
}

#[derive(Component)]
struct SceneImage;

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<Entity, With<GameCamera>>,
) {
    let mut image = Image::new_target_texture(1280, 800, TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = ImageSampler::linear();
    let image = images.add(image);
    for camera in &cameras {
        commands.entity(camera).insert((
            Camera {
                order: -1,
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
        ));
    }
    let presentation = commands
        .spawn((
            Camera2d,
            Camera {
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            IsDefaultUiCamera,
            PresentationCamera,
        ))
        .id();
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        ImageNode::new(image),
        GlobalZIndex(-1000),
        UiTargetCamera(presentation),
        SceneImage,
    ));
}

fn update_render(
    state: Res<DisplayState>,
    mut images: ResMut<Assets<Image>>,
    mut scene: Query<(&ImageNode, &mut Node), With<SceneImage>>,
) {
    let source = if state.actual.fullscreen {
        state.actual.fullscreen_size
    } else {
        contain(state.physical_size, [state.texture_limit; 2])
    };
    for (image, mut node) in &mut scene {
        if let Some(mut image) = images.get_mut(&image.image)
            && [image.width(), image.height()] != source
        {
            image.resize(bevy::render::render_resource::Extent3d {
                width: source[0],
                height: source[1],
                depth_or_array_layers: 1,
            });
        }
        let [width, height] = letterbox(source, state.physical_size);
        node.width = percent(width);
        node.height = percent(height);
        node.left = percent((100.0 - width) * 0.5);
        node.top = percent((100.0 - height) * 0.5);
    }
}

fn fits(size: [u32; 2], limit: [u32; 2]) -> bool {
    size[0] <= limit[0] && size[1] <= limit[1]
}

fn contain(size: [u32; 2], limit: [u32; 2]) -> [u32; 2] {
    let size = size.map(|axis| axis.max(1));
    let limit = limit.map(|axis| axis.max(1));
    let ratio = (limit[0] as f64 / size[0] as f64)
        .min(limit[1] as f64 / size[1] as f64)
        .min(1.0);
    size.map(|axis| ((axis as f64 * ratio).floor() as u32).max(1))
}

fn letterbox(source: [u32; 2], surface: [u32; 2]) -> [f32; 2] {
    let source = source.map(|axis| axis.max(1) as f64);
    let surface = surface.map(|axis| axis.max(1) as f64);
    let scale = (surface[0] / source[0]).min(surface[1] / source[1]);
    [
        (source[0] * scale / surface[0] * 100.0) as f32,
        (source[1] * scale / surface[1] * 100.0) as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_sizes_preserve_aspect_and_filter_per_mode() {
        assert_eq!(contain([1280, 800], [1000, 600]), [960, 600]);
        assert_eq!(contain([0, 0], [0, 0]), [1, 1]);
        assert_eq!(letterbox([640, 480], [1280, 800]), [83.333336, 100.0]);
        assert_eq!(letterbox([1920, 1080], [1280, 800]), [100.0, 90.0]);
        let mut state = DisplayState::new(DisplaySettings::default());
        state.native_size = Some([1920, 1080]);
        state.window_limit = Some([1920, 1040]);
        assert_eq!(
            state
                .resolution_options(true)
                .iter()
                .filter(|&&size| size == [1920, 1080])
                .count(),
            1
        );
        assert!(!state.resolution_options(false).contains(&[1920, 1080]));
        state.native_size = Some([1000, 600]);
        assert_eq!(state.resolution_options(true), vec![[1000, 600]]);
    }

    fn observation(size: [u32; 2], fullscreen: bool) -> Observation {
        Observation {
            size,
            native: Some([2560, 1600]),
            window_limit: Some([2560, 1560]),
            fullscreen,
            managed: false,
            refresh_limit: 60_000,
            area: None,
            scale_factor: 1.0,
            outer_position: None,
            decoration: [0; 2],
        }
    }

    #[test]
    fn changed_work_areas_request_one_containment_and_preserve_actual_readback() {
        let monitor = PhysicalRect {
            x: -1920,
            y: -120,
            width: 1920,
            height: 1080,
        };
        let area = AreaInfo {
            monitor,
            work_area: Some(PhysicalRect {
                x: -1880,
                y: -80,
                width: 1600,
                height: 900,
            }),
            source: AreaSource::X11,
            resize_allowed: None,
        };
        let observed = Observation {
            area: Some(area),
            window_limit: Some([1580, 860]),
            outer_position: Some([-1900, -100]),
            decoration: [20, 40],
            ..observation([1900, 1000], false)
        };
        let mut state = DisplayState::new(DisplaySettings::default());
        state.request = None;
        state.observe(observed, Duration::ZERO);
        let target = state.request.take().unwrap();
        assert_eq!(target.window_size, [1580, 831]);
        assert_eq!(state.position_request.take(), Some([-1880, -80]));
        assert!(state.pending);
        assert_eq!(state.actual().window_size, [1900, 1000]);
        // Simulate the same production request/readback path, including a WM refusal
        state.in_flight = Some(Pending {
            target,
            started: Duration::ZERO,
            size_sent: false,
            mode_change: false,
        });
        assert_eq!(
            state.window_size_request(Duration::ZERO, false),
            Some([1580, 831])
        );
        for now in [Duration::from_secs(1), Duration::from_secs(3)] {
            state.observe(
                Observation {
                    area: Some(area),
                    window_limit: Some([1580, 860]),
                    ..observation([1900, 1000], false)
                },
                now,
            );
            assert!(state.request.is_none());
            assert!(state.position_request.is_none());
        }
        assert!(!state.pending);
        assert_eq!(state.notice.key, "display.size_unconfirmed");
        assert_eq!(state.actual().window_size, [1900, 1000]);
        // A manual resize in the same environment must never start a retry loop
        state.observe(
            Observation {
                area: Some(area),
                window_limit: Some([1580, 860]),
                ..observation([2000, 1100], false)
            },
            Duration::from_secs(4),
        );
        assert!(state.request.is_none());
        assert_eq!(state.actual().window_size, [2000, 1100]);
        // A DPI transition is a new environment and earns exactly one new attempt
        state.observe(
            Observation {
                area: Some(area),
                window_limit: Some([1560, 820]),
                scale_factor: 2.0,
                ..observation([2000, 1100], false)
            },
            Duration::from_secs(5),
        );
        assert_eq!(state.request.take().unwrap().window_size, [1490, 820]);
        assert_eq!(
            contain_position([0, 0], [1000, 700], area.work_area.unwrap()),
            [-1280, 0]
        );
        let next_monitor = PhysicalRect {
            x: 0,
            y: 0,
            width: 1280,
            height: 800,
        };
        state.observe(
            Observation {
                area: Some(AreaInfo {
                    monitor: next_monitor,
                    work_area: Some(PhysicalRect {
                        height: 760,
                        ..next_monitor
                    }),
                    ..area
                }),
                window_limit: Some([1260, 720]),
                outer_position: Some([-400, 0]),
                decoration: [20, 40],
                scale_factor: 2.0,
                ..observation([1490, 820], false)
            },
            Duration::from_secs(6),
        );
        assert_eq!(state.request.take().unwrap().window_size, [1260, 693]);
        assert_eq!(state.position_request.take(), Some([0, 0]));
    }

    #[test]
    fn unknown_or_managed_outputs_keep_geometry_authoritative() {
        let monitor = PhysicalRect {
            x: 100,
            y: -300,
            width: 1000,
            height: 800,
        };
        let area = AreaInfo {
            monitor,
            work_area: None,
            source: AreaSource::Unknown,
            resize_allowed: None,
        };
        let mut state = DisplayState::new(DisplaySettings::default());
        state.request = None;
        state.observe(
            Observation {
                area: Some(area),
                window_limit: Some([980, 760]),
                managed: true,
                ..observation([1280, 800], false)
            },
            Duration::ZERO,
        );
        assert!(state.request.is_none());
        assert!(state.window_managed);
        assert_eq!(state.notice.key, "display.managed");
        state.observe(
            Observation {
                area: Some(area),
                window_limit: Some([980, 760]),
                // A Wayland-style missing global position must not invent a move
                outer_position: None,
                ..observation([1280, 800], false)
            },
            Duration::from_secs(1),
        );
        assert_eq!(state.request.take().unwrap().window_size, [980, 612]);
        assert!(state.position_request.is_none());
        assert!(!state.window_managed);
        assert_eq!(state.notice.key, "display.work_area_unavailable");
        let fullscreen = DisplaySettings {
            fullscreen: true,
            ..state.actual()
        };
        state.in_flight = Some(Pending {
            target: fullscreen,
            started: Duration::ZERO,
            size_sent: false,
            mode_change: true,
        });
        state.observe(
            Observation {
                native: None,
                window_limit: None,
                ..observation([1280, 800], true)
            },
            Duration::from_secs(4),
        );
        assert!(state.pending);
        assert!(!state.actual().fullscreen);
        assert_eq!(state.notice.key, "display.mode_pending");
        assert!(state.position_request.is_none());
    }

    #[test]
    fn monitor_frame_rates_resolve_defaults_and_clamp_only_over_limit_choices() {
        let cases: [(u32, &[u32]); 6] = [
            (60_000, &[60_000]),
            (144_000, &[60_000, 120_000, 144_000]),
            (280_000, &[60_000, 120_000, 180_000, 240_000, 280_000]),
            (48_000, &[48_000]),
            (59_940, &[59_940]),
            (120_000, &[60_000, 120_000]),
        ];
        let mut state = DisplayState::new(DisplaySettings::default());
        let mut pacing = PacingSettings::default();
        state.normalize_pacing(&mut pacing);
        assert_eq!(pacing.frame_limit, FrameLimit::Display);
        assert_eq!(
            state.frame_rates(),
            [FrameLimit::Limited(60_000), FrameLimit::Unlimited]
        );
        for (maximum, expected) in cases {
            state.observe(
                Observation {
                    refresh_limit: maximum,
                    ..observation([1280, 800], false)
                },
                Duration::ZERO,
            );
            let mut rates: Vec<_> = expected.iter().copied().map(FrameLimit::Limited).collect();
            rates.push(FrameLimit::Unlimited);
            assert_eq!(state.frame_rates(), rates);
            for (choice, result) in [
                (FrameLimit::Display, FrameLimit::Limited(maximum)),
                (FrameLimit::Limited(300_000), FrameLimit::Limited(maximum)),
                (FrameLimit::Limited(30_000), FrameLimit::Limited(30_000)),
                (FrameLimit::Unlimited, FrameLimit::Unlimited),
            ] {
                pacing.frame_limit = choice;
                state.normalize_pacing(&mut pacing);
                assert_eq!(pacing.frame_limit, result);
            }
        }
        assert_eq!(
            refresh_limit([60_000, 144_000, 120_000], Some(60_000)),
            144_000
        );
        assert_eq!(refresh_limit([], Some(59_940)), 59_940);
        assert_eq!(refresh_limit([0, 999], Some(0)), 60_000);
        assert_eq!(refresh_limit([], None), 60_000);
        state.refresh_limit = None;
        state.set_headless_surface([1280, 800]);
        pacing.frame_limit = FrameLimit::Display;
        state.normalize_pacing(&mut pacing);
        assert_eq!(pacing.frame_limit, FrameLimit::Limited(60_000));
    }

    #[test]
    fn frame_gate_waits_for_each_deadline_without_catchup_and_vsync_is_independent() {
        let start = Instant::now();
        let mut pacing = FramePacing {
            limit: FrameLimit::Limited(60_000),
            last_start: None,
        };
        assert_eq!(pacing.remaining(start), Duration::ZERO);
        pacing.last_start = Some(start);
        let period = Duration::from_nanos(16_666_667);
        assert_eq!(pacing.remaining(start), period);
        assert_eq!(
            pacing.remaining(start + Duration::from_millis(1)),
            period - Duration::from_millis(1)
        );
        assert_eq!(pacing.remaining(start + period), Duration::ZERO);
        assert_eq!(
            pacing.remaining(start + Duration::from_secs(1)),
            Duration::ZERO
        );
        pacing.last_start = Some(start + Duration::from_secs(1));
        assert_eq!(pacing.remaining(start + Duration::from_secs(1)), period);
        pacing.limit = FrameLimit::Limited(59_940);
        assert_eq!(
            pacing.remaining(start + Duration::from_secs(1)),
            Duration::from_nanos(16_683_351)
        );
        pacing.limit = FrameLimit::Unlimited;
        assert_eq!(pacing.remaining(start), Duration::ZERO);
        pacing.limit = FrameLimit::Display;
        assert_eq!(pacing.remaining(start + Duration::from_secs(1)), period);
        pacing.limit = FrameLimit::Limited(1);
        assert_eq!(
            pacing.remaining(start + Duration::from_secs(1)),
            Duration::from_secs(1)
        );

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(pacing)
            .init_resource::<SettingsMenu>()
            .add_systems(PostUpdate, apply_pacing);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.update();
        assert_eq!(
            app.world().resource::<FramePacing>().limit,
            FrameLimit::Display
        );
        assert_eq!(
            app.world().get::<Window>(window).unwrap().present_mode,
            PresentMode::AutoNoVsync
        );
        app.world_mut().resource_mut::<SettingsMenu>().values.pacing = PacingSettings {
            frame_limit: FrameLimit::Unlimited,
            vsync: true,
        };
        app.update();
        assert_eq!(
            app.world().resource::<FramePacing>().limit,
            FrameLimit::Unlimited
        );
        assert_eq!(
            app.world().get::<Window>(window).unwrap().present_mode,
            PresentMode::AutoVsync
        );
    }

    #[test]
    fn production_frame_gate_limits_real_updates_before_the_input_probe() {
        #[derive(Resource, Default)]
        struct FrameStarts(Vec<Instant>);

        let initial = PacingSettings {
            frame_limit: FrameLimit::Limited(120_000),
            vsync: false,
        };
        let mut settings = SettingsMenu::default();
        settings.values.pacing = initial;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(settings)
            .init_resource::<FrameStarts>();
        install_frame_pacing(&mut app, initial);
        app.add_systems(
            First,
            (|pacing: Res<FramePacing>, mut starts: ResMut<FrameStarts>| {
                // Read the gate's actual instant so probe scheduling jitter cannot shorten a gap
                starts.0.push(pacing.last_start.unwrap());
            })
            .after(DisplaySystems::Pace),
        );
        for _ in 0..10 {
            app.update();
        }
        let starts = &app.world().resource::<FrameStarts>().0;
        assert_eq!(starts.len(), 10);
        let period = Duration::from_nanos(8_333_334);
        assert!(starts.windows(2).all(|pair| pair[1] - pair[0] >= period));
        app.world_mut()
            .resource_mut::<SettingsMenu>()
            .values
            .pacing
            .frame_limit = FrameLimit::Unlimited;
        app.update();
        assert_eq!(
            app.world()
                .resource::<FramePacing>()
                .remaining(Instant::now()),
            Duration::ZERO
        );
    }

    #[test]
    fn mode_transitions_need_geometry_while_rejected_resize_can_save_actual() {
        let initial = DisplaySettings::default();
        let fullscreen = DisplaySettings {
            fullscreen: true,
            ..initial
        };
        let mut state = DisplayState::new(initial);
        state.request = None;
        state.in_flight = Some(Pending {
            target: fullscreen,
            started: Duration::ZERO,
            size_sent: false,
            mode_change: true,
        });
        // A desired fullscreen flag and the old geometry cannot become confirmable
        state.observe(observation([1280, 800], true), Duration::from_secs(3));
        assert!(state.pending);
        assert!(!state.actual().fullscreen);
        assert_eq!(state.actual().window_size, [1280, 800]);
        state.request(initial);
        assert!(state.in_flight.as_ref().unwrap().mode_change);
        assert!(state.pending);
        // The final enter geometry confirms that mode, before testing an exit
        state.observe(observation([2560, 1600], true), Duration::from_secs(4));
        assert!(state.actual().fullscreen);
        state.request = None;
        state.pending = true;
        state.in_flight = Some(Pending {
            target: initial,
            started: Duration::from_secs(5),
            size_sent: false,
            mode_change: true,
        });
        state.observe(observation([2560, 1600], true), Duration::from_secs(8));
        assert!(state.pending);
        assert_eq!(
            state.window_size_request(Duration::from_secs(8), true),
            None
        );
        // Raw windowed arrives early; send the restore but keep confirmed fullscreen
        state.observe(observation([2560, 1600], false), Duration::from_secs(9));
        assert!(state.actual().fullscreen);
        assert_eq!(
            state.window_size_request(Duration::from_secs(9), false),
            Some([1280, 800])
        );
        state.observe(observation([2560, 1600], false), Duration::from_secs(12));
        assert!(state.pending);
        assert!(state.actual().fullscreen);
        // Exit geometry is corroborated even when the WM declines the restore size
        state.observe(observation([1200, 700], false), Duration::from_secs(13));
        assert!(!state.pending);
        assert!(!state.actual().fullscreen);
        assert_eq!(state.actual().window_size, [1200, 700]);
        assert!(state.in_flight.is_none());
        // Only an established windowed resize can accept a different actual size
        state.in_flight = Some(Pending {
            target: DisplaySettings {
                window_size: [1600, 900],
                ..initial
            },
            started: Duration::from_secs(14),
            size_sent: true,
            mode_change: false,
        });
        state.pending = true;
        state.observe(observation([1200, 700], false), Duration::from_secs(17));
        assert!(!state.pending);
        assert_eq!(state.actual().window_size, [1200, 700]);
        assert!(state.in_flight.is_none());
        assert_eq!(
            state.window_size_request(Duration::from_secs(18), false),
            None
        );
        // A mode discrepancy must not be treated as a rejected ordinary resize
        state.in_flight = Some(Pending {
            target: initial,
            started: Duration::from_secs(19),
            size_sent: true,
            mode_change: false,
        });
        state.observe(observation([2560, 1600], true), Duration::from_secs(22));
        assert!(state.pending);
        assert!(!state.actual().fullscreen);
    }

    #[test]
    fn offscreen_rendering_keeps_ui_at_surface_resolution() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<Image>>();
        let game = app
            .world_mut()
            .spawn((Camera3d::default(), GameCamera))
            .id();
        install(&mut app, DisplaySettings::default());
        app.update();
        assert!(app.world().resource::<DisplayState>().pending);
        {
            let mut state = app.world_mut().resource_mut::<DisplayState>();
            state.set_headless_surface([1280, 800]);
            state.request(DisplaySettings {
                fullscreen: true,
                fullscreen_size: [640, 480],
                ..default()
            });
            assert!(!state.pending);
        }
        app.update();
        let RenderTarget::Image(target) = app.world().get::<RenderTarget>(game).unwrap() else {
            panic!("Game camera must render into its own image");
        };
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&target.handle)
            .unwrap();
        assert_eq!([image.width(), image.height()], [640, 480]);
        let mut presentation = app
            .world_mut()
            .query_filtered::<(&Camera, &RenderTarget), With<IsDefaultUiCamera>>();
        let (camera, target) = presentation.single(app.world()).unwrap();
        assert_eq!(camera.order, 0);
        assert!(matches!(target, RenderTarget::Window(_)));
        let mut background = app.world_mut().query_filtered::<&Node, With<SceneImage>>();
        let node = background.single(app.world()).unwrap();
        assert_eq!(node.height, percent(100));
        assert_eq!(node.width, percent(83.333336));
        app.world_mut()
            .resource_mut::<DisplayState>()
            .request(DisplaySettings::default());
        app.update();
        let RenderTarget::Image(target) = app.world().get::<RenderTarget>(game).unwrap() else {
            unreachable!()
        };
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&target.handle)
            .unwrap();
        assert_eq!([image.width(), image.height()], [1280, 800]);
    }
}
