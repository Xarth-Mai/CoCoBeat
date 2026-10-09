//! Original blue/pink plush characters; animation reads presentation facts only

use std::f32::consts::{PI, TAU};

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};

use super::{SceneEntity, part};
use crate::view::VisualState;

#[derive(Clone, Copy)]
enum Joint {
    Eye,
    Ear(f32),
    Crest,
    Hand(f32),
    Foot(f32),
    Mouth,
    Badge,
}

#[derive(Component)]
pub(crate) struct CharacterJoint {
    player: usize,
    joint: Joint,
    rest: Transform,
}

fn joint(player: usize, kind: Joint, rest: Transform) -> impl Bundle {
    (
        SceneEntity,
        rest,
        Visibility::default(),
        CharacterJoint {
            player,
            joint: kind,
            rest,
        },
    )
}

/// A closed continuous surface, with analytic normals shared across the angular seam
/// Positive taper widens the belly; bend curls the upper tip without a cone seam
fn plush_mesh(size: Vec3, taper: f32, bend: f32) -> Mesh {
    const SIDES: usize = 40;
    const ROWS: usize = 24;
    let mut positions = Vec::with_capacity((ROWS + 1) * SIDES);
    let mut normals = Vec::with_capacity((ROWS + 1) * SIDES);
    let mut uvs = Vec::with_capacity((ROWS + 1) * SIDES);
    let mut indices = Vec::with_capacity((ROWS - 1) * SIDES * 6);
    for row in 0..=ROWS {
        let latitude = -PI * 0.5 + PI * row as f32 / ROWS as f32;
        let (y, mut c) = latitude.sin_cos();
        if row == 0 || row == ROWS {
            c = 0.0;
        }
        let radius = c * (1.0 - taper * y);
        let radial_derivative = -y * (1.0 - taper * y) - taper * c * c;
        let curl = bend * (y + 1.0).powi(2) * 0.25;
        let curl_derivative = bend * (y + 1.0) * c * 0.5;
        for side in 0..SIDES {
            let angle = TAU * side as f32 / SIDES as f32;
            let (s, a) = angle.sin_cos();
            positions.push([size.x * radius * a + curl, size.y * y, size.z * radius * s]);
            normals.push(
                Vec3::new(
                    size.y * c * a / size.x,
                    -radial_derivative - curl_derivative * a / size.x,
                    size.y * c * s / size.z,
                )
                .normalize()
                .to_array(),
            );
            uvs.push([side as f32 / SIDES as f32, row as f32 / ROWS as f32]);
        }
    }
    for row in 0..ROWS {
        for side in 0..SIDES {
            let a = (row * SIDES + side) as u32;
            let b = (row * SIDES + (side + 1) % SIDES) as u32;
            let c = a + SIDES as u32;
            let d = b + SIDES as u32;
            if row != 0 {
                indices.extend([a, c, b]);
            }
            if row != ROWS - 1 {
                indices.extend([b, c, d]);
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

fn body_front(size: Vec3, x: f32, y: f32) -> f32 {
    let latitude = y / size.y;
    let radius = (1.0 - latitude * latitude).sqrt() * (1.0 - 0.12 * latitude);
    size.z * (radius * radius - (x / size.x).powi(2)).sqrt()
}

// The thin closed chest patch follows the belly instead of disappearing behind it
fn chest_mesh(size: Vec3) -> Mesh {
    let mut mesh = plush_mesh(Vec3::new(0.21, 0.16, 0.022), 0.0, 0.0);
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    else {
        unreachable!("plush positions are Float32x3")
    };
    for position in positions {
        position[1] -= 0.3;
        position[2] += body_front(size, position[0], position[1]) + 0.006;
    }
    mesh.with_computed_smooth_normals()
}

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    player: usize,
) {
    let color = [Color::srgb(0.47, 0.82, 0.98), Color::srgb(1.0, 0.65, 0.79)][player];
    let skin = materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.52,
        reflectance: 0.34,
        ..default()
    });
    let cream = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.95, 0.91),
        perceptual_roughness: 0.65,
        ..default()
    });
    let blush = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.49, 0.62),
        perceptual_roughness: 0.65,
        ..default()
    });
    let ink = materials.add(StandardMaterial {
        base_color: Color::srgb(0.035, 0.048, 0.095),
        perceptual_roughness: 0.28,
        ..default()
    });
    let iris = materials.add(StandardMaterial {
        base_color: [Color::srgb(0.09, 0.35, 0.51), Color::srgb(0.42, 0.15, 0.34)][player],
        perceptual_roughness: 0.2,
        ..default()
    });
    let glint = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.98, 0.96),
        unlit: true,
        ..default()
    });
    let accent = materials.add(StandardMaterial {
        base_color: color,
        emissive: LinearRgba::from(color) * 0.13,
        metallic: 0.16,
        perceptual_roughness: 0.3,
        ..default()
    });
    let size = if player == 0 {
        Vec3::new(0.59, 0.64, 0.5)
    } else {
        Vec3::new(0.7, 0.53, 0.5)
    };
    let body = meshes.add(plush_mesh(size, 0.12, 0.0));
    let chest = meshes.add(chest_mesh(size));
    let small = meshes.add(Sphere::new(1.0).mesh().uv(20, 12));
    let mitten = meshes.add(
        Capsule3d::new(0.095, 0.17)
            .mesh()
            .longitudes(20)
            .latitudes(12),
    );
    let shoe = meshes.add(plush_mesh(Vec3::new(0.21, 0.115, 0.27), 0.0, 0.0));
    let smile = meshes.add(
        Torus::new(0.047, 0.059)
            .mesh()
            .major_resolution(20)
            .minor_resolution(6)
            .angle_range(0.0..=PI),
    );
    parent.spawn(part(&body, &skin, Vec3::ZERO, Vec3::ONE));

    if player == 0 {
        let ear = meshes.add(plush_mesh(Vec3::new(0.14, 0.34, 0.13), 0.18, 0.025));
        let inset = meshes.add(plush_mesh(Vec3::new(0.078, 0.25, 0.018), 0.18, 0.025));
        for side in [-1.0, 1.0] {
            let rest = Transform::from_xyz(side * 0.28, 0.48, 0.0)
                .with_rotation(Quat::from_rotation_z(-side * 0.12));
            parent
                .spawn(joint(player, Joint::Ear(side), rest))
                .with_children(|ear_root| {
                    ear_root.spawn(part(&ear, &skin, Vec3::new(0.0, 0.28, 0.0), Vec3::ONE));
                    ear_root.spawn(part(
                        &inset,
                        &cream,
                        Vec3::new(0.0, 0.285, 0.118),
                        Vec3::ONE,
                    ));
                });
        }
    } else {
        let crest = meshes.add(plush_mesh(Vec3::new(0.24, 0.34, 0.17), 0.62, 0.12));
        let inset = meshes.add(plush_mesh(Vec3::new(0.12, 0.23, 0.018), 0.5, 0.085));
        let rest = Transform::from_xyz(-0.055, 0.49, 0.0);
        parent
            .spawn(joint(player, Joint::Crest, rest))
            .with_children(|crest_root| {
                crest_root.spawn(part(&crest, &skin, Vec3::new(0.0, 0.16, 0.0), Vec3::ONE));
                crest_root.spawn(part(&inset, &cream, Vec3::new(0.0, 0.13, 0.156), Vec3::ONE));
            });
    }

    for side in [-1.0, 1.0] {
        let rest = Transform::from_xyz(
            side * 0.19,
            0.115,
            body_front(size, side * 0.19, 0.115) + 0.04,
        )
        .with_rotation(Quat::from_rotation_y(side * 0.18));
        parent
            .spawn(joint(player, Joint::Eye, rest))
            .with_children(|eye| {
                for (material, position, scale) in [
                    (&cream, Vec3::ZERO, Vec3::new(0.104, 0.135, 0.028)),
                    (
                        &iris,
                        Vec3::new(0.0, -0.012, 0.026),
                        Vec3::new(0.062, 0.077, 0.014),
                    ),
                    (
                        &ink,
                        Vec3::new(0.0, -0.012, 0.038),
                        Vec3::new(0.033, 0.051, 0.01),
                    ),
                    (
                        &glint,
                        Vec3::new(-0.018, 0.03, 0.046),
                        Vec3::new(0.019, 0.025, 0.007),
                    ),
                    (
                        &glint,
                        Vec3::new(0.022, -0.039, 0.047),
                        Vec3::new(0.008, 0.01, 0.005),
                    ),
                ] {
                    eye.spawn(part(
                        &small,
                        material,
                        position * Vec3::new(1.12, 1.12, 1.0),
                        scale * Vec3::new(1.12, 1.12, 1.0),
                    ));
                }
            });
        parent.spawn(part(
            &small,
            &blush,
            Vec3::new(side * 0.325, -0.066, 0.416),
            Vec3::new(0.094, 0.045, 0.014),
        ));
        let hand_rest = Transform::from_xyz(side * size.x * 0.88, -0.21, 0.01)
            .with_rotation(Quat::from_rotation_z(side * 0.23));
        parent
            .spawn(joint(player, Joint::Hand(side), hand_rest))
            .with_children(|hand| {
                hand.spawn(part(&mitten, &skin, Vec3::new(0.0, -0.10, 0.0), Vec3::ONE));
            });
        let foot_rest = Transform::from_xyz(side * 0.28, -0.58, 0.095);
        parent
            .spawn(joint(player, Joint::Foot(side), foot_rest))
            .with_children(|foot| {
                foot.spawn(part(&shoe, &skin, Vec3::ZERO, Vec3::ONE));
            });
    }
    let mouth_rest =
        Transform::from_xyz(0.0, -0.075, 0.503).with_rotation(Quat::from_rotation_x(PI * 0.5));
    parent
        .spawn(joint(player, Joint::Mouth, mouth_rest))
        .with_children(|mouth| {
            mouth.spawn(part(&smile, &ink, Vec3::ZERO, Vec3::ONE));
        });
    parent.spawn(part(&chest, &cream, Vec3::ZERO, Vec3::ONE));
    let chest_relief = 0.022 * (1.0_f32 - (0.015_f32 / 0.16).powi(2)).sqrt();
    let badge_rest = Transform::from_xyz(
        0.0,
        -0.285,
        body_front(size, 0.0, -0.285) + 0.006 + chest_relief + 0.018,
    )
    .with_rotation(Quat::from_rotation_z(PI * 0.25));
    parent
        .spawn(joint(player, Joint::Badge, badge_rest))
        .with_children(|badge| {
            badge.spawn(part(
                &small,
                &accent,
                Vec3::ZERO,
                Vec3::new(0.07, 0.09, 0.03),
            ));
        });
}

pub(crate) fn animate(
    state: Res<VisualState>,
    time: Res<Time>,
    mut idle_seconds: Local<f64>,
    mut joints: Query<(&CharacterJoint, &mut Transform)>,
) {
    // Paused/recovering/transition poses are held, including an in-progress blink
    if state.transitioning || state.paused {
        return;
    }
    if !state.running && !state.ready {
        for (part, mut transform) in &mut joints {
            *transform = part.rest;
        }
        return;
    }
    let seconds = if state.ready {
        *idle_seconds += time.delta_secs_f64();
        *idle_seconds
    } else {
        state.song_seconds.max(0.0)
    };
    for (part, mut transform) in &mut joints {
        let player = part.player;
        let phase = seconds + player as f64 * 0.39;
        let blink_phase = phase.rem_euclid(4.1) as f32;
        let sway = (phase * 2.1).sin() as f32;
        let step = (phase * f64::from(TAU)).sin() as f32;
        let blink = (1.0 - ((blink_phase - 0.15) / 0.085).abs()).clamp(0.0, 1.0);
        let hit = if state.running {
            state.hit_pulses[player].clamp(0.0, 1.0)
        } else {
            0.0
        };
        let miss = if state.running {
            state.miss_pulses[player].clamp(0.0, 1.0)
        } else {
            0.0
        };
        let sync = if state.running {
            state
                .free_sync_pulse
                .max(state.anchor_sync_pulse)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        *transform = part.rest;
        match part.joint {
            Joint::Eye => {
                transform.scale.y = (1.0 - blink * 0.96) * (1.0 - miss * 0.22);
            }
            Joint::Ear(side) => {
                transform.rotation *=
                    Quat::from_rotation_z(side * (0.045 * sway + hit * 0.24 + sync * 0.12));
            }
            Joint::Crest => {
                transform.rotation *= Quat::from_rotation_z(0.045 * sway + hit * 0.18);
            }
            Joint::Hand(side) => {
                transform.rotation *= Quat::from_rotation_z(-side * (hit * 0.6 + sync * 0.42));
                transform.rotation *= Quat::from_rotation_x(step * 0.06);
            }
            Joint::Foot(side) => {
                transform.rotation *= Quat::from_rotation_x(side * step * 0.075);
            }
            Joint::Mouth => {
                transform.scale = Vec3::new(1.0 + sync * 0.18, 1.0, 1.0 - miss * 0.75);
            }
            Joint::Badge => {
                transform.scale = Vec3::splat(1.0 + sync * 0.2 + hit * 0.08);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn ready_blink_running_song_pose_and_paused_partial_blink_use_actual_ecs() {
        let mut world = World::new();
        world.insert_resource(VisualState {
            ready: true,
            hit_pulses: [1.0; 2],
            free_sync_pulse: 1.0,
            ..default()
        });
        world.insert_resource(Time::<()>::default());
        let mut schedule = Schedule::default();
        schedule.add_systems(animate);
        let rest = Transform::from_xyz(0.1, 0.2, 0.3).with_rotation(Quat::from_rotation_x(0.2));
        let entities: Vec<_> = [
            Joint::Eye,
            Joint::Ear(-1.0),
            Joint::Crest,
            Joint::Hand(1.0),
            Joint::Foot(-1.0),
            Joint::Mouth,
            Joint::Badge,
        ]
        .into_iter()
        .map(|kind| world.spawn(joint(0, kind, rest)).id())
        .collect();
        let poses = |world: &World| {
            entities
                .iter()
                .map(|entity| *world.get::<Transform>(*entity).unwrap())
                .collect::<Vec<_>>()
        };

        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f64(0.1075));
        schedule.run(&mut world);
        let ready = poses(&world);
        assert!((ready[0].scale.y - 0.52).abs() < 1e-5);
        assert_eq!(ready[6].scale, Vec3::ONE);

        {
            let mut state = world.resource_mut::<VisualState>();
            state.ready = false;
            state.running = true;
            state.song_seconds = 3.0;
            state.hit_pulses = [0.6; 2];
            state.miss_pulses = [0.25; 2];
            state.free_sync_pulse = 0.7;
        }
        schedule.run(&mut world);
        let running = poses(&world);
        assert!((running[0].scale.y - 0.945).abs() < 1e-5);
        assert!((running[6].scale.x - 1.188).abs() < 1e-5);
        assert_ne!(running, ready);
        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f64(1.7));
        schedule.run(&mut world);
        assert_eq!(poses(&world), running);

        world.resource_mut::<VisualState>().song_seconds = 4.2075;
        schedule.run(&mut world);
        let partial_blink = poses(&world);
        assert!((partial_blink[0].scale.y - 0.4914).abs() < 1e-5);
        {
            let mut state = world.resource_mut::<VisualState>();
            state.running = false;
            state.paused = true;
            state.song_seconds = 99.0;
            state.hit_pulses = [0.0; 2];
            state.miss_pulses = [0.0; 2];
            state.free_sync_pulse = 0.0;
        }
        for seconds in [0.5, 10.0] {
            world
                .resource_mut::<Time>()
                .advance_by(Duration::from_secs_f64(seconds));
            schedule.run(&mut world);
            assert_eq!(poses(&world), partial_blink);
        }
        world.resource_mut::<VisualState>().paused = false;
        schedule.run(&mut world);
        let mut joints = world.query::<(&CharacterJoint, &Transform)>();
        assert!(
            joints
                .iter(&world)
                .all(|(joint, transform)| *transform == joint.rest)
        );
    }

    #[test]
    fn continuous_plush_profiles_have_finite_unit_normals_and_outward_triangles() {
        for (size, taper, bend) in [
            (Vec3::new(0.59, 0.64, 0.5), 0.12, 0.0),
            (Vec3::new(0.7, 0.53, 0.5), 0.12, 0.0),
            (Vec3::new(0.14, 0.34, 0.13), 0.18, 0.025),
            (Vec3::new(0.078, 0.25, 0.018), 0.18, 0.025),
            (Vec3::new(0.24, 0.34, 0.17), 0.62, 0.12),
            (Vec3::new(0.12, 0.23, 0.018), 0.5, 0.085),
            (Vec3::new(0.21, 0.115, 0.27), 0.0, 0.0),
        ] {
            let mesh = plush_mesh(size, taper, bend);
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions missing")
            };
            let Some(VertexAttributeValues::Float32x3(normals)) =
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
            else {
                panic!("normals missing")
            };
            let Some(Indices::U32(indices)) = mesh.indices() else {
                panic!("indices missing")
            };
            assert_eq!(positions.len(), 1000);
            assert_eq!(indices.len(), 5520);
            assert!(positions.iter().all(|p| Vec3::from_array(*p).is_finite()));
            assert!(
                normals
                    .iter()
                    .all(|n| (Vec3::from_array(*n).length() - 1.0).abs() < 1e-5)
            );
            for &[a, b, c] in indices.as_chunks::<3>().0 {
                let [pa, pb, pc] = [a, b, c].map(|i: u32| Vec3::from_array(positions[i as usize]));
                let average = [a, b, c]
                    .map(|i| Vec3::from_array(normals[i as usize]))
                    .into_iter()
                    .sum::<Vec3>();
                assert!((pb - pa).cross(pc - pa).dot(average) > 0.0);
            }
        }
    }
}
