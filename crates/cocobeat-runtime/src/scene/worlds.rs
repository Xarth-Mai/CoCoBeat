use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};
use serde::Deserialize;

use super::SceneEntity;

pub(super) struct WorldPalette<'a> {
    pub building: &'a Handle<StandardMaterial>,
    pub dark: &'a Handle<StandardMaterial>,
    pub signs: &'a [Handle<StandardMaterial>; 2],
}

use crate::{presentation::WorldTheme, view::VisualState};

const LAYOUT: &str = include_str!("../../../../assets/scenes/four-worlds.json");
const FOREST_BANK_CENTER: f32 = -0.15;
const FOREST_BANK_HEIGHT: f32 = 0.45;
const STREAM_WATER_CENTER: f32 = 0.31;
const STREAM_WATER_HEIGHT: f32 = 0.05;

#[derive(Deserialize)]
struct WorldLayout {
    world: String,
    sky: [f32; 3],
    horizon: [f32; 3],
    ground: [f32; 3],
    sections: Vec<SectionLayout>,
}

#[derive(Deserialize)]
struct SectionLayout {
    name: String,
    landmarks: Vec<(Landmark, [f32; 3], f32)>,
}

#[derive(Clone, Copy, Deserialize)]
enum Landmark {
    Pagoda,
    LanternGate,
    Moon,
    Railway,
    Train,
    Speaker,
    Halo,
    Tree,
    Mushrooms,
    Stream,
    Crystal,
    Vines,
    ToyHouse,
    Lollipop,
    Balloons,
    Wheel,
    Carousel,
    Island,
}

#[derive(Component)]
pub(super) struct WorldRoot {
    world: WorldTheme,
    section: Option<usize>,
    origin: Vec3,
    drift: bool,
}

#[derive(Component)]
pub(super) struct LandmarkMotion {
    origin: Transform,
    spin: f32,
    bob: f32,
    phase: f32,
}

pub(super) fn animate(
    state: Res<VisualState>,
    mut roots: Query<(&WorldRoot, &mut Transform, &mut Visibility), Without<LandmarkMotion>>,
    mut moving: Query<(&LandmarkMotion, &mut Transform), Without<WorldRoot>>,
) {
    let seconds = state.song_time.frames().max(0) as f64 / 48_000.0;
    let current = state.presentation.section_index % 3;
    let progress = if state.ready || state.quality.reduced_motion {
        1.0
    } else {
        (state.presentation.section_elapsed / 1.2).clamp(0.0, 1.0)
    };
    let reveal = progress * progress * (3.0 - 2.0 * progress);
    for (root, mut transform, mut visibility) in &mut roots {
        let current_section = root.section.is_none_or(|section| section == current);
        let departing = state.presentation.section_index > 0
            && root.section == Some((current + 2) % 3)
            && progress < 1.0;
        let shown = root.world == state.presentation.world && (current_section || departing);
        *visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        transform.translation = root.origin;
        if root.section.is_some() {
            // Authored sections emerge below the stage, leaving the cue corridor open
            let amount = if departing { reveal } else { 1.0 - reveal };
            transform.translation.y -= amount * 18.0;
        }
        if root.section.is_some() && state.presentation.section_index / 3 % 2 == 1 {
            transform.translation.x = -transform.translation.x;
        }
        if root.drift && !state.quality.reduced_motion {
            transform.translation.z += (seconds as f32 * 0.4).sin() * 4.5;
        }
    }
    for (motion, mut transform) in &mut moving {
        *transform = motion.origin;
        if !state.quality.reduced_motion {
            transform.rotate_local_z(seconds as f32 * motion.spin);
            transform.translation.y += (seconds as f32 * 0.8 + motion.phase).sin() * motion.bob;
        }
    }
}

struct Parts<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    root: Entity,
    meshes: &'a [Handle<Mesh>; 7],
    materials: &'a [Handle<StandardMaterial>; 10],
}

impl Parts<'_, '_, '_> {
    fn scenery(&mut self, world: WorldTheme, side_offset: f32) {
        for side in [-1.0, 1.0] {
            let curb = side * (5.4 + side_offset);
            match world {
                WorldTheme::Neon => {
                    self.block(
                        5,
                        Vec3::new(curb + side * 1.3, -0.05, -24.0),
                        Vec3::new(3.0, 0.22, 64.0),
                    );
                    self.block(
                        7,
                        Vec3::new(curb - side * 0.15, 0.07, -24.0),
                        Vec3::new(0.18, 0.2, 64.0),
                    );
                    for index in 0..10 {
                        let z = 4.0 - index as f32 * 6.3;
                        self.block(
                            1,
                            Vec3::new(curb + side * 1.2, 0.073, z),
                            Vec3::new(2.7, 0.014, 0.045),
                        );
                    }
                    for index in 0..4 {
                        let z = -4.0 - index as f32 * 13.0;
                        let x = curb + side * 0.75;
                        self.block(7, Vec3::new(x, 0.35, z), Vec3::new(0.9, 0.65, 1.5));
                        self.orb(5, Vec3::new(x, 0.78, z), Vec3::new(0.65, 0.45, 0.9));
                        self.bar(
                            7,
                            Vec3::new(curb, 0.0, z - 3.0),
                            Vec3::new(curb, 3.0, z - 3.0),
                            0.075,
                        );
                        self.orb(4, Vec3::new(curb, 3.06, z - 3.0), Vec3::new(0.2, 0.24, 0.2));
                        self.part(
                            3,
                            7,
                            Vec3::new(curb, 3.26, z - 3.0),
                            Vec3::new(0.3, 0.16, 0.3),
                            Quat::IDENTITY,
                        );
                    }
                    for index in 0..3 {
                        let x = side * (12.8 + side_offset + (index % 2) as f32 * 1.8);
                        let z = -12.0 - index as f32 * 18.0;
                        let height = [9.5, 13.0, 7.8][index];
                        self.block(
                            0,
                            Vec3::new(x, height * 0.5, z),
                            Vec3::new(5.0, height, 7.0),
                        );
                        self.block(6, Vec3::new(x, height, z), Vec3::new(5.4, 0.36, 7.4));
                        self.block(7, Vec3::new(x, 2.7, z + 3.6), Vec3::new(5.3, 0.2, 0.5));
                        for row in 0..3 {
                            let y = 3.9 + row as f32 * 1.75;
                            self.block(1, Vec3::new(x, y, z + 3.55), Vec3::new(3.9, 0.95, 0.1));
                            for offset in [-1.25, 0.0, 1.25] {
                                self.block(
                                    if (row + index) % 2 == 0 { 4 } else { 6 },
                                    Vec3::new(x + offset, y, z + 3.62),
                                    Vec3::new(0.68, 0.7, 0.06),
                                );
                            }
                        }
                        self.block(
                            3,
                            Vec3::new(x - side * 2.55, 5.0, z + 2.0),
                            Vec3::new(0.08, 1.9, 0.55),
                        );
                    }
                }
                WorldTheme::Forest => {
                    self.orb(
                        5,
                        Vec3::new(curb + side * 2.2, FOREST_BANK_CENTER, -24.0),
                        Vec3::new(4.0, FOREST_BANK_HEIGHT, 35.0),
                    );
                    for index in 0..6 {
                        let z = 3.0 - index as f32 * 10.0;
                        let x = curb + side * (0.5 + (index % 2) as f32 * 1.0);
                        self.orb(
                            1,
                            Vec3::new(x + side * 1.4, 0.1, z),
                            Vec3::new(1.1, 0.65, 1.4),
                        );
                        self.orb(
                            6,
                            Vec3::new(x + side * 1.5, 0.62, z),
                            Vec3::new(0.8, 0.14, 1.1),
                        );
                        for leaf in -2..=2 {
                            let angle = leaf as f32 * 0.4;
                            self.part(
                                1,
                                5 + (index % 2),
                                Vec3::new(x + angle * 0.8, 0.32 + angle.abs() * 0.3, z + 0.8),
                                Vec3::new(0.12, 0.75, 0.28),
                                Quat::from_rotation_z(-angle),
                            );
                        }
                    }
                    for index in 0..3 {
                        let p = Vec3::new(
                            side * (14.0 + side_offset),
                            4.0,
                            -18.0 - index as f32 * 19.0,
                        );
                        self.part(5, 0, p, Vec3::new(1.2, 9.0, 1.2), Quat::IDENTITY);
                        self.orb(5, p + Vec3::Y * 5.0, Vec3::new(5.0, 3.2, 4.0));
                        self.orb(
                            6,
                            p + Vec3::new(side * 2.0, 6.3, 0.5),
                            Vec3::new(3.6, 2.5, 3.0),
                        );
                        self.orb(
                            5,
                            p + Vec3::new(-side * 2.3, 4.2, 1.0),
                            Vec3::new(3.3, 2.5, 3.0),
                        );
                        let stem = Vec3::new(curb + side * 1.5, 0.5, p.z + 4.0);
                        self.part(5, 7, stem, Vec3::new(0.15, 1.0, 0.15), Quat::IDENTITY);
                        self.part(
                            4,
                            6,
                            stem + Vec3::Y * 0.5,
                            Vec3::new(0.9, 0.45, 0.9),
                            Quat::IDENTITY,
                        );
                        self.orb(4, stem + Vec3::new(0.25, 0.66, 0.45), Vec3::splat(0.07));
                    }
                }
                WorldTheme::Candy => {
                    self.block(
                        7,
                        Vec3::new(curb + side * 1.1, -0.04, -24.0),
                        Vec3::new(3.0, 0.24, 64.0),
                    );
                    for index in 0..8 {
                        let z = 5.0 - index as f32 * 8.0;
                        self.block(
                            6,
                            Vec3::new(curb + side * 1.1, 0.085, z),
                            Vec3::new(2.9, 0.025, 2.2),
                        );
                        self.bar(7, Vec3::new(curb, 0.15, z), Vec3::new(curb, 1.0, z), 0.08);
                        self.orb(6, Vec3::new(curb, 1.05, z), Vec3::splat(0.14));
                    }
                    for y in [0.45, 0.8] {
                        self.block(6, Vec3::new(curb, y, -23.0), Vec3::new(0.07, 0.09, 56.0));
                    }
                    for index in 0..3 {
                        let p = Vec3::new(
                            side * (11.2 + side_offset),
                            0.0,
                            -13.0 - index as f32 * 18.0,
                        );
                        self.block(0, p + Vec3::Y * 1.1, Vec3::new(4.4, 2.2, 3.8));
                        self.part(
                            4,
                            6,
                            p + Vec3::Y * 2.2,
                            Vec3::new(3.1, 1.5, 2.7),
                            Quat::IDENTITY,
                        );
                        self.block(4, p + Vec3::new(0.0, 1.2, 1.93), Vec3::new(3.3, 0.8, 0.07));
                        for stripe in [-1.2, 0.0, 1.2] {
                            self.block(
                                7,
                                p + Vec3::new(stripe, 1.2, 1.99),
                                Vec3::new(0.16, 0.95, 0.05),
                            );
                        }
                        self.bar(7, p + Vec3::Y * 3.2, p + Vec3::Y * 5.1, 0.055);
                        self.part(
                            0,
                            3,
                            p + Vec3::new(0.45, 4.8, 0.0),
                            Vec3::new(0.9, 0.42, 0.03),
                            Quat::from_rotation_z(0.15),
                        );
                    }
                }
                WorldTheme::StarSea => {
                    for index in 0..6 {
                        let z = 3.0 - index as f32 * 10.5;
                        let x = curb + side * (1.5 + (index % 2) as f32);
                        for layer in 0..3 {
                            self.part(
                                6,
                                if layer == 1 { 6 } else { 5 },
                                Vec3::new(x, -0.6 + layer as f32 * 0.26, z),
                                Vec3::new(2.2 - layer as f32 * 0.2, 0.35, 2.6),
                                Quat::IDENTITY,
                            );
                        }
                        for spike in 0..2 {
                            self.crystal(
                                spike,
                                Vec3::new(
                                    x + spike as f32 * side * 0.8,
                                    0.4 + spike as f32 * 0.3,
                                    z,
                                ),
                                Vec3::new(0.3, 1.4 + spike as f32 * 0.5, 0.3),
                                Quat::from_rotation_z(-side * 0.25),
                            );
                        }
                    }
                    for index in 0..3 {
                        let p = Vec3::new(
                            side * (17.0 + index as f32 * 3.0),
                            5.0 + index as f32 * 2.6,
                            -28.0 - index as f32 * 18.0,
                        );
                        self.part(6, 1, p, Vec3::new(5.2, 5.5, 3.2), Quat::from_rotation_z(PI));
                        self.orb(6, p + Vec3::Y * 2.3, Vec3::new(5.3, 0.35, 3.3));
                        self.part(
                            6,
                            5,
                            p + Vec3::Y * 3.7,
                            Vec3::new(0.8, 2.9, 0.8),
                            Quat::IDENTITY,
                        );
                        self.part(
                            3,
                            3,
                            p + Vec3::Y * 1.5,
                            Vec3::new(5.5, 0.2, 3.5),
                            Quat::IDENTITY,
                        );
                    }
                }
            }
        }
        // Silhouettes behind the playable horizon replace a flat sky/floor seam
        for index in 0..7 {
            let x = (index as f32 - 3.0) * 13.0;
            let height = [11.0, 17.0, 9.0, 12.0, 8.0, 15.0, 10.0][index];
            match world {
                WorldTheme::Neon => {
                    self.block(
                        1,
                        Vec3::new(x, height * 0.5, -82.0),
                        Vec3::new(8.0, height, 6.0),
                    );
                    self.block(
                        6,
                        Vec3::new(x, height + 0.2, -82.0),
                        Vec3::new(8.3, 0.4, 6.3),
                    );
                    for y in [height * 0.4, height * 0.75] {
                        self.block(4, Vec3::new(x, y, -78.95), Vec3::new(5.0, 0.17, 0.06));
                    }
                }
                WorldTheme::Forest => {
                    self.orb(
                        1,
                        Vec3::new(x, height * 0.5, -84.0),
                        Vec3::new(12.0, height * 0.8, 6.0),
                    );
                    self.orb(
                        5,
                        Vec3::new(x - 3.0, height * 0.7, -80.0),
                        Vec3::new(9.0, height * 0.5, 5.0),
                    );
                }
                WorldTheme::Candy => {
                    self.orb(
                        5,
                        Vec3::new(x, 1.5, -82.0),
                        Vec3::new(12.0, height * 0.65, 8.0),
                    );
                    self.orb(
                        6,
                        Vec3::new(x + 4.0, height * 0.8, -84.0),
                        Vec3::new(3.8, 2.4, 1.0),
                    );
                }
                WorldTheme::StarSea => {
                    for star in 0..5 {
                        self.orb(
                            4,
                            Vec3::new(
                                x + (star * 7 % 11) as f32,
                                height + star as f32 * 3.4,
                                -82.0,
                            ),
                            Vec3::splat(0.075 + (star % 2) as f32 * 0.06),
                        );
                    }
                }
            }
        }
    }

    fn part(
        &mut self,
        shape: usize,
        color: usize,
        position: Vec3,
        scale: Vec3,
        rotation: Quat,
    ) -> Entity {
        self.commands
            .spawn((
                SceneEntity,
                Mesh3d(self.meshes[shape].clone()),
                MeshMaterial3d(self.materials[color].clone()),
                Transform::from_translation(position)
                    .with_scale(scale)
                    .with_rotation(rotation),
                ChildOf(self.root),
            ))
            .id()
    }

    fn block(&mut self, color: usize, position: Vec3, scale: Vec3) {
        self.part(0, color, position, scale, Quat::IDENTITY);
    }

    fn orb(&mut self, color: usize, position: Vec3, scale: Vec3) -> Entity {
        self.part(1, color, position, scale, Quat::IDENTITY)
    }

    fn bar(&mut self, color: usize, a: Vec3, b: Vec3, radius: f32) {
        let direction = b - a;
        self.part(
            2,
            color,
            (a + b) * 0.5,
            Vec3::new(radius, direction.length(), radius),
            Quat::from_rotation_arc(Vec3::Y, direction.normalize()),
        );
    }

    fn crystal(&mut self, variant: usize, position: Vec3, scale: Vec3, rotation: Quat) {
        self.part(6, 8 + variant % 2, position, scale, rotation);
        // A narrow surface seam leaves the colored facets available to physical lighting
        self.part(
            6,
            2 + variant % 2,
            position + rotation * Vec3::new(0.0, 0.0, scale.z * 0.88),
            scale * Vec3::new(0.09, 0.62, 0.04),
            rotation,
        );
    }

    fn motion(
        &mut self,
        entity: Entity,
        position: Vec3,
        scale: Vec3,
        rotation: Quat,
        spin: f32,
        bob: f32,
    ) {
        self.commands.entity(entity).insert(LandmarkMotion {
            origin: Transform::from_translation(position)
                .with_scale(scale)
                .with_rotation(rotation),
            spin,
            bob,
            phase: position.x,
        });
    }

    fn landmark(&mut self, kind: Landmark) {
        let upright = Quat::IDENTITY;
        let facing = Quat::from_rotation_x(FRAC_PI_2);
        match kind {
            Landmark::Pagoda | Landmark::ToyHouse => {
                let toy = matches!(kind, Landmark::ToyHouse);
                self.block(0, Vec3::new(0.0, 1.4, 0.0), Vec3::new(3.4, 2.8, 3.2));
                for level in 0..if toy { 1 } else { 2 } {
                    let y = 2.7 + level as f32 * 1.6;
                    self.part(
                        4,
                        6,
                        Vec3::new(0.0, y, 0.0),
                        Vec3::new(2.8 - level as f32 * 0.6, 1.0, 2.2 - level as f32 * 0.4),
                        upright,
                    );
                    self.part(
                        3,
                        if toy { 3 } else { 2 },
                        Vec3::new(0.0, y + 0.2, 0.0),
                        Vec3::new(2.8 - level as f32 * 0.6, 0.32, 2.2 - level as f32 * 0.4),
                        upright,
                    );
                    for rib in [-0.7, 0.0, 0.7] {
                        self.bar(
                            5,
                            Vec3::new(rib, y + 0.85, 0.3),
                            Vec3::new(rib * 2.1, y + 0.27, 1.8 - level as f32 * 0.3),
                            0.035,
                        );
                    }
                    if !toy && level == 0 {
                        self.block(0, Vec3::new(0.0, 3.3, 0.0), Vec3::new(2.3, 1.4, 2.1));
                    }
                }
                for x in [-1.0, 1.0] {
                    self.block(4, Vec3::new(x, 1.4, 1.63), Vec3::new(0.65, 1.1, 0.07));
                    self.block(7, Vec3::new(x, 0.82, 1.7), Vec3::new(0.83, 0.12, 0.16));
                    self.block(7, Vec3::new(x, 1.98, 1.7), Vec3::new(0.83, 0.12, 0.16));
                    self.block(1, Vec3::new(x, 1.4, 1.69), Vec3::new(0.065, 1.1, 0.05));
                    self.bar(1, Vec3::new(x, 0.0, 1.7), Vec3::new(x, 2.7, 1.7), 0.08);
                }
                self.block(1, Vec3::new(0.0, 0.75, 1.65), Vec3::new(0.58, 1.5, 0.09));
            }
            Landmark::LanternGate => {
                for x in [-1.6, 1.6] {
                    self.bar(1, Vec3::new(x, 0.0, 0.0), Vec3::new(x, 5.2, 0.0), 0.13);
                }
                self.bar(3, Vec3::new(-2.0, 5.1, 0.0), Vec3::new(2.0, 5.1, 0.0), 0.12);
                for (index, x) in [-1.25, 0.0, 1.25].into_iter().enumerate() {
                    self.bar(1, Vec3::new(x, 5.1, 0.0), Vec3::new(x, 4.4, 0.0), 0.025);
                    self.orb(
                        2 + index % 2,
                        Vec3::new(x, 4.05, 0.0),
                        Vec3::new(0.38, 0.52, 0.38),
                    );
                }
            }
            Landmark::Railway => {
                for z in [-4.0, 4.0] {
                    self.block(0, Vec3::new(0.0, 2.6, z), Vec3::new(0.55, 5.2, 0.8));
                }
                self.block(1, Vec3::new(0.0, 5.2, 0.0), Vec3::new(3.0, 0.45, 12.0));
                for x in [-1.2, 1.2] {
                    self.block(2, Vec3::new(x, 5.5, 0.0), Vec3::new(0.06, 0.08, 12.0));
                }
                for z in [-3.0, 3.0] {
                    self.bar(0, Vec3::new(-1.3, 5.3, z), Vec3::new(1.3, 8.2, z), 0.12);
                }
            }
            Landmark::Train => {
                self.part(
                    5,
                    3,
                    Vec3::ZERO,
                    Vec3::new(1.2, 1.2, 5.0),
                    Quat::from_rotation_x(FRAC_PI_2),
                );
                for z in [-2.0, 0.0, 2.0] {
                    self.block(4, Vec3::new(1.02, 0.0, z), Vec3::new(0.05, 0.6, 1.3));
                }
            }
            Landmark::Speaker => {
                self.block(1, Vec3::new(0.0, 2.2, 0.0), Vec3::new(2.3, 4.4, 1.4));
                for y in [1.1, 3.1] {
                    self.part(3, 2, Vec3::new(0.0, y, 0.8), Vec3::splat(0.88), facing);
                    self.orb(0, Vec3::new(0.0, y, 0.8), Vec3::new(0.62, 0.62, 0.2));
                }
                self.bar(
                    3,
                    Vec3::new(-1.22, 0.0, 0.0),
                    Vec3::new(-1.22, 4.5, 0.0),
                    0.06,
                );
            }
            Landmark::Moon => {
                self.orb(4, Vec3::ZERO, Vec3::splat(1.0));
            }
            Landmark::Halo => {
                for (index, scale) in [1.0, 0.78].into_iter().enumerate() {
                    let rotation = Quat::from_rotation_x(FRAC_PI_2 + index as f32 * 0.22);
                    let entity = self.part(3, 2 + index, Vec3::ZERO, Vec3::splat(scale), rotation);
                    self.motion(
                        entity,
                        Vec3::ZERO,
                        Vec3::splat(scale),
                        rotation,
                        if index == 0 { 0.05 } else { -0.08 },
                        0.0,
                    );
                }
            }
            Landmark::Tree => {
                self.part(
                    5,
                    0,
                    Vec3::new(0.0, 2.7, 0.0),
                    Vec3::new(0.9, 5.4, 0.9),
                    upright,
                );
                for (index, x) in [-1.5, 0.0, 1.5].into_iter().enumerate() {
                    let top = Vec3::new(x, 5.0 + if index == 1 { 1.4 } else { 0.0 }, 0.0);
                    self.bar(0, Vec3::new(0.0, 2.2, 0.0), top, 0.24);
                    self.orb(5, top, Vec3::new(2.1, 1.45, 1.75));
                    self.orb(
                        6,
                        top + Vec3::new(-0.6, 0.55, 0.3),
                        Vec3::new(1.4, 1.2, 1.2),
                    );
                    self.orb(4, top + Vec3::new(x * 0.3, -0.6, 0.8), Vec3::splat(0.14));
                }
            }
            Landmark::Mushrooms => {
                for (index, x) in [-1.0, 0.6, 1.5].into_iter().enumerate() {
                    let h = [1.5, 2.7, 0.9][index];
                    self.part(
                        5,
                        7,
                        Vec3::new(x, h * 0.4, 0.0),
                        Vec3::new(0.25, h * 0.8, 0.25),
                        upright,
                    );
                    self.part(
                        4,
                        6,
                        Vec3::new(x, h, 0.0),
                        Vec3::new(h * 0.7, h * 0.45, h * 0.7),
                        upright,
                    );
                    for dot in [-0.5, 0.0, 0.5] {
                        self.orb(
                            4,
                            Vec3::new(x + dot * h * 0.55, h * 1.2, h * 0.38),
                            Vec3::splat(h * 0.045),
                        );
                    }
                }
            }
            Landmark::Stream => {
                // The smallest authored Stream must clear the shared raised forest bank
                self.orb(1, Vec3::new(0.0, 0.25, 0.0), Vec3::new(3.02, 0.05, 6.2));
                self.orb(
                    8,
                    Vec3::new(0.0, STREAM_WATER_CENTER, 0.0),
                    Vec3::new(2.8, STREAM_WATER_HEIGHT, 6.0),
                );
                for (index, z) in [-3.5, 0.0, 3.5].into_iter().enumerate() {
                    self.orb(
                        1,
                        Vec3::new(if index % 2 == 0 { -2.2 } else { 2.2 }, 0.1, z),
                        Vec3::new(1.0, 0.6, 1.4),
                    );
                    self.part(
                        3,
                        2,
                        Vec3::new(0.0, STREAM_WATER_CENTER + STREAM_WATER_HEIGHT, z),
                        Vec3::new(1.6, 0.15, 0.9),
                        upright,
                    );
                }
            }
            Landmark::Crystal => {
                for (index, x) in [-0.9, 0.0, 0.8].into_iter().enumerate() {
                    self.crystal(
                        index,
                        Vec3::new(x, 1.8, 0.0),
                        Vec3::new(0.65, [2.6, 4.6, 3.2][index], 0.7),
                        Quat::from_rotation_z(-x * 0.22),
                    );
                }
                self.orb(1, Vec3::new(0.0, -0.25, 0.0), Vec3::new(2.0, 0.4, 1.6));
            }
            Landmark::Vines => {
                for x in [-1.0, 1.0] {
                    for index in 0..4 {
                        let y = index as f32 * 1.3;
                        self.bar(
                            5,
                            Vec3::new(x, y, 0.0),
                            Vec3::new(x + 0.4, y + 1.2, 0.0),
                            0.065,
                        );
                        let p = Vec3::new(x + 0.6, y, 0.0);
                        let s = Vec3::new(0.5, 0.24, 0.17);
                        let entity = self.orb(2, p, s);
                        self.motion(entity, p, s, upright, 0.0, 0.06);
                    }
                }
            }
            Landmark::Lollipop => {
                self.bar(7, Vec3::ZERO, Vec3::Y * 3.8, 0.12);
                self.orb(9, Vec3::Y * 4.0, Vec3::new(1.2, 1.2, 0.32));
                self.part(3, 4, Vec3::new(0.0, 4.0, 0.33), Vec3::splat(0.75), facing);
                self.orb(8, Vec3::new(0.0, 4.0, 0.36), Vec3::splat(0.24));
            }
            Landmark::Balloons => {
                for (index, x) in [-1.0, 0.0, 1.0].into_iter().enumerate() {
                    let p = Vec3::new(x, 2.4 + if index == 1 { 0.7 } else { 0.0 }, 0.0);
                    self.bar(7, Vec3::ZERO, p, 0.018);
                    let s = Vec3::new(0.8, 1.0, 0.7);
                    let entity = self.orb(8 + index % 2, p, s);
                    self.motion(entity, p, s, upright, 0.0, 0.12);
                }
            }
            Landmark::Wheel => {
                self.bar(7, Vec3::new(-1.8, -5.0, 0.0), Vec3::ZERO, 0.18);
                self.bar(7, Vec3::new(1.8, -5.0, 0.0), Vec3::ZERO, 0.18);
                let rotor = self
                    .commands
                    .spawn((
                        SceneEntity,
                        Transform::default(),
                        Visibility::Inherited,
                        ChildOf(self.root),
                    ))
                    .id();
                self.motion(rotor, Vec3::ZERO, Vec3::ONE, upright, 0.08, 0.0);
                let old_root = self.root;
                self.root = rotor;
                self.part(3, 3, Vec3::ZERO, Vec3::splat(3.6), facing);
                for index in 0..6 {
                    let angle = index as f32 * TAU / 6.0;
                    let point = Vec3::new(angle.cos(), angle.sin(), 0.0) * 3.6;
                    self.bar(7, Vec3::ZERO, point, 0.055);
                    self.orb(8 + index % 2, point, Vec3::new(0.6, 0.6, 0.5));
                    self.block(
                        4,
                        point + Vec3::new(0.0, 0.03, 0.49),
                        Vec3::new(0.25, 0.17, 0.035),
                    );
                }
                self.root = old_root;
            }
            Landmark::Carousel => {
                self.part(4, 9, Vec3::Y * 4.5, Vec3::new(3.0, 1.5, 3.0), upright);
                self.part(3, 3, Vec3::Y * 4.8, Vec3::new(3.0, 0.22, 3.0), upright);
                self.bar(7, Vec3::ZERO, Vec3::Y * 4.8, 0.22);
                self.part(2, 0, Vec3::Y * 0.1, Vec3::new(3.0, 0.25, 3.0), upright);
                for index in 0..4 {
                    let angle = index as f32 * FRAC_PI_2;
                    let p = Vec3::new(angle.cos() * 1.9, 1.8, angle.sin() * 1.9);
                    self.bar(7, p.with_y(0.3), p.with_y(4.0), 0.045);
                    let s = Vec3::new(0.7, 0.35, 0.4);
                    let entity = self.orb(8 + index % 2, p, s);
                    self.motion(entity, p, s, upright, 0.0, 0.3);
                }
            }
            Landmark::Island => {
                self.part(
                    6,
                    1,
                    Vec3::new(0.0, -1.7, 0.0),
                    Vec3::new(3.0, 4.0, 2.2),
                    Quat::from_rotation_z(PI),
                );
                self.orb(5, Vec3::ZERO, Vec3::new(3.1, 0.4, 2.3));
                self.crystal(
                    0,
                    Vec3::new(-0.8, 1.1, 0.0),
                    Vec3::new(0.7, 2.4, 0.7),
                    upright,
                );
                self.part(
                    3,
                    3,
                    Vec3::new(0.0, -0.4, 0.0),
                    Vec3::new(3.3, 0.3, 2.5),
                    upright,
                );
            }
        }
    }
}

pub(super) fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    shapes: [&Handle<Mesh>; 2],
    palette: WorldPalette<'_>,
    side_offset: f32,
) -> Vec<Entity> {
    let layouts: Vec<WorldLayout> =
        serde_json::from_str(LAYOUT).expect("checked authored world layouts");
    let shapes = [
        shapes[0].clone(),
        shapes[1].clone(),
        meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(12)),
        meshes.add(
            Torus::new(0.94, 1.0)
                .mesh()
                .major_resolution(48)
                .minor_resolution(6),
        ),
        meshes.add(profile_mesh(
            &[
                (0.0, 0.04),
                (0.2, 1.0),
                (0.38, 0.92),
                (0.7, 0.68),
                (1.0, 0.0),
            ],
            20,
        )),
        meshes.add(profile_mesh(
            &[(-0.5, 0.65), (-0.3, 0.9), (0.2, 0.55), (0.5, 0.4)],
            12,
        )),
        meshes.add(profile_mesh(&[(-0.5, 0.65), (0.18, 1.0), (0.5, 0.0)], 6)),
    ];
    let themes = [
        WorldTheme::Neon,
        WorldTheme::Forest,
        WorldTheme::Candy,
        WorldTheme::StarSea,
    ];
    let mut roots = Vec::new();
    for (theme, layout) in themes.into_iter().zip(layouts) {
        let colors = match theme {
            WorldTheme::Neon => [
                [0.28, 0.24, 0.34],
                [0.025, 0.035, 0.065],
                [0.10, 0.65, 1.0],
                [1.0, 0.12, 0.48],
                [1.0, 0.64, 0.27],
                [0.26, 0.25, 0.4],
                [0.14, 0.27, 0.37],
                [0.63, 0.52, 0.46],
                [0.16, 0.50, 0.72],
                [0.77, 0.23, 0.43],
            ],
            WorldTheme::Forest => [
                [0.16, 0.10, 0.10],
                [0.06, 0.11, 0.10],
                [0.10, 0.85, 0.65],
                [0.48, 0.35, 1.0],
                [0.8, 1.0, 0.55],
                [0.035, 0.29, 0.21],
                [0.18, 0.37, 0.29],
                [0.63, 0.67, 0.49],
                [0.14, 0.51, 0.47],
                [0.39, 0.24, 0.62],
            ],
            WorldTheme::Candy => [
                [0.64, 0.23, 0.38],
                [0.23, 0.10, 0.28],
                [0.20, 0.78, 1.0],
                [1.0, 0.29, 0.57],
                [1.0, 0.86, 0.48],
                [0.48, 0.23, 0.64],
                [0.85, 0.35, 0.49],
                [0.94, 0.76, 0.57],
                [0.16, 0.58, 0.82],
                [0.82, 0.22, 0.44],
            ],
            WorldTheme::StarSea => [
                [0.16, 0.17, 0.35],
                [0.065, 0.045, 0.16],
                [0.20, 0.70, 1.0],
                [0.63, 0.27, 1.0],
                [0.78, 0.87, 1.0],
                [0.22, 0.19, 0.45],
                [0.22, 0.39, 0.54],
                [0.49, 0.48, 0.68],
                [0.13, 0.48, 0.71],
                [0.46, 0.19, 0.69],
            ],
        };
        let mut world_materials = colors.map(|rgb| {
            materials.add(StandardMaterial {
                base_color: Color::srgb_from_array(rgb),
                metallic: if theme == WorldTheme::StarSea {
                    0.45
                } else {
                    0.12
                },
                perceptual_roughness: 0.35,
                ..default()
            })
        });
        for index in [2, 3, 4] {
            if let Some(mut material) = materials.get_mut(&world_materials[index]) {
                material.emissive = LinearRgba::from(Color::srgb_from_array(colors[index]))
                    * if index == 4 { 2.5 } else { 4.0 };
            }
        }
        if theme == WorldTheme::Neon {
            if let Some(mut material) = materials.get_mut(palette.building) {
                material.base_color = Color::srgb_from_array(colors[0]);
            }
            world_materials[0] = palette.building.clone();
            world_materials[1] = palette.dark.clone();
            // Retain shared street material handles so runtime lighting updates reach this world
            world_materials[2] = palette.signs[0].clone();
            world_materials[3] = palette.signs[1].clone();
        }
        for (section_index, section) in layout.sections.iter().enumerate() {
            for &(kind, position, scale) in &section.landmarks {
                let mut origin = Vec3::from_array(position);
                if origin.x != 0.0 {
                    origin.x += origin.x.signum() * side_offset;
                }
                let root = commands
                    .spawn((
                        SceneEntity,
                        Name::new(format!("{} / {}", layout.world, section.name)),
                        WorldRoot {
                            world: theme,
                            section: Some(section_index),
                            origin,
                            drift: matches!(kind, Landmark::Train),
                        },
                        Transform::from_translation(origin).with_scale(Vec3::splat(scale)),
                        Visibility::Hidden,
                    ))
                    .id();
                roots.push(root);
                Parts {
                    commands,
                    root,
                    meshes: &shapes,
                    materials: &world_materials,
                }
                .landmark(kind);
            }
        }
        let background = commands
            .spawn((
                SceneEntity,
                Name::new(format!("{} atmosphere", layout.world)),
                WorldRoot {
                    world: theme,
                    section: None,
                    origin: Vec3::ZERO,
                    drift: false,
                },
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        roots.push(background);
        let ground = materials.add(StandardMaterial {
            base_color: Color::srgb_from_array(layout.ground),
            perceptual_roughness: if theme == WorldTheme::Forest {
                0.22
            } else {
                0.4
            },
            metallic: 0.25,
            ..default()
        });
        commands.spawn((
            SceneEntity,
            Mesh3d(shapes[0].clone()),
            MeshMaterial3d(ground),
            Transform::from_xyz(0.0, -0.42, -38.0).with_scale(Vec3::new(150.0, 0.1, 160.0)),
            ChildOf(background),
        ));
        let sky = meshes.add(sky_mesh(layout.horizon, layout.sky));
        let sky_material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            ..default()
        });
        commands.spawn((
            SceneEntity,
            Mesh3d(sky),
            MeshMaterial3d(sky_material),
            Transform::from_xyz(0.0, 26.0, -100.0).with_scale(Vec3::new(160.0, 90.0, 0.3)),
            ChildOf(background),
        ));
        // Atmospheric points are outside the central cue corridor and all worlds reuse one mesh
        let mut parts = Parts {
            commands,
            root: background,
            meshes: &shapes,
            materials: &world_materials,
        };
        parts.scenery(theme, side_offset);
        for index in 0..12 {
            let side = if index % 2 == 0 { -1.0 } else { 1.0 };
            let p = Vec3::new(
                side * (8.0 + (index * 7 % 19) as f32),
                6.0 + (index * 11 % 17) as f32,
                -26.0 - (index * 13 % 45) as f32,
            );
            let scale = Vec3::splat(if theme == WorldTheme::StarSea {
                0.10
            } else {
                0.06
            });
            let entity = parts.orb(4, p, scale);
            parts.motion(entity, p, scale, Quat::IDENTITY, 0.0, 0.18);
        }
    }
    roots
}

fn sky_mesh(bottom: [f32; 3], top: [f32; 3]) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::default());
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        unreachable!()
    };
    let bottom = Color::srgb_from_array(bottom).to_linear();
    let top = Color::srgb_from_array(top).to_linear();
    let colors: Vec<[f32; 4]> = positions
        .iter()
        .map(|p| bottom.mix(&top, p[1] + 0.5).to_f32_array())
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh
}

fn profile_mesh(profile: &[(f32, f32)], sides: usize) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for pair in profile.windows(2) {
        let [(y0, r0), (y1, r1)] = [pair[0], pair[1]];
        for side in 0..sides {
            let a = side as f32 * TAU / sides as f32;
            let b = (side + 1) as f32 * TAU / sides as f32;
            let base = positions.len() as u32;
            for (y, r, angle) in [(y0, r0, a), (y0, r0, b), (y1, r1, a), (y1, r1, b)] {
                positions.push([angle.cos() * r, y, angle.sin() * r]);
                let angle = if sides == 6 { (a + b) * 0.5 } else { angle };
                normals.push(
                    Vec3::new(angle.cos() * (y1 - y0), r0 - r1, angle.sin() * (y1 - y0))
                        .normalize()
                        .to_array(),
                );
            }
            indices.extend([base, base + 2, base + 1, base + 1, base + 2, base + 3]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_world_hides_wheel_grandchildren_through_native_visibility_propagation() {
        fn setup_wheel(
            mut commands: Commands,
            mut meshes: ResMut<Assets<Mesh>>,
            mut materials: ResMut<Assets<StandardMaterial>>,
        ) {
            let root = commands
                .spawn((
                    WorldRoot {
                        world: WorldTheme::Candy,
                        section: Some(1),
                        origin: Vec3::ZERO,
                        drift: false,
                    },
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id();
            let shape = meshes.add(Cuboid::default());
            let material = materials.add(StandardMaterial::default());
            Parts {
                commands: &mut commands,
                root,
                meshes: &std::array::from_fn(|_| shape.clone()),
                materials: &std::array::from_fn(|_| material.clone()),
            }
            .landmark(Landmark::Wheel);
        }

        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            bevy::asset::AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
            TransformPlugin,
            bevy::camera::visibility::VisibilityPlugin,
        ))
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<VisualState>()
        .add_systems(Startup, setup_wheel)
        .add_systems(Update, animate);
        for (world, visible) in [
            (WorldTheme::Neon, false),
            (WorldTheme::Candy, true),
            (WorldTheme::Forest, false),
        ] {
            {
                let mut state = app.world_mut().resource_mut::<VisualState>();
                state.presentation.world = world;
                state.presentation.section_index = 1;
                state.presentation.section_elapsed = 2.0;
            }
            app.update();
            let mut meshes = app
                .world_mut()
                .query_filtered::<&InheritedVisibility, With<Mesh3d>>();
            assert_eq!(meshes.iter(app.world()).count(), 21);
            assert!(
                meshes
                    .iter(app.world())
                    .all(|inherited| inherited.get() == visible)
            );
        }
    }

    #[test]
    fn authored_worlds_have_three_distinct_spatial_sections_and_clear_play_area() {
        let worlds: Vec<WorldLayout> = serde_json::from_str(LAYOUT).unwrap();
        assert_eq!(worlds.len(), 4);
        for world in worlds {
            assert_eq!(world.sections.len(), 3);
            let names: std::collections::HashSet<_> =
                world.sections.iter().map(|s| &s.name).collect();
            assert_eq!(names.len(), 3);
            for section in world.sections {
                assert!(section.landmarks.len() >= 4);
                for (kind, position, scale) in section.landmarks {
                    assert!(position.into_iter().all(f32::is_finite) && scale > 0.0);
                    assert!(
                        position[0].abs() >= 6.0
                            || matches!(kind, Landmark::Halo) && position[1] - scale >= 6.0
                    );
                    if matches!(kind, Landmark::Stream) {
                        assert!(
                            position[1] + scale * (STREAM_WATER_CENTER - STREAM_WATER_HEIGHT)
                                > FOREST_BANK_CENTER + FOREST_BANK_HEIGHT,
                            "the complete water surface must clear the raised forest bank"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn section_choice_and_motion_are_reconstructed_from_song_cursor() {
        let mut app = App::new();
        app.init_resource::<VisualState>()
            .add_systems(Update, animate);
        let rotating = app
            .world_mut()
            .spawn((
                LandmarkMotion {
                    origin: Transform::from_xyz(8.0, 4.0, -20.0),
                    spin: 0.2,
                    bob: 0.3,
                    phase: 0.0,
                },
                Transform::default(),
            ))
            .id();
        let first = app
            .world_mut()
            .spawn((
                WorldRoot {
                    world: WorldTheme::Neon,
                    section: Some(0),
                    origin: Vec3::new(7.0, 0.0, -10.0),
                    drift: false,
                },
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        let next = app
            .world_mut()
            .spawn((
                WorldRoot {
                    world: WorldTheme::Neon,
                    section: Some(1),
                    origin: Vec3::new(8.0, 0.0, -10.0),
                    drift: false,
                },
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.presentation.world = WorldTheme::Neon;
            state.presentation.section_elapsed = 2.0;
        }
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(first).unwrap(),
            Visibility::Inherited
        );
        assert_eq!(
            *app.world().get::<Visibility>(next).unwrap(),
            Visibility::Hidden
        );
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.presentation.section_index = 1;
            state.presentation.section_elapsed = 2.0;
        }
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(first).unwrap(),
            Visibility::Hidden
        );
        assert_eq!(
            *app.world().get::<Visibility>(next).unwrap(),
            Visibility::Inherited
        );
        assert_eq!(
            app.world().get::<Transform>(next).unwrap().translation,
            Vec3::new(8.0, 0.0, -10.0)
        );
        app.world_mut().resource_mut::<VisualState>().song_time =
            cocobeat_schema::SongTime::from_frames(240_000);
        app.update();
        let pose = *app.world().get::<Transform>(rotating).unwrap();
        app.update();
        assert_eq!(*app.world().get::<Transform>(rotating).unwrap(), pose);
        app.world_mut().resource_mut::<VisualState>().song_time = cocobeat_schema::SongTime::ZERO;
        app.update();
        assert_eq!(
            *app.world().get::<Transform>(rotating).unwrap(),
            Transform::from_xyz(8.0, 4.0, -20.0)
        );
        app.world_mut()
            .resource_mut::<VisualState>()
            .quality
            .reduced_motion = true;
        app.world_mut().resource_mut::<VisualState>().song_time =
            cocobeat_schema::SongTime::from_frames(240_000);
        app.update();
        assert_eq!(
            *app.world().get::<Transform>(rotating).unwrap(),
            Transform::from_xyz(8.0, 4.0, -20.0)
        );
    }
}
