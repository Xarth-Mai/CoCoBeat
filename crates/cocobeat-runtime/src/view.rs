use std::f32::consts::PI;

use bevy::{
    camera::Hdr, core_pipeline::tonemapping::Tonemapping, light::NotShadowCaster,
    post_process::bloom::Bloom, prelude::*, render::view::Msaa, text::FontSource,
    transform::TransformSystems, ui::UiSystems,
};

use crate::{
    brand_intro::{BrandIntroLayout, BrandIntroPhase, BrandIntroStatus, BrandIntroSystems},
    display::GameCamera,
    i18n::{Locale, Message},
    input::SettingsAction,
    settings::{AntiAliasing, QualitySettings, RainAmount},
    settings_menu::SettingsPresentation,
    ui_assets::UiAssets,
};

#[derive(Resource, Default)]
pub(crate) struct VisualState {
    pub song_seconds: f64,
    pub hit_pulses: [f32; 2],
    pub sync_pulse: f32,
    pub resonance: f32,
    pub status: String,
    pub running: bool,
    pub locale: Locale,
    pub settings: Option<SettingsPresentation>,
    pub quality: QualitySettings,
}

#[derive(Component)]
enum Motion {
    Spirit(usize),
    Ripple(usize),
    SharedRing,
    Street(f32),
    Rain(usize),
}

#[derive(Component)]
enum UiText {
    Status,
    Subtitle,
    Clock,
    Player(usize),
    RowPrefix(usize),
    RowName(usize),
    RowSuffix(usize),
}

#[derive(Component)]
pub(crate) struct StatusPanel;

#[derive(Component)]
struct Subtitle;

#[derive(Component)]
enum HudNode {
    Progress,
    Rows,
    Row(usize),
    Flag(usize),
}

#[derive(Component)]
struct SettingsRows;

#[derive(Component)]
pub(crate) struct SettingsRowNode(pub usize);

#[derive(Component)]
struct PlayerLabel;

#[derive(Component)]
struct LanguageFlag(usize);

#[derive(Resource, Default)]
pub(crate) struct SettingsScroll {
    can_up: bool,
    can_down: bool,
    request: i8,
    recenter: bool,
}

impl SettingsScroll {
    pub fn handle(&mut self, action: SettingsAction) -> bool {
        self.request = match action {
            SettingsAction::Up if self.can_up => -1,
            SettingsAction::Down if self.can_down => 1,
            _ => return false,
        };
        true
    }

    pub fn reset(&mut self) {
        *self = Self {
            recenter: true,
            ..default()
        };
    }
}

pub fn install(app: &mut App) {
    app.init_resource::<VisualState>()
        .init_resource::<SettingsScroll>()
        .insert_resource(ClearColor(Color::srgb(0.012, 0.017, 0.042)))
        .add_systems(Startup, (setup, setup_hud))
        .add_systems(
            Update,
            update_brand_layout.before(BrandIntroSystems::Advance),
        )
        .add_systems(
            PostUpdate,
            (
                (apply_quality, animate).chain(),
                (ensure_settings_rows, update_hud, layout_hud).chain(),
            )
                .before(TransformSystems::Propagate)
                .before(UiSystems::Prepare),
        )
        .add_systems(PostUpdate, scroll_settings.after(UiSystems::Layout));
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
        Hdr,
        bevy::camera::ShadowLodOrigin,
        Tonemapping::Reinhard,
        Transform::from_xyz(0.0, 6.0, 12.5).looking_at(Vec3::new(0.0, 0.6, -6.0), Vec3::Y),
        AmbientLight {
            color: Color::srgb(0.54, 0.62, 0.85),
            brightness: 180.0,
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
                Vec3::new((phase * 3.7).sin() * 6.0, 0.0, -(((drop * 7) % 31) as f32)),
                Vec3::new(0.012, 0.24, 0.012),
            ),
            Motion::Rain(drop),
            NotShadowCaster,
        ));
    }
}

fn apply_quality(
    mut commands: Commands,
    state: Res<VisualState>,
    mut applied: Local<Option<QualitySettings>>,
    cameras: Query<Entity, With<GameCamera>>,
    mut lights: Query<&mut PointLight>,
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
                color: Color::srgb(0.012, 0.017, 0.042),
                falloff: FogFalloff::Linear {
                    start: 12.0,
                    end: 48.0,
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

fn setup_hud(mut commands: Commands, state: Res<VisualState>, assets: Res<UiAssets>) {
    let font =
        |font_size: f32| TextFont::from_font_size(font_size).with_font(assets.font(state.locale));
    let colors = [Color::srgb(0.12, 0.92, 0.9), Color::srgb(0.96, 0.28, 0.67)];
    commands.spawn((
        Text::default(),
        UiText::Subtitle,
        font(12.0),
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
        UiText::Clock,
        font(16.0),
        TextColor(Color::srgb(0.83, 0.87, 0.93)),
        TextLayout::justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            top: px(34),
            right: px(36),
            ..default()
        },
    ));
    for (player, color) in colors.into_iter().enumerate() {
        commands.spawn((
            Text::default(),
            UiText::Player(player),
            PlayerLabel,
            font(13.0),
            TextColor(color),
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
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
            BackgroundColor(Color::srgba(0.015, 0.022, 0.045, 0.94)),
            GlobalZIndex(0),
            StatusPanel,
        ))
        .with_children(|panel| {
            let color = TextColor(Color::srgb(0.87, 0.91, 0.96));
            panel.spawn((
                Text::default(),
                font(14.0),
                color,
                UiText::Status,
                Node {
                    flex_shrink: 0.0,
                    min_width: px(0),
                    ..default()
                },
            ));
            panel.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    flex_shrink: 0.0,
                    ..default()
                },
                SettingsRows,
                HudNode::Rows,
            ));
        });
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
            HudNode::Progress,
        ));
}

fn ensure_settings_rows(
    mut commands: Commands,
    state: Res<VisualState>,
    assets: Res<UiAssets>,
    parent: Query<Entity, With<SettingsRows>>,
    existing: Query<&SettingsRowNode>,
) {
    let Some(settings) = &state.settings else {
        return;
    };
    let Ok(parent) = parent.single() else {
        return;
    };
    let count = existing.iter().count();
    for index in count..settings.rows.len() {
        commands.entity(parent).with_children(|parent| {
            parent
                .spawn((
                    Node {
                        align_items: AlignItems::Start,
                        column_gap: px(6),
                        flex_shrink: 0.0,
                        min_width: px(0),
                        ..default()
                    },
                    SettingsRowNode(index),
                    HudNode::Row(index),
                ))
                .with_children(|row| {
                    row.spawn((
                        ImageNode::new(assets.flag(state.locale)),
                        Node {
                            width: px(24),
                            height: px(18),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        LanguageFlag(index),
                        HudNode::Flag(index),
                    ));
                    let font = TextFont::from_font_size(14.0).with_font(assets.font(state.locale));
                    let color = TextColor(Color::srgb(0.87, 0.91, 0.96));
                    row.spawn((
                        Text::default(),
                        font.clone(),
                        color,
                        UiText::RowPrefix(index),
                        Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            ..default()
                        },
                    ))
                    .with_children(|text| {
                        text.spawn((
                            TextSpan::default(),
                            font.clone(),
                            color,
                            UiText::RowName(index),
                        ));
                        text.spawn((TextSpan::default(), font, color, UiText::RowSuffix(index)));
                    });
                });
        });
    }
}

fn layout_hud(
    state: Res<VisualState>,
    cameras: Query<&Camera, With<IsDefaultUiCamera>>,
    mut panels: Query<&mut Node, With<StatusPanel>>,
    mut auxiliary: Query<(&UiText, &mut Node), Without<StatusPanel>>,
) {
    let Some(viewport) = cameras
        .single()
        .ok()
        .and_then(Camera::logical_viewport_size)
    else {
        return;
    };
    let margin = (viewport.min_element() * 0.035).clamp(4.0, 28.0);
    let compact = viewport.x < 900.0 || viewport.y < 600.0;
    for mut panel in &mut panels {
        let mut node = panel.clone();
        node.left = px(margin);
        node.right = px(margin);
        node.bottom = px(margin);
        node.padding = UiRect::all(px((viewport.x * 0.04).clamp(6.0, 16.0)));
        node.min_height = px(if state.settings.is_some() { 0.0 } else { 104.0 });
        node.max_height = if state.settings.is_some() {
            let (origin, width) = dock_geometry(viewport);
            let top = origin.y + width * 180.0 / 840.0 + if compact { 8.0 } else { 28.0 };
            px((viewport.y - margin - top).max(1.0))
        } else {
            Val::Auto
        };
        node.overflow = if state.settings.is_some() {
            Overflow::scroll_y()
        } else {
            Overflow::DEFAULT
        };
        if *panel != node {
            *panel = node;
        }
    }
    for (kind, mut node) in &mut auxiliary {
        if matches!(kind, UiText::Subtitle | UiText::Clock) {
            let display = if state.settings.is_some() && compact {
                Display::None
            } else {
                Display::Flex
            };
            if node.display != display {
                node.display = display;
            }
        }
    }
}

fn dock_geometry(viewport: Vec2) -> (Vec2, f32) {
    let origin = if viewport.x < 480.0 || viewport.y < 320.0 {
        Vec2::new(12.0, 8.0)
    } else {
        Vec2::new(36.0, 24.0)
    };
    (origin, (viewport.x * 0.32).min(240.0))
}

type SettingsLayout = (usize, usize, Vec2, f32, f32);

fn scroll_settings(
    state: Res<VisualState>,
    mut control: ResMut<SettingsScroll>,
    mut previous: Local<Option<SettingsLayout>>,
    mut panels: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<StatusPanel>>,
    rows: Query<(&SettingsRowNode, &ComputedNode, &UiGlobalTransform)>,
) {
    let Some(settings) = &state.settings else {
        control.reset();
        *previous = None;
        for (_, _, mut scroll) in &mut panels {
            scroll.0 = Vec2::ZERO;
        }
        return;
    };
    let Some(index) = settings.rows.iter().position(|row| row.selected) else {
        control.reset();
        return;
    };
    let Ok((panel, panel_transform, mut scroll)) = panels.single_mut() else {
        return;
    };
    let Some((_, row, transform)) = rows.iter().find(|(row, _, _)| row.0 == index) else {
        return;
    };
    if panel.size.y <= 0.0 || row.size.y <= 0.0 {
        control.reset();
        return;
    }
    let key = (
        settings.rows.len(),
        index,
        panel.size,
        panel.inverse_scale_factor,
        row.size.y,
    );
    let changed = control.recenter || previous.as_ref() != Some(&key);
    *previous = Some(key);
    if changed {
        control.reset();
    }
    let top = panel_transform.translation.y - panel.size.y * 0.5
        + panel.padding.min_inset.y
        + panel.border.min_inset.y;
    let bottom = panel_transform.translation.y + panel.size.y * 0.5
        - panel.padding.max_inset.y
        - panel.border.max_inset.y;
    let row_top = transform.translation.y - row.size.y * 0.5;
    let row_bottom = transform.translation.y + row.size.y * 0.5;
    let height = (bottom - top).max(1.0);
    let oversized = row.size.y > height;
    let delta = if oversized {
        if changed || row_top > top || row_bottom < bottom {
            row_top - top
        } else if control.request < 0 {
            (row_top - top).max(-height * 0.8)
        } else if control.request > 0 {
            (row_bottom - bottom).min(height * 0.8)
        } else {
            0.0
        }
    } else if row_top < top {
        row_top - top
    } else if row_bottom > bottom {
        row_bottom - bottom
    } else {
        0.0
    };
    let limit = (panel.content_size.y - panel.size.y).max(0.0) * panel.inverse_scale_factor;
    let target = panel.scroll_position.y + delta;
    let target = if delta > 0.0 {
        target.ceil()
    } else {
        target.floor()
    };
    let target = target.max(0.0) * panel.inverse_scale_factor;
    scroll.y = target.clamp(0.0, limit);
    let moved = scroll.y / panel.inverse_scale_factor - panel.scroll_position.y;
    control.can_up = oversized && row_top - moved < top - 1.0;
    control.can_down = oversized && row_bottom - moved > bottom + 1.0;
    control.request = 0;
    control.recenter = false;
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
    let (origin, width) = dock_geometry(viewport);
    let dock_rect = Rect::from_corners(origin, origin + Vec2::new(width, width * 180.0 / 840.0));
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
            Motion::Rain(index) => {
                let phase = index as f32 * 0.73;
                transform.translation.y = 8.0 - (time.elapsed_secs() * 6.0 + phase).rem_euclid(8.0);
                transform.rotation = Quat::from_rotation_z(-0.15);
            }
        }
    }
}

fn update_hud(
    (state, assets, intro): (
        Res<VisualState>,
        Res<UiAssets>,
        Option<Res<BrandIntroStatus>>,
    ),
    mut texts: Query<(&UiText, &mut Text, &mut TextFont), Without<TextSpan>>,
    mut spans: Query<(&UiText, &mut TextSpan, &mut TextFont), Without<Text>>,
    mut flags: Query<(&LanguageFlag, &mut ImageNode)>,
    mut nodes: Query<(&HudNode, &mut Node)>,
    mut panel: Query<&mut GlobalZIndex, With<StatusPanel>>,
    mut labels: Query<&mut Visibility, With<PlayerLabel>>,
) {
    if !state.is_changed()
        && !assets.is_changed()
        && !intro.as_ref().is_some_and(|intro| intro.is_changed())
    {
        return;
    }
    // The brand backdrop reveals the scene and HUD; only startup errors render above it
    let failed = intro.is_some_and(|intro| intro.phase == BrandIntroPhase::Failed);
    for mut visible in &mut labels {
        *visible = if state.settings.is_some() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    for mut layer in &mut panel {
        layer.0 = if failed { 1001 } else { 0 };
    }
    let locale = state.locale;
    let rows = state
        .settings
        .as_ref()
        .map(|settings| settings.rows.as_slice())
        .unwrap_or_default();
    for (kind, mut text, mut font) in &mut texts {
        let value = match *kind {
            UiText::Status => state
                .settings
                .as_ref()
                .map(|settings| settings.title.clone())
                .unwrap_or_else(|| state.status.clone()),
            UiText::Subtitle => locale.text("hud.subtitle").into(),
            UiText::Clock => Message::with(
                "hud.clock",
                [
                    (
                        "elapsed",
                        format!("{:05.1}", state.song_seconds.clamp(0.0, 64.0)),
                    ),
                    ("duration", "64.0".into()),
                    (
                        "resonance",
                        format!("{:3.0}", state.resonance.clamp(0.0, 1.0) * 100.0),
                    ),
                    (
                        "state",
                        locale
                            .text(if state.running {
                                "hud.playing"
                            } else {
                                "hud.resting"
                            })
                            .into(),
                    ),
                ],
            )
            .render(locale),
            UiText::Player(player) => locale
                .text(if player == 0 {
                    "hud.player_one"
                } else {
                    "hud.player_two"
                })
                .into(),
            UiText::RowPrefix(index) => rows
                .get(index)
                .map(|row| {
                    let prefix = if row.language.is_some() {
                        row.text
                            .split_once("{language}")
                            .expect("native language placeholder")
                            .0
                    } else {
                        &row.text
                    };
                    format!("{} {prefix}", if row.selected { ">" } else { " " })
                })
                .unwrap_or_default(),
            UiText::RowName(_) | UiText::RowSuffix(_) => unreachable!(),
        };
        if text.0 != value {
            text.0 = value;
        }
        let source = FontSource::Handle(assets.font(locale));
        if font.font != source {
            font.font = source;
        }
    }
    for (kind, mut text, mut font) in &mut spans {
        let (value, text_locale) = match *kind {
            UiText::RowName(index) => {
                let language = rows.get(index).and_then(|row| row.language);
                (
                    language
                        .map(|language| language.native_name().to_owned())
                        .unwrap_or_default(),
                    language.unwrap_or(locale),
                )
            }
            UiText::RowSuffix(index) => (
                rows.get(index)
                    .filter(|row| row.language.is_some())
                    .and_then(|row| row.text.split_once("{language}"))
                    .map(|(_, suffix)| suffix.to_owned())
                    .unwrap_or_default(),
                locale,
            ),
            _ => unreachable!(),
        };
        if text.0 != value {
            text.0 = value;
        }
        let source = FontSource::Handle(assets.font(text_locale));
        if font.font != source {
            font.font = source;
        }
    }
    for (flag, mut image) in &mut flags {
        if let Some(language) = rows.get(flag.0).and_then(|row| row.language) {
            let flag = assets.flag(language);
            if image.image != flag {
                image.image = flag;
            }
        }
    }
    for (kind, mut node) in &mut nodes {
        let visible = match *kind {
            HudNode::Progress => {
                node.width = percent((state.song_seconds / 64.0).clamp(0.0, 1.0) as f32 * 100.0);
                continue;
            }
            HudNode::Rows => state.settings.is_some(),
            HudNode::Row(index) => rows.get(index).is_some(),
            HudNode::Flag(index) => rows.get(index).is_some_and(|row| row.language.is_some()),
        };
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_changes_apply_to_the_scene_and_preserve_core_feedback() {
        use crate::{display::PresentationCamera, settings::QualityPreset};

        let mut app = App::new();
        // Match VisibilityPlugin's Mesh3d requirement without installing the render app
        app.register_required_components::<Mesh3d, Visibility>()
            .insert_resource(VisualState {
                song_seconds: 12.5,
                hit_pulses: [0.8, 0.5],
                sync_pulse: 0.7,
                ..default()
            })
            .init_resource::<Time>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, (apply_quality, animate).chain());
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
        assert_eq!(core.len(), 29);
        assert!(core.iter().all(|(_, _, v)| *v != Visibility::Hidden));
        let meshes = app.world().resource::<Assets<Mesh>>().len();
        let materials = app.world().resource::<Assets<StandardMaterial>>().len();
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
            let mut lights = app.world_mut().query::<&PointLight>();
            assert_eq!(lights.iter(app.world()).count(), 2);
            assert!(
                lights
                    .iter(app.world())
                    .all(|light| light.shadow_maps_enabled == effects)
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

    fn hud_app() -> App {
        let mut app = App::new();
        app.init_resource::<VisualState>()
            .init_resource::<Assets<Font>>()
            .init_resource::<Assets<Image>>()
            .add_systems(Startup, setup_hud)
            .add_systems(PostUpdate, (ensure_settings_rows, update_hud).chain());
        crate::ui_assets::install(&mut app).unwrap();
        app
    }

    #[test]
    fn measured_rows_scroll_into_small_and_scaled_viewports() {
        use crate::settings_menu::SettingsRow;
        use bevy::{
            app::{HierarchyPropagatePlugin, PropagateSet},
            camera::{ComputedCameraValues, RenderTargetInfo},
            text::FontCx,
            ui::{ui_layout_system, ui_surface::UiSurface, update::propagate_ui_target_cameras},
        };
        for (size, scale) in [
            (UVec2::new(180, 120), 1.0),
            (UVec2::new(400, 300), 1.0),
            (UVec2::new(1280, 800), 2.0),
        ] {
            let mut app = App::new();
            app.add_plugins((
                TaskPoolPlugin::default(),
                HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
                HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
            ))
            .init_resource::<UiScale>()
            .init_resource::<UiSurface>()
            .init_resource::<FontCx>()
            .init_resource::<SettingsScroll>()
            .insert_resource(VisualState {
                settings: Some(SettingsPresentation {
                    title: "Settings".into(),
                    rows: (0..5).map(|_| SettingsRow::default()).collect(),
                }),
                ..default()
            })
            .add_systems(
                PostUpdate,
                (
                    layout_hud,
                    propagate_ui_target_cameras,
                    ui_layout_system,
                    scroll_settings,
                )
                    .chain(),
            )
            .configure_sets(
                PostUpdate,
                (
                    PropagateSet::<ComputedUiTargetCamera>::default(),
                    PropagateSet::<ComputedUiRenderTargetInfo>::default(),
                )
                    .after(propagate_ui_target_cameras)
                    .before(ui_layout_system),
            );
            let camera = app
                .world_mut()
                .spawn((
                    Camera2d,
                    IsDefaultUiCamera,
                    Camera {
                        computed: ComputedCameraValues {
                            target_info: Some(RenderTargetInfo {
                                physical_size: size,
                                scale_factor: scale,
                            }),
                            ..default()
                        },
                        ..default()
                    },
                ))
                .id();
            let panel = app
                .world_mut()
                .spawn((
                    StatusPanel,
                    Node {
                        position_type: PositionType::Absolute,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        ..default()
                    },
                ))
                .id();
            // Exercise the real layout system with differently sized rows; font wrapping is checked by GPU captures
            let rows: Vec<_> = [20, 240, 40, 120, 30]
                .into_iter()
                .enumerate()
                .map(|(index, height)| {
                    app.world_mut()
                        .spawn((
                            Node {
                                height: px(height),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            SettingsRowNode(index),
                            ChildOf(panel),
                        ))
                        .id()
                })
                .collect();
            let bounds = |world: &World, row: Entity| {
                let panel_node = world.get::<ComputedNode>(panel).unwrap();
                let panel_position = world.get::<UiGlobalTransform>(panel).unwrap().translation.y;
                let row_node = world.get::<ComputedNode>(row).unwrap();
                let row_position = world.get::<UiGlobalTransform>(row).unwrap().translation.y;
                (
                    panel_position - panel_node.size.y * 0.5 + panel_node.padding.min_inset.y,
                    panel_position + panel_node.size.y * 0.5 - panel_node.padding.max_inset.y,
                    row_position - row_node.size.y * 0.5,
                    row_position + row_node.size.y * 0.5,
                )
            };
            for index in [0, 4, 2, 1] {
                for (i, row) in app
                    .world_mut()
                    .resource_mut::<VisualState>()
                    .settings
                    .as_mut()
                    .unwrap()
                    .rows
                    .iter_mut()
                    .enumerate()
                {
                    row.selected = i == index;
                }
                for _ in 0..3 {
                    app.update();
                }
                let panel_size = app.world().get::<ComputedNode>(panel).unwrap().size;
                let panel_center = app
                    .world()
                    .get::<UiGlobalTransform>(panel)
                    .unwrap()
                    .translation;
                assert!((panel_center - panel_size * 0.5).cmpge(Vec2::ZERO).all());
                assert!(
                    (panel_center + panel_size * 0.5)
                        .cmple(size.as_vec2())
                        .all()
                );
                let (top, bottom, row_top, row_bottom) = bounds(app.world(), rows[index]);
                assert!(
                    row_top >= top - 1.0,
                    "size={size}, scale={scale}, index={index}"
                );
                if row_bottom - row_top <= bottom - top {
                    assert!(row_bottom <= bottom + 1.0);
                } else {
                    assert!(
                        !app.world_mut()
                            .resource_mut::<SettingsScroll>()
                            .handle(SettingsAction::Confirm)
                    );
                    assert!(
                        !app.world_mut()
                            .resource_mut::<SettingsScroll>()
                            .handle(SettingsAction::Back)
                    );
                    for action in [SettingsAction::Down, SettingsAction::Up] {
                        let mut pages = 0;
                        while app
                            .world_mut()
                            .resource_mut::<SettingsScroll>()
                            .handle(action)
                        {
                            pages += 1;
                            assert!(pages < 20);
                            app.update();
                            app.update();
                        }
                        let (top, bottom, row_top, row_bottom) = bounds(app.world(), rows[index]);
                        if matches!(action, SettingsAction::Down) {
                            assert!(row_bottom <= bottom + 1.0);
                        } else {
                            assert!(row_top >= top - 1.0);
                        }
                    }
                }
            }
            app.world_mut()
                .get_mut::<Camera>(camera)
                .unwrap()
                .computed
                .target_info = Some(RenderTargetInfo {
                physical_size: UVec2::new(320, 240),
                scale_factor: 2.0,
            });
            for _ in 0..3 {
                app.update();
            }
            let (top, _, row_top, _) = bounds(app.world(), rows[1]);
            assert!(row_top >= top - 1.0);
            app.world_mut().resource_mut::<VisualState>().settings = None;
            app.update();
            assert_eq!(
                app.world().get::<ScrollPosition>(panel).unwrap().0,
                Vec2::ZERO
            );
            assert!(
                !app.world_mut()
                    .resource_mut::<SettingsScroll>()
                    .handle(SettingsAction::Down)
            );
        }
    }

    #[test]
    fn failed_intro_keeps_the_existing_status_above_its_backdrop() {
        let mut app = hud_app();
        app.insert_resource(VisualState {
            status: "Startup failed: audio unavailable\nClose window to exit".into(),
            ..default()
        });
        app.update();
        let panel = app
            .world_mut()
            .query_filtered::<Entity, With<StatusPanel>>()
            .single(app.world())
            .unwrap();
        assert_eq!(app.world().get::<GlobalZIndex>(panel).unwrap().0, 0);
        app.insert_resource(BrandIntroStatus {
            phase: BrandIntroPhase::Failed,
            ..default()
        });
        app.update();
        assert_eq!(app.world().get::<GlobalZIndex>(panel).unwrap().0, 1001);
        let mut texts = app.world_mut().query::<(&UiText, &Text)>();
        let (_, status) = texts
            .iter(app.world())
            .find(|(kind, _)| matches!(kind, UiText::Status))
            .unwrap();
        assert_eq!(status.0, app.world().resource::<VisualState>().status);
    }

    #[test]
    fn locale_switches_update_hud_fonts_and_native_language_rows() {
        use crate::settings_menu::SettingsRow;
        let mut app = hud_app();
        for locale in Locale::ALL {
            {
                let mut state = app.world_mut().resource_mut::<VisualState>();
                state.locale = locale;
                state.settings = Some(SettingsPresentation {
                    title: locale.text("settings.language_title").into(),
                    rows: Locale::ALL
                        .into_iter()
                        .map(|language| SettingsRow {
                            text: "{language}".into(),
                            language: Some(language),
                            selected: language == locale,
                        })
                        .collect(),
                });
            }
            app.update();
            let assets = app.world().resource::<UiAssets>();
            let ui_font = FontSource::Handle(assets.font(locale));
            for entity in app.world().iter_entities() {
                let Some(kind) = entity.get::<UiText>() else {
                    continue;
                };
                let font = entity.get::<TextFont>().unwrap();
                match *kind {
                    UiText::RowName(index) => {
                        let language = Locale::ALL[index];
                        assert_eq!(font.font, FontSource::Handle(assets.font(language)));
                        assert_eq!(entity.get::<TextSpan>().unwrap().0, language.native_name());
                    }
                    UiText::RowSuffix(_) => {
                        assert_eq!(font.font, ui_font);
                        assert!(entity.get::<TextSpan>().unwrap().0.is_empty());
                    }
                    UiText::RowPrefix(index) => {
                        assert_eq!(font.font, ui_font);
                        assert_eq!(
                            entity.get::<Text>().unwrap().0.starts_with(">"),
                            Locale::ALL[index] == locale
                        );
                    }
                    _ => assert_eq!(font.font, ui_font),
                }
            }
            let mut flags = app.world_mut().query::<(&LanguageFlag, &ImageNode)>();
            assert_eq!(flags.iter(app.world()).count(), Locale::ALL.len());
            for (flag, image) in flags.iter(app.world()) {
                assert_eq!(
                    image.image,
                    app.world().resource::<UiAssets>().flag(Locale::ALL[flag.0])
                );
            }
        }
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.locale = Locale::EnUs;
            state.settings = Some(SettingsPresentation {
                title: "Settings".into(),
                rows: vec![SettingsRow {
                    text: Locale::EnUs.text("settings.language").into(),
                    language: Some(Locale::Ko),
                    selected: true,
                }],
            });
        }
        app.update();
        let mut texts = app.world_mut().query::<(&UiText, &Text)>();
        assert!(
            texts.iter(app.world()).any(
                |(kind, text)| matches!(kind, UiText::RowPrefix(0)) && text.0 == "> Language: "
            )
        );
        let mut spans = app.world_mut().query::<(&UiText, &TextSpan, &TextFont)>();
        assert!(spans.iter(app.world()).any(|(kind, text, font)| matches!(
            kind,
            UiText::RowName(0)
        ) && text.0
            == Locale::Ko.native_name()
            && font.font
                == FontSource::Handle(app.world().resource::<UiAssets>().font(Locale::Ko))));
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.settings = None;
        }
        app.update();
        let mut nodes = app.world_mut().query::<(&HudNode, &Node)>();
        assert!(nodes.iter(app.world()).all(|(kind, node)| {
            matches!(kind, HudNode::Progress) || node.display == Display::None
        }));
        assert!(
            app.world_mut()
                .query_filtered::<&Visibility, With<PlayerLabel>>()
                .iter(app.world())
                .all(|visibility| *visibility == Visibility::Inherited)
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
