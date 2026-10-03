use bevy::{
    prelude::*,
    text::{FontSize, FontSource},
    transform::TransformSystems,
    ui::UiSystems,
};

use crate::{
    brand_intro::{BrandIntroLayout, BrandIntroPhase, BrandIntroStatus, BrandIntroSystems},
    i18n::{Locale, Message},
    input::{MenuKind, MenuPresentation, MenuRow, MenuRowRole, MenuScroll},
    scene,
    settings::QualitySettings,
    ui_assets::UiAssets,
};

#[derive(Resource, Default)]
pub(crate) struct VisualState {
    pub song_seconds: f64,
    pub duration_seconds: f64,
    pub hit_pulses: [f32; 2],
    pub free_sync_pulse: f32,
    pub anchor_sync_pulse: f32,
    pub anchor_sync_precise: bool,
    pub miss_pulses: [f32; 2],
    pub next_anchor_seconds: Option<f64>,
    pub section_hint: Option<String>,
    pub next_section_seconds: Option<f64>,
    pub resonance: f32,
    pub status: String,
    pub running: bool,
    pub transitioning: bool,
    pub locale: Locale,
    pub menu: Option<MenuPresentation>,
    pub quality: QualitySettings,
}

#[derive(Component)]
enum UiText {
    Status,
    OwnerHint,
    Subtitle,
    Clock,
    Player(usize),
    Device(usize),
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
    Waiting,
    Rows,
    Row(usize),
    Flag(usize),
    Players,
    OwnerHint,
}

#[derive(Component)]
struct MenuRows;

#[derive(Component)]
pub(crate) struct MenuRowNode(pub usize);

#[derive(Component)]
struct PlayerLabel;

#[derive(Component)]
struct PlayerCards;

#[derive(Component)]
struct LanguageFlag(usize);

#[derive(Default)]
struct FocusFeedback {
    selected: Option<(MenuKind, usize)>,
    remaining: f32,
}

pub fn install(app: &mut App) {
    app.init_resource::<VisualState>()
        .init_resource::<MenuScroll>()
        .insert_resource(ClearColor(Color::srgb(0.012, 0.017, 0.042)))
        .add_systems(Startup, (scene::setup, setup_hud))
        .add_systems(
            Update,
            update_brand_layout.before(BrandIntroSystems::Advance),
        )
        .add_systems(
            PostUpdate,
            (
                (scene::apply_quality, scene::animate, scene::update_signs).chain(),
                (ensure_menu_rows, update_hud, layout_hud).chain(),
            )
                .before(TransformSystems::Propagate)
                .before(UiSystems::Prepare),
        )
        .add_systems(PostUpdate, scroll_menu.after(UiSystems::Layout));
}

fn setup_hud(mut commands: Commands, state: Res<VisualState>, assets: Res<UiAssets>) {
    let font =
        |font_size: f32| TextFont::from_font_size(font_size).with_font(assets.font(state.locale));
    let colors = [Color::srgb(0.12, 0.92, 0.9), Color::srgb(0.96, 0.28, 0.67)];
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(88),
                left: px(38),
                width: percent(45),
                max_width: px(600),
                height: px(20),
                overflow: Overflow::clip(),
                ..default()
            },
            Subtitle,
        ))
        .with_child((
            Text::default(),
            UiText::Subtitle,
            font(13.0),
            TextColor(Color::srgb(0.72, 0.79, 0.88)),
            TextLayout::no_wrap(),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
        ));
    commands.spawn((
        Text::default(),
        UiText::Clock,
        font(17.0),
        TextColor(Color::srgb(0.92, 0.95, 0.99)),
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
                bottom: px(150),
                ..default()
            },
        ));
    }
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(28),
                width: px(390),
                bottom: px(28),
                padding: UiRect::all(px(16)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(22)),
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.039, 0.074, 0.96)),
            BorderColor::all(Color::srgba(0.42, 0.59, 0.76, 0.38)),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.02, 0.35),
                px(0),
                px(8),
                px(0),
                px(28),
            ),
            GlobalZIndex(0),
            StatusPanel,
        ))
        .with_children(|panel| {
            panel.spawn((
                HudNode::Waiting,
                IgnoreScroll(BVec2::TRUE),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(1),
                    left: percent(0),
                    width: percent(18),
                    height: px(2),
                    border_radius: BorderRadius::all(px(1)),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.57, 0.93, 0.97)),
            ));
            let color = TextColor(Color::srgb(0.87, 0.91, 0.96));
            panel.spawn((
                Text::default(),
                font(20.0),
                color,
                UiText::Status,
                Node {
                    flex_shrink: 0.0,
                    min_width: px(0),
                    ..default()
                },
            ));
            panel.spawn((
                Text::default(),
                font(14.0),
                TextColor(Color::srgb(0.88, 0.95, 0.98)),
                UiText::OwnerHint,
                HudNode::OwnerHint,
                BackgroundColor(Color::srgb(0.06, 0.12, 0.17)),
                Node {
                    flex_shrink: 0.0,
                    min_width: px(0),
                    padding: UiRect::all(px(8)),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
            ));
            panel
                .spawn((
                    Node {
                        column_gap: px(8),
                        row_gap: px(8),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    PlayerCards,
                    HudNode::Players,
                ))
                .with_children(|cards| {
                    for (player, color) in colors.into_iter().enumerate() {
                        cards
                            .spawn((
                                Node {
                                    flex_grow: 1.0,
                                    flex_basis: px(0),
                                    min_width: px(0),
                                    padding: UiRect::all(px(10)),
                                    border: UiRect::left(px(3)),
                                    border_radius: BorderRadius::all(px(10)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.055, 0.077, 0.12)),
                                BorderColor::all(color),
                            ))
                            .with_child((
                                Text::default(),
                                UiText::Device(player),
                                font(13.0),
                                TextColor(Color::srgb(0.90, 0.94, 0.99)),
                                Node {
                                    min_width: px(0),
                                    flex_grow: 1.0,
                                    flex_basis: px(0),
                                    ..default()
                                },
                            ));
                    }
                });
            panel.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    flex_shrink: 0.0,
                    ..default()
                },
                MenuRows,
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

fn ensure_menu_rows(
    mut commands: Commands,
    state: Res<VisualState>,
    assets: Res<UiAssets>,
    parent: Query<Entity, With<MenuRows>>,
    existing: Query<&MenuRowNode>,
) {
    let Some(menu) = &state.menu else {
        return;
    };
    let Ok(parent) = parent.single() else {
        return;
    };
    let count = existing.iter().count();
    for index in count..menu.rows.len() {
        commands.entity(parent).with_children(|parent| {
            parent
                .spawn((
                    Node {
                        align_items: AlignItems::Start,
                        column_gap: px(6),
                        flex_shrink: 0.0,
                        min_width: px(0),
                        padding: UiRect::axes(px(10), px(9)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(10)),
                        ..default()
                    },
                    MenuRowNode(index),
                    HudNode::Row(index),
                    BackgroundColor::default(),
                    BorderColor::default(),
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

#[allow(clippy::type_complexity)]
fn layout_hud(
    state: Res<VisualState>,
    cameras: Query<&Camera, With<IsDefaultUiCamera>>,
    mut panels: Query<&mut Node, With<StatusPanel>>,
    mut auxiliary: Query<(&UiText, &mut Node), Without<StatusPanel>>,
    mut cards: Query<
        &mut Node,
        (
            With<PlayerCards>,
            Without<StatusPanel>,
            Without<UiText>,
            Without<Subtitle>,
        ),
    >,
    mut subtitles: Query<&mut Node, (With<Subtitle>, Without<StatusPanel>, Without<UiText>)>,
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
        node.right = Val::Auto;
        node.padding = UiRect::all(px((viewport.x * 0.04).clamp(6.0, 16.0)));
        node.min_height = px(0);
        if let Some(menu) = &state.menu {
            let (origin, width) = dock_geometry(viewport);
            let top = origin.y + width * 180.0 / 840.0 + if compact { 8.0 } else { 44.0 };
            node.top = px(top);
            node.bottom = Val::Auto;
            node.width =
                px((viewport.x - 2.0 * margin)
                    .max(1.0)
                    .min(if menu.kind == MenuKind::Settings {
                        620.0
                    } else {
                        390.0
                    }));
            node.max_height = px((viewport.y - margin - top).max(1.0));
            node.overflow = Overflow::scroll_y();
        } else {
            node.top = Val::Auto;
            node.bottom = px(margin);
            node.width = px((viewport.x - 2.0 * margin).clamp(1.0, 600.0));
            node.max_height = Val::Auto;
            node.padding = UiRect::axes(px(12), px(8));
            node.overflow = Overflow::DEFAULT;
            node.display = if state.status.is_empty() {
                Display::None
            } else {
                Display::Flex
            };
        }
        if state.menu.is_some() {
            node.display = Display::Flex;
        }
        if *panel != node {
            *panel = node;
        }
    }
    for mut node in &mut cards {
        let direction = if viewport.x < 380.0 {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        };
        if node.flex_direction != direction {
            node.flex_direction = direction;
        }
    }
    // This optional decoration remains hidden in compact layouts
    for mut node in &mut subtitles {
        let display = if compact {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (kind, mut node) in &mut auxiliary {
        if matches!(kind, UiText::Clock) {
            let display = if state.menu.is_some() {
                Display::None
            } else {
                Display::Flex
            };
            if node.display != display {
                node.display = display;
            }
            node.right = px(margin);
            node.top = px(margin);
            node.max_width = px((viewport.x - 2.0 * margin).max(1.0));
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

fn scroll_menu(
    state: Res<VisualState>,
    mut control: ResMut<MenuScroll>,
    mut previous: Local<Option<SettingsLayout>>,
    mut panels: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<StatusPanel>>,
    rows: Query<(&MenuRowNode, &ComputedNode, &UiGlobalTransform)>,
) {
    let Some(menu) = &state.menu else {
        control.reset();
        *previous = None;
        for (_, _, mut scroll) in &mut panels {
            scroll.0 = Vec2::ZERO;
        }
        return;
    };
    let Some(index) = menu.rows.iter().position(|row| row.selected) else {
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
        menu.rows.len(),
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

#[allow(clippy::type_complexity)]
fn update_hud(
    (state, assets, intro, time, mut feedback): (
        Res<VisualState>,
        Res<UiAssets>,
        Option<Res<BrandIntroStatus>>,
        Res<Time>,
        Local<FocusFeedback>,
    ),
    mut texts: Query<(&UiText, &mut Text, &mut TextFont, &mut TextColor), Without<TextSpan>>,
    mut spans: Query<(&UiText, &mut TextSpan, &mut TextFont, &mut TextColor), Without<Text>>,
    mut flags: Query<(&LanguageFlag, &mut ImageNode)>,
    mut nodes: Query<(
        &HudNode,
        &mut Node,
        Option<&mut BackgroundColor>,
        Option<&mut BorderColor>,
    )>,
    mut panel: Query<
        (&mut GlobalZIndex, &mut BackgroundColor),
        (With<StatusPanel>, Without<HudNode>),
    >,
    mut labels: Query<&mut Visibility, With<PlayerLabel>>,
) {
    let selected = state.menu.as_ref().and_then(|menu| {
        menu.rows
            .iter()
            .position(|row| row.selected)
            .map(|index| (menu.kind, index))
    });
    let was_animating = feedback.remaining > 0.0;
    if feedback.selected != selected {
        feedback.selected = selected;
        feedback.remaining = if selected.is_some() { 0.16 } else { 0.0 };
    } else {
        feedback.remaining = (feedback.remaining - time.delta_secs()).max(0.0);
    }
    if !state.is_changed()
        && !assets.is_changed()
        && !intro.as_ref().is_some_and(|intro| intro.is_changed())
        && !was_animating
        && feedback.remaining == 0.0
        && !state.transitioning
    {
        return;
    }
    // The brand backdrop reveals the scene and HUD; only startup errors render above it
    let failed = intro.is_some_and(|intro| intro.phase == BrandIntroPhase::Failed);
    for mut visible in &mut labels {
        *visible = if state.menu.is_some() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    for (mut layer, mut background) in &mut panel {
        layer.0 = if failed { 1001 } else { 0 };
        background.0 = if state.menu.is_some() || failed {
            Color::srgba(0.025, 0.039, 0.074, 0.96)
        } else {
            Color::srgba(0.025, 0.039, 0.074, 0.80)
        };
    }
    let locale = state.locale;
    let duration = state.duration_seconds.max(0.0);
    let elapsed = state.song_seconds.clamp(0.0, duration);
    let rows = state
        .menu
        .as_ref()
        .map(|menu| menu.rows.as_slice())
        .unwrap_or_default();
    for (kind, mut text, mut font, mut color) in &mut texts {
        let value = match *kind {
            UiText::Status => state
                .menu
                .as_ref()
                .map(|menu| menu.title.clone())
                .unwrap_or_else(|| state.status.clone()),
            UiText::OwnerHint => state
                .menu
                .as_ref()
                .and_then(|menu| menu.owner_hint.clone())
                .unwrap_or_default(),
            UiText::Subtitle => state
                .section_hint
                .clone()
                .unwrap_or_else(|| locale.text("hud.subtitle").into()),
            UiText::Clock => Message::with(
                "hud.clock",
                [
                    ("elapsed", format!("{elapsed:05.1}")),
                    ("duration", format!("{duration:.1}")),
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
            UiText::Device(player) => state
                .menu
                .as_ref()
                .and_then(|menu| menu.players.as_ref())
                .map(|players| players[player].clone())
                .unwrap_or_default(),
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
        if let UiText::RowPrefix(index) = *kind
            && let Some(row) = rows.get(index)
        {
            let size = FontSize::Px(row_font_size(row.role));
            if font.font_size != size {
                font.font_size = size;
            }
            color.0 = row_text_color(row);
        } else if matches!(kind, UiText::Status) {
            let size = FontSize::Px(if state.menu.is_some() { 20.0 } else { 14.0 });
            if font.font_size != size {
                font.font_size = size;
            }
        }
    }
    for (kind, mut text, mut font, mut color) in &mut spans {
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
        if let UiText::RowName(index) | UiText::RowSuffix(index) = *kind
            && let Some(row) = rows.get(index)
        {
            let size = FontSize::Px(row_font_size(row.role));
            if font.font_size != size {
                font.font_size = size;
            }
            color.0 = row_text_color(row);
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
    for (kind, mut node, background, border) in &mut nodes {
        let visible = match *kind {
            HudNode::Progress => {
                node.width = percent(if duration > 0.0 {
                    (elapsed / duration) as f32 * 100.0
                } else {
                    0.0
                });
                continue;
            }
            HudNode::Waiting => {
                node.left = percent(36.0 * (1.0 + (time.elapsed_secs() * 4.0).sin()));
                state.transitioning && state.menu.is_some()
            }
            HudNode::Rows => state.menu.is_some(),
            HudNode::OwnerHint => state
                .menu
                .as_ref()
                .is_some_and(|menu| menu.owner_hint.is_some()),
            HudNode::Players => state
                .menu
                .as_ref()
                .is_some_and(|menu| menu.players.is_some()),
            HudNode::Row(index) => {
                if let Some(row) = rows.get(index) {
                    let primary = row.role == MenuRowRole::Primary;
                    let information = row.role == MenuRowRole::Information;
                    node.padding = UiRect::axes(px(10), px(if primary { 13 } else { 9 }));
                    node.margin = UiRect::top(px(
                        if information
                            && index > 0
                            && rows[index - 1].role != MenuRowRole::Information
                        {
                            8
                        } else {
                            0
                        },
                    ));
                    if let Some(mut background) = background {
                        background.0 = match (row.selected, primary, information) {
                            (true, true, _) => Color::srgb(0.42, 0.89, 0.94),
                            (true, _, _) => Color::srgb(0.11, 0.23, 0.32),
                            (_, true, _) => Color::srgb(0.10, 0.23, 0.30),
                            (_, _, true) => Color::NONE,
                            _ => Color::srgb(0.055, 0.078, 0.12),
                        };
                    }
                    if let Some(mut border) = border {
                        *border = BorderColor::all(if row.selected {
                            let strength = feedback.remaining / 0.16;
                            Color::srgb(
                                0.57 + 0.18 * strength,
                                0.93 + 0.05 * strength,
                                0.97 + 0.03 * strength,
                            )
                        } else {
                            Color::NONE
                        });
                    }
                    true
                } else {
                    false
                }
            }
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

fn row_font_size(role: MenuRowRole) -> f32 {
    match role {
        MenuRowRole::Primary => 22.0,
        MenuRowRole::Action => 16.0,
        MenuRowRole::Information => 14.0,
    }
}

fn row_text_color(row: &MenuRow) -> Color {
    if row.selected && row.role == MenuRowRole::Primary {
        Color::srgb(0.025, 0.07, 0.11)
    } else if row.role == MenuRowRole::Information && !row.selected {
        Color::srgb(0.75, 0.82, 0.91)
    } else {
        Color::srgb(0.94, 0.97, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hud_app() -> App {
        let mut app = App::new();
        app.init_resource::<VisualState>()
            .init_resource::<Time>()
            .init_resource::<Assets<Font>>()
            .init_resource::<bevy::text::FontCx>()
            .init_resource::<Assets<Image>>()
            .add_systems(Startup, setup_hud)
            .add_systems(PostUpdate, (ensure_menu_rows, update_hud).chain());
        crate::ui_assets::install(&mut app).unwrap();
        app
    }

    #[test]
    fn focus_feedback_is_brief_and_never_delays_or_moves_the_selected_row() {
        let mut app = hud_app();
        app.world_mut().resource_mut::<VisualState>().menu = Some(MenuPresentation {
            rows: ["Start", "Settings"]
                .into_iter()
                .enumerate()
                .map(|(index, text)| MenuRow {
                    text: text.into(),
                    selected: index == 0,
                    ..default()
                })
                .collect(),
            ..default()
        });
        app.update();
        let mut rows = app
            .world_mut()
            .query::<(Entity, &MenuRowNode)>()
            .iter(app.world())
            .map(|(entity, row)| (row.0, entity))
            .collect::<Vec<_>>();
        rows.sort_by_key(|(index, _)| *index);
        let [first, second] = [rows[0].1, rows[1].1];
        let geometry =
            [first, second].map(|entity| app.world().get::<Node>(entity).unwrap().clone());
        let highlighted = app.world().get::<BorderColor>(first).unwrap().top;
        let settled = Color::srgb(0.57, 0.93, 0.97);
        assert_ne!(highlighted, settled);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(40));
        app.update();
        assert_ne!(
            app.world().get::<BorderColor>(first).unwrap().top,
            highlighted
        );
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            let rows = &mut state.menu.as_mut().unwrap().rows;
            rows[0].selected = false;
            rows[1].selected = true;
        }
        app.update();
        assert_eq!(
            app.world().get::<BorderColor>(first).unwrap().top,
            Color::NONE
        );
        assert_eq!(
            app.world().get::<BorderColor>(second).unwrap().top,
            highlighted
        );
        let prefix = app
            .world_mut()
            .query::<(&UiText, &Text)>()
            .iter(app.world())
            .find(|(kind, _)| matches!(kind, UiText::RowPrefix(1)))
            .unwrap()
            .1;
        assert_eq!(prefix.0, "> Settings");
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(80));
        app.update();
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.locale = Locale::ZhCn;
            state.menu.as_mut().unwrap().owner_hint = Some("P2 controller".into());
        }
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(80));
        app.update();
        assert_eq!(app.world().get::<BorderColor>(second).unwrap().top, settled);
        for (entity, node) in [first, second].into_iter().zip(geometry) {
            assert_eq!(app.world().get::<Node>(entity).unwrap(), &node);
        }
        for color in app.world_mut().query::<&TextColor>().iter(app.world()) {
            assert_eq!(color.0.alpha(), 1.0);
        }
        app.world_mut().resource_mut::<VisualState>().menu = None;
        app.update();
        assert_eq!(
            app.world().get::<Node>(second).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn waiting_marker_stops_with_the_transition_and_progress_keeps_song_time() {
        let mut app = hud_app();
        app.insert_resource(VisualState {
            song_seconds: 16.0,
            duration_seconds: 64.0,
            menu: Some(MenuPresentation::default()),
            ..default()
        });
        let sample = |app: &mut App| {
            let mut nodes = app.world_mut().query::<(&HudNode, &Node)>();
            let waiting = nodes
                .iter(app.world())
                .find(|(kind, _)| matches!(kind, HudNode::Waiting))
                .unwrap()
                .1;
            let progress = nodes
                .iter(app.world())
                .find(|(kind, _)| matches!(kind, HudNode::Progress))
                .unwrap()
                .1;
            (waiting.display, waiting.left, progress.width)
        };
        app.update();
        let waiting = app
            .world_mut()
            .query::<(&HudNode, &IgnoreScroll)>()
            .iter(app.world())
            .find(|(kind, _)| matches!(kind, HudNode::Waiting))
            .unwrap()
            .1;
        assert_eq!(waiting.0, BVec2::TRUE);
        assert_eq!(sample(&mut app).0, Display::None);
        app.world_mut().resource_mut::<VisualState>().transitioning = true;
        app.update();
        let first = sample(&mut app);
        assert_eq!(first.0, Display::Flex);
        assert_eq!(first.2, percent(25));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(100));
        app.update();
        let moved = sample(&mut app);
        assert_ne!(moved.1, first.1);
        assert_eq!(moved.2, first.2);
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.transitioning = false;
            state.song_seconds = 0.0;
        }
        app.update();
        let stopped = sample(&mut app);
        assert_eq!(stopped.0, Display::None);
        assert_eq!(stopped.2, percent(0));
    }

    #[test]
    fn clock_and_progress_follow_loaded_duration_and_clamp_the_same_song_time() {
        let mut app = hud_app();
        for (duration, elapsed, progress, clock) in [
            (0.0, 0.0, 0.0, "000.0 / 0.0 SEC"),
            (0.1, 0.025, 25.0, "000.0 / 0.1 SEC"),
            (0.1, 0.2, 100.0, "000.1 / 0.1 SEC"),
            (90.0, 45.0, 50.0, "045.0 / 90.0 SEC"),
            (90.0, 120.0, 100.0, "090.0 / 90.0 SEC"),
            (90.0, -1.0, 0.0, "000.0 / 90.0 SEC"),
            (0.0, 45.0, 0.0, "000.0 / 0.0 SEC"),
        ] {
            {
                let mut state = app.world_mut().resource_mut::<VisualState>();
                state.locale = Locale::EnUs;
                state.duration_seconds = duration;
                state.song_seconds = elapsed;
            }
            app.update();
            let actual_clock = app
                .world_mut()
                .query::<(&UiText, &Text)>()
                .iter(app.world())
                .find(|(kind, _)| matches!(kind, UiText::Clock))
                .unwrap()
                .1
                .0
                .clone();
            let actual_progress = app
                .world_mut()
                .query::<(&HudNode, &Node)>()
                .iter(app.world())
                .find(|(kind, _)| matches!(kind, HudNode::Progress))
                .unwrap()
                .1
                .width;
            assert_eq!(actual_clock.lines().next(), Some(clock));
            assert_eq!(actual_progress, percent(progress));
            assert_eq!(app.world().resource::<VisualState>().song_seconds, elapsed);
        }
    }

    fn measured_hud_app() -> (App, Entity) {
        use bevy::{
            app::{HierarchyPropagatePlugin, PropagateSet},
            asset::AssetPlugin,
            camera::{ComputedCameraValues, RenderTargetInfo},
            text::{TextPlugin, detect_text_needs_rerender, load_font_assets_into_font_collection},
            ui::{
                ui_layout_system,
                ui_surface::UiSurface,
                update::{propagate_ui_target_cameras, update_clipping_system},
                widget::{measure_text_system, text_system},
            },
        };
        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            AssetPlugin::default(),
            TextPlugin,
            HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
            HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
        ))
        .init_resource::<VisualState>()
        .init_resource::<BrandIntroLayout>()
        .init_resource::<Time>()
        .init_resource::<MenuScroll>()
        .init_resource::<Assets<Image>>()
        .init_resource::<UiScale>()
        .init_resource::<UiSurface>()
        .add_systems(Startup, setup_hud)
        .add_systems(Update, update_brand_layout)
        .add_systems(PostUpdate, (ensure_menu_rows, update_hud).chain())
        .add_systems(
            PostUpdate,
            (
                layout_hud.after(update_hud),
                propagate_ui_target_cameras,
                measure_text_system
                    .after(detect_text_needs_rerender)
                    .after(load_font_assets_into_font_collection),
                ui_layout_system,
                text_system,
                update_clipping_system,
            )
                .chain(),
        )
        .add_systems(PostUpdate, scroll_menu.after(text_system))
        .configure_sets(
            PostUpdate,
            (
                PropagateSet::<ComputedUiTargetCamera>::default(),
                PropagateSet::<ComputedUiRenderTargetInfo>::default(),
            )
                .after(propagate_ui_target_cameras)
                .before(measure_text_system),
        );
        crate::ui_assets::install(&mut app).unwrap();
        let camera = app
            .world_mut()
            .spawn((
                Camera2d,
                IsDefaultUiCamera,
                Camera {
                    computed: ComputedCameraValues {
                        target_info: Some(RenderTargetInfo {
                            physical_size: UVec2::new(1280, 800),
                            scale_factor: 1.0,
                        }),
                        ..default()
                    },
                    ..default()
                },
            ))
            .id();
        (app, camera)
    }

    #[test]
    fn optional_section_hint_uses_original_fallback_and_clips_long_single_lines() {
        use bevy::{camera::RenderTargetInfo, text::TextLayoutInfo};

        let (mut app, camera) = measured_hud_app();
        app.world_mut().resource_mut::<VisualState>().locale = Locale::EnUs;
        app.update();
        let subtitle = app
            .world_mut()
            .query::<(Entity, &UiText)>()
            .iter(app.world())
            .find(|(_, kind)| matches!(kind, UiText::Subtitle))
            .unwrap()
            .0;
        let frame = app.world().get::<ChildOf>(subtitle).unwrap().parent();
        assert_eq!(
            app.world().get::<Text>(subtitle).unwrap().0,
            Locale::EnUs.text("hud.subtitle")
        );
        let hint = Message::with(
            "hud.section_recent",
            [("label", format!("#7 {}", "W".repeat(256)))],
        )
        .render(Locale::EnUs);
        app.world_mut().resource_mut::<VisualState>().section_hint = Some(hint.clone());
        for (size, scale, visible) in [
            (UVec2::new(1280, 800), 1.0, true),
            (UVec2::new(900, 600), 1.0, true),
            (UVec2::new(899, 800), 1.0, false),
            (UVec2::new(1280, 599), 1.0, false),
            (UVec2::new(1280, 800), 2.0, false),
        ] {
            app.world_mut()
                .get_mut::<Camera>(camera)
                .unwrap()
                .computed
                .target_info = Some(RenderTargetInfo {
                physical_size: size,
                scale_factor: scale,
            });
            for _ in 0..4 {
                app.update();
            }
            assert_eq!(app.world().get::<Text>(subtitle).unwrap().0, hint);
            let node = app.world().get::<Node>(frame).unwrap();
            assert_eq!(
                node.display,
                if visible {
                    Display::Flex
                } else {
                    Display::None
                }
            );
            let clip = app.world().get::<CalculatedClip>(subtitle).unwrap().clip;
            if visible {
                let layout = app.world().get::<TextLayoutInfo>(subtitle).unwrap();
                let expected_width = (size.x as f32 * 0.45).min(600.0);
                assert!((clip.width() - expected_width).abs() < 1.0, "{clip:?}");
                assert_eq!(clip.height(), 20.0);
                assert!(layout.size.x > clip.width());
                assert!(layout.size.y <= clip.height());
                assert!(!layout.glyphs.is_empty());
                let dock = app.world().resource::<BrandIntroLayout>().dock_rect;
                assert!(clip.min.y > dock.max.y);
                assert!(clip.max.y < dock.max.y + 44.0);
                let clock = app
                    .world_mut()
                    .query::<(&UiText, &ComputedNode, &UiGlobalTransform)>()
                    .iter(app.world())
                    .find(|(kind, _, _)| matches!(kind, UiText::Clock))
                    .map(|(_, node, transform)| {
                        Rect::from_center_size(transform.translation, node.size)
                    })
                    .unwrap();
                assert!(clip.max.x < clock.min.x || clip.min.y > clock.max.y);
            } else {
                assert!(clip.is_empty());
            }
        }
        {
            let mut state = app.world_mut().resource_mut::<VisualState>();
            state.section_hint = None;
            state.locale = Locale::ZhCn;
        }
        app.update();
        assert_eq!(
            app.world().get::<Text>(subtitle).unwrap().0,
            Locale::ZhCn.text("hud.subtitle")
        );
    }

    #[test]
    fn menu_summary_measures_wrapped_device_and_owner_text() {
        use bevy::{camera::RenderTargetInfo, text::TextLayoutInfo};

        let (mut app, camera) = measured_hud_app();
        let players = [
            "P1：键盘 KeyF 可用 / 可选手柄".to_owned(),
            "P2：键盘 KeyJ 可用 / 可选手柄".to_owned(),
        ];
        app.world_mut().resource_mut::<VisualState>().menu = Some(MenuPresentation {
            title: "就绪".into(),
            players: Some(players.clone()),
            rows: (0..7)
                .map(|index| MenuRow {
                    text: if index == 0 { "开始" } else { "菜单操作" }.into(),
                    selected: index == 0,
                    role: if index == 0 {
                        MenuRowRole::Primary
                    } else {
                        MenuRowRole::Action
                    },
                    ..default()
                })
                .collect(),
            ..default()
        });
        app.world_mut().resource_mut::<VisualState>().locale = Locale::ZhCn;
        for (physical_size, scale_factor, owner) in [
            (UVec2::new(1280, 800), 1.0, "键盘"),
            (UVec2::new(400, 300), 1.0, "P2 手柄"),
            (UVec2::new(320, 240), 2.0, "P1 手柄"),
        ] {
            let owner_hint =
                Message::with("menu.owner", [("device", owner.into())]).render(Locale::ZhCn);
            app.world_mut()
                .resource_mut::<VisualState>()
                .menu
                .as_mut()
                .unwrap()
                .owner_hint = Some(owner_hint.clone());
            app.world_mut()
                .get_mut::<Camera>(camera)
                .unwrap()
                .computed
                .target_info = Some(RenderTargetInfo {
                physical_size,
                scale_factor,
            });
            for _ in 0..4 {
                app.update();
            }
            let mut count = 0;
            let mut owner_count = 0;
            for (entity, kind, text, node, layout) in app
                .world_mut()
                .query::<(Entity, &UiText, &Text, &ComputedNode, &TextLayoutInfo)>()
                .iter(app.world())
            {
                if let UiText::Device(player) = *kind {
                    let parent = app.world().get::<ChildOf>(entity).unwrap().parent();
                    let content = app
                        .world()
                        .get::<ComputedNode>(parent)
                        .unwrap()
                        .content_box()
                        .size();
                    assert_eq!(text.0, players[player]);
                    assert!(
                        node.size.x > 0.0 && node.size.y > 0.0,
                        "device {player}: {:?}",
                        node.size
                    );
                    assert!(!layout.glyphs.is_empty());
                    assert!(
                        layout.size.cmple(node.size + Vec2::ONE).all(),
                        "text {:?}, node {:?}",
                        layout.size,
                        node.size
                    );
                    assert!(
                        node.size.cmple(content + Vec2::ONE).all(),
                        "node {:?}, card {:?}",
                        node.size,
                        content
                    );
                    count += 1;
                } else if matches!(kind, UiText::OwnerHint) {
                    assert_eq!(text.0, owner_hint);
                    assert!(!layout.glyphs.is_empty());
                    assert!(
                        layout
                            .size
                            .cmple(node.content_box().size() + Vec2::ONE)
                            .all(),
                        "owner text {:?}, content {:?}",
                        layout.size,
                        node.content_box().size()
                    );
                    assert_eq!(
                        app.world().get::<TextFont>(entity).unwrap().font_size,
                        FontSize::Px(14.0)
                    );
                    let panel = app.world().get::<ChildOf>(entity).unwrap().parent();
                    assert!(app.world().get::<StatusPanel>(panel).is_some());
                    assert!(
                        node.size.x
                            <= app
                                .world()
                                .get::<ComputedNode>(panel)
                                .unwrap()
                                .content_box()
                                .size()
                                .x
                                + 1.0
                    );
                    if scale_factor == 1.0 {
                        let panel_node = app.world().get::<ComputedNode>(panel).unwrap();
                        let panel_y = app
                            .world()
                            .get::<UiGlobalTransform>(panel)
                            .unwrap()
                            .translation
                            .y;
                        let owner_y = app
                            .world()
                            .get::<UiGlobalTransform>(entity)
                            .unwrap()
                            .translation
                            .y;
                        assert!(owner_y - node.size.y * 0.5 >= panel_y - panel_node.size.y * 0.5);
                        assert!(owner_y + node.size.y * 0.5 <= panel_y + panel_node.size.y * 0.5);
                    }
                    owner_count += 1;
                }
            }
            assert_eq!(count, 2);
            assert_eq!(owner_count, 1);
        }
    }

    #[test]
    fn measured_rows_scroll_into_small_and_scaled_viewports() {
        use crate::input::{MenuRow, SettingsAction};
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
            .init_resource::<MenuScroll>()
            .insert_resource(VisualState {
                menu: Some(MenuPresentation {
                    title: "Settings".into(),
                    rows: (0..5).map(|_| MenuRow::default()).collect(),
                    kind: MenuKind::Settings,
                    ..default()
                }),
                ..default()
            })
            .add_systems(
                PostUpdate,
                (
                    layout_hud,
                    propagate_ui_target_cameras,
                    ui_layout_system,
                    scroll_menu,
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
                            MenuRowNode(index),
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
                    .menu
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
                            .resource_mut::<MenuScroll>()
                            .handle(SettingsAction::Confirm)
                    );
                    assert!(
                        !app.world_mut()
                            .resource_mut::<MenuScroll>()
                            .handle(SettingsAction::Back)
                    );
                    for action in [SettingsAction::Down, SettingsAction::Up] {
                        let mut pages = 0;
                        while app.world_mut().resource_mut::<MenuScroll>().handle(action) {
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
            // New pages can reuse the same row index, count and dimensions
            for title in ["Ready", "Settings", "Binding"] {
                app.world_mut()
                    .resource_mut::<VisualState>()
                    .menu
                    .as_mut()
                    .unwrap()
                    .title = title.into();
                app.world_mut().resource_mut::<MenuScroll>().reset();
                assert!(
                    !app.world_mut()
                        .resource_mut::<MenuScroll>()
                        .handle(SettingsAction::Down)
                );
                for _ in 0..3 {
                    app.update();
                }
                let (top, _, row_top, _) = bounds(app.world(), rows[1]);
                assert!(row_top >= top - 1.0);
                assert!(
                    app.world_mut()
                        .resource_mut::<MenuScroll>()
                        .handle(SettingsAction::Down)
                );
                app.update();
                app.update();
                let (top, _, row_top, _) = bounds(app.world(), rows[1]);
                assert!(row_top < top - 1.0);
            }
            app.world_mut().resource_mut::<VisualState>().menu = None;
            app.update();
            assert_eq!(
                app.world().get::<ScrollPosition>(panel).unwrap().0,
                Vec2::ZERO
            );
            assert!(
                !app.world_mut()
                    .resource_mut::<MenuScroll>()
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
        use crate::input::MenuRow;
        let mut app = hud_app();
        for locale in Locale::ALL {
            {
                let mut state = app.world_mut().resource_mut::<VisualState>();
                state.locale = locale;
                state.menu = Some(MenuPresentation {
                    title: locale.text("settings.language_title").into(),
                    rows: Locale::ALL
                        .into_iter()
                        .map(|language| MenuRow {
                            text: "{language}".into(),
                            language: Some(language),
                            selected: language == locale,
                            ..default()
                        })
                        .collect(),
                    kind: MenuKind::Settings,
                    ..default()
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
            state.menu = Some(MenuPresentation {
                title: "Settings".into(),
                rows: vec![MenuRow {
                    text: Locale::EnUs.text("settings.language").into(),
                    language: Some(Locale::Ko),
                    selected: true,
                    ..default()
                }],
                kind: MenuKind::Settings,
                ..default()
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
            state.menu = None;
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
}
