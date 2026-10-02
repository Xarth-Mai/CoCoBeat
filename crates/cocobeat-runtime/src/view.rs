use std::f32::consts::PI;

use bevy::{
    core_pipeline::tonemapping::Tonemapping, prelude::*, transform::TransformSystems, ui::UiSystems,
};

use crate::{
    brand_intro::{BrandIntroLayout, BrandIntroPhase, BrandIntroStatus, BrandIntroSystems},
    display::GameCamera,
};

#[derive(Resource, Default)]
pub struct VisualState {
    pub song_seconds: f64,
    pub hit_pulses: [f32; 2],
    pub sync_pulse: f32,
    pub resonance: f32,
    pub status: String,
    pub running: bool,
    pub settings_open: bool,
}

#[derive(Component)]
enum Motion {
    Spirit(usize),
    Ripple(usize),
    SharedRing,
    Street(f32),
    Rain(f32),
}

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct StatusPanel;

#[derive(Component)]
struct Subtitle;

#[derive(Component)]
struct ClockText;

#[derive(Component)]
struct PlayerLabel;

#[derive(Component)]
struct ProgressFill;

pub fn install(app: &mut App) {
    app.init_resource::<VisualState>()
        .insert_resource(ClearColor(Color::srgb(0.012, 0.017, 0.042)))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            update_brand_layout.before(BrandIntroSystems::Advance),
        )
        .add_systems(
            PostUpdate,
            (animate, update_hud)
                .before(TransformSystems::Propagate)
                .before(UiSystems::Prepare),
        );
}

fn part(
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    position: Vec3,
    scale: Vec3,
) -> impl Bundle {
    (
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(position).with_scale(scale),
    )
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        GameCamera,
        bevy::camera::ShadowLodOrigin,
        Tonemapping::Reinhard,
        Transform::from_xyz(0.0, 6.0, 12.5).looking_at(Vec3::new(0.0, 0.6, -6.0), Vec3::Y),
        AmbientLight {
            color: Color::srgb(0.54, 0.62, 0.85),
            brightness: 180.0,
            ..default()
        },
        DistanceFog {
            color: Color::srgb(0.012, 0.017, 0.042),
            falloff: FogFalloff::Linear {
                start: 12.0,
                end: 48.0,
            },
            ..default()
        },
    ));

    let cube = meshes.add(Cuboid::default());
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(24, 16));
    let cone = meshes.add(Cone::new(0.28, 0.65));
    let ring = meshes.add(
        Torus::new(0.92, 1.0)
            .mesh()
            .major_resolution(48)
            .minor_resolution(8),
    );
    let half_ring = meshes.add(
        Torus::new(0.92, 1.0)
            .mesh()
            .major_resolution(24)
            .minor_resolution(8)
            .angle_range(0.0..=PI),
    );
    let asphalt = materials.add(StandardMaterial {
        base_color: Color::srgb(0.018, 0.025, 0.045),
        metallic: 0.65,
        perceptual_roughness: 0.22,
        ..default()
    });
    let building = materials.add(StandardMaterial {
        base_color: Color::srgb(0.033, 0.037, 0.072),
        perceptual_roughness: 0.8,
        ..default()
    });
    let dark = materials.add(Color::srgb(0.01, 0.02, 0.04));
    let white = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.95, 0.8),
        emissive: LinearRgba::rgb(1.0, 1.0, 0.6),
        ..default()
    });
    let colors = [Color::srgb(0.12, 0.92, 0.9), Color::srgb(0.96, 0.28, 0.67)];
    let neon = colors.map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * 2.5,
            ..default()
        })
    });
    let bodies = colors.map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * 0.1,
            perceptual_roughness: 0.42,
            ..default()
        })
    });
    let rain = materials.add(StandardMaterial {
        base_color: Color::srgba(0.5, 0.62, 0.8, 0.25),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    commands.spawn(part(
        &cube,
        &asphalt,
        Vec3::new(0.0, -0.1, -15.0),
        Vec3::new(9.0, 0.2, 54.0),
    ));
    for (player, side) in [-1.0, 1.0].into_iter().enumerate() {
        commands.spawn(part(
            &cube,
            &neon[player],
            Vec3::new(side * 3.5, 0.02, -15.0),
            Vec3::new(0.025, 0.025, 54.0),
        ));
        commands.spawn((
            PointLight {
                color: colors[player],
                intensity: 28_000.0,
                range: 13.0,
                radius: 2.0,
                ..default()
            },
            Transform::from_xyz(side * 3.0, 3.0, 0.5),
        ));
        for block in 0..9 {
            let z = 5.0 - block as f32 * 5.2;
            let height = 4.0 + (block % 3) as f32 * 2.5;
            commands.spawn(part(
                &cube,
                &building,
                Vec3::new(side * 6.0, height / 2.0, z),
                Vec3::new(3.5, height, 4.3),
            ));
            commands.spawn(part(
                &cube,
                &neon[player],
                Vec3::new(side * 4.22, height * 0.62, z),
                Vec3::new(0.08, 0.09, 2.2),
            ));
            commands.spawn(part(
                &cube,
                &neon[player],
                Vec3::new(side * 4.22, height * 0.62 - 0.3, z),
                Vec3::new(0.08, 0.035, 1.3),
            ));
        }
        let x = side * 1.35;
        commands
            .spawn((
                Transform::from_xyz(x, 0.85, 0.0),
                Visibility::default(),
                Motion::Spirit(player),
            ))
            .with_children(|parent| {
                let body_scale = if player == 0 {
                    Vec3::new(0.55, 0.6, 0.48)
                } else {
                    Vec3::new(0.67, 0.45, 0.47)
                };
                parent.spawn(part(&sphere, &bodies[player], Vec3::ZERO, body_scale));
                if player == 0 {
                    for ear in [-1.0, 1.0] {
                        parent.spawn(part(
                            &sphere,
                            &bodies[player],
                            Vec3::new(ear * 0.29, 0.61, 0.0),
                            Vec3::new(0.14, 0.32, 0.16),
                        ));
                        parent.spawn(part(
                            &sphere,
                            &dark,
                            Vec3::new(ear * 0.2, 0.1, 0.44),
                            Vec3::new(0.065, 0.09, 0.045),
                        ));
                    }
                } else {
                    parent.spawn(part(
                        &cone,
                        &bodies[player],
                        Vec3::new(0.0, 0.57, 0.0),
                        Vec3::ONE,
                    ));
                    parent.spawn(part(
                        &sphere,
                        &dark,
                        Vec3::new(0.0, 0.04, 0.43),
                        Vec3::new(0.34, 0.095, 0.06),
                    ));
                    for eye in [-1.0, 1.0] {
                        parent.spawn(part(
                            &sphere,
                            &white,
                            Vec3::new(eye * 0.14, 0.04, 0.485),
                            Vec3::splat(0.032),
                        ));
                    }
                }
                for foot in [-1.0, 1.0] {
                    parent.spawn(part(
                        &sphere,
                        &bodies[player],
                        Vec3::new(foot * 0.31, -0.46, 0.12),
                        Vec3::new(0.22, 0.12, 0.25),
                    ));
                }
            });
        commands.spawn((
            part(
                &half_ring,
                &neon[player],
                Vec3::new(x, 0.04, 0.0),
                Vec3::ONE,
            ),
            Motion::Ripple(player),
            Visibility::Hidden,
        ));
        for mark in 0..12 {
            let offset = mark as f32 * 3.0;
            commands.spawn((
                part(
                    &cube,
                    &neon[player],
                    Vec3::new(x, 0.015, -offset),
                    Vec3::new(0.045, 0.015, 0.52),
                ),
                Motion::Street(offset),
            ));
        }
    }
    commands.spawn((
        part(&ring, &white, Vec3::new(0.0, 0.07, 0.0), Vec3::splat(2.5)),
        Motion::SharedRing,
        Visibility::Hidden,
    ));
    for drop in 0..48 {
        let phase = drop as f32 * 0.73;
        commands.spawn((
            part(
                &cube,
                &rain,
                Vec3::new((phase * 3.7).sin() * 6.0, 0.0, -((drop * 7) % 31) as f32),
                Vec3::new(0.012, 0.24, 0.012),
            ),
            Motion::Rain(phase),
        ));
    }

    commands.spawn((
        Text::new("AFTER HOURS   /   A DUET IN THE RAIN"),
        TextFont::from_font_size(12.0),
        TextColor(Color::srgb(0.56, 0.66, 0.78)),
        Node {
            position_type: PositionType::Absolute,
            top: px(88),
            left: px(38),
            ..default()
        },
        Subtitle,
    ));
    commands.spawn((
        Text::default(),
        TextFont::from_font_size(16.0),
        TextColor(Color::srgb(0.83, 0.87, 0.93)),
        TextLayout::justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            top: px(34),
            right: px(36),
            ..default()
        },
        ClockText,
    ));
    for (player, label) in ["P1  /  TWO EARS", "P2  /  ONE CROWN"]
        .into_iter()
        .enumerate()
    {
        commands.spawn((
            Text::new(label),
            PlayerLabel,
            TextFont::from_font_size(13.0),
            TextColor(colors[player]),
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                left: percent(if player == 0 { 32.0 } else { 48.0 }),
                width: percent(20),
                bottom: px(180),
                ..default()
            },
        ));
    }
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(28),
                right: px(28),
                bottom: px(28),
                padding: UiRect::all(px(16)),
                min_height: px(104),
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.022, 0.045, 0.94)),
            GlobalZIndex(0),
            StatusPanel,
        ))
        .with_child((
            Text::default(),
            TextFont::from_font_size(14.0),
            TextColor(Color::srgb(0.87, 0.91, 0.96)),
            StatusText,
        ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0),
                width: percent(100),
                height: px(3),
                ..default()
            },
            BackgroundColor(Color::srgb(0.1, 0.14, 0.2)),
        ))
        .with_child((
            Node {
                width: percent(0),
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::srgb(0.45, 0.92, 0.87)),
            ProgressFill,
        ));
}

fn update_brand_layout(
    layout: Option<ResMut<BrandIntroLayout>>,
    cameras: Query<&Camera, With<IsDefaultUiCamera>>,
    mut subtitle: Query<&mut Node, With<Subtitle>>,
) {
    let Some(mut layout) = layout else {
        return;
    };
    let Some(viewport) = cameras
        .single()
        .ok()
        .and_then(Camera::logical_viewport_size)
    else {
        return;
    };
    let width = (viewport.x * 0.32).min(240.0);
    let dock_rect = Rect::from_corners(
        Vec2::new(36.0, 24.0),
        Vec2::new(36.0 + width, 24.0 + width * 180.0 / 840.0),
    );
    if layout.dock_rect != dock_rect {
        layout.dock_rect = dock_rect;
    }
    for mut node in &mut subtitle {
        let left = px(dock_rect.min.x + 2.0);
        let top = px(dock_rect.max.y + 12.0);
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
    }
}

fn animate(
    state: Res<VisualState>,
    time: Res<Time>,
    mut objects: Query<(&Motion, &mut Transform, &mut Visibility)>,
) {
    let song = state.song_seconds as f32;
    for (motion, mut transform, mut visibility) in &mut objects {
        match *motion {
            Motion::Spirit(player) => {
                let hit = state.hit_pulses[player].clamp(0.0, 1.0);
                transform.translation.y = 0.85 + (song * PI * 2.0).sin() * 0.035;
                transform.scale = Vec3::new(1.0 + hit * 0.25, 1.0 - hit * 0.25, 1.0 + hit * 0.12);
            }
            Motion::Ripple(player) => {
                let hit = state.hit_pulses[player].clamp(0.0, 1.0);
                *visibility = if hit > 0.01 {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                transform.scale = Vec3::splat(1.0 + (1.0 - hit) * 0.9);
                transform.rotation = Quat::from_rotation_y(player as f32 * PI);
            }
            Motion::SharedRing => {
                let sync = state.sync_pulse.clamp(0.0, 1.0);
                *visibility = if sync > 0.01 {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                transform.scale = Vec3::splat(2.2 + (1.0 - sync) * 1.6);
            }
            Motion::Street(offset) => {
                transform.translation.z = 6.0 - (offset - song * 3.0).rem_euclid(36.0);
            }
            Motion::Rain(phase) => {
                transform.translation.y = 8.0 - (time.elapsed_secs() * 6.0 + phase).rem_euclid(8.0);
                transform.rotation = Quat::from_rotation_z(-0.15);
            }
        }
    }
}

fn update_hud(
    state: Res<VisualState>,
    intro: Option<Res<BrandIntroStatus>>,
    mut status: Query<&mut Text, (With<StatusText>, Without<ClockText>)>,
    mut clock: Query<&mut Text, (With<ClockText>, Without<StatusText>)>,
    mut progress: Query<&mut Node, With<ProgressFill>>,
    mut panel: Query<&mut GlobalZIndex, With<StatusPanel>>,
    mut labels: Query<&mut Visibility, With<PlayerLabel>>,
) {
    if !state.is_changed() && !intro.as_ref().is_some_and(|intro| intro.is_changed()) {
        return;
    }
    // The brand backdrop reveals the scene and HUD; only startup errors render above it
    let failed = intro.is_some_and(|intro| intro.phase == BrandIntroPhase::Failed);
    for mut visible in &mut labels {
        *visible = if state.settings_open {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    for mut layer in &mut panel {
        layer.0 = if failed { 1001 } else { 0 };
    }
    for mut text in &mut status {
        if text.0 != state.status {
            text.0.clone_from(&state.status);
        }
    }
    for mut text in &mut clock {
        let value = format!(
            "{:05.1} / 64.0 SEC\nRESONANCE {:3.0}%  /  {}",
            state.song_seconds.clamp(0.0, 64.0),
            state.resonance.clamp(0.0, 1.0) * 100.0,
            if state.running { "PLAYING" } else { "RESTING" },
        );
        if text.0 != value {
            text.0 = value;
        }
    }
    for mut node in &mut progress {
        node.width = percent((state.song_seconds / 64.0).clamp(0.0, 1.0) as f32 * 100.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_intro_keeps_the_existing_status_above_its_backdrop() {
        let mut app = App::new();
        app.insert_resource(VisualState {
            status: "Startup failed: audio unavailable\nClose window to exit".into(),
            ..default()
        })
        .add_systems(PostUpdate, update_hud);
        let panel = app
            .world_mut()
            .spawn((StatusPanel, GlobalZIndex(0), StatusText, Text::default()))
            .id();
        app.update();
        assert_eq!(app.world().get::<GlobalZIndex>(panel).unwrap().0, 0);
        app.insert_resource(BrandIntroStatus {
            phase: BrandIntroPhase::Failed,
            ..default()
        });
        app.update();
        assert_eq!(app.world().get::<GlobalZIndex>(panel).unwrap().0, 1001);
        assert_eq!(
            app.world().get::<Text>(panel).unwrap().0,
            app.world().resource::<VisualState>().status
        );
    }

    #[test]
    fn presentation_follows_song_time_and_only_facts_light_the_shared_ring() {
        let mut app = App::new();
        app.init_resource::<VisualState>()
            .init_resource::<Time>()
            .add_systems(PostUpdate, animate);
        let street = app
            .world_mut()
            .spawn((
                Motion::Street(9.0),
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        let shared = app
            .world_mut()
            .spawn((
                Motion::SharedRing,
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        app.world_mut().resource_mut::<VisualState>().song_seconds = 2.0;
        app.update();
        let position = app.world().get::<Transform>(street).unwrap().translation;
        assert_eq!(
            *app.world().get::<Visibility>(shared).unwrap(),
            Visibility::Hidden
        );
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(10));
        app.update();
        assert_eq!(
            app.world().get::<Transform>(street).unwrap().translation,
            position
        );
        app.world_mut().resource_mut::<VisualState>().sync_pulse = 1.0;
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(shared).unwrap(),
            Visibility::Visible
        );
        assert_eq!(app.world().resource::<VisualState>().song_seconds, 2.0);
    }
}
