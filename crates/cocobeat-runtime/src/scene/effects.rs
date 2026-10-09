use std::f32::consts::{PI, TAU};

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use cocobeat_schema::SongTime;

use super::{SceneEntity, StageScene};

const PLAYER_X: f32 = 1.35;
use crate::{
    settings::{QualityPreset, RainAmount},
    view::VisualState,
};

// Reused across StageScene refreshes; each scene has exactly 162 decoration entities
#[derive(Resource)]
pub(crate) struct EffectAssets {
    meshes: [Handle<Mesh>; 5],
    materials: [Handle<StandardMaterial>; 13],
}

#[derive(Component, Clone, Copy)]
pub(super) enum Effect {
    Splash(usize, usize),
    Note(usize, usize),
    FreeArc(usize, usize),
    AnchorArc(usize, usize),
    Bloom(usize, usize),
    Miss(usize, usize),
    Ambient(usize, usize),
    Rain(usize),
}

#[derive(Resource, Default)]
pub(super) struct FrozenFeedback {
    song_time: Option<SongTime>,
    hit: [f32; 2],
    miss: [f32; 2],
    free: f32,
    anchor: f32,
    precise: bool,
    rain_clock: f32,
}

fn star_mesh() -> Mesh {
    let mut positions = vec![[0.0, 0.0, 0.0]];
    for index in 0..8 {
        let angle = index as f32 * PI / 4.0;
        let radius = if index % 2 == 0 { 1.0 } else { 0.24 };
        positions.push([angle.cos() * radius, angle.sin() * radius, 0.0]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 9])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0; 2]; 9])
    .with_inserted_indices(Indices::U32(
        (0..8).flat_map(|i| [0, i + 1, (i + 1) % 8 + 1]).collect(),
    ))
}

fn note_mesh() -> Mesh {
    let mut mesh = Sphere::new(1.0).mesh().uv(12, 8).transformed_by(
        Transform::from_xyz(-0.2, -0.3, 0.0)
            .with_scale(Vec3::new(0.28, 0.18, 0.09))
            .with_rotation(Quat::from_rotation_z(0.3)),
    );
    for (position, scale, turn) in [
        (Vec3::new(0.02, 0.11, 0.0), Vec3::new(0.07, 0.8, 0.07), 0.0),
        (
            Vec3::new(0.18, 0.38, 0.0),
            Vec3::new(0.35, 0.1, 0.07),
            -0.45,
        ),
    ] {
        let part = Mesh::from(Cuboid::default()).transformed_by(
            Transform::from_translation(position)
                .with_scale(scale)
                .with_rotation(Quat::from_rotation_z(turn)),
        );
        mesh.merge(&part)
            .expect("native sphere and cuboid share vertex attributes");
    }
    mesh
}

pub(super) fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    cached: Option<&EffectAssets>,
) {
    let fresh = if cached.is_none() {
        let colors = [Color::srgb(0.32, 0.78, 1.0), Color::srgb(1.0, 0.48, 0.73)];
        Some(EffectAssets {
            meshes: [
                meshes.add(Sphere::new(1.0).mesh().uv(12, 8)),
                meshes.add(note_mesh()),
                meshes.add(star_mesh()),
                meshes.add(Cuboid::default()),
                meshes.add(
                    Torus::new(0.91, 1.0)
                        .mesh()
                        .major_resolution(24)
                        .minor_resolution(6)
                        .angle_range(0.0..=PI * 1.35),
                ),
            ],
            materials: std::array::from_fn(|index| {
                let color = if index == 12 {
                    Color::srgb(0.53, 0.70, 0.87)
                } else if index / 2 == 4 {
                    Color::srgb(0.45, 0.32, 0.43)
                } else {
                    colors[index % 2]
                };
                materials.add(StandardMaterial {
                    base_color: color.with_alpha(0.0),
                    emissive: LinearRgba::from(color) * 0.1,
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: if index < 2 || index == 12 { 0.15 } else { 0.4 },
                    reflectance: 0.65,
                    cull_mode: None,
                    ..default()
                })
            }),
        })
    } else {
        None
    };
    let assets = cached.or(fresh.as_ref()).expect("effect asset pool exists");
    let mut add = |effect, mesh: usize, material: usize| {
        commands.spawn((
            SceneEntity,
            effect,
            Mesh3d(assets.meshes[mesh].clone()),
            MeshMaterial3d(assets.materials[material].clone()),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
        ));
    };
    for player in 0..2 {
        for index in 0..10 {
            add(Effect::Splash(player, index), 0, player);
        }
        for index in 0..4 {
            add(Effect::Note(player, index), 1, 2 + player);
        }
        for index in 0..14 {
            add(Effect::FreeArc(player, index), 0, 4 + player);
        }
        for index in 0..18 {
            add(Effect::AnchorArc(player, index), 0, 6 + player);
        }
        for index in 0..4 {
            add(Effect::Bloom(player, index), 2, 6 + player);
        }
        for index in 0..3 {
            add(Effect::Miss(player, index), 4, 8 + player);
        }
        for index in 0..4 {
            add(Effect::Ambient(player, index), 2, 10 + player);
        }
    }
    for index in 0..48 {
        add(Effect::Rain(index), 3, 12);
    }
    if let Some(fresh) = fresh {
        commands.insert_resource(fresh);
    }
    commands.insert_resource(FrozenFeedback::default());
}

fn envelope(pulse: f32) -> f32 {
    let attack = ((1.0 - pulse) / 0.12).clamp(0.0, 1.0);
    pulse * pulse * (0.12 + 0.88 * attack * attack * (3.0 - 2.0 * attack))
}

pub(super) fn animate(
    state: Res<VisualState>,
    time: Res<Time>,
    assets: Res<EffectAssets>,
    stage: Option<Res<StageScene>>,
    mut frozen: ResMut<FrozenFeedback>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut objects: Query<(&Effect, &mut Transform, &mut Visibility)>,
) {
    // app feedback decays on UI time even while paused; pin decorations to the song cursor
    if frozen.song_time.is_none()
        || (!state.paused && !state.transitioning)
        || frozen.song_time != Some(state.song_time)
    {
        frozen.song_time = Some(state.song_time);
        frozen.hit = state.hit_pulses.map(|p| p.clamp(0.0, 1.0));
        frozen.miss = state.miss_pulses.map(|p| p.clamp(0.0, 1.0));
        frozen.free = state.free_sync_pulse.clamp(0.0, 1.0);
        frozen.anchor = state.anchor_sync_pulse.clamp(0.0, 1.0);
        frozen.precise = state.anchor_sync_precise;
    }
    if !state.transitioning && (state.running || state.ready) {
        frozen.rain_clock += time.delta_secs().min(0.1) * if state.running { 1.0 } else { 0.22 };
    }
    let detail = state.quality.bloom && state.quality.preset != QualityPreset::Low;
    let high = state.quality.preset == QualityPreset::High;
    let grade = if frozen.precise { 1.0 } else { 0.68 };
    let slope = stage
        .as_ref()
        .and_then(|s| s.0.sample(state.song_time.clamp(SongTime::ZERO, s.0.end())))
        .map_or(Quat::IDENTITY, |s| {
            Quat::from_rotation_x((s.slope_y_ppm as f32 / 1_000_000.0).atan())
        });
    let song = state.song_seconds as f32;
    for (index, handle) in assets.materials.iter().enumerate() {
        let (alpha, glow) = match index / 2 {
            0 => (frozen.hit[index % 2].powi(2) * 0.72, 0.32),
            1 => (frozen.hit[index % 2].powi(2), 1.6),
            2 => (envelope(frozen.free) * 0.7, 1.4),
            3 => (envelope(frozen.anchor) * grade, 2.1),
            4 => (frozen.miss[index % 2].powi(2) * 0.32, 0.04),
            5 => (0.26, 0.3),
            _ => (0.38, 0.08),
        };
        if let Some(mut material) = materials.get_mut(handle) {
            material.base_color.set_alpha(alpha);
            material.emissive = LinearRgba::from(material.base_color.with_alpha(1.0)) * glow;
        }
    }
    let drops = match state.quality.rain {
        RainAmount::Off => 0,
        RainAmount::Quarter => 12,
        RainAmount::Half => 24,
        RainAmount::Full => 48,
    };
    for (effect, mut transform, mut visibility) in &mut objects {
        let (position, scale, rotation, visible) = match *effect {
            Effect::Splash(player, index) => {
                let age = 1.0 - frozen.hit[player];
                let angle = index as f32 * TAU / 10.0;
                let radius = 0.22 + age * 0.75;
                (
                    Vec3::new(
                        (player as f32 * 2.0 - 1.0) * PLAYER_X + angle.cos() * radius,
                        0.06 + (age * PI).sin() * (0.16 + (index % 3) as f32 * 0.04),
                        angle.sin() * radius,
                    ),
                    Vec3::new(0.038, 0.07 * (1.0 - age) + 0.016, 0.038),
                    Quat::from_rotation_z(angle.sin() * 0.5),
                    detail && frozen.hit[player] > 0.01 && (high || index < 6),
                )
            }
            Effect::Note(player, index) => {
                let age = 1.0 - frozen.hit[player];
                let angle = index as f32 * TAU / 4.0;
                (
                    Vec3::new(
                        (player as f32 * 2.0 - 1.0) * PLAYER_X + angle.cos() * (0.38 + age * 0.6),
                        0.65 + age * 0.9,
                        0.1 + angle.sin() * 0.35,
                    ),
                    Vec3::splat(0.17 * (1.0 - age * 0.45)),
                    Quat::from_rotation_z(angle * 0.2 + age * 0.8),
                    detail && frozen.hit[player] > 0.01 && (high || index < 2),
                )
            }
            Effect::FreeArc(player, index) | Effect::AnchorArc(player, index) => {
                let anchor = matches!(*effect, Effect::AnchorArc(..));
                let pulse = if anchor { frozen.anchor } else { frozen.free };
                let age = 1.0 - pulse;
                let progress = (age * 1.65 - index as f32 * 0.028).clamp(0.0, 1.0);
                let side = player as f32 * 2.0 - 1.0;
                (
                    Vec3::new(
                        side * PLAYER_X * (1.0 - progress),
                        0.58 + (progress * PI).sin() * 0.72,
                        -0.30 - progress * 0.5,
                    ),
                    Vec3::splat(0.04 + index as f32 * 0.0018),
                    Quat::IDENTITY,
                    detail
                        && pulse > 0.01
                        && progress > 0.0
                        && progress < 1.0
                        && (high || index % 2 == 0),
                )
            }
            Effect::Bloom(player, index) => {
                let age = 1.0 - frozen.anchor;
                let angle = index as f32 * TAU / 4.0 + player as f32 * PI / 4.0;
                (
                    Vec3::new(
                        angle.cos() * (0.12 + age * 0.52),
                        1.12 + angle.sin() * (0.12 + age * 0.52),
                        -0.8,
                    ),
                    Vec3::splat(0.11 + age * 0.10),
                    Quat::from_rotation_z(angle + age * 0.8),
                    detail && frozen.anchor > 0.01 && age > 0.2 && (high || index < 2),
                )
            }
            Effect::Miss(player, index) => {
                let age = 1.0 - frozen.miss[player];
                let radius = 0.50 + age * 0.45 + index as f32 * 0.08;
                (
                    Vec3::new(
                        (player as f32 * 2.0 - 1.0) * PLAYER_X,
                        0.035 + index as f32 * 0.003,
                        0.0,
                    ),
                    Vec3::new(radius, 0.045, radius),
                    Quat::IDENTITY,
                    detail && frozen.miss[player] > 0.01 && (high || index == 0),
                )
            }
            Effect::Ambient(player, index) => {
                let phase = index as f32 * 1.8 + player as f32 * PI;
                (
                    Vec3::new(
                        (player as f32 * 2.0 - 1.0) * (3.6 + index as f32 * 0.25),
                        1.4 + (song * 0.7 + phase).sin() * 0.15,
                        -5.0 - index as f32 * 3.0,
                    ),
                    Vec3::splat(0.035),
                    Quat::from_rotation_z(phase + song * 0.12),
                    detail && (high || index < 2),
                )
            }
            Effect::Rain(index) => {
                let phase = index as f32 * 0.73;
                (
                    Vec3::new(
                        (phase * 3.7).sin() * 5.8,
                        7.0 - (frozen.rain_clock * 5.5 + phase).rem_euclid(7.0),
                        -2.0 - ((index * 7) % 29) as f32,
                    ),
                    Vec3::new(0.009, 0.16 + (index % 4) as f32 * 0.045, 0.009),
                    Quat::from_rotation_z(-0.17),
                    index < drops,
                )
            }
        };
        transform.translation = slope * position;
        transform.scale = scale;
        transform.rotation = slope * rotation;
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::system::RunSystemOnce, mesh::VertexAttributeValues};
    use std::time::Duration;

    #[test]
    fn decorative_meshes_have_finite_positions_and_valid_indices() {
        for mesh in [star_mesh(), note_mesh()] {
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions");
            };
            assert!(positions.iter().flatten().all(|v| v.is_finite()));
            assert!(mesh.indices().unwrap().iter().all(|i| i < positions.len()));
        }
        for pulse in [0.0, 0.2, 0.5, 0.9, 1.0] {
            assert!((0.0..=1.0).contains(&envelope(pulse)));
        }
    }

    fn effect_app() -> App {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Time>()
            .init_resource::<VisualState>()
            .add_systems(Update, animate);
        app.world_mut()
            .run_system_once(
                |mut commands: Commands,
                 mut meshes: ResMut<Assets<Mesh>>,
                 mut materials: ResMut<Assets<StandardMaterial>>| {
                    spawn(&mut commands, &mut meshes, &mut materials, None);
                },
            )
            .unwrap();
        app
    }

    fn tick(app: &mut App) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(100));
        app.update();
    }

    #[test]
    fn paused_feedback_freezes_then_terminal_settles_and_ready_moves_rain() {
        let mut app = effect_app();
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.running = true;
            state.ready = false;
            state.hit_pulses = [1.0; 2];
        }
        tick(&mut app);
        let before: Vec<_> = app
            .world_mut()
            .query::<(Entity, &Transform, &Visibility)>()
            .iter(app.world())
            .map(|(e, t, v)| (e, *t, *v))
            .collect();
        let rain_clock = app.world().resource::<FrozenFeedback>().rain_clock;
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            assert_eq!(state.song_time, SongTime::ZERO);
            state.running = false;
            state.paused = true;
            state.hit_pulses = [0.0; 2];
        }
        tick(&mut app);
        let after: Vec<_> = app
            .world_mut()
            .query::<(Entity, &Transform, &Visibility)>()
            .iter(app.world())
            .map(|(e, t, v)| (e, *t, *v))
            .collect();
        assert_eq!(before, after);
        assert_eq!(
            app.world().resource::<FrozenFeedback>().rain_clock,
            rain_clock
        );
        assert_eq!(app.world().resource::<FrozenFeedback>().hit, [1.0; 2]);
        app.world_mut().resource_mut::<VisualState>().paused = false;
        tick(&mut app);
        assert_eq!(app.world().resource::<FrozenFeedback>().hit, [0.0; 2]);
        assert_eq!(
            app.world().resource::<FrozenFeedback>().rain_clock,
            rain_clock
        );
        let mut effects = app.world_mut().query::<(&Effect, &Visibility)>();
        assert!(effects.iter(app.world()).all(|(effect, visible)| {
            !matches!(effect, Effect::Splash(..) | Effect::Note(..))
                || *visible == Visibility::Hidden
        }));
        app.world_mut().resource_mut::<VisualState>().ready = true;
        tick(&mut app);
        assert!(app.world().resource::<FrozenFeedback>().rain_clock > rain_clock);
        assert_eq!(app.world().resource::<FrozenFeedback>().hit, [0.0; 2]);
    }

    #[test]
    fn refresh_at_same_cursor_clears_feedback_and_reuses_assets() {
        let mut app = effect_app();
        app.world_mut().resource_mut::<VisualState>().hit_pulses = [1.0; 2];
        tick(&mut app);
        assert_eq!(app.world().resource::<FrozenFeedback>().hit, [1.0; 2]);
        let effects_before = app.world_mut().query::<&Effect>().iter(app.world()).count();
        let assets_before = (
            app.world().resource::<Assets<Mesh>>().len(),
            app.world().resource::<Assets<StandardMaterial>>().len(),
        );
        app.world_mut()
            .run_system_once(
                |mut commands: Commands,
                 mut meshes: ResMut<Assets<Mesh>>,
                 mut materials: ResMut<Assets<StandardMaterial>>,
                 cached: Res<EffectAssets>,
                 roots: Query<Entity, With<Effect>>| {
                    for entity in &roots {
                        commands.entity(entity).despawn();
                    }
                    spawn(&mut commands, &mut meshes, &mut materials, Some(&cached));
                },
            )
            .unwrap();
        assert_eq!(app.world().resource::<FrozenFeedback>().hit, [0.0; 2]);
        assert_eq!(app.world().resource::<FrozenFeedback>().song_time, None);
        assert_eq!(
            effects_before,
            app.world_mut().query::<&Effect>().iter(app.world()).count()
        );
        assert_eq!(
            assets_before,
            (
                app.world().resource::<Assets<Mesh>>().len(),
                app.world().resource::<Assets<StandardMaterial>>().len()
            )
        );
        app.world_mut().resource_mut::<VisualState>().hit_pulses = [0.0; 2];
        tick(&mut app);
        let mut effects = app.world_mut().query::<(&Effect, &Visibility)>();
        for (effect, visible) in effects.iter(app.world()) {
            if matches!(effect, Effect::Splash(..) | Effect::Note(..)) {
                assert_eq!(*visible, Visibility::Hidden);
            }
        }
    }

    #[test]
    fn low_hides_optional_effects_without_touching_necessary_motion() {
        let mut app = effect_app();
        let sentinel = Transform::from_xyz(0.1, 0.2, 0.3);
        let rings: Vec<_> = [
            super::super::Motion::FreeRing,
            super::super::Motion::AnchorRing(0),
            super::super::Motion::AnchorPreview,
        ]
        .into_iter()
        .map(|motion| {
            app.world_mut()
                .spawn((motion, sentinel, Visibility::Visible))
                .id()
        })
        .collect();
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.quality.set_preset(QualityPreset::Low);
            state.hit_pulses = [0.8; 2];
            state.free_sync_pulse = 0.8;
            state.anchor_sync_pulse = 0.8;
            state.miss_pulses = [0.8; 2];
        }
        tick(&mut app);
        let mut effects = app.world_mut().query::<(&Effect, &Visibility)>();
        for (effect, visible) in effects.iter(app.world()) {
            if !matches!(effect, Effect::Rain(_)) {
                assert_eq!(*visible, Visibility::Hidden);
            }
        }
        for entity in rings {
            assert_eq!(app.world().get::<Transform>(entity), Some(&sentinel));
            assert_eq!(
                app.world().get::<Visibility>(entity),
                Some(&Visibility::Visible)
            );
        }
    }
}
