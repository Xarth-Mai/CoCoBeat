use std::time::Duration;

use bevy::{
    camera::{CameraUpdateSystems, RenderTarget},
    ecs::system::NonSendMarker,
    image::ImageSampler,
    prelude::*,
    render::{render_resource::TextureFormat, renderer::RenderDevice},
    window::{MonitorSelection, PrimaryWindow, WindowMode},
    winit::WINIT_WINDOWS,
};

use crate::settings::DisplaySettings;

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
}

struct Pending {
    target: DisplaySettings,
    started: Duration,
    size_sent: bool,
    mode_change: bool,
}

#[derive(Resource)]
pub(crate) struct DisplayState {
    pub physical_size: [u32; 2],
    pub native_size: Option<[u32; 2]>,
    pub window_managed: bool,
    pub pending: bool,
    pub notice: String,
    actual: DisplaySettings,
    request: Option<DisplaySettings>,
    in_flight: Option<Pending>,
    window_limit: Option<[u32; 2]>,
    texture_limit: u32,
    headless_surface: Option<[u32; 2]>,
    has_requested: bool,
}

impl DisplayState {
    pub(crate) fn new(initial: DisplaySettings) -> Self {
        Self {
            physical_size: [1280, 800],
            native_size: None,
            window_managed: false,
            pending: true,
            notice: String::new(),
            actual: initial,
            request: Some(initial),
            in_flight: None,
            window_limit: None,
            texture_limit: 16384,
            headless_surface: None,
            has_requested: false,
        }
    }

    pub(crate) fn actual(&self) -> DisplaySettings {
        self.actual
    }

    pub(crate) fn request(&mut self, settings: DisplaySettings) {
        self.request = Some(settings);
        self.pending = true;
        self.notice.clear();
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

    // Explicitly simulated output for the existing offscreen smoke, never native acceptance
    pub(crate) fn set_headless_surface(&mut self, size: [u32; 2]) {
        self.headless_surface = Some(size.map(|axis| axis.max(1)));
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
                self.notice =
                    "Simulated offscreen display; native display acceptance NOT RUN".into();
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
        self.physical_size = observed.size;
        self.native_size = observed.native;
        self.window_limit = observed.window_limit;
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
                    "Desktop work area unavailable; the actual window size is shown".into()
                } else {
                    "Requested size not observed; confirm to save the actual window size".into()
                };
                // Accepting an observed resize must leave no deferred adapter action
                self.in_flight = None;
                self.pending = false;
            } else {
                self.pending = true;
                if elapsed {
                    self.notice =
                        "Waiting for display mode and size; confirm remains disabled".into();
                }
            }
        } else if !observed.fullscreen || observed.native == Some(observed.size) {
            self.actual.fullscreen = observed.fullscreen;
            if !observed.fullscreen {
                self.actual.window_size = observed.size;
            }
        }
        if self.window_managed {
            self.notice = "Window manager controls this window size".into();
        }
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
    _main_thread: NonSendMarker,
) {
    if let Some(device) = device {
        state.texture_limit = device.limits().max_texture_dimension_2d;
    }
    let Ok((entity, mut window)) = windows.single_mut() else {
        state.apply_headless();
        return;
    };
    let observed = WINIT_WINDOWS.with_borrow(|windows| {
        let native = windows.get_window(entity)?;
        let inner = native.inner_size();
        // Zero extent while minimized is not a persistent resolution choice
        if inner.width == 0 || inner.height == 0 {
            return None;
        }
        let size = [inner.width, inner.height];
        let monitor = native.current_monitor();
        let native_size = monitor.map(|monitor| {
            let size = monitor.size();
            [size.width, size.height]
        });
        let outer = native.outer_size();
        let border = [
            outer.width.saturating_sub(inner.width),
            outer.height.saturating_sub(inner.height),
        ];
        Some(Observation {
            size,
            native: native_size,
            // Winit has no work-area API; this is only a monitor/decorations upper bound
            window_limit: native_size.map(|size| {
                [
                    size[0].saturating_sub(border[0]).max(1),
                    size[1].saturating_sub(border[1]).max(1),
                ]
            }),
            fullscreen: native.fullscreen().is_some(),
            managed: native.is_maximized() || !native.is_resizable(),
        })
    });
    let Some(observed) = observed else {
        return;
    };
    let now = time.elapsed();
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
        }
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
