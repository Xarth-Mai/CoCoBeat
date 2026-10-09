//! Original blue/pink plush characters; animation reads presentation facts only

use std::f32::consts::{PI, TAU};

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};

use super::{SceneEntity, part};
use crate::view::VisualState;

/// Authored poses use mirrored player-local coordinates so every clip works for both rigs
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    travel: Vec3,
    turn: Vec3,
    squash: f32,
    arms: [f32; 2],
    feet: [f32; 2],
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            travel: Vec3::ZERO,
            turn: Vec3::ZERO,
            squash: 0.0,
            arms: [0.0; 2],
            feet: [0.0; 2],
        }
    }
}

impl Pose {
    fn blend(self, other: Self, weight: f32) -> Self {
        let lerp = |a, b| a + (b - a) * weight;
        Self {
            travel: self.travel.lerp(other.travel, weight),
            turn: self.turn.lerp(other.turn, weight),
            squash: lerp(self.squash, other.squash),
            arms: std::array::from_fn(|i| lerp(self.arms[i], other.arms[i])),
            feet: std::array::from_fn(|i| lerp(self.feet[i], other.feet[i])),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Clip {
    Solo(u8),
    Duo(u8),
}

impl Clip {
    fn duration(self) -> f64 {
        match self {
            Self::Solo(i) => [0.46, 0.60, 0.70, 0.72, 0.90, 0.94, 0.68, 0.66][usize::from(i % 8)],
            Self::Duo(i) => [1.15, 1.30, 1.60, 1.55, 1.30, 1.65][usize::from(i % 6)],
        }
    }

    /// Five authored keys: anticipation, contact/release, follow-through and return
    fn keys(self) -> [Pose; 5] {
        let mut keys = [Pose::default(); 5];
        let (anticipate, contact, follow) = match self {
            Self::Solo(i) => match i % 8 {
                // Tap: alternating hands and a planted heel
                0 => (
                    pose(
                        [0.0, -0.045, 0.0],
                        [0.12, 0.0, -0.08],
                        0.13,
                        [-0.3, 0.75],
                        [0.0, 0.3],
                    ),
                    pose(
                        [0.04, 0.09, 0.0],
                        [-0.08, 0.12, 0.12],
                        -0.09,
                        [0.7, -0.2],
                        [0.3, -0.1],
                    ),
                    pose(
                        [0.0, 0.015, 0.0],
                        [0.0, -0.07, -0.03],
                        0.02,
                        [0.15, 0.25],
                        [0.0, 0.0],
                    ),
                ),
                // Step: body weight crosses the support foot, then returns
                1 => (
                    pose(
                        [-0.11, -0.035, 0.0],
                        [0.0, -0.12, -0.12],
                        0.08,
                        [0.3, -0.2],
                        [-0.3, 0.2],
                    ),
                    pose(
                        [0.32, 0.12, 0.10],
                        [0.0, 0.32, 0.22],
                        -0.05,
                        [-0.3, 0.8],
                        [0.65, -0.2],
                    ),
                    pose(
                        [0.12, 0.035, 0.04],
                        [0.0, 0.12, 0.05],
                        0.03,
                        [0.4, 0.2],
                        [-0.2, 0.3],
                    ),
                ),
                // Hop: broad silhouette at apex, planted compressed landing
                2 => (
                    pose(
                        [0.0, -0.055, 0.0],
                        [0.12, 0.0, 0.0],
                        0.19,
                        [-0.25, -0.25],
                        [0.25, 0.25],
                    ),
                    pose(
                        [0.0, 0.68, 0.0],
                        [-0.08, 0.08, -0.08],
                        -0.13,
                        [1.2, 1.2],
                        [-0.65, -0.65],
                    ),
                    pose(
                        [0.0, -0.045, 0.0],
                        [0.08, 0.0, 0.03],
                        0.16,
                        [0.45, 0.45],
                        [0.15, 0.15],
                    ),
                ),
                3 => (
                    pose(
                        [-0.10, -0.04, 0.0],
                        [0.0, -0.20, -0.16],
                        0.15,
                        [0.0, 0.4],
                        [0.2, 0.1],
                    ),
                    pose(
                        [0.38, 0.45, 0.12],
                        [-0.08, 0.48, 0.30],
                        -0.10,
                        [1.35, 0.5],
                        [-0.65, 0.6],
                    ),
                    pose(
                        [0.18, -0.03, 0.0],
                        [0.03, 0.12, 0.12],
                        0.12,
                        [0.4, 0.8],
                        [0.2, -0.2],
                    ),
                ),
                // Half turn shows the back briefly, then presents the face again
                4 => (
                    pose(
                        [-0.06, -0.035, 0.0],
                        [0.0, -0.35, -0.10],
                        0.12,
                        [-0.25, 0.7],
                        [0.4, -0.3],
                    ),
                    pose(
                        [0.10, 0.28, 0.08],
                        [0.0, PI, 0.14],
                        -0.08,
                        [1.1, 1.1],
                        [-0.3, 0.4],
                    ),
                    pose(
                        [0.05, 0.03, 0.0],
                        [0.0, TAU - 0.25, -0.06],
                        0.09,
                        [0.65, 0.35],
                        [0.1, -0.2],
                    ),
                ),
                5 => (
                    pose(
                        [0.0, -0.02, 0.0],
                        [0.0, 0.10, -0.06],
                        0.05,
                        [0.15, 0.65],
                        [0.0, 0.0],
                    ),
                    pose(
                        [0.10, 0.18, 0.0],
                        [0.0, 0.28, 0.19],
                        -0.03,
                        [0.35, 2.4],
                        [0.0, -0.5],
                    ),
                    pose(
                        [-0.05, 0.04, 0.0],
                        [0.0, -0.12, -0.10],
                        0.01,
                        [0.3, 1.6],
                        [-0.25, 0.0],
                    ),
                ),
                // Recovery is responsive and warm, not a punitive fall or lengthy lockout
                6 => (
                    pose(
                        [-0.10, -0.055, 0.08],
                        [0.24, -0.18, -0.22],
                        0.16,
                        [0.8, 0.8],
                        [0.45, -0.2],
                    ),
                    pose(
                        [0.08, 0.16, 0.04],
                        [-0.12, 0.14, 0.13],
                        -0.08,
                        [1.0, 0.5],
                        [-0.3, 0.5],
                    ),
                    pose(
                        [0.0, 0.02, 0.0],
                        [0.0, 0.0, -0.04],
                        0.03,
                        [0.3, 0.5],
                        [0.1, 0.0],
                    ),
                ),
                _ => (
                    pose(
                        [0.0, -0.045, 0.0],
                        [0.10, 0.0, 0.08],
                        0.18,
                        [-0.3, -0.3],
                        [0.2, 0.2],
                    ),
                    pose(
                        [0.14, 0.40, 0.0],
                        [-0.12, -0.32, -0.22],
                        -0.17,
                        [0.8, 1.5],
                        [-0.55, 0.5],
                    ),
                    pose(
                        [-0.05, 0.10, 0.0],
                        [0.03, 0.18, 0.10],
                        0.08,
                        [0.7, 0.3],
                        [0.15, -0.2],
                    ),
                ),
            },
            Self::Duo(i) => match i % 6 {
                // Local +x is always toward the partner
                0 => (
                    pose(
                        [0.20, -0.03, 0.0],
                        [0.0, 0.25, -0.12],
                        0.11,
                        [0.4, 0.1],
                        [0.3, -0.3],
                    ),
                    pose(
                        [0.71, 0.20, 0.0],
                        [0.0, 0.22, 0.26],
                        -0.05,
                        [0.7, 0.25],
                        [-0.3, 0.4],
                    ),
                    pose(
                        [0.28, 0.28, 0.0],
                        [0.0, -0.12, -0.24],
                        0.04,
                        [1.1, 1.2],
                        [0.4, -0.3],
                    ),
                ),
                1 => (
                    pose(
                        [0.30, -0.04, 0.0],
                        [0.0, 0.50, -0.08],
                        0.13,
                        [0.1, 0.65],
                        [0.25, 0.0],
                    ),
                    pose(
                        [0.69, 0.40, 0.0],
                        [0.0, 0.62, 0.11],
                        -0.08,
                        [0.25, 2.2],
                        [-0.4, -0.4],
                    ),
                    pose(
                        [0.30, 0.20, 0.0],
                        [0.0, 0.20, -0.16],
                        0.04,
                        [0.6, 1.1],
                        [0.2, -0.3],
                    ),
                ),
                2 => (
                    pose(
                        [0.16, -0.035, 0.32],
                        [0.0, 0.45, -0.10],
                        0.15,
                        [0.1, 0.7],
                        [0.3, -0.2],
                    ),
                    pose(
                        [2.35, 0.74, 0.48],
                        [0.0, 1.1, 0.25],
                        -0.10,
                        [1.3, 1.0],
                        [-0.75, 0.5],
                    ),
                    pose(
                        [1.30, 0.38, -0.48],
                        [0.0, -0.75, -0.20],
                        -0.04,
                        [1.0, 1.4],
                        [0.5, -0.7],
                    ),
                ),
                3 => (
                    pose(
                        [0.15, -0.04, 0.0],
                        [0.0, -0.45, -0.13],
                        0.14,
                        [0.7, -0.2],
                        [0.3, -0.3],
                    ),
                    pose(
                        [0.48, 0.46, 0.0],
                        [0.0, PI, 0.22],
                        -0.07,
                        [1.45, 1.45],
                        [-0.4, 0.4],
                    ),
                    pose(
                        [0.22, 0.07, 0.0],
                        [0.0, TAU - 0.22, -0.10],
                        0.10,
                        [0.6, 0.6],
                        [0.2, -0.2],
                    ),
                ),
                4 => (
                    pose(
                        [0.25, -0.055, 0.0],
                        [0.10, 0.20, -0.08],
                        0.20,
                        [-0.2, 0.1],
                        [0.3, 0.3],
                    ),
                    pose(
                        [0.50, 0.94, 0.0],
                        [-0.10, 0.12, 0.08],
                        -0.13,
                        [1.65, 1.65],
                        [-0.6, -0.6],
                    ),
                    pose(
                        [0.20, -0.05, 0.0],
                        [0.10, 0.0, 0.03],
                        0.18,
                        [0.7, 0.8],
                        [0.2, 0.2],
                    ),
                ),
                _ => (
                    pose(
                        [0.22, -0.025, 0.0],
                        [0.0, 0.30, -0.10],
                        0.08,
                        [0.4, 0.65],
                        [0.2, -0.2],
                    ),
                    pose(
                        [0.62, 0.38, 0.0],
                        [0.0, 0.08, 0.12],
                        -0.05,
                        [2.2, 1.2],
                        [-0.4, 0.5],
                    ),
                    pose(
                        [0.48, 0.12, 0.0],
                        [0.0, -0.12, -0.16],
                        0.04,
                        [1.65, 1.1],
                        [0.0, -0.25],
                    ),
                ),
            },
        };
        keys[1] = anticipate;
        keys[2] = contact;
        keys[3] = follow;
        if matches!(self, Self::Solo(4) | Self::Duo(3)) {
            keys[4].turn.y = TAU;
        }
        keys
    }
}

fn pose(travel: [f32; 3], turn: [f32; 3], squash: f32, arms: [f32; 2], feet: [f32; 2]) -> Pose {
    Pose {
        travel: travel.into(),
        turn: turn.into(),
        squash,
        arms,
        feet,
    }
}

#[derive(Clone, Copy, Debug)]
struct MotionEvent {
    id: u64,
    seconds: f64,
    clip: Clip,
    strength: f32,
    from: Pose,
}

/// Latest event and its interrupted pose are enough to reconstruct any live frame
/// Replay seeking must reset then feed events in their original order, as the core already does
#[derive(Clone, Debug, Default)]
pub(crate) struct CharacterMotion {
    events: [Option<MotionEvent>; 2],
    last_solo: [Option<u8>; 2],
    last_duo: Option<u8>,
}

impl CharacterMotion {
    pub(crate) fn hit(&mut self, player: usize, id: u64, seconds: f64, strength: f32) {
        if player >= 2 || !seconds.is_finite() || !strength.is_finite() {
            return;
        }
        if self.events[player]
            .is_some_and(|event| event.id == id && matches!(event.clip, Clip::Solo(_)))
        {
            return;
        }
        let mut variant = (id.wrapping_mul(0x9e37_79b9).rotate_left(13) % 7) as u8;
        if variant >= 6 {
            variant = 7;
        }
        if self.last_solo[player] == Some(variant) {
            variant = (variant + 1) % 6;
        }
        self.last_solo[player] = Some(variant);
        self.start(player, id, seconds, strength, Clip::Solo(variant));
    }

    pub(crate) fn sync(&mut self, id: u64, seconds: f64, strength: f32) {
        if !seconds.is_finite() || !strength.is_finite() {
            return;
        }
        if self.events.iter().all(|event| {
            event.is_some_and(|event| event.id == id && matches!(event.clip, Clip::Duo(_)))
        }) {
            return;
        }
        let mut variant = (id.wrapping_mul(0x85eb_ca6b).rotate_left(7) % 6) as u8;
        if self.last_duo == Some(variant) {
            variant = (variant + 1) % 6;
        }
        self.last_duo = Some(variant);
        for player in 0..2 {
            self.start(player, id, seconds, strength, Clip::Duo(variant));
        }
    }

    pub(crate) fn miss(&mut self, player: usize, id: u64, seconds: f64) {
        if player < 2 && seconds.is_finite() {
            self.start(player, id, seconds, 0.65, Clip::Solo(6));
        }
    }

    fn start(&mut self, player: usize, id: u64, seconds: f64, strength: f32, clip: Clip) {
        let mut from = self.sample(player, seconds);
        from.turn.y = (from.turn.y + PI).rem_euclid(TAU) - PI;
        self.events[player] = Some(MotionEvent {
            id,
            seconds,
            clip,
            strength: strength.clamp(0.35, 1.0),
            from,
        });
    }

    fn sample(&self, player: usize, seconds: f64) -> Pose {
        let Some(event) = self.events[player] else {
            return Pose::default();
        };
        let age = seconds - event.seconds;
        if age < 0.0 || age >= event.clip.duration() {
            return Pose::default();
        }
        let phase = (age / event.clip.duration()) as f32;
        let keys = event.clip.keys();
        let times = [0.0, 0.15, 0.43, 0.72, 1.0];
        let index = times
            .windows(2)
            .position(|pair| phase < pair[1])
            .unwrap_or(3);
        let progress = (phase - times[index]) / (times[index + 1] - times[index]);
        let eased = progress * progress * (3.0 - 2.0 * progress);
        let target = keys[index].blend(keys[index + 1], eased);
        // Preserve full turns and crossing trajectories; strength scales only the body accent
        let mut target = target;
        target.travel.y *= event.strength;
        target.squash *= event.strength;
        let blend = (age as f32 / 0.09).clamp(0.0, 1.0);
        event
            .from
            .blend(target, blend * blend * (3.0 - 2.0 * blend))
    }
}

pub(crate) fn root_transform(state: &VisualState, player: usize) -> Transform {
    let direction = if player == 0 { 1.0 } else { -1.0 };
    let mut pose = if state.running || state.paused || state.transitioning {
        state.motion.sample(player, state.song_seconds)
    } else {
        Pose::default()
    };
    let amount = if state.quality.reduced_motion {
        0.18
    } else {
        1.0
    };
    let style = match state.presentation.motion_style() {
        crate::presentation::MotionStyle::Gentle => 0.75,
        crate::presentation::MotionStyle::Playful => 1.15,
        crate::presentation::MotionStyle::Energetic => 1.0,
    };
    pose.travel *= amount;
    pose.travel.y *= style;
    pose.turn *= amount;
    pose.turn.z *= style;
    pose.squash *= amount * style;
    let breathe = if state.running || state.paused || state.transitioning {
        (state.song_seconds as f32 * PI * 2.0).sin() * 0.015 * amount
    } else {
        0.0
    };
    Transform {
        translation: Vec3::new(
            -direction * 1.35 + pose.travel.x * direction,
            0.735 + pose.travel.y + breathe,
            pose.travel.z * direction,
        ),
        rotation: Quat::from_euler(
            EulerRot::YXZ,
            pose.turn.y * direction,
            pose.turn.x,
            pose.turn.z * direction,
        ),
        scale: Vec3::new(
            1.0 + pose.squash * 0.55,
            1.0 - pose.squash,
            1.0 + pose.squash * 0.35,
        ),
    }
}

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
        let motion = if state.running {
            state.motion.sample(player, seconds)
        } else {
            Pose::default()
        };
        let amount = if state.quality.reduced_motion {
            0.18
        } else {
            1.0
        };
        let gesture = amount
            * match state.presentation.motion_style() {
                crate::presentation::MotionStyle::Gentle => 0.8,
                crate::presentation::MotionStyle::Playful => 1.1,
                crate::presentation::MotionStyle::Energetic => 1.0,
            };
        let direction = if player == 0 { 1.0 } else { -1.0 };
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
                transform.rotation *= Quat::from_rotation_z(
                    side * (0.045 * sway + hit * 0.24 + sync * 0.12)
                        + (motion.turn.z * direction - motion.travel.y * side * 0.45) * amount,
                );
                transform.rotation *= Quat::from_rotation_x(motion.squash * 1.2 * amount);
            }
            Joint::Crest => {
                transform.rotation *= Quat::from_rotation_z(
                    0.045 * sway + hit * 0.18 + motion.turn.z * direction * 0.75 * amount,
                );
                transform.rotation *= Quat::from_rotation_x(-motion.travel.y * 0.45 * amount);
            }
            Joint::Hand(side) => {
                let index = usize::from(side == direction);
                transform.rotation *=
                    Quat::from_rotation_z(-side * (hit * 0.3 + motion.arms[index] * gesture));
                transform.rotation *=
                    Quat::from_rotation_x(step * 0.06 + motion.travel.y * 0.4 * amount);
                transform.translation.y +=
                    (motion.arms[index] / 2.4).clamp(0.0, 1.0) * 0.46 * gesture;
            }
            Joint::Foot(side) => {
                let index = usize::from(side == direction);
                transform.rotation *=
                    Quat::from_rotation_x(side * step * 0.075 + motion.feet[index] * gesture);
                transform.translation.y += motion.feet[index].abs() * 0.10 * amount;
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
    fn authored_clips_are_distinct_bounded_and_return_home() {
        let clips = (0..8).map(Clip::Solo).chain((0..6).map(Clip::Duo));
        let mut silhouettes = Vec::new();
        for clip in clips {
            let mut motion = CharacterMotion::default();
            motion.start(0, 1, 2.0, 1.0, clip);
            let contact = motion.sample(0, 2.0 + clip.duration() * 0.43);
            assert!(!silhouettes.contains(&contact), "duplicate clip: {clip:?}");
            silhouettes.push(contact);
            assert_eq!(motion.sample(0, 1.99), Pose::default());
            assert_eq!(
                motion.sample(0, 2.0 + clip.duration() + 1e-6),
                Pose::default()
            );
            for step in 0..100 {
                let pose = motion.sample(0, 2.0 + clip.duration() * f64::from(step) / 100.0);
                assert!(pose.travel.is_finite());
                assert!(pose.travel.x.abs() <= 2.36);
                assert!((-0.06..=0.95).contains(&pose.travel.y));
                assert!(pose.travel.z.abs() <= 0.49);
                assert!(pose.squash.abs() <= 0.21);
            }
        }
    }

    #[test]
    fn rapid_interrupt_replay_and_nonrepeating_selection_use_event_clock() {
        let mut motion = CharacterMotion::default();
        motion.sync(14, 1.0, 1.0);
        let before = motion.sample(0, 1.4);
        motion.hit(0, 25, 1.4, 1.0);
        let interrupted = motion.sample(0, 1.4);
        assert_eq!(before.travel, interrupted.travel);
        assert_eq!(before.arms, interrupted.arms);
        assert_eq!(before.squash, interrupted.squash);
        let first = motion.sample(0, 1.44);
        let mut replay = CharacterMotion::default();
        replay.sync(14, 1.0, 1.0);
        replay.hit(0, 25, 1.4, 1.0);
        assert_eq!(first, replay.sample(0, 1.44));
        assert_ne!(first, motion.sample(0, 1.50));
        let mut previous = None;
        for id in 1..80 {
            motion.hit(0, id, id as f64, 0.8);
            let clip = motion.events[0].unwrap().clip;
            assert_ne!(Some(clip), previous);
            previous = Some(clip);
        }
        let mut previous = None;
        for id in 1..80 {
            motion.sync(id, id as f64, 1.0);
            let clip = motion.events[0].unwrap().clip;
            assert_ne!(Some(clip), previous);
            previous = Some(clip);
        }
    }

    #[test]
    fn crossing_keeps_depth_separation_and_accessible_motion_preserves_identity() {
        let mut state = VisualState {
            running: true,
            song_seconds: 0.688,
            ..default()
        };
        for player in 0..2 {
            state.motion.start(player, 1, 0.0, 1.0, Clip::Duo(2));
        }
        let a = root_transform(&state, 0);
        let b = root_transform(&state, 1);
        assert!(a.translation.x > 0.9 && b.translation.x < -0.9);
        assert!((a.translation.z - b.translation.z).abs() > 0.9);
        state.quality.reduced_motion = true;
        let reduced = root_transform(&state, 0);
        assert!((reduced.translation.x + 1.35).abs() < 0.45);
        assert!(reduced.translation.y < a.translation.y);
        state.running = false;
        state.paused = true;
        assert_eq!(root_transform(&state, 0).rotation, reduced.rotation);
    }

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
