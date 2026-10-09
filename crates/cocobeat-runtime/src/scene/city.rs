use std::{cell::Cell, f32::consts::FRAC_PI_2};

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};

use super::SceneEntity;
use crate::view::VisualState;

#[derive(Component)]
pub(super) struct CityScroll {
    anchor_z: f32,
    offset_z: f32,
}

pub(super) fn animate(state: Res<VisualState>, mut blocks: Query<(&CityScroll, &mut Transform)>) {
    let distance = (state.song_time.frames().max(0) as f64 / 16_000.0).rem_euclid(54.0) as f32;
    for (block, mut transform) in &mut blocks {
        // The entire shop, including its trailing lantern, passes behind the camera first
        let anchor = 14.0 - (14.0 - block.anchor_z - distance).rem_euclid(54.0);
        transform.translation.z = anchor + block.offset_z;
    }
}

pub(super) struct CityPalette<'a> {
    pub building: &'a Handle<StandardMaterial>,
    pub pavement: &'a Handle<StandardMaterial>,
    pub dark: &'a Handle<StandardMaterial>,
    pub window: &'a Handle<StandardMaterial>,
    pub signs: &'a [Handle<StandardMaterial>; 2],
}

/// Return the authored backdrop entities for the existing StageBackground parent
pub(super) fn spawn_city(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    shapes: [&Handle<Mesh>; 2],
    palette: CityPalette<'_>,
    side_offset: f32,
) -> Vec<Entity> {
    let [cube, sphere] = shapes;
    let rounded = meshes.add(rounded_block());
    let sky = meshes.add(night_sky());
    let icon_ring = meshes.add(
        Torus::new(0.72, 1.0)
            .mesh()
            .major_resolution(24)
            .minor_resolution(6),
    );
    let cornice = materials.add(StandardMaterial {
        base_color: Color::srgb(0.29, 0.27, 0.35),
        perceptual_roughness: 0.72,
        ..default()
    });
    let cream = materials.add(Color::srgb(0.78, 0.65, 0.51));
    let warm = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.78, 0.48),
        emissive: LinearRgba::rgb(0.85, 0.40, 0.13),
        perceptual_roughness: 0.55,
        ..default()
    });
    let upper_windows = [
        Color::srgb(0.56, 0.43, 0.29),
        Color::srgb(0.24, 0.40, 0.49),
        Color::srgb(0.46, 0.29, 0.39),
    ]
    .map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * 0.12,
            ..default()
        })
    });
    let upper_walls = [Color::srgb(0.16, 0.21, 0.28), Color::srgb(0.24, 0.18, 0.25)]
        .map(|color| materials.add(color));
    let skyline = materials.add(Color::srgb(0.095, 0.12, 0.20));
    let horizon = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let far_city = materials.add(StandardMaterial {
        base_color: Color::srgb(0.055, 0.061, 0.105),
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let moon = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.60, 0.74),
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let mut entities = Vec::with_capacity(520);
    let street_anchor = Cell::new(None);
    let mut place = |mesh: &Handle<Mesh>,
                     material: &Handle<StandardMaterial>,
                     position: Vec3,
                     scale: Vec3,
                     rotation: Quat| {
        let entity = commands
            .spawn((
                SceneEntity,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(position)
                    .with_scale(scale)
                    .with_rotation(rotation),
            ))
            .id();
        // Every part of a shop retains its offset when the whole shop wraps
        if let Some(anchor_z) = street_anchor.get() {
            commands.entity(entity).insert(CityScroll {
                anchor_z,
                offset_z: position.z - anchor_z,
            });
        }
        entities.push(entity);
    };
    let upright = Quat::IDENTITY;
    let wall_ring = Quat::from_rotation_z(FRAC_PI_2);
    for (player, side) in [-1.0, 1.0].into_iter().enumerate() {
        let accent = &palette.signs[player];
        let facade = side * (4.22 + side_offset);
        let heights = if player == 0 {
            [4.8, 6.2, 4.4, 7.5, 6.0, 4.8]
        } else {
            [5.6, 4.8, 6.2, 6.0, 7.5, 5.6]
        };
        for (block, height) in heights.into_iter().enumerate() {
            let z = 3.0 - block as f32 * 9.0;
            street_anchor.set(Some(z));
            let body = Vec3::new(side * (6.0 + side_offset), height * 0.5, z);
            place(
                &rounded,
                palette.building,
                body,
                Vec3::new(3.6, height, 6.6),
                upright,
            );
            place(
                &rounded,
                &cornice,
                body.with_y(height + 0.08),
                Vec3::new(3.8, 0.20, 6.9),
                upright,
            );
            place(
                cube,
                palette.pavement,
                body.with_y(0.22),
                Vec3::new(3.7, 0.44, 6.7),
                upright,
            );
            place(
                cube,
                &cream,
                Vec3::new(facade, 2.9, z),
                Vec3::new(0.12, 0.10, 6.0),
                upright,
            );
            place(
                cube,
                &upper_walls[(player + block) % 2],
                Vec3::new(facade - side * 0.015, (3.15 + height) * 0.5, z),
                Vec3::new(0.07, height - 3.15, 5.7),
                upright,
            );
            for dz in [-0.65, 0.65] {
                place(
                    cube,
                    &cornice,
                    Vec3::new(facade - side * 0.07, (3.15 + height) * 0.5, z + dz),
                    Vec3::new(0.05, height - 3.15, 0.07),
                    upright,
                );
            }
            // Each shop has a recessed door and a pair of warm display windows
            place(
                cube,
                palette.dark,
                Vec3::new(facade, 1.32, z),
                Vec3::new(0.14, 2.1, 5.5),
                upright,
            );
            place(
                cube,
                &warm,
                Vec3::new(facade - side * 0.09, 1.25, z),
                Vec3::new(0.06, 1.75, 0.8),
                upright,
            );
            for dz in [-1.65, 1.65] {
                place(
                    cube,
                    palette.window,
                    Vec3::new(facade - side * 0.09, 1.25, z + dz),
                    Vec3::new(0.06, 1.45, 1.9),
                    upright,
                );
                place(
                    cube,
                    &cream,
                    Vec3::new(facade - side * 0.14, 1.25, z + dz),
                    Vec3::new(0.04, 1.5, 0.045),
                    upright,
                );
                for row in 0..if height > 5.0 { 2 } else { 1 } {
                    place(
                        cube,
                        &upper_windows[(player + block + row + usize::from(dz > 0.0)) % 3],
                        Vec3::new(
                            facade - side * 0.09,
                            3.48 + row as f32 * 0.86,
                            z + dz + if player == 1 { 0.18 } else { 0.0 },
                        ),
                        Vec3::new(0.06, 0.39, if block == 1 { 0.75 } else { 0.96 }),
                        upright,
                    );
                }
            }
            for dz in [-2.9, 2.9] {
                place(
                    cube,
                    &cornice,
                    Vec3::new(facade, 1.35, z + dz),
                    Vec3::new(0.18, 2.5, 0.17),
                    upright,
                );
            }
            // A compact awning, with original alternating geometry instead of sign text
            place(
                cube,
                &cornice,
                Vec3::new(facade - side * 0.17, 2.65, z),
                Vec3::new(0.54, 0.20, 5.9),
                upright,
            );
            for dz in [-1.8, 0.0, 1.8] {
                place(
                    cube,
                    accent,
                    Vec3::new(facade - side * 0.20, 2.64, z + dz),
                    Vec3::new(0.55, 0.22, 0.74),
                    upright,
                );
            }
            let sign_x = facade - side * 0.18;
            place(
                cube,
                &cornice,
                Vec3::new(sign_x, 3.30, z),
                Vec3::new(0.10, 1.0, 1.25),
                upright,
            );
            let front = sign_x - side * 0.09;
            match (block + player) % 3 {
                0 => {
                    // Cafe: cup, handle, saucer and a rising steam dot
                    place(
                        cube,
                        &warm,
                        Vec3::new(front, 3.27, z),
                        Vec3::new(0.10, 0.35, 0.44),
                        upright,
                    );
                    place(
                        &icon_ring,
                        &warm,
                        Vec3::new(front, 3.28, z + 0.26),
                        Vec3::splat(0.15),
                        wall_ring,
                    );
                    place(
                        cube,
                        &cream,
                        Vec3::new(front, 3.04, z),
                        Vec3::new(0.10, 0.06, 0.66),
                        upright,
                    );
                    place(
                        sphere,
                        &warm,
                        Vec3::new(front, 3.59, z - 0.09),
                        Vec3::new(0.05, 0.09, 0.045),
                        upright,
                    );
                }
                1 => {
                    // Record store: vinyl grooves, paper label and a tilted tonearm
                    place(
                        &icon_ring,
                        accent,
                        Vec3::new(front, 3.30, z),
                        Vec3::splat(0.37),
                        wall_ring,
                    );
                    place(
                        &icon_ring,
                        &cream,
                        Vec3::new(front - side * 0.02, 3.30, z),
                        Vec3::splat(0.24),
                        wall_ring,
                    );
                    place(
                        sphere,
                        &warm,
                        Vec3::new(front - side * 0.04, 3.30, z),
                        Vec3::new(0.05, 0.11, 0.11),
                        upright,
                    );
                    place(
                        cube,
                        &cream,
                        Vec3::new(front, 3.44, z + 0.35),
                        Vec3::new(0.07, 0.45, 0.045),
                        Quat::from_rotation_x(-0.35),
                    );
                }
                _ => {
                    // Metro: train cabin with two windows and two wheels
                    place(
                        cube,
                        accent,
                        Vec3::new(front, 3.35, z),
                        Vec3::new(0.10, 0.50, 0.60),
                        upright,
                    );
                    for dz in [-0.16, 0.16] {
                        place(
                            cube,
                            palette.dark,
                            Vec3::new(front - side * 0.07, 3.43, z + dz),
                            Vec3::new(0.06, 0.14, 0.18),
                            upright,
                        );
                        place(
                            sphere,
                            &warm,
                            Vec3::new(front, 3.01, z + dz),
                            Vec3::new(0.06, 0.075, 0.075),
                            upright,
                        );
                    }
                }
            }
            // Pavement joints stay outside the original playable Stage width
            for joint in 0..3 {
                place(
                    cube,
                    &cornice,
                    Vec3::new(
                        side * (4.65 + side_offset),
                        0.035,
                        z - 2.6 + joint as f32 * 2.6,
                    ),
                    Vec3::new(0.65, 0.015, 0.045),
                    upright,
                );
            }
            let lamp_x = side * (4.18 + side_offset);
            let lamp_z = z - 3.6;
            place(
                cube,
                &cornice,
                Vec3::new(lamp_x, 1.38, lamp_z),
                Vec3::new(0.10, 2.76, 0.10),
                upright,
            );
            place(
                cube,
                &cornice,
                Vec3::new(lamp_x, 0.15, lamp_z),
                Vec3::new(0.28, 0.30, 0.28),
                upright,
            );
            place(
                cube,
                &cream,
                Vec3::new(lamp_x, 2.84, lamp_z),
                Vec3::new(0.40, 0.10, 0.40),
                upright,
            );
            place(
                sphere,
                &warm,
                Vec3::new(lamp_x, 2.69, lamp_z),
                Vec3::new(0.15, 0.15, 0.15),
                upright,
            );
        }
        street_anchor.set(None);
        // Two depth layers leave the central horizon and projected cue corridor open
        for block in 0..6 {
            let x = side * (9.5 + side_offset + block as f32 * 3.5);
            let height = [10.0, 13.0, 8.0, 15.0, 11.0, 9.0][block];
            let z = -48.0 - (block % 2) as f32 * 12.0;
            place(
                cube,
                &skyline,
                Vec3::new(x, height * 0.5, z),
                Vec3::new(3.0, height, 4.0),
                upright,
            );
            place(
                cube,
                &skyline,
                Vec3::new(x, height + 0.12, z),
                Vec3::new(3.05, 0.24, 4.05),
                upright,
            );
            if block % 2 == 0 {
                place(
                    cube,
                    palette.window,
                    Vec3::new(x - side * 1.52, height * 0.72, z),
                    Vec3::new(0.04, 0.21, 2.4),
                    upright,
                );
            }
        }
    }
    for (x, height) in [
        (-12.0, 8.0),
        (-6.0, 5.0),
        (0.0, 9.0),
        (6.0, 6.0),
        (12.0, 11.0),
    ] {
        place(
            cube,
            &far_city,
            Vec3::new(x, height * 0.5, -94.0),
            Vec3::new(4.0, height, 2.0),
            upright,
        );
    }
    // A low world floor grounds the distant city without changing the Stage strip
    place(
        cube,
        palette.dark,
        Vec3::new(0.0, -0.22, -76.0),
        Vec3::new(160.0, 0.1, 176.0),
        upright,
    );
    // An opaque sky layer is strictly behind the street and every authored building
    place(
        &sky,
        &horizon,
        Vec3::new(0.0, 24.0, -108.0),
        Vec3::new(140.0, 72.0, 0.5),
        upright,
    );
    place(
        sphere,
        &moon,
        Vec3::new(-9.5, 11.5, -101.0),
        Vec3::new(1.8, 1.8, 0.45),
        upright,
    );
    entities
}

fn rounded_block() -> Mesh {
    const STEPS: u32 = 16;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for (normal, u, v) in [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    ] {
        let base = positions.len() as u32;
        for row in 0..=STEPS {
            for col in 0..=STEPS {
                let uv = Vec2::new(col as f32, row as f32) / STEPS as f32;
                let p = normal * 0.5 + u * (uv.x - 0.5) + v * (uv.y - 0.5);
                let core = p.clamp(Vec3::splat(-0.44), Vec3::splat(0.44));
                let direction = (p - core).normalize();
                positions.push((core + direction * 0.06).to_array());
                normals.push(direction.to_array());
                uvs.push(uv.to_array());
                if row < STEPS && col < STEPS {
                    let a = base + row * (STEPS + 1) + col;
                    let c = a + STEPS + 1;
                    indices.extend([a, a + 1, c, a + 1, c + 1, c]);
                }
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

fn night_sky() -> Mesh {
    let mut mesh = Mesh::from(Cuboid::default());
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        unreachable!()
    };
    let bottom = Color::srgb(0.14, 0.13, 0.26).to_linear();
    let top = Color::srgb(0.025, 0.038, 0.09).to_linear();
    let colors: Vec<[f32; 4]> = positions
        .iter()
        .map(|p| bottom.mix(&top, p[1] + 0.5).to_f32_array())
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn street_motion_uses_song_frames_and_holds_while_the_cursor_is_paused() {
        let mut app = App::new();
        app.init_resource::<VisualState>()
            .init_resource::<Time>()
            .add_systems(Update, animate);
        let original = Transform::from_xyz(-8.0, 3.0, -6.0).with_scale(Vec3::new(3.6, 6.0, 6.6));
        let building = app
            .world_mut()
            .spawn((
                CityScroll {
                    anchor_z: -6.0,
                    offset_z: 0.0,
                },
                original,
            ))
            .id();
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.song_time = cocobeat_schema::SongTime::from_frames(48_000);
            state.song_seconds = 999.0;
            state.running = true;
        }
        app.update();
        let moved = *app.world().get::<Transform>(building).unwrap();
        assert_eq!(moved.translation, Vec3::new(-8.0, 3.0, -3.0));
        assert_eq!(moved.scale, original.scale);
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.running = false;
            state.paused = true;
        }
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(20));
        app.update();
        assert_eq!(*app.world().get::<Transform>(building).unwrap(), moved);
        app.world_mut().resource_mut::<VisualState>().song_time =
            cocobeat_schema::SongTime::from_frames(20 * 48_000);
        app.update();
        assert_eq!(
            app.world()
                .get::<Transform>(building)
                .unwrap()
                .translation
                .z,
            0.0
        );
    }

    #[test]
    fn shop_parts_wrap_together_after_passing_the_camera() {
        let mut app = App::new();
        app.init_resource::<VisualState>()
            .add_systems(Update, animate);
        let parts: Vec<_> = [0.0, -1.65, -3.6]
            .into_iter()
            .map(|offset_z| {
                app.world_mut()
                    .spawn((
                        CityScroll {
                            anchor_z: 3.0,
                            offset_z,
                        },
                        Transform::from_xyz(7.0, 2.0, 3.0 + offset_z),
                    ))
                    .id()
            })
            .collect();
        for (frames, expected_anchor) in [(175_999, 13.999_937), (176_001, -39.999_94)] {
            app.world_mut().resource_mut::<VisualState>().song_time =
                cocobeat_schema::SongTime::from_frames(frames);
            app.update();
            for (&entity, offset) in parts.iter().zip([0.0, -1.65, -3.6]) {
                let position = app.world().get::<Transform>(entity).unwrap().translation;
                assert!((position.z - expected_anchor - offset).abs() < 1e-5);
                assert_eq!(position.x, 7.0);
                assert_eq!(position.y, 2.0);
                if frames == 175_999 {
                    assert!(position.z > 9.2);
                }
            }
        }
    }

    #[test]
    fn rounded_buildings_keep_bounds_and_outward_unit_normals() {
        let mesh = rounded_block();
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals")
        };
        assert!(
            positions
                .iter()
                .flatten()
                .all(|p| p.is_finite() && p.abs() <= 0.500001)
        );
        assert!(
            normals
                .iter()
                .all(|n| (Vec3::from_array(*n).length() - 1.0).abs() < 1e-5)
        );
        let indices: Vec<_> = mesh.indices().unwrap().iter().collect();
        for &[a, b, c] in indices.as_chunks::<3>().0 {
            let [pa, pb, pc] = [a, b, c].map(|i: usize| Vec3::from_array(positions[i]));
            let normal = [a, b, c]
                .map(|i| Vec3::from_array(normals[i]))
                .into_iter()
                .sum::<Vec3>();
            assert!((pb - pa).cross(pc - pa).dot(normal) > 0.0);
        }
    }
}
