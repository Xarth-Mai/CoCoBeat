use std::f32::consts::PI;

use bevy::{
    camera::{ClearColorConfig, Hdr},
    core_pipeline::tonemapping::Tonemapping,
    light::NotShadowCaster,
    post_process::bloom::Bloom,
    prelude::*,
    render::view::Msaa,
};

use crate::{
    display::GameCamera,
    settings::{AntiAliasing, QualitySettings, RainAmount},
    view::VisualState,
};

const DISTANT_COLOR: Color = Color::srgb(0.055, 0.07, 0.12);

#[derive(Resource)]
pub(crate) struct SignMaterials([Handle<StandardMaterial>; 2]);

#[derive(Component)]
pub(crate) struct KeyLight;

#[derive(Component)]
pub(crate) enum Motion {
    Spirit(usize),
    Ripple(usize),
    FreeRing,
    AnchorRing(usize),
    AnchorPreview,
    Street(f32),
    Rain(usize),
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

pub(crate) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(DISTANT_COLOR),
            ..default()
        },
        GameCamera,
        Hdr,
        bevy::camera::ShadowLodOrigin,
        Tonemapping::Reinhard,
        Transform::from_xyz(0.0, 5.8, 11.8).looking_at(Vec3::new(0.0, 0.9, -4.0), Vec3::Y),
        AmbientLight {
            color: Color::srgb(0.7, 0.76, 0.9),
            brightness: 180.0,
            ..default()
        },
    ));

    let cube = meshes.add(Cuboid::default());
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(24, 16));
    let cone = meshes.add(Cone::new(0.28, 0.65));
    let ring = meshes.add(
        Torus::new(0.965, 1.0)
            .mesh()
            .major_resolution(64)
            .minor_resolution(8),
    );
    let half_ring = meshes.add(
        Torus::new(0.95, 1.0)
            .mesh()
            .major_resolution(32)
            .minor_resolution(8)
            .angle_range(0.0..=PI),
    );
    let asphalt = materials.add(StandardMaterial {
        base_color: Color::srgb(0.055, 0.075, 0.11),
        metallic: 0.4,
        perceptual_roughness: 0.28,
        ..default()
    });
    let building = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.145, 0.22),
        perceptual_roughness: 0.8,
        ..default()
    });
    let distant = materials.add(Color::srgb(0.065, 0.08, 0.15));
    let pavement = materials.add(Color::srgb(0.14, 0.16, 0.23));
    let dark = materials.add(Color::srgb(0.025, 0.035, 0.075));
    let white = materials.add(StandardMaterial {
        base_color: Color::srgb(0.98, 0.98, 1.0),
        emissive: LinearRgba::rgb(0.3, 0.3, 0.3),
        ..default()
    });
    let colors = [Color::srgb(0.25, 0.8, 1.0), Color::srgb(1.0, 0.45, 0.73)];
    let neon = colors.map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * 2.5,
            ..default()
        })
    });
    let signs = colors.map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * 0.35,
            ..default()
        })
    });
    let bodies = [Color::srgb(0.5, 0.84, 0.98), Color::srgb(1.0, 0.67, 0.81)].map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * 0.03,
            perceptual_roughness: 0.4,
            reflectance: 0.6,
            ..default()
        })
    });
    let rain = materials.add(StandardMaterial {
        base_color: Color::srgba(0.5, 0.62, 0.8, 0.25),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let puddle = materials.add(StandardMaterial {
        base_color: Color::srgb(0.09, 0.13, 0.18),
        metallic: 0.65,
        perceptual_roughness: 0.12,
        ..default()
    });
    let reflections = colors.map(|color| {
        materials.add(StandardMaterial {
            base_color: color.with_alpha(0.11),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })
    });
    let cheeks = materials.add(Color::srgb(1.0, 0.48, 0.65));
    let window = materials.add(StandardMaterial {
        base_color: Color::srgb(0.38, 0.26, 0.22),
        emissive: LinearRgba::rgb(0.2, 0.1, 0.045),
        ..default()
    });

    commands.spawn(part(
        &cube,
        &asphalt,
        Vec3::new(0.0, -0.1, -15.0),
        Vec3::new(9.0, 0.2, 54.0),
    ));
    commands.spawn((
        PointLight {
            color: Color::srgb(1.0, 0.94, 0.86),
            intensity: 350_000.0,
            range: 25.0,
            radius: 1.25,
            ..default()
        },
        Transform::from_xyz(-2.2, 4.8, 3.6),
        KeyLight,
    ));
    for (player, side) in [-1.0, 1.0].into_iter().enumerate() {
        commands.spawn(part(
            &cube,
            &pavement,
            Vec3::new(side * 4.05, 0.08, -15.0),
            Vec3::new(1.1, 0.16, 54.0),
        ));
        commands.spawn(part(
            &cube,
            &neon[player],
            Vec3::new(side * 3.5, 0.02, -15.0),
            Vec3::new(0.025, 0.025, 54.0),
        ));
        commands.spawn((
            PointLight {
                color: colors[player],
                intensity: 16_000.0,
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
                &signs[player],
                Vec3::new(side * 4.22, height * 0.62, z),
                Vec3::new(0.08, 0.09, 2.2),
            ));
            commands.spawn(part(
                &cube,
                &signs[player],
                Vec3::new(side * 4.22, height * 0.62 - 0.3, z),
                Vec3::new(0.08, 0.035, 1.3),
            ));
            if block % 3 == 0 {
                commands.spawn(part(
                    &cube,
                    &window,
                    Vec3::new(side * 4.23, 1.15, z),
                    Vec3::new(0.04, 1.6, 1.1),
                ));
                commands.spawn(part(
                    &cube,
                    &pavement,
                    Vec3::new(side * 4.05, 2.1, z),
                    Vec3::new(0.7, 0.1, 1.6),
                ));
                commands.spawn((
                    part(
                        &sphere,
                        &puddle,
                        Vec3::new(side * 2.8, 0.006, z - 1.0),
                        Vec3::new(0.5, 0.005, 1.1),
                    ),
                    NotShadowCaster,
                ));
                // Decorative colour streaks on wet pavement, not screen-space reflections
                commands.spawn((
                    part(
                        &cube,
                        &reflections[player],
                        Vec3::new(side * 2.8, 0.016, z - 0.9),
                        Vec3::new(0.12, 0.002, 1.2),
                    ),
                    NotShadowCaster,
                ));
            }
        }
        for block in 0..3 {
            let height = 8.0 + block as f32 * 2.0;
            commands.spawn(part(
                &cube,
                &distant,
                Vec3::new(
                    side * (3.5 + block as f32 * 3.5),
                    height * 0.5,
                    -42.0 - block as f32 * 6.0,
                ),
                Vec3::new(5.0, height, 4.0),
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
                            &white,
                            Vec3::new(ear * 0.2, 0.1, 0.435),
                            Vec3::new(0.095, 0.12, 0.05),
                        ));
                    }
                } else {
                    parent.spawn(part(
                        &cone,
                        &bodies[player],
                        Vec3::new(0.0, 0.57, 0.0),
                        Vec3::ONE,
                    ));
                    for eye in [-1.0, 1.0] {
                        parent.spawn(part(
                            &sphere,
                            &white,
                            Vec3::new(eye * 0.2, 0.08, 0.435),
                            Vec3::new(0.095, 0.1, 0.05),
                        ));
                    }
                }
                for eye in [-1.0, 1.0] {
                    parent.spawn(part(
                        &sphere,
                        &dark,
                        Vec3::new(eye * 0.2, 0.09, 0.477),
                        Vec3::new(0.049, 0.067, 0.023),
                    ));
                    parent.spawn(part(
                        &sphere,
                        &white,
                        Vec3::new(eye * 0.2 - 0.012, 0.12, 0.496),
                        Vec3::splat(0.016),
                    ));
                    parent.spawn(part(
                        &sphere,
                        &cheeks,
                        Vec3::new(eye * 0.32, -0.06, 0.42),
                        Vec3::new(0.085, 0.037, 0.022),
                    ));
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
        let ripple = materials.add(StandardMaterial {
            base_color: colors[player],
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        commands.spawn((
            part(&half_ring, &ripple, Vec3::new(x, 0.04, 0.0), Vec3::ONE),
            Motion::Ripple(player),
            Visibility::Hidden,
            NotShadowCaster,
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
    for (motion, color) in [
        (Motion::FreeRing, Color::srgb(0.68, 0.88, 1.0)),
        (Motion::AnchorRing(0), Color::srgb(1.0, 0.94, 0.84)),
        (Motion::AnchorRing(1), Color::srgb(1.0, 0.63, 0.82)),
    ] {
        let material = materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        commands.spawn((
            part(&ring, &material, Vec3::new(0.0, 0.035, 0.0), Vec3::ONE),
            motion,
            Visibility::Hidden,
            NotShadowCaster,
        ));
    }
    let preview = materials.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.86, 1.0, 0.65),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    for (x, scale) in [
        (-2.5, Vec3::new(0.06, 0.35, 0.12)),
        (2.5, Vec3::new(0.06, 0.35, 0.12)),
        (0.0, Vec3::new(1.2, 0.025, 0.07)),
    ] {
        commands.spawn((
            part(
                &cube,
                &preview,
                Vec3::new(x, scale.y * 0.5 + 0.02, 0.0),
                scale,
            ),
            Motion::AnchorPreview,
            Visibility::Hidden,
            NotShadowCaster,
        ));
    }
    for drop in 0..48 {
        let phase = drop as f32 * 0.73;
        commands.spawn((
            part(
                &cube,
                &rain,
                Vec3::new((phase * 3.7).sin() * 6.0, 0.0, -(((drop * 7) % 31) as f32)),
                Vec3::new(0.012, 0.24, 0.012),
            ),
            Motion::Rain(drop),
            NotShadowCaster,
        ));
    }
    commands.insert_resource(SignMaterials(signs));
}

pub(crate) fn update_signs(
    state: Res<VisualState>,
    signs: Res<SignMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut applied: Local<Option<f32>>,
) {
    let resonance = if state.resonance.is_finite() {
        state.resonance.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let strength = 0.35 + resonance * 2.15;
    if *applied == Some(strength) {
        return;
    }
    for handle in &signs.0 {
        if let Some(mut material) = materials.get_mut(handle) {
            material.emissive = LinearRgba::from(material.base_color) * strength;
        }
    }
    *applied = Some(strength);
}

pub(crate) fn apply_quality(
    mut commands: Commands,
    state: Res<VisualState>,
    mut applied: Local<Option<QualitySettings>>,
    cameras: Query<Entity, With<GameCamera>>,
    mut lights: Query<&mut PointLight, With<KeyLight>>,
    mut objects: Query<(&Motion, &mut Visibility)>,
) {
    let quality = state.quality;
    if *applied == Some(quality) || cameras.is_empty() {
        return;
    }
    for camera in &cameras {
        let mut camera = commands.entity(camera);
        camera.insert(match quality.antialiasing {
            AntiAliasing::Off => Msaa::Off,
            AntiAliasing::Msaa2 => Msaa::Sample2,
            AntiAliasing::Msaa4 => Msaa::Sample4,
        });
        if quality.fog {
            camera.insert(DistanceFog {
                color: DISTANT_COLOR,
                falloff: FogFalloff::Linear {
                    start: 10.0,
                    end: 54.0,
                },
                ..default()
            });
        } else {
            camera.remove::<DistanceFog>();
        }
        if quality.bloom {
            camera.insert(Bloom::default());
        } else {
            camera.remove::<Bloom>();
        }
    }
    for mut light in &mut lights {
        light.shadow_maps_enabled = quality.shadows;
    }
    let drops = match quality.rain {
        RainAmount::Off => 0,
        RainAmount::Quarter => 12,
        RainAmount::Half => 24,
        RainAmount::Full => 48,
    };
    for (motion, mut visibility) in &mut objects {
        if let Motion::Rain(index) = *motion {
            *visibility = if index < drops {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    *applied = Some(quality);
}

pub(crate) fn animate(
    state: Res<VisualState>,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut objects: Query<(
        &Motion,
        &mut Transform,
        &mut Visibility,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
) {
    let song = state.song_seconds as f32;
    for (motion, mut transform, mut visibility, material) in &mut objects {
        let opacity = match *motion {
            Motion::Spirit(player) => {
                let hit = state.hit_pulses[player].clamp(0.0, 1.0);
                let miss = state.miss_pulses[player].clamp(0.0, 1.0);
                transform.translation.y = 0.85 + (song * PI * 2.0).sin() * 0.035;
                transform.scale = Vec3::new(1.0 + hit * 0.16, 1.0 - hit * 0.16, 1.0 + hit * 0.08);
                transform.rotation = Quat::from_rotation_x(miss * 0.1);
                None
            }
            Motion::Ripple(player) => {
                let hit = state.hit_pulses[player].clamp(0.0, 1.0);
                *visibility = if hit > 0.01 {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                let radius = 0.65 + (1.0 - hit) * 0.65;
                transform.scale = Vec3::new(radius, 0.2, radius);
                transform.rotation = Quat::from_rotation_y((player as f32 * 2.0 - 1.0) * PI * 0.5);
                Some(hit * hit)
            }
            Motion::FreeRing | Motion::AnchorRing(_) => {
                let (sync, radius, alpha) = match *motion {
                    Motion::AnchorRing(index) => (
                        state.anchor_sync_pulse.clamp(0.0, 1.0),
                        2.15 + index as f32 * 0.25,
                        (0.95 - index as f32 * 0.3)
                            * if state.anchor_sync_precise { 1.0 } else { 0.68 },
                    ),
                    _ => (state.free_sync_pulse.clamp(0.0, 1.0), 1.95, 0.65),
                };
                *visibility = if sync > 0.01 {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
                let radius = radius + (1.0 - sync * sync) * 0.9;
                transform.scale = Vec3::new(radius, 0.2, radius);
                // Grade changes strength only; the same age drives growth and lifetime
                let attack = ((1.0 - sync) / 0.1).clamp(0.0, 1.0);
                let attack = 0.15 + 0.85 * attack * attack * (3.0 - 2.0 * attack);
                Some(attack * sync * sync * alpha)
            }
            Motion::AnchorPreview => {
                let ahead = state.next_anchor_seconds.map(|at| at - state.song_seconds);
                if let Some(ahead) = ahead.filter(|ahead| (0.0..=4.0).contains(ahead)) {
                    *visibility = Visibility::Visible;
                    transform.translation.z = -ahead as f32 * 3.0;
                    Some(0.25 + (1.0 - ahead as f32 / 4.0) * 0.4)
                } else {
                    *visibility = Visibility::Hidden;
                    Some(0.0)
                }
            }
            Motion::Street(offset) => {
                transform.translation.z = 6.0 - (offset - song * 3.0).rem_euclid(36.0);
                None
            }
            Motion::Rain(index) => {
                let phase = index as f32 * 0.73;
                transform.translation.y = 8.0 - (time.elapsed_secs() * 6.0 + phase).rem_euclid(8.0);
                transform.rotation = Quat::from_rotation_z(-0.15);
                None
            }
        };
        if let Some(opacity) = opacity
            && let Some(material) = material
            && let Some(mut material) = materials.get_mut(&material.0)
        {
            material.base_color.set_alpha(opacity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_and_resonance_preserve_core_feedback_and_geometry() {
        use crate::{display::PresentationCamera, settings::QualityPreset};

        let mut app = App::new();
        // Match VisibilityPlugin's Mesh3d requirement without installing the render app
        app.register_required_components::<Mesh3d, Visibility>()
            .insert_resource(VisualState {
                song_seconds: 12.5,
                hit_pulses: [0.8, 0.5],
                free_sync_pulse: 0.7,
                anchor_sync_pulse: 0.7,
                next_anchor_seconds: Some(14.5),
                ..default()
            })
            .init_resource::<Time>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, (apply_quality, animate, update_signs).chain());
        let presentation = app
            .world_mut()
            .spawn((Camera2d, PresentationCamera, Msaa::Off))
            .id();
        app.update();
        let camera = app
            .world_mut()
            .query_filtered::<Entity, With<GameCamera>>()
            .single(app.world())
            .unwrap();
        let mut motions = app
            .world_mut()
            .query::<(Entity, &Motion, &Transform, &Visibility)>();
        let core: Vec<_> = motions
            .iter(app.world())
            .filter(|(_, motion, _, _)| !matches!(motion, Motion::Rain(_)))
            .map(|(entity, _, transform, visibility)| (entity, *transform, *visibility))
            .collect();
        assert_eq!(core.len(), 34);
        assert!(core.iter().all(|(_, _, v)| *v != Visibility::Hidden));
        let meshes = app.world().resource::<Assets<Mesh>>().len();
        let materials = app.world().resource::<Assets<StandardMaterial>>().len();
        assert!(meshes <= 12);
        assert!(materials <= 32);
        let sign_ids = app
            .world()
            .resource::<SignMaterials>()
            .0
            .each_ref()
            .map(Handle::id);
        let mut visible_meshes = app
            .world_mut()
            .query::<(&Transform, &MeshMaterial3d<StandardMaterial>)>();
        let mut sign_count = 0;
        assert!(visible_meshes.iter(app.world()).count() <= 220);
        for (transform, material) in visible_meshes.iter(app.world()) {
            let sign = transform.translation.x.abs() == 4.22 && transform.scale.x == 0.08;
            assert_eq!(sign_ids.contains(&material.0.id()), sign);
            sign_count += usize::from(sign);
        }
        assert_eq!(sign_count, 36);
        let mut transforms = app.world_mut().query::<(Entity, &Transform)>();
        let transforms: Vec<_> = transforms
            .iter(app.world())
            .map(|(entity, transform)| (entity, *transform))
            .collect();
        let feedback_materials: Vec<_> = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .iter()
            .filter(|(id, _)| !sign_ids.contains(id))
            .map(|(id, material)| (id, material.emissive))
            .collect();
        for (resonance, expected_strength) in [
            (0.0, 0.35),
            (0.5, 1.425),
            (1.0, 2.5),
            (0.0, 0.35),
            (-1.0, 0.35),
            (2.0, 2.5),
            (f32::NAN, 0.35),
        ] {
            app.world_mut().resource_mut::<VisualState>().resonance = resonance;
            app.update();
            let materials = app.world().resource::<Assets<StandardMaterial>>();
            for id in sign_ids {
                let material = materials.get(id).unwrap();
                let expected = LinearRgba::from(material.base_color) * expected_strength;
                assert!((material.emissive.red - expected.red).abs() < 1e-6);
                assert!((material.emissive.green - expected.green).abs() < 1e-6);
                assert!((material.emissive.blue - expected.blue).abs() < 1e-6);
            }
            for &(id, emissive) in &feedback_materials {
                assert_eq!(materials.get(id).unwrap().emissive, emissive);
            }
            for &(entity, transform) in &transforms {
                assert_eq!(*app.world().get::<Transform>(entity).unwrap(), transform);
            }
            for &(entity, _, visibility) in &core {
                assert_eq!(*app.world().get::<Visibility>(entity).unwrap(), visibility);
            }
            assert_eq!(app.world().resource::<VisualState>().song_seconds, 12.5);
        }
        let all_off = QualitySettings {
            preset: QualityPreset::Custom,
            antialiasing: AntiAliasing::Off,
            rain: RainAmount::Off,
            fog: false,
            shadows: false,
            bloom: false,
        };

        for (preset, msaa, drops, effects) in [
            (QualityPreset::Low, Msaa::Off, 12, false),
            (QualityPreset::Medium, Msaa::Sample2, 24, true),
            (QualityPreset::High, Msaa::Sample4, 48, true),
            (QualityPreset::Custom, Msaa::Off, 0, false),
            (QualityPreset::Medium, Msaa::Sample2, 24, true),
        ] {
            let mut quality = all_off;
            quality.set_preset(preset);
            app.world_mut().resource_mut::<VisualState>().quality = quality;
            app.update();
            assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), msaa);
            assert!(app.world().get::<Hdr>(camera).is_some());
            assert_eq!(app.world().get::<Bloom>(camera).is_some(), effects);
            assert_eq!(
                app.world().get::<DistanceFog>(camera).is_some(),
                preset != QualityPreset::Custom
            );
            let mut lights = app.world_mut().query::<(&PointLight, Option<&KeyLight>)>();
            assert_eq!(lights.iter(app.world()).count(), 3);
            assert_eq!(
                lights
                    .iter(app.world())
                    .filter(|(_, key)| key.is_some())
                    .count(),
                1
            );
            assert!(
                lights
                    .iter(app.world())
                    .all(|(light, key)| light.shadow_maps_enabled == (effects && key.is_some()))
            );
            let mut rain = app
                .world_mut()
                .query::<(&Motion, &Visibility, Option<&NotShadowCaster>)>();
            let rain: Vec<_> = rain
                .iter(app.world())
                .filter_map(|(motion, visibility, shadow)| match *motion {
                    Motion::Rain(index) => Some((index, visibility, shadow)),
                    _ => None,
                })
                .collect();
            assert_eq!(rain.len(), 48);
            assert!(rain.into_iter().all(|(index, visibility, shadow)| {
                (*visibility != Visibility::Hidden) == (index < drops) && shadow.is_some()
            }));
            for &(entity, transform, visibility) in &core {
                assert_eq!(*app.world().get::<Transform>(entity).unwrap(), transform);
                assert_eq!(*app.world().get::<Visibility>(entity).unwrap(), visibility);
            }
            assert_eq!(app.world().resource::<VisualState>().song_seconds, 12.5);
            assert_eq!(app.world().resource::<Assets<Mesh>>().len(), meshes);
            assert_eq!(
                app.world().resource::<Assets<StandardMaterial>>().len(),
                materials
            );
            assert_eq!(*app.world().get::<Msaa>(presentation).unwrap(), Msaa::Off);
            assert!(app.world().get::<Hdr>(presentation).is_none());
            assert!(app.world().get::<Bloom>(presentation).is_none());
            assert!(app.world().get::<DistanceFog>(presentation).is_none());
        }
    }

    #[test]
    fn song_time_drives_preview_and_confirmed_facts_choose_distinct_feedback() {
        let mut app = App::new();
        app.register_required_components::<Mesh3d, Visibility>()
            .init_resource::<VisualState>()
            .init_resource::<Time>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, animate);
        app.update();
        let mut entities = app.world_mut().query::<(Entity, &Motion)>();
        let mut selected = [None; 7];
        for (entity, motion) in entities.iter(app.world()) {
            let index = match motion {
                Motion::Spirit(0) => 0,
                Motion::Ripple(0) => 1,
                Motion::FreeRing => 2,
                Motion::AnchorRing(0) => 3,
                Motion::AnchorPreview => 4,
                Motion::Street(9.0) => 5,
                Motion::AnchorRing(1) => 6,
                _ => continue,
            };
            selected[index] = Some(entity);
        }
        let [spirit, local, free, anchor, preview, street, outer] = selected.map(Option::unwrap);
        let base = *app.world().get::<Transform>(spirit).unwrap();
        for (local_pulse, free_pulse, anchor_pulse, expected) in [
            (0.0, 0.0, 0.0, [false, false, false]),
            (1.0, 0.0, 0.0, [true, false, false]),
            (0.0, 1.0, 0.0, [false, true, false]),
            (0.0, 0.0, 1.0, [false, false, true]),
        ] {
            {
                let mut state = app.world_mut().resource_mut::<VisualState>();
                state.hit_pulses[0] = local_pulse;
                state.free_sync_pulse = free_pulse;
                state.anchor_sync_pulse = anchor_pulse;
            }
            app.update();
            for (entity, visible) in [local, free, anchor].into_iter().zip(expected) {
                assert_eq!(
                    *app.world().get::<Visibility>(entity).unwrap() == Visibility::Visible,
                    visible
                );
            }
            assert_eq!(
                app.world().get::<Transform>(spirit).unwrap().translation,
                base.translation
            );
        }
        app.world_mut()
            .resource_mut::<VisualState>()
            .anchor_sync_pulse = 0.9;
        app.update();
        let material = app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(anchor)
            .unwrap()
            .0
            .clone();
        let alpha = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap()
            .base_color
            .alpha();
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.anchor_sync_pulse = 0.5;
            state.miss_pulses[0] = 1.0;
        }
        app.update();
        let faded = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap()
            .base_color
            .alpha();
        assert!(faded > 0.0 && faded < alpha);
        let pose = app.world().get::<Transform>(spirit).unwrap();
        assert_eq!(pose.translation, base.translation);
        assert!(pose.rotation.angle_between(base.rotation) <= 0.11);
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.song_seconds = 2.0;
            state.next_anchor_seconds = Some(6.0);
            state.anchor_sync_pulse = 0.0;
        }
        app.update();
        let track_position = app.world().get::<Transform>(street).unwrap().translation;
        for resonance in [0.0, 1.0] {
            app.world_mut().resource_mut::<VisualState>().resonance = resonance;
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs(10));
            app.update();
            assert_eq!(
                app.world().get::<Transform>(street).unwrap().translation,
                track_position
            );
            assert_eq!(
                app.world().get::<Transform>(preview).unwrap().translation.z,
                -12.0
            );
            assert_eq!(
                *app.world().get::<Visibility>(preview).unwrap(),
                Visibility::Visible
            );
            assert_eq!(
                *app.world().get::<Visibility>(free).unwrap(),
                Visibility::Hidden
            );
            assert_eq!(
                *app.world().get::<Visibility>(anchor).unwrap(),
                Visibility::Hidden
            );
        }
        for (song_seconds, expected_z) in [(4.0, -6.0), (6.0, 0.0)] {
            app.world_mut().resource_mut::<VisualState>().song_seconds = song_seconds;
            app.update();
            assert_eq!(
                app.world().get::<Transform>(preview).unwrap().translation.z,
                expected_z
            );
        }
        for next in [None, Some(5.0), Some(10.001), Some(f64::NAN)] {
            app.world_mut()
                .resource_mut::<VisualState>()
                .next_anchor_seconds = next;
            app.update();
            assert_eq!(
                *app.world().get::<Visibility>(preview).unwrap(),
                Visibility::Hidden
            );
        }

        // Both grades finish the same motion; only confirmed precision changes brightness
        let samples = [false, true].map(|precise| {
            [1.0, 0.975, 0.95, 0.9, 0.75, 0.5, 0.25, 0.01, 0.0].map(|remaining| {
                {
                    let mut state = app.world_mut().resource_mut::<VisualState>();
                    state.anchor_sync_precise = precise;
                    state.anchor_sync_pulse = remaining;
                    state.free_sync_pulse = remaining;
                }
                app.update();
                [anchor, outer, free].map(|entity| {
                    let material = app
                        .world()
                        .get::<MeshMaterial3d<StandardMaterial>>(entity)
                        .unwrap();
                    (
                        *app.world().get::<Transform>(entity).unwrap(),
                        *app.world().get::<Visibility>(entity).unwrap(),
                        app.world()
                            .resource::<Assets<StandardMaterial>>()
                            .get(&material.0)
                            .unwrap()
                            .base_color
                            .alpha(),
                    )
                })
            })
        });
        for (good, precise) in samples[0].iter().zip(&samples[1]) {
            for (ring, (good, precise)) in good.iter().zip(precise).enumerate() {
                assert_eq!(good.0, precise.0);
                assert_eq!(good.1, precise.1);
                let strength = if ring == 2 { 1.0 } else { 0.68 };
                assert!((good.2 - precise.2 * strength).abs() < 1e-6);
            }
        }
        for ring in 0..3 {
            let alpha = samples[1].map(|sample| sample[ring].2);
            assert!(alpha[0] > 0.0);
            assert!(alpha[0] < alpha[1]);
            assert!(alpha[1] < alpha[2]);
            assert!(alpha[2] < alpha[3]);
            assert!(alpha[3..].windows(2).all(|w| w[0] > w[1]));
            assert_eq!(alpha[8], 0.0);
        }
    }
}
