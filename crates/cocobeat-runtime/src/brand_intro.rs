//! Native brand presentation, independent of song time, audio devices and gameplay

use std::path::{Path, PathBuf};

use bevy::{
    asset::{LoadState, io::embedded::EmbeddedAssetRegistry},
    image::{ImageLoaderSettings, ImageSampler},
    prelude::*,
    render::{
        ExtractSchedule, MainWorld, RenderApp,
        render_asset::RenderAssets,
        render_resource::{
            AsBindGroup, CachedPipelineState, PipelineCache, PipelineDescriptor,
            RenderPipelineDescriptor, ShaderType,
        },
    },
    shader::{ShaderCacheError, ShaderRef},
    ui_render::{PreparedUiMaterial, ui_material::UiMaterialKey},
};

const PIPELINE_LABEL: &str = "cocobeat_brand_intro";
const CANVAS: Vec2 = Vec2::new(840.0, 180.0);
const IMPACT_TIMES: [f64; 3] = [0.90, 2.10, 3.35];
// Contact points on the two C crowns and B, in the shared mask canvas
const IMPACT_POINTS: [Vec2; 3] = [
    Vec2::new(90.0, 10.0),
    Vec2::new(318.0, 11.0),
    Vec2::new(475.0, 16.0),
];
const DOCK_START: f64 = 5.0;
const END: f64 = DOCK_START + 0.45;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrandIntroPhase {
    #[default]
    Loading,
    Playing,
    Docking,
    Complete,
    Failed,
}

#[derive(Resource, Debug, Default)]
pub struct BrandIntroStatus {
    pub phase: BrandIntroPhase,
    pub elapsed_seconds: f64,
    pub reveal_progress: f32,
    pub error: Option<String>,
}

impl BrandIntroStatus {
    pub fn is_complete(&self) -> bool {
        self.phase == BrandIntroPhase::Complete
    }
}

/// Viewport-relative logical UI pixels, unaffected by the operating system DPI scale
#[derive(Resource, Debug)]
pub struct BrandIntroLayout {
    pub dock_rect: Rect,
}

impl Default for BrandIntroLayout {
    fn default() -> Self {
        Self {
            dock_rect: Rect::from_corners(Vec2::new(36.0, 24.0), Vec2::new(276.0, 75.43)),
        }
    }
}

#[derive(Resource, Debug, Default)]
pub struct BrandIntroControl {
    pub suspended: bool,
}

#[derive(Message, Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrandImpact {
    Co1,
    Co2,
    Beat,
}

#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BrandIntroSystems {
    Advance,
}

#[derive(Resource, Default)]
struct GpuReadiness {
    ready: bool,
    error: Option<String>,
}

#[derive(Resource)]
struct BrandAssets {
    textures: [Handle<Image>; 5],
    shader: Handle<Shader>,
    materials: Vec<Handle<BrandMaterial>>,
}

#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
struct BrandUniform {
    paint: Vec4,
    effect: Vec4,
    eyes: Vec4,
    origins: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
struct BrandMaterial {
    #[uniform(0)]
    values: BrandUniform,
    #[texture(1)]
    #[sampler(2)]
    co1: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    co2: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    beat: Handle<Image>,
    #[texture(7)]
    #[sampler(8)]
    eyes_blue: Handle<Image>,
    #[texture(9)]
    #[sampler(10)]
    eyes_pink: Handle<Image>,
}

impl UiMaterial for BrandMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://cocobeat_brand/brand.wgsl".into()
    }

    fn specialize(descriptor: &mut RenderPipelineDescriptor, _: UiMaterialKey<Self>) {
        descriptor.label = Some(PIPELINE_LABEL.into());
    }
}

#[derive(Component)]
struct Backdrop;

#[derive(Component, Clone, Copy)]
enum Part {
    Wordmark,
    Blob,
    Splash(usize),
}

/// Call after DefaultPlugins, only in the normal game or a brand preview app
pub fn install(app: &mut App) {
    {
        let embedded = app.world().resource::<EmbeddedAssetRegistry>();
        for (name, bytes) in [
            (
                "brand.wgsl",
                include_bytes!("../../../assets/brand/shaders/brand.wgsl").as_slice(),
            ),
            (
                "logo_co1.png",
                include_bytes!("../../../assets/brand/masks/logo_co1.png").as_slice(),
            ),
            (
                "logo_co2.png",
                include_bytes!("../../../assets/brand/masks/logo_co2.png").as_slice(),
            ),
            (
                "logo_beat.png",
                include_bytes!("../../../assets/brand/masks/logo_beat.png").as_slice(),
            ),
            (
                "eyes_blue.png",
                include_bytes!("../../../assets/brand/masks/eyes_blue.png").as_slice(),
            ),
            (
                "eyes_pink.png",
                include_bytes!("../../../assets/brand/masks/eyes_pink.png").as_slice(),
            ),
        ] {
            // Stable virtual names avoid paths escaping the embedded asset source
            embedded.insert_asset(
                PathBuf::new(),
                Path::new(&format!("cocobeat_brand/{name}")),
                bytes,
            );
        }
    }
    app.add_plugins(UiMaterialPlugin::<BrandMaterial>::default())
        .init_resource::<BrandIntroStatus>()
        .init_resource::<BrandIntroLayout>()
        .init_resource::<BrandIntroControl>()
        .init_resource::<GpuReadiness>()
        .add_message::<BrandImpact>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (advance, present)
                .chain()
                .in_set(BrandIntroSystems::Advance),
        );
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(ExtractSchedule, read_gpu_readiness);
    } else {
        app.world_mut().resource_mut::<GpuReadiness>().error =
            Some("Brand intro requires Bevy RenderApp".into());
    }
}

fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut materials: ResMut<Assets<BrandMaterial>>,
) {
    let textures = [
        "logo_co1",
        "logo_co2",
        "logo_beat",
        "eyes_blue",
        "eyes_pink",
    ]
    .map(|name| {
        server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.sampler = ImageSampler::linear();
            })
            .load(format!("embedded://cocobeat_brand/{name}.png"))
    });
    let shader = server.load("embedded://cocobeat_brand/brand.wgsl");
    let mut handles = Vec::new();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(1000),
            Backdrop,
        ))
        .with_children(|parent| {
            for part in std::iter::once(Part::Wordmark)
                .chain(std::iter::once(Part::Blob))
                .chain((0..12).map(Part::Splash))
            {
                let handle = materials.add(BrandMaterial {
                    values: BrandUniform {
                        paint: Vec4::ZERO,
                        effect: Vec4::ZERO,
                        eyes: Vec4::ZERO,
                        origins: Vec4::ZERO,
                    },
                    co1: textures[0].clone(),
                    co2: textures[1].clone(),
                    beat: textures[2].clone(),
                    eyes_blue: textures[3].clone(),
                    eyes_pink: textures[4].clone(),
                });
                handles.push(handle.clone());
                parent.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(1),
                        height: px(1),
                        ..default()
                    },
                    MaterialNode(handle),
                    part,
                ));
            }
        });
    commands.insert_resource(BrandAssets {
        textures,
        shader,
        materials: handles,
    });
}

fn read_gpu_readiness(
    mut main: ResMut<MainWorld>,
    cache: Res<PipelineCache>,
    prepared: Res<RenderAssets<PreparedUiMaterial<BrandMaterial>>>,
) {
    let Some(assets) = main.get_resource::<BrandAssets>() else {
        return;
    };
    let mut ready = assets
        .materials
        .iter()
        .all(|h| prepared.get(h.id()).is_some());
    let mut seen = false;
    let mut error = None;
    for pipeline in cache.pipelines() {
        let PipelineDescriptor::RenderPipelineDescriptor(descriptor) = &pipeline.descriptor else {
            continue;
        };
        if descriptor.label.as_deref() != Some(PIPELINE_LABEL) {
            continue;
        }
        seen = true;
        match &pipeline.state {
            CachedPipelineState::Ok(_) => {}
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => ready = false,
            CachedPipelineState::Err(failure) => {
                ready = false;
                error = Some(failure.to_string());
            }
            _ => ready = false,
        }
    }
    let mut result = main.resource_mut::<GpuReadiness>();
    result.ready = seen && ready;
    result.error = error;
}

fn advance(
    time: Res<Time<Real>>,
    control: Res<BrandIntroControl>,
    gpu: Res<GpuReadiness>,
    (server, assets): (Res<AssetServer>, Res<BrandAssets>),
    mut status: ResMut<BrandIntroStatus>,
    mut impacts: MessageWriter<BrandImpact>,
    mut was_suspended: Local<bool>,
) {
    if matches!(
        status.phase,
        BrandIntroPhase::Failed | BrandIntroPhase::Complete
    ) {
        return;
    }
    let load_error = assets
        .textures
        .iter()
        .map(|h| h.id().untyped())
        .chain(std::iter::once(assets.shader.id().untyped()))
        .find_map(|id| match server.load_state(id) {
            LoadState::Failed(error) => Some(error.to_string()),
            _ => None,
        });
    if let Some(error) = gpu.error.clone().or(load_error) {
        status.phase = BrandIntroPhase::Failed;
        status.error = Some(error);
        return;
    }
    let resumed = *was_suspended && !control.suspended;
    *was_suspended = control.suspended;
    if control.suspended || resumed {
        return;
    }
    if status.phase == BrandIntroPhase::Loading {
        if gpu.ready {
            status.phase = BrandIntroPhase::Playing;
        }
        // The first ready frame displays the exact white-wordmark starting pose
        return;
    }
    let old = status.elapsed_seconds;
    let next = (old + time.delta_secs_f64()).min(END);
    for impact in crossed_impacts(old, next) {
        impacts.write(impact);
    }
    status.elapsed_seconds = next;
    status.reveal_progress = reveal_at(next);
    status.phase = phase_at(next);
}

fn phase_at(t: f64) -> BrandIntroPhase {
    if t >= END {
        BrandIntroPhase::Complete
    } else if t >= DOCK_START {
        BrandIntroPhase::Docking
    } else {
        BrandIntroPhase::Playing
    }
}

fn crossed_impacts(old: f64, next: f64) -> impl Iterator<Item = BrandImpact> {
    IMPACT_TIMES
        .into_iter()
        .zip([BrandImpact::Co1, BrandImpact::Co2, BrandImpact::Beat])
        .filter_map(move |(t, impact)| (old < t && next >= t).then_some(impact))
}

fn smooth(value: f32) -> f32 {
    let x = value.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn progress(t: f32, start: f32, end: f32) -> f32 {
    smooth((t - start) / (end - start))
}

pub(crate) fn reveal_at(t: f64) -> f32 {
    CubicSegment::new_bezier_easing((0.45, 0.0), (0.20, 1.0))
        .ease(((t - DOCK_START) / (END - DOCK_START)) as f32)
}

fn paint_at(t: f32) -> Vec4 {
    // Impact drives a fast spread that slows as the pigment settles
    let curve = CubicSegment::new_bezier_easing((0.22, 0.75), (0.30, 1.0));
    Vec4::new(
        curve.ease((t - 0.90) / (1.42 - 0.90)),
        curve.ease((t - 2.10) / (2.70 - 2.10)),
        curve.ease((t - 3.35) / (3.80 - 3.35)),
        0.0,
    )
}

fn eyes_at(t: f32) -> Vec4 {
    let inward = progress(t, 2.40, 2.66);
    let settle = 1.0 - progress(t, 3.02, 3.55);
    Vec4::new(
        progress(t, 0.90, 1.12),
        progress(t, 2.10, 2.32),
        (-4.0 + 8.0 * inward) * settle,
        (4.0 - 6.0 * inward) * settle,
    )
}

#[derive(Debug)]
struct BlobPose {
    center: Vec2,
    size: Vec2,
    palette: f32,
    opacity: f32,
}

fn blob_at(t: f32) -> BlobPose {
    let [co1, co2, beat] = IMPACT_POINTS;
    let (contact, stretch) = if t < 0.90 {
        let u = ((t - 0.40) / 0.50).clamp(0.0, 1.0);
        (
            CubicSegment::new_bezier([
                co1 + Vec2::new(0.0, -220.0),
                co1 + Vec2::new(0.0, -208.0),
                co1 + Vec2::new(0.0, -156.0),
                co1,
            ])
            .position(u),
            Vec2::new(0.76, 1.35),
        )
    } else if t < 2.10 {
        let u = ((t - 0.90) / 1.20).clamp(0.0, 1.0);
        let bounce = (std::f32::consts::PI * u).sin();
        (
            CubicSegment::new_bezier([
                co1,
                co1 + Vec2::new(34.0, -272.0),
                co2 + Vec2::new(-52.0, -264.0),
                co2,
            ])
            .position(u),
            Vec2::new(1.0 - bounce * 0.16, 1.0 + bounce * 0.25),
        )
    } else {
        let u = ((t - 2.10) / 1.25).clamp(0.0, 1.0);
        let bounce = (std::f32::consts::PI * u).sin();
        (
            CubicSegment::new_bezier([
                co2,
                co2 + Vec2::new(25.0, -252.0),
                beat + Vec2::new(-42.0, -248.0),
                beat,
            ])
            .position(u),
            Vec2::new(1.0 - bounce * 0.13, 1.0 + bounce * 0.25),
        )
    };
    let squash = IMPACT_TIMES
        .iter()
        .map(|&hit| {
            let age = t - hit as f32;
            if (0.0..0.14).contains(&age) {
                1.0 - age / 0.14
            } else {
                0.0
            }
        })
        .fold(0.0, f32::max);
    let merge = 1.0 - progress(t, 3.35, 3.55);
    let size = Vec2::splat(48.0)
        * stretch
        * Vec2::new(1.0 + squash * 0.5, 1.0 - squash * 0.48)
        * merge.max(0.01);
    BlobPose {
        // The SDF silhouette radius is 0.88 of the quad half-size
        center: contact - Vec2::Y * size.y * 0.44,
        size,
        palette: progress(t, 0.50, 0.75) + progress(t, 1.15, 1.55) + progress(t, 2.35, 2.75),
        opacity: if t < 0.40 { 0.0 } else { merge },
    }
}

fn fitted(rect: Rect) -> Rect {
    let scale = (rect.width() / CANVAS.x)
        .min(rect.height() / CANVAS.y)
        .max(0.0);
    Rect::from_center_size(rect.center(), CANVAS * scale)
}

fn present(
    status: Res<BrandIntroStatus>,
    layout: Res<BrandIntroLayout>,
    cameras: Query<&Camera>,
    mut backdrop: Query<&mut BackgroundColor, With<Backdrop>>,
    mut parts: Query<(&Part, &mut Node, &MaterialNode<BrandMaterial>)>,
    mut materials: ResMut<Assets<BrandMaterial>>,
) {
    let viewport = cameras
        .iter()
        .find(|camera| camera.is_active)
        .and_then(Camera::logical_viewport_size)
        .unwrap_or(Vec2::new(1280.0, 800.0));
    let width = (viewport.x * 0.66).min(viewport.y * 1.35).min(1100.0);
    let intro = Rect::from_center_size(
        viewport * Vec2::new(0.5, 0.49),
        Vec2::new(width, width * CANVAS.y / CANVAS.x),
    );
    let target = fitted(layout.dock_rect);
    let amount = status.reveal_progress;
    let rect = Rect::from_corners(
        intro.min.lerp(target.min, amount),
        intro.max.lerp(target.max, amount),
    );
    let scale = rect.width() / CANVAS.x;
    let t = status.elapsed_seconds as f32;
    let visible = !matches!(
        status.phase,
        BrandIntroPhase::Loading | BrandIntroPhase::Failed
    );
    for mut color in &mut backdrop {
        color.0 = Color::srgba(0.0, 0.0, 0.0, 1.0 - amount);
    }
    let blob = blob_at(t);
    let origins = Vec4::new(
        IMPACT_POINTS[0].x / CANVAS.x,
        IMPACT_POINTS[0].y / CANVAS.y,
        IMPACT_POINTS[1].x / CANVAS.x,
        IMPACT_POINTS[1].y / CANVAS.y,
    );
    for (part, mut node, handle) in &mut parts {
        let (center, size, values) = match *part {
            Part::Wordmark => (
                rect.center(),
                rect.size(),
                BrandUniform {
                    paint: paint_at(t),
                    effect: Vec4::new(0.0, 0.0, f32::from(visible), 0.0),
                    eyes: eyes_at(t),
                    origins,
                },
            ),
            Part::Blob => (
                rect.min + blob.center * scale,
                blob.size * scale,
                BrandUniform {
                    paint: Vec4::ZERO,
                    effect: Vec4::new(1.0, blob.palette, blob.opacity * f32::from(visible), 0.0),
                    eyes: Vec4::ZERO,
                    origins: Vec4::ZERO,
                },
            ),
            Part::Splash(index) => {
                let impact = index / 4;
                let age = t - IMPACT_TIMES[impact] as f32;
                let u = (age / 0.38).clamp(0.0, 1.0);
                let origin = IMPACT_POINTS[impact];
                let direction = [
                    Vec2::new(-1.0, -1.4),
                    Vec2::new(-0.35, -2.0),
                    Vec2::new(0.6, -1.7),
                    Vec2::new(1.3, -0.7),
                ][index % 4];
                let center = origin + direction * (u * 54.0) + Vec2::Y * (u * u * 72.0);
                let opacity = if (0.0..0.38).contains(&age) {
                    1.0 - smooth(u)
                } else {
                    0.0
                };
                (
                    rect.min + center * scale,
                    Vec2::splat((10.0 - u * 7.0) * scale),
                    BrandUniform {
                        paint: Vec4::ZERO,
                        effect: Vec4::new(
                            1.0,
                            (impact + 1) as f32,
                            opacity * f32::from(visible),
                            0.0,
                        ),
                        eyes: Vec4::ZERO,
                        origins: Vec4::ZERO,
                    },
                )
            }
        };
        node.left = px(center.x - size.x * 0.5);
        node.top = px(center.y - size.y * 0.5);
        node.width = px(size.x.max(0.1));
        node.height = px(size.y.max(0.1));
        if let Some(mut material) = materials.get_mut(&handle.0)
            && material.values != values
        {
            material.values = values;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_and_focus_pause_do_not_consume_the_timeline() {
        let mut app = App::new();
        app.add_plugins((bevy::app::TaskPoolPlugin::default(), AssetPlugin::default()))
            .init_resource::<Time<Real>>()
            .init_resource::<BrandIntroControl>()
            .init_resource::<BrandIntroStatus>()
            .init_resource::<GpuReadiness>()
            .insert_resource(BrandAssets {
                textures: std::array::from_fn(|_| Handle::default()),
                shader: Handle::default(),
                materials: vec![],
            })
            .add_message::<BrandImpact>()
            .add_systems(Update, advance);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(10));
        app.update();
        assert_eq!(
            app.world().resource::<BrandIntroStatus>().phase,
            BrandIntroPhase::Loading
        );
        app.world_mut().resource_mut::<GpuReadiness>().ready = true;
        app.update();
        assert_eq!(
            app.world().resource::<BrandIntroStatus>().elapsed_seconds,
            0.0
        );
        app.world_mut()
            .resource_mut::<BrandIntroControl>()
            .suspended = true;
        app.update();
        assert_eq!(
            app.world().resource::<BrandIntroStatus>().elapsed_seconds,
            0.0
        );
        app.world_mut()
            .resource_mut::<BrandIntroControl>()
            .suspended = false;
        // A restored window can deliver a delta spanning the entire suspension
        app.update();
        assert_eq!(
            app.world().resource::<BrandIntroStatus>().elapsed_seconds,
            0.0
        );
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(900));
        app.update();
        assert_eq!(
            app.world().resource::<BrandIntroStatus>().elapsed_seconds,
            0.9
        );
        let events: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<BrandImpact>>()
            .drain()
            .collect();
        assert_eq!(events, [BrandImpact::Co1]);
        app.world_mut().resource_mut::<GpuReadiness>().error =
            Some("shader compilation failed".into());
        app.update();
        assert_eq!(
            app.world().resource::<BrandIntroStatus>().phase,
            BrandIntroPhase::Failed
        );
        assert!(!app.world().resource::<BrandIntroStatus>().is_complete());
    }

    #[test]
    fn events_cross_once_even_after_a_stall() {
        assert_eq!(
            crossed_impacts(0.0, 0.90).collect::<Vec<_>>(),
            [BrandImpact::Co1]
        );
        assert_eq!(
            crossed_impacts(0.90, 3.8).collect::<Vec<_>>(),
            [BrandImpact::Co2, BrandImpact::Beat]
        );
        assert_eq!(crossed_impacts(3.8, 4.45).count(), 0);
        assert_eq!(crossed_impacts(2.10, 2.10).count(), 0);
        assert_eq!(crossed_impacts(0.0, 4.45).count(), 3);
    }

    #[test]
    fn final_paint_and_phases_are_exact() {
        assert_eq!(paint_at(0.0), Vec4::ZERO);
        assert_eq!(paint_at(3.8), Vec4::new(1.0, 1.0, 1.0, 0.0));
        for t in [4.0, 5.0, 5.45, 60.0] {
            assert_eq!(paint_at(t), paint_at(3.8));
        }
        assert_eq!(phase_at(3.8), BrandIntroPhase::Playing);
        assert_eq!(phase_at(4.0), BrandIntroPhase::Playing);
        assert_eq!(phase_at(4.99), BrandIntroPhase::Playing);
        assert_eq!(phase_at(5.0), BrandIntroPhase::Docking);
        assert_eq!(phase_at(5.45), BrandIntroPhase::Complete);
        assert_eq!(smooth(1.0), 1.0);
        assert_eq!(blob_at(3.8).opacity, 0.0);
        assert_eq!(eyes_at(3.8), Vec4::new(1.0, 1.0, 0.0, 0.0));
        for t in [4.0, 5.0, 5.45, 60.0] {
            assert_eq!(eyes_at(t), eyes_at(3.8));
        }
    }

    #[test]
    fn blob_lands_on_each_letter_and_docking_preserves_aspect() {
        for (t, expected) in [
            (0.90, Vec2::new(90.0, 10.0)),
            (2.10, Vec2::new(318.0, 11.0)),
            (3.35, Vec2::new(475.0, 16.0)),
        ] {
            let blob = blob_at(t);
            let bottom = blob.center + Vec2::Y * blob.size.y * 0.44;
            assert!(bottom.distance(expected) < 0.001);
        }
        let target = fitted(Rect::from_corners(
            Vec2::new(36.0, 24.0),
            Vec2::new(400.0, 80.0),
        ));
        assert!((target.width() / target.height() - CANVAS.x / CANVAS.y).abs() < 0.001);
    }

    #[test]
    fn eyes_wake_after_their_own_impact_then_exchange_a_glance() {
        assert_eq!(eyes_at(0.90).xy(), Vec2::ZERO);
        assert_eq!(eyes_at(1.12).xy(), Vec2::new(1.0, 0.0));
        assert_eq!(eyes_at(2.10).xy(), Vec2::new(1.0, 0.0));
        assert_eq!(eyes_at(2.32).xy(), Vec2::ONE);
        assert_eq!(eyes_at(2.32).zw(), Vec2::new(-4.0, 4.0));
        assert_eq!(eyes_at(2.66).zw(), Vec2::new(4.0, -2.0));
        assert_eq!(eyes_at(3.02), eyes_at(2.66));
        assert_eq!(eyes_at(3.55).zw(), Vec2::ZERO);
    }

    #[test]
    fn blob_colors_are_readable_before_each_landing() {
        assert!(blob_at(0.60).palette > 0.0);
        assert_eq!(blob_at(0.75).palette, 1.0);
        assert!(blob_at(1.30).palette > 1.0);
        assert_eq!(blob_at(1.55).palette, 2.0);
        assert!(blob_at(2.50).palette > 2.0);
        assert_eq!(blob_at(2.75).palette, 3.0);
    }

    #[test]
    fn pigment_slows_down_and_docking_eases_in_and_out() {
        for (index, start, end) in [(0, 0.90, 1.42), (1, 2.10, 2.70), (2, 3.35, 3.80)] {
            let samples: [f32; 5] =
                std::array::from_fn(|i| paint_at(start + (end - start) * i as f32 / 4.0)[index]);
            assert_eq!(samples[0], 0.0);
            assert_eq!(samples[4], 1.0);
            let steps: Vec<_> = samples.windows(2).map(|pair| pair[1] - pair[0]).collect();
            assert!(steps.iter().all(|step| *step > 0.0));
            assert!(steps.windows(2).all(|pair| pair[0] > pair[1]));
        }
        let samples: [f32; 5] =
            std::array::from_fn(|i| reveal_at(DOCK_START + (END - DOCK_START) * i as f64 / 4.0));
        assert_eq!(reveal_at(0.0), 0.0);
        assert_eq!(samples[0], 0.0);
        assert_eq!(samples[4], 1.0);
        assert_eq!(reveal_at(60.0), 1.0);
        let steps: Vec<_> = samples.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(steps.iter().all(|step| *step > 0.0));
        assert!(steps[1] > steps[0]);
        assert!(steps[2] > steps[3]);
    }

    #[test]
    fn bounces_remain_visible_and_move_towards_the_next_letter() {
        let scale = 1280.0 * 0.66 / 840.0;
        let logo_top = 720.0 * 0.49 - 180.0 * scale * 0.5;
        let mut last_x = IMPACT_POINTS[0].x;
        for frame in 90..=335 {
            let pose = blob_at(frame as f32 / 100.0);
            let visible_top = logo_top + (pose.center.y - pose.size.y * 0.44) * scale;
            assert!(visible_top > 0.0, "Clipped bounce at frame {frame}");
            assert!(pose.center.x >= last_x, "Reversed bounce at frame {frame}");
            last_x = pose.center.x;
        }
    }
}
