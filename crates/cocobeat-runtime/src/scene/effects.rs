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
    presentation::WorldTheme,
    settings::{QualityPreset, RainAmount},
    view::VisualState,
};

// Fixed pools are reused across StageScene refreshes and never grow with hit density
#[derive(Resource)]
pub(crate) struct EffectAssets {
    meshes: [Handle<Mesh>; 6],
    materials: [Handle<StandardMaterial>; 21],
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
    Contact(usize),
    Partner(usize),
    Ribbon(usize, usize),
    Shock(usize, usize),
    Climax(usize, usize),
    Rain(usize),
}

#[derive(Component)]
pub(super) struct FeedbackLight(usize);

#[derive(Resource, Default)]
pub(super) struct FrozenFeedback {
    song_time: Option<SongTime>,
    hit: [f32; 2],
    miss: [f32; 2],
    free: f32,
    anchor: f32,
    precise: bool,
    rain_clock: f32,
    seed: u64,
    streak: u32,
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
                meshes.add(
                    Torus::new(0.96, 1.0)
                        .mesh()
                        .major_resolution(40)
                        .minor_resolution(6),
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
                    perceptual_roughness: 0.65,
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
        add(Effect::Contact(player), 5, 13 + player);
        add(Effect::Partner(player), 5, 15 + player);
        for index in 0..12 {
            add(Effect::Ribbon(player, index), 3, 17 + player);
        }
        for index in 0..2 {
            add(Effect::Shock(player, index), 5, 15 + player);
        }
        for index in 0..8 {
            add(Effect::Climax(player, index), 2, 19 + player);
        }
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
    for player in 0..2 {
        commands.spawn((
            SceneEntity,
            FeedbackLight(player),
            Visibility::Hidden,
            PointLight {
                intensity: 0.0,
                range: 3.5,
                radius: 0.35,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz((player as f32 * 2.0 - 1.0) * PLAYER_X, 0.65, 0.0),
        ));
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

fn palette(world: WorldTheme) -> [Color; 2] {
    match world {
        WorldTheme::Neon => [Color::srgb(0.20, 0.76, 1.0), Color::srgb(1.0, 0.30, 0.62)],
        WorldTheme::Forest => [Color::srgb(0.25, 1.0, 0.72), Color::srgb(1.0, 0.76, 0.32)],
        WorldTheme::Candy => [Color::srgb(0.42, 0.70, 1.0), Color::srgb(1.0, 0.46, 0.76)],
        WorldTheme::StarSea => [Color::srgb(0.50, 0.65, 1.0), Color::srgb(0.90, 0.54, 1.0)],
    }
}

fn ribbon_point(start: Vec3, progress: f32, side: f32, variant: u64, world: WorldTheme) -> Vec3 {
    let arch = (progress * PI).sin();
    let flourish = match world {
        WorldTheme::Neon => Vec3::ZERO,
        WorldTheme::Forest => {
            Vec3::new(side * (progress * TAU).sin() * arch * 0.2, arch * 0.25, 0.0)
        }
        WorldTheme::Candy => Vec3::Y * (progress * TAU).sin().abs() * arch * 0.35,
        WorldTheme::StarSea => Vec3::new(
            side * (progress * TAU).sin() * arch * 0.30,
            0.0,
            (progress * TAU).cos() * arch * 0.35,
        ),
    };
    start.lerp(Vec3::new(0.0, 1.3, -0.7), progress)
        + flourish
        + Vec3::new(
            side * arch
                * if variant.is_multiple_of(2) {
                    0.20
                } else {
                    -0.20
                },
            arch * (0.38 + (variant % 3) as f32 * 0.13),
            -arch * 0.25,
        )
}

fn material_feedback(index: usize, frozen: &FrozenFeedback, grade: f32) -> (f32, f32) {
    match index {
        0..=1 => (frozen.hit[index].powi(2) * 0.88, 3.0),
        2..=3 => (frozen.hit[index - 2].powi(2), 5.0),
        4..=5 => (envelope(frozen.free).sqrt() * 0.9, 16.0),
        6..=7 => (envelope(frozen.anchor).sqrt() * grade, 22.0),
        8..=9 => (frozen.miss[index - 8].powi(2) * 0.36, 0.08),
        10..=11 => (0.38, 1.0),
        12 => (0.28, 0.06),
        13..=14 => (frozen.hit[index - 13].powi(2), 8.0),
        15..=16 => (envelope(frozen.free.max(frozen.anchor)) * 0.42, 4.0),
        17..=18 => (
            envelope(frozen.free.max(frozen.anchor)).sqrt() * grade,
            20.0,
        ),
        _ => (envelope(frozen.anchor).sqrt() * grade, 26.0),
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn animate(
    state: Res<VisualState>,
    time: Res<Time>,
    assets: Res<EffectAssets>,
    stage: Option<Res<StageScene>>,
    mut frozen: ResMut<FrozenFeedback>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut objects: Query<
        (&Effect, &mut Transform, &mut Visibility, &mut Mesh3d),
        Without<FeedbackLight>,
    >,
    mut lights: Query<
        (
            &FeedbackLight,
            &mut Transform,
            &mut PointLight,
            &mut Visibility,
        ),
        Without<Effect>,
    >,
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
        frozen.seed = state.presentation.event_seed;
        frozen.streak = state.presentation.sync_streak;
    }
    if state.running && !state.transitioning {
        frozen.rain_clock = state.song_seconds as f32;
    } else if state.ready && !state.transitioning {
        frozen.rain_clock += time.delta_secs().min(0.1) * 0.22;
    }
    let detail = state.quality.preset != QualityPreset::Low;
    let high = state.quality.preset == QualityPreset::High;
    let reduced_motion = state.quality.reduced_motion;
    let reduced_flashes = state.quality.reduced_flashes;
    let colors = palette(state.presentation.world);
    let feet: [Vec3; 2] = std::array::from_fn(|player| {
        super::characters::root_transform(&state, player).translation - Vec3::Y * 0.735
    });
    let grade = if frozen.precise { 1.0 } else { 0.68 };
    let slope = stage
        .as_ref()
        .and_then(|s| s.0.sample(state.song_time.clamp(SongTime::ZERO, s.0.end())))
        .map_or(Quat::IDENTITY, |s| {
            Quat::from_rotation_x((s.slope_y_ppm as f32 / 1_000_000.0).atan())
        });
    let song = state.song_seconds as f32;
    for (index, handle) in assets.materials.iter().enumerate() {
        let (alpha, glow) = material_feedback(index, &frozen, grade);
        if let Some(mut material) = materials.get_mut(handle) {
            if index != 12 && !(8..=9).contains(&index) {
                material.base_color = colors[if index < 13 {
                    index % 2
                } else {
                    (index - 13) % 2
                }];
            }
            material.base_color.set_alpha(alpha);
            // Blend already fades the core; a second alpha multiplication erased its HDR tail
            let emission_fade = if matches!(index, 4..=7 | 17..=20) {
                1.0
            } else {
                alpha
            };
            material.emissive = LinearRgba::from(material.base_color.with_alpha(1.0))
                * glow
                * emission_fade
                * if reduced_flashes { 0.20 } else { 1.0 };
        }
    }
    for (light, mut transform, mut point, mut visibility) in &mut lights {
        transform.translation = slope * (feet[light.0] + Vec3::Y * 0.55);
        point.color = colors[light.0];
        point.intensity = if detail && !reduced_flashes {
            9_500.0 * frozen.hit[light.0].powi(2)
                + 18_000.0 * envelope(frozen.free.max(frozen.anchor))
        } else {
            0.0
        };
        *visibility = if point.intensity > 1.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    let drops = match state.quality.rain {
        RainAmount::Off => 0,
        RainAmount::Quarter => 12,
        RainAmount::Half => 24,
        RainAmount::Full => 48,
    };
    for (effect, mut transform, mut visibility, mut mesh) in &mut objects {
        if matches!(effect, Effect::Splash(..) | Effect::Note(..)) {
            let index = match state.presentation.world {
                WorldTheme::Neon => usize::from(matches!(effect, Effect::Note(..))),
                WorldTheme::Forest => 2,
                WorldTheme::Candy => 3,
                WorldTheme::StarSea => 2,
            };
            if mesh.0 != assets.meshes[index] {
                mesh.0 = assets.meshes[index].clone();
            }
        }
        let (position, scale, rotation, visible) = match *effect {
            Effect::Splash(player, index) => {
                let age = 1.0 - frozen.hit[player];
                let angle = index as f32 * TAU / 10.0 + (frozen.seed % 11) as f32 * 0.17;
                let radius = 0.22 + age * 0.75;
                (
                    feet[player]
                        + Vec3::new(
                            angle.cos() * radius,
                            0.06 + (age * PI).sin() * (0.16 + (index % 3) as f32 * 0.04),
                            angle.sin() * radius,
                        ),
                    Vec3::new(0.055, 0.11 * (1.0 - age) + 0.016, 0.055),
                    Quat::from_rotation_z(angle.sin() * 0.5),
                    detail && !reduced_motion && frozen.hit[player] > 0.01 && (high || index < 6),
                )
            }
            Effect::Note(player, index) => {
                let age = 1.0 - frozen.hit[player];
                let angle = index as f32 * TAU / 4.0;
                (
                    feet[player]
                        + Vec3::new(
                            angle.cos() * (0.38 + age * 0.6),
                            0.65 + age * 0.9,
                            0.1 + angle.sin() * 0.35,
                        ),
                    Vec3::splat(0.20 * (1.0 - age * 0.45)),
                    Quat::from_rotation_z(angle * 0.2 + age * 0.8),
                    detail && !reduced_motion && frozen.hit[player] > 0.01 && (high || index < 2),
                )
            }
            Effect::FreeArc(player, index) | Effect::AnchorArc(player, index) => {
                let anchor = matches!(*effect, Effect::AnchorArc(..));
                let pulse = if anchor { frozen.anchor } else { frozen.free };
                let age = 1.0 - pulse;
                let progress = (age * 1.65 - index as f32 * 0.028).clamp(0.0, 1.0);
                let side = player as f32 * 2.0 - 1.0;
                (
                    ribbon_point(
                        feet[player] + Vec3::Y * 0.55,
                        progress,
                        side,
                        frozen.seed,
                        state.presentation.world,
                    ),
                    Vec3::splat(0.014 + index as f32 * 0.0009),
                    Quat::IDENTITY,
                    detail
                        && !reduced_motion
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
                    detail
                        && !reduced_motion
                        && frozen.anchor > 0.01
                        && age > 0.2
                        && (high || index < 2),
                )
            }
            Effect::Miss(player, index) => {
                let age = 1.0 - frozen.miss[player];
                let radius = 0.50 + age * 0.45 + index as f32 * 0.08;
                (
                    feet[player] + Vec3::new(0.0, 0.035 + index as f32 * 0.003, 0.0),
                    Vec3::new(radius, 0.045, radius),
                    Quat::IDENTITY,
                    detail && frozen.miss[player] > 0.01 && (high || index == 0),
                )
            }
            Effect::Ambient(player, index) => {
                let phase = index as f32 * 1.8 + player as f32 * PI;
                let movement = if reduced_motion { 0.0 } else { song };
                let forest = state.presentation.world == WorldTheme::Forest;
                (
                    Vec3::new(
                        (player as f32 * 2.0 - 1.0) * (3.6 + index as f32 * 0.25),
                        1.4 + (movement * 0.7 + phase).sin() * if forest { 0.65 } else { 0.15 },
                        -5.0 - index as f32 * 3.0,
                    ),
                    Vec3::splat(if forest { 0.060 } else { 0.035 }),
                    Quat::from_rotation_z(phase + movement * 0.12),
                    detail && (high || index < 2),
                )
            }
            Effect::Contact(player) => {
                let age = 1.0 - frozen.hit[player];
                let radius = if reduced_motion {
                    0.52
                } else {
                    0.32 + age * 0.70
                };
                (
                    Vec3::new(feet[player].x, 0.045, feet[player].z),
                    Vec3::new(radius, 0.55, radius),
                    Quat::IDENTITY,
                    frozen.hit[player] > 0.015,
                )
            }
            Effect::Partner(player) => {
                let pulse = frozen.free.max(frozen.anchor);
                let age = 1.0 - pulse;
                let radius = 0.55 + if reduced_motion { 0.0 } else { age * 0.30 };
                (
                    Vec3::new(feet[player].x, 0.03, feet[player].z),
                    Vec3::new(radius, 0.28, radius),
                    Quat::IDENTITY,
                    detail && pulse > 0.015,
                )
            }
            Effect::Ribbon(player, index) => {
                let pulse = frozen.free.max(frozen.anchor);
                let age = 1.0 - pulse;
                let head = (age * 2.4).clamp(0.0, 1.0);
                let tail = ((age - 0.25) * 2.4).clamp(0.0, 1.0);
                let a = tail + (head - tail) * index as f32 / 12.0;
                let b = tail + (head - tail) * (index + 1) as f32 / 12.0;
                let start = feet[player] + Vec3::Y * 0.55;
                let side = player as f32 * 2.0 - 1.0;
                let from = ribbon_point(start, a, side, frozen.seed, state.presentation.world);
                let to = ribbon_point(start, b, side, frozen.seed, state.presentation.world);
                let delta = to - from;
                let width = 0.04 + frozen.streak.min(5) as f32 * 0.009;
                (
                    (from + to) * 0.5,
                    Vec3::new(width, delta.length(), width * 0.6),
                    Quat::from_rotation_arc(Vec3::Y, delta.try_normalize().unwrap_or(Vec3::Y)),
                    detail && !reduced_motion && pulse > 0.015 && head > tail,
                )
            }
            Effect::Shock(player, index) => {
                let pulse = frozen.free.max(frozen.anchor);
                let age = 1.0 - pulse;
                let phase = (age - 0.16 - index as f32 * 0.10).max(0.0);
                let radius = 0.28 + phase * (1.6 + frozen.streak.min(5) as f32 * 0.1);
                (
                    Vec3::new(0.0, 0.055 + player as f32 * 0.008, -0.5),
                    Vec3::new(radius, 0.16, radius),
                    if state.presentation.world == WorldTheme::StarSea {
                        Quat::from_rotation_z((player as f32 * 2.0 - 1.0) * 0.28)
                    } else {
                        Quat::IDENTITY
                    },
                    detail
                        && !reduced_motion
                        && pulse > 0.05
                        && phase > 0.0
                        && (index == 0 || (index == 1 && frozen.streak >= 5 && high)),
                )
            }
            Effect::Climax(player, index) => {
                let age = 1.0 - frozen.anchor;
                let angle = index as f32 * TAU / 8.0
                    + player as f32 * PI / 8.0
                    + (frozen.seed % 5) as f32 * 0.24;
                let spread = ((age - 0.20).max(0.0) * 2.0).min(1.0);
                let direction = Vec3::new(angle.cos(), angle.sin().abs(), -0.25);
                let size = (0.035 + spread * 0.10) * (1.0 - age * 0.55);
                (
                    Vec3::new(0.0, 1.2, -0.9) + direction * spread * 2.4,
                    Vec3::new(size * 0.55, size * (1.5 + spread * 1.6), size),
                    Quat::from_rotation_z(angle - PI * 0.5),
                    detail
                        && !reduced_motion
                        && frozen.anchor > 0.04
                        && age > 0.20
                        && frozen.streak >= 3
                        && (high || index % 2 == 0),
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
                    index < drops
                        && !reduced_motion
                        && state.presentation.world == WorldTheme::Neon,
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
    #[allow(clippy::type_complexity)]
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
                 roots: Query<Entity, Or<(With<Effect>, With<FeedbackLight>)>>| {
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
            app.world_mut()
                .query::<&FeedbackLight>()
                .iter(app.world())
                .count(),
            2
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
            if !matches!(effect, Effect::Rain(_) | Effect::Contact(_)) {
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

    #[test]
    fn bloom_toggle_keeps_feedback_and_accessibility_reduces_only_ornament() {
        let mut app = effect_app();
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.quality.bloom = false;
            state.hit_pulses = [0.75; 2];
            state.anchor_sync_pulse = 0.65;
            state.presentation.sync_streak = 4;
        }
        tick(&mut app);
        let count = |app: &mut App| {
            app.world_mut()
                .query::<(&Effect, &Visibility)>()
                .iter(app.world())
                .filter(|(effect, visibility)| {
                    !matches!(effect, Effect::Rain(_)) && **visibility != Visibility::Hidden
                })
                .count()
        };
        let without_bloom = count(&mut app);
        assert!(without_bloom > 20);
        let core = app.world().resource::<EffectAssets>().materials[6].clone();
        let core_green = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&core)
            .unwrap()
            .emissive
            .green;
        assert!(core_green > 5.0, "confirmed core must retain HDR headroom");
        app.world_mut().resource_mut::<VisualState>().quality.bloom = true;
        tick(&mut app);
        assert_eq!(count(&mut app), without_bloom);
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.quality.reduced_motion = true;
            state.quality.reduced_flashes = true;
        }
        tick(&mut app);
        assert!(count(&mut app) < without_bloom);
        let mut effects = app.world_mut().query::<(&Effect, &Visibility)>();
        assert!(
            effects
                .iter(app.world())
                .any(|(effect, visibility)| matches!(effect, Effect::Contact(_))
                    && *visibility != Visibility::Hidden)
        );
        let mut lights = app.world_mut().query::<&PointLight>();
        assert!(lights.iter(app.world()).all(|light| light.intensity == 0.0));
        assert!(
            (app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&core)
                .unwrap()
                .emissive
                .green
                - core_green * 0.2)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn world_ribbons_have_distinct_paths_and_shared_endpoints() {
        let start = Vec3::new(-1.35, 0.55, 0.0);
        let worlds = [
            WorldTheme::Neon,
            WorldTheme::Forest,
            WorldTheme::Candy,
            WorldTheme::StarSea,
        ];
        let midpoints = worlds.map(|world| ribbon_point(start, 0.37, -1.0, 12, world));
        for (index, world) in worlds.into_iter().enumerate() {
            assert!(ribbon_point(start, 0.0, -1.0, 12, world).abs_diff_eq(start, 1e-5));
            assert!(
                ribbon_point(start, 1.0, -1.0, 12, world)
                    .abs_diff_eq(Vec3::new(0.0, 1.3, -0.7), 1e-5)
            );
            assert!(midpoints[index].is_finite());
            for other in &midpoints[index + 1..] {
                assert!(midpoints[index].distance(*other) > 0.01);
            }
        }
    }
}
