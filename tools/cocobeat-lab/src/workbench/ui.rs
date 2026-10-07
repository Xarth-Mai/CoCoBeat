use super::*;
use bevy::{
    asset::RenderAssetUsages,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cocobeat_runtime::UiAssets;

const ROWS: usize = 16;
const ROW_HEIGHT: f32 = 28.0;
const INK: Color = Color::srgb(0.87, 0.91, 0.96);

#[derive(Component, Clone, Copy, Debug)]
pub(super) enum Hit {
    Action(Action),
    Timeline,
    Row(usize),
    Details,
}

#[derive(Component)]
pub(super) enum Part {
    Title,
    Owner,
    Cursor,
    Audition,
    Range,
    ListTab,
    Row(usize),
    DetailText,
    FrameText,
    ModalTitle,
}

#[derive(Component)]
pub(super) enum BoxPart {
    Toolbar(usize),
    Canvas,
    Tab(usize),
    Row(usize),
    Detail,
    Frame,
    Apply,
    Modal,
    Keep,
    Discard,
}

#[derive(Resource, Default)]
pub(super) struct View {
    image: Handle<Image>,
    pub canvas: Rect,
    pub first_row: usize,
    pub row_count: usize,
    raster: Option<Raster>,
    last_paint: Option<PaintKey>,
}

struct Raster {
    key: (u32, u32, i64, i64),
    pixels: Vec<u8>,
}

#[derive(PartialEq, Eq)]
struct PaintKey {
    viewport: (u32, u32, i64, i64),
    cursor: i64,
    audition: Option<i64>,
    selected: Option<u64>,
    drag: Option<(u64, i64)>,
    revision: u64,
    record_selection: Option<usize>,
}

fn key(action: Action) -> &'static str {
    match action {
        Action::PlayPause => "audition.toggle",
        Action::Stop => "audition.stop",
        Action::Seek => "audition.seek",
        Action::Undo => "workbench.undo",
        Action::Redo => "workbench.redo",
        Action::Add => "workbench.add",
        Action::Remove => "workbench.remove",
        Action::Export => "workbench.export",
        Action::Back => "settings.back",
        Action::ZoomIn => "workbench.zoom_in",
        Action::ZoomOut => "workbench.zoom_out",
        Action::Previous => "workbench.previous",
        Action::Next => "workbench.next",
        Action::List => "workbench.anchors",
        Action::Details => "workbench.details",
        Action::Frame => "workbench.frame",
        Action::Apply => "workbench.apply",
        Action::Keep => "workbench.keep",
        Action::Discard => "workbench.discard",
    }
}

fn node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        ..default()
    }
}

pub(super) fn setup(
    mut commands: Commands,
    state: Res<Workbench>,
    fonts: Res<UiAssets>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.spawn(Camera2d);
    let font = |size| TextFont::from_font_size(size).with_font(fonts.font(state.locale));
    for (part, size) in [
        (Part::Title, 20.0),
        (Part::Owner, 13.0),
        (Part::Cursor, 14.0),
        (Part::Audition, 12.0),
        (Part::Range, 12.0),
    ] {
        commands.spawn((Text::default(), font(size), TextColor(INK), node(), part));
    }
    for (index, &action) in state.toolbar().iter().enumerate() {
        commands
            .spawn((
                Node {
                    border: UiRect::all(px(1)),
                    padding: UiRect::axes(px(6), px(4)),
                    border_radius: BorderRadius::all(px(9)),
                    ..node()
                },
                BackgroundColor(Color::srgb(0.06, 0.09, 0.16)),
                BorderColor::all(Color::srgb(0.2, 0.28, 0.39)),
                Hit::Action(action),
                BoxPart::Toolbar(index),
            ))
            .with_child((
                Text::new(state.locale.text(key(action))),
                font(13.0),
                TextColor(INK),
            ));
    }
    let image = images.add(Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[6, 10, 21, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    commands.spawn((
        ImageNode {
            image: image.clone(),
            image_mode: NodeImageMode::Stretch,
            visual_box: bevy::ui::VisualBox::BorderBox,
            ..default()
        },
        Node {
            border: UiRect::all(px(1)),
            ..node()
        },
        BorderColor::all(INK),
        Hit::Timeline,
        BoxPart::Canvas,
    ));
    for (index, action) in [
        Action::List,
        Action::Details,
        Action::Previous,
        Action::Next,
    ]
    .into_iter()
    .enumerate()
    {
        let text = if action == Action::List {
            String::new()
        } else {
            state.locale.text(key(action)).into()
        };
        let mut entity = commands.spawn((
            Node {
                padding: UiRect::all(px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..node()
            },
            BackgroundColor(Color::srgb(0.05, 0.075, 0.13)),
            BorderColor::all(Color::srgb(0.19, 0.25, 0.36)),
            Hit::Action(action),
            BoxPart::Tab(index),
        ));
        if action == Action::List {
            entity.with_child((Text::new(text), font(13.0), TextColor(INK), Part::ListTab));
        } else {
            entity.with_child((Text::new(text), font(13.0), TextColor(INK)));
        }
    }
    for index in 0..ROWS {
        commands
            .spawn((
                Node {
                    padding: UiRect::axes(px(8), px(3)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(6)),
                    overflow: Overflow::clip(),
                    ..node()
                },
                BackgroundColor(Color::srgb(0.03, 0.05, 0.09)),
                BorderColor::all(Color::srgb(0.11, 0.16, 0.25)),
                Hit::Row(index),
                BoxPart::Row(index),
            ))
            .with_child((
                Text::default(),
                font(14.0),
                TextColor(INK),
                Part::Row(index),
            ));
    }
    commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                padding: UiRect::all(px(8)),
                ..node()
            },
            ScrollPosition::default(),
            BackgroundColor(Color::srgb(0.028, 0.045, 0.08)),
            Hit::Details,
            BoxPart::Detail,
        ))
        .with_child((
            Text::default(),
            font(14.0),
            TextColor(INK),
            TextLayout::new(Justify::Left, bevy::text::LineBreak::AnyCharacter),
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                ..default()
            },
            Part::DetailText,
        ));
    commands
        .spawn((
            Node {
                padding: UiRect::all(px(5)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..node()
            },
            BackgroundColor(Color::srgb(0.04, 0.06, 0.1)),
            BorderColor::all(Color::srgb(0.25, 0.3, 0.4)),
            Hit::Action(Action::Frame),
            BoxPart::Frame,
        ))
        .with_child((Text::default(), font(15.0), TextColor(INK), Part::FrameText));
    commands
        .spawn((
            Node {
                padding: UiRect::all(px(5)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..node()
            },
            BackgroundColor(Color::srgb(0.07, 0.09, 0.16)),
            BorderColor::all(Color::srgb(0.25, 0.3, 0.4)),
            Hit::Action(Action::Apply),
            BoxPart::Apply,
        ))
        .with_child((
            Text::new(state.locale.text("workbench.apply")),
            font(13.0),
            TextColor(INK),
        ));
    commands
        .spawn((
            Node {
                padding: UiRect::all(px(18)),
                border_radius: BorderRadius::all(px(12)),
                ..node()
            },
            GlobalZIndex(10),
            BackgroundColor(Color::srgb(0.04, 0.055, 0.1)),
            BoxPart::Modal,
        ))
        .with_child((
            Text::new(state.locale.text("workbench.discard_title")),
            font(20.0),
            TextColor(INK),
            Part::ModalTitle,
        ));
    for (part, action) in [
        (BoxPart::Keep, Action::Keep),
        (BoxPart::Discard, Action::Discard),
    ] {
        commands
            .spawn((
                Node {
                    padding: UiRect::all(px(8)),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(8)),
                    ..node()
                },
                GlobalZIndex(11),
                BackgroundColor(Color::srgb(0.07, 0.1, 0.18)),
                BorderColor::all(INK),
                part,
                Hit::Action(action),
            ))
            .with_child((
                Text::new(state.locale.text(key(action))),
                font(15.0),
                TextColor(INK),
            ));
    }
    commands.insert_resource(View {
        image,
        canvas: Rect::default(),
        first_row: 0,
        row_count: 0,
        raster: None,
        last_paint: None,
    });
}

fn place(node: &mut Node, rect: Rect, visible: bool) {
    node.display = if visible {
        Display::Flex
    } else {
        Display::None
    };
    node.left = px(rect.min.x);
    node.top = px(rect.min.y);
    node.width = px(rect.width().max(0.0));
    node.height = px(rect.height().max(0.0));
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_corners(Vec2::new(x, y), Vec2::new(x + w, y + h))
}

pub(super) fn update(
    mut state: ResMut<Workbench>,
    (controls, windows): (Res<input::Controls>, Query<&Window>),
    mut view: ResMut<View>,
    mut boxes: Query<(&BoxPart, &mut Node, Option<&mut BorderColor>)>,
    mut texts: Query<(&Part, &mut Text, &mut Node), Without<BoxPart>>,
    mut scrolls: Query<(&Hit, &ComputedNode, &mut ScrollPosition)>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let w = window.width().max(1.0);
    let h = window.height().max(1.0);
    let compact = w < 1000.0;
    let columns = state.toolbar().len().min(if compact { 4 } else { 8 });
    let toolbar_y = 65.0;
    let toolbar_height = state.toolbar().len().div_ceil(columns) as f32 * 38.0;
    let wave_y = toolbar_y + toolbar_height + 10.0;
    let wave_height = (h * 0.27).clamp(80.0, 240.0);
    let bottom_y = wave_y + wave_height + 66.0;
    let bottom_height = (h - bottom_y - 12.0).max(30.0);
    let full_width = (w - 24.0).max(1.0);
    let list_width = if compact {
        full_width
    } else {
        full_width * 0.48
    };
    let detail_x = if compact { 12.0 } else { 24.0 + list_width };
    let detail_width = if compact {
        full_width
    } else {
        full_width - list_width - 12.0
    };
    let detail_visible = !compact || state.details;
    let list_visible = !compact || !state.details;
    view.canvas = rect(12.0, wave_y, full_width, wave_height);
    view.row_count = (((bottom_height - 36.0) / ROW_HEIGHT).floor().max(1.0) as usize).min(ROWS);
    let selected_index = state.selected_index();
    let length = state.record_count();
    view.first_row = view.first_row.min(length.saturating_sub(view.row_count));
    if selected_index < view.first_row {
        view.first_row = selected_index;
    }
    if selected_index >= view.first_row + view.row_count {
        view.first_row = selected_index + 1 - view.row_count;
    }
    for (part, mut node, border) in &mut boxes {
        let (position, visible, focused) = match *part {
            BoxPart::Toolbar(i) => {
                let width = (full_width - (columns - 1) as f32 * 6.0) / columns as f32;
                (
                    rect(
                        12.0 + (i % columns) as f32 * (width + 6.0),
                        toolbar_y + (i / columns) as f32 * 38.0,
                        width,
                        34.0,
                    ),
                    true,
                    state.focus == Focus::Toolbar(i),
                )
            }
            BoxPart::Canvas => (view.canvas, true, state.focus == Focus::Timeline),
            BoxPart::Tab(i) => {
                let width = (list_width - 18.0) / 4.0;
                (
                    rect(12.0 + i as f32 * (width + 6.0), bottom_y, width, 30.0),
                    true,
                    matches!((i, state.focus), (0, Focus::List) | (1, Focus::Details)),
                )
            }
            BoxPart::Row(i) => (
                rect(
                    12.0,
                    bottom_y + 36.0 + i as f32 * ROW_HEIGHT,
                    list_width,
                    ROW_HEIGHT - 2.0,
                ),
                list_visible && i < view.row_count && view.first_row + i < length,
                selected_index == view.first_row + i,
            ),
            BoxPart::Frame => (
                rect(
                    detail_x,
                    bottom_y + if compact { 36.0 } else { 0.0 },
                    detail_width * 0.65 - 6.0,
                    32.0,
                ),
                detail_visible && !state.is_read_only(),
                state.focus == Focus::Frame,
            ),
            BoxPart::Apply => (
                rect(
                    detail_x + detail_width * 0.65,
                    bottom_y + if compact { 36.0 } else { 0.0 },
                    detail_width * 0.35,
                    32.0,
                ),
                detail_visible && !state.is_read_only(),
                false,
            ),
            BoxPart::Detail => {
                let offset = if state.is_read_only() {
                    if compact { 36.0 } else { 0.0 }
                } else if compact {
                    74.0
                } else {
                    40.0
                };
                (
                    rect(
                        detail_x,
                        bottom_y + offset,
                        detail_width,
                        bottom_height - offset,
                    ),
                    detail_visible,
                    state.focus == Focus::Details,
                )
            }
            BoxPart::Modal => (
                rect(24.0, h * 0.32, (w - 48.0).max(1.0), 160.0),
                state.close_confirm,
                false,
            ),
            BoxPart::Keep => (
                rect(40.0, h * 0.32 + 90.0, (w - 96.0) * 0.5, 54.0),
                state.close_confirm,
                !state.discard_selected,
            ),
            BoxPart::Discard => (
                rect(w * 0.5 + 8.0, h * 0.32 + 90.0, (w - 96.0) * 0.5, 54.0),
                state.close_confirm,
                state.discard_selected,
            ),
        };
        place(&mut node, position, visible);
        if let Some(mut border) = border {
            *border = BorderColor::all(if focused {
                Color::srgb(0.55, 0.82, 0.96)
            } else {
                Color::srgb(0.17, 0.23, 0.34)
            });
        }
    }
    let doc = &state.document;
    for (part, mut text, mut node) in &mut texts {
        let value = match *part {
            Part::Title => {
                place(&mut node, rect(12.0, 8.0, full_width, 26.0), true);
                if state.replay.is_some() {
                    state.locale.text("replay.title").into()
                } else if state.candidates.is_some() {
                    state.locale.text("candidates.title").into()
                } else {
                    format!(
                        "{} · {}",
                        state.locale.text("workbench.title"),
                        state.locale.text(if doc.dirty {
                            "workbench.dirty"
                        } else {
                            "workbench.clean"
                        })
                    )
                }
            }
            Part::Owner => {
                place(&mut node, rect(12.0, 36.0, full_width, 28.0), true);
                match controls.owner {
                    None => state.locale.text("workbench.owner_none").into(),
                    Some(InputSource::Keyboard) => {
                        state.locale.text("workbench.owner_keyboard").into()
                    }
                    Some(InputSource::Pad(pad)) => state.text(
                        "workbench.owner_pad",
                        [(
                            "number",
                            (controls
                                .pads
                                .iter()
                                .position(|known| *known == pad)
                                .unwrap_or(0)
                                + 1)
                            .to_string(),
                        )],
                    ),
                }
            }
            Part::Range => {
                place(
                    &mut node,
                    rect(12.0, wave_y + wave_height + 2.0, full_width, 18.0),
                    true,
                );
                if state.replay.is_some() {
                    format!(
                        "{}–{} · {}",
                        doc.start,
                        doc.start + doc.span,
                        state.locale.text("replay.legend")
                    )
                } else if state.candidates.is_some() {
                    format!(
                        "{}–{} · {}",
                        doc.start,
                        doc.start + doc.span,
                        state.locale.text("candidates.legend")
                    )
                } else {
                    state.text(
                        "workbench.range",
                        [
                            ("start", doc.start.to_string()),
                            ("end", (doc.start + doc.span).to_string()),
                        ],
                    )
                }
            }
            Part::Cursor => {
                place(
                    &mut node,
                    rect(12.0, wave_y + wave_height + 21.0, full_width, 20.0),
                    true,
                );
                state.text(
                    "workbench.cursor",
                    [
                        ("frame", doc.cursor.to_string()),
                        ("end", doc.end.to_string()),
                        ("seconds", format!("{:.6}", doc.cursor as f64 / 48_000.0)),
                    ],
                )
            }
            Part::Audition => {
                place(
                    &mut node,
                    rect(12.0, wave_y + wave_height + 43.0, full_width, 20.0),
                    true,
                );
                state.text(
                    "audition.cursor",
                    [
                        (
                            "state",
                            state.locale.text(state.audition.status_key()).to_owned(),
                        ),
                        (
                            "frame",
                            state
                                .audition
                                .position
                                .map_or_else(|| "—".into(), |frame| frame.to_string()),
                        ),
                        (
                            "target",
                            state
                                .audition
                                .target
                                .map_or_else(|| "—".into(), |frame| frame.to_string()),
                        ),
                    ],
                )
            }
            Part::Row(i) => {
                if let Some(replay) = &state.replay {
                    replay.row(view.first_row + i, state.locale)
                } else if let Some(candidates) = &state.candidates {
                    candidates.row(view.first_row + i, state.locale)
                } else {
                    doc.editor
                        .anchors()
                        .get(view.first_row + i)
                        .map_or_else(String::new, |a| {
                            format!("{}  ·  {}", a.id, a.song_time.frames())
                        })
                }
            }
            Part::ListTab => state.text(
                if state.replay.is_some() {
                    "replay.records"
                } else if state.candidates.is_some() {
                    "candidates.records"
                } else {
                    "workbench.anchors"
                },
                [("count", length.to_string())],
            ),
            Part::FrameText => {
                let mut value = if doc.editing_frame {
                    doc.frame.clone()
                } else {
                    doc.selected()
                        .map_or_else(|| "—".into(), |a| a.song_time.frames().to_string())
                };
                if doc.editing_frame {
                    value.insert(doc.caret, '|');
                }
                format!("{}: {value}", state.locale.text("workbench.frame"))
            }
            Part::DetailText if state.replay.is_some() => {
                let replay = state
                    .replay
                    .as_ref()
                    .expect("Replay details require a recording");
                [
                    state.notice.clone(),
                    replay.details(state.locale),
                    state.locale.text("replay.help").to_owned(),
                    state.locale.text("audition.help").to_owned(),
                ]
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
            }
            Part::DetailText if state.candidates.is_some() => {
                let candidates = state
                    .candidates
                    .as_ref()
                    .expect("Candidate details require a proposal");
                [
                    state.notice.clone(),
                    candidates.details(state.locale),
                    state.locale.text("candidates.help").to_owned(),
                    state.locale.text("audition.help").to_owned(),
                ]
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
            }
            Part::DetailText => {
                let selection = doc.selected().map_or_else(
                    || state.locale.text("workbench.no_anchor").into(),
                    |a| {
                        state.text(
                            "workbench.selected",
                            [
                                ("id", a.id.to_string()),
                                ("frame", a.song_time.frames().to_string()),
                            ],
                        )
                    },
                );
                let cue = doc
                    .sections
                    .iter()
                    .rev()
                    .find(|cue| cue.time.frames() <= doc.cursor)
                    .map_or_else(
                        || state.locale.text("workbench.no_cue").into(),
                        |cue| {
                            state.text(
                                "workbench.cue",
                                [
                                    ("id", cue.id.to_string()),
                                    ("frame", cue.time.frames().to_string()),
                                    ("label", cue.label.clone()),
                                ],
                            )
                        },
                    );
                let cluster = doc
                    .selected()
                    .filter(|anchor| {
                        (doc.start..=doc.start + doc.span).contains(&anchor.song_time.frames())
                    })
                    .map_or_else(String::new, |selected| {
                        let width = full_width.ceil().clamp(1.0, 4096.0) as i64;
                        let column = |frame: i64| {
                            (((frame - doc.start) as f64 / doc.span as f64 * width as f64).round()
                                as i64)
                                .clamp(0, width - 1)
                        };
                        let selected_column = column(selected.song_time.frames());
                        let count = doc
                            .editor
                            .anchors()
                            .iter()
                            .filter(|anchor| {
                                (doc.start..=doc.start + doc.span)
                                    .contains(&anchor.song_time.frames())
                                    && column(anchor.song_time.frames()) == selected_column
                            })
                            .count();
                        if count > 1 {
                            state.text("workbench.cluster", [("count", count.to_string())])
                        } else {
                            String::new()
                        }
                    });
                [
                    state.notice.clone(),
                    selection,
                    cluster,
                    cue,
                    state.text(
                        "workbench.source",
                        [("identity", identity(state.source_hash))],
                    ),
                    state.text(
                        "workbench.destination",
                        [(
                            "path",
                            state
                                .destination
                                .as_ref()
                                .expect("Editor has an export destination")
                                .display()
                                .to_string(),
                        )],
                    ),
                    state.locale.text("workbench.help").into(),
                    state.locale.text("audition.help").into(),
                    state.locale.text("workbench.pad_help").into(),
                    state.locale.text("workbench.scroll").into(),
                ]
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
            }
            Part::ModalTitle => state.locale.text("workbench.discard_title").into(),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (hit, computed, mut scroll) in &mut scrolls {
        if !matches!(hit, Hit::Details) {
            continue;
        }
        let maximum =
            ((computed.content_size.y - computed.size.y) * computed.inverse_scale_factor).max(0.0);
        state.detail_scroll = state.detail_scroll.clamp(0.0, maximum);
        scroll.0.y = state.detail_scroll;
    }
    let doc = &state.document;
    let width = full_width.ceil().clamp(1.0, 4096.0) as u32;
    let height = wave_height.ceil().clamp(1.0, 512.0) as u32;
    let viewport = (width, height, doc.start, doc.span);
    let paint = PaintKey {
        viewport,
        cursor: doc.cursor,
        audition: state.audition.position,
        selected: doc.selected,
        drag: doc.drag,
        revision: doc.revision,
        record_selection: state.is_read_only().then_some(state.selected_index()),
    };
    if view
        .raster
        .as_ref()
        .is_none_or(|raster| raster.key != viewport)
    {
        view.raster = Some(Raster {
            key: viewport,
            pixels: waveform_pixels(&state.wave, doc, width, height),
        });
    }
    if view.last_paint.as_ref() != Some(&paint) {
        if let Some(mut image) = images.get_mut(&view.image) {
            *image = paint_wave(
                &view
                    .raster
                    .as_ref()
                    .expect("Waveform raster is initialized")
                    .pixels,
                doc,
                state.replay.as_ref(),
                state.candidates.as_ref(),
                state.audition.position,
                width,
                height,
            );
        }
        view.last_paint = Some(paint);
    }
}

fn waveform_pixels(wave: &Waveform, doc: &Document, width: u32, height: u32) -> Vec<u8> {
    let mut data = vec![0; (width * height * 4) as usize];
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&[7, 12, 25, 255]);
    }
    let mut line = |x: u32, top: u32, bottom: u32, color: [u8; 4]| {
        for y in top.min(height)..bottom.min(height) {
            let offset = ((y * width + x.min(width - 1)) * 4) as usize;
            data[offset..offset + 4].copy_from_slice(&color);
        }
    };
    for x in 0..width {
        let start = doc.start + doc.span * i64::from(x) / i64::from(width);
        let end =
            doc.start + (doc.span * i64::from(x + 1) + i64::from(width) - 1) / i64::from(width);
        for (channel, peak) in wave.peaks(start, end).into_iter().enumerate() {
            let center = height as f32 * (0.19 + channel as f32 * 0.34);
            let amplitude = height as f32 * 0.145;
            let top = (center - peak[1].clamp(-1.0, 1.0) * amplitude)
                .floor()
                .max(0.0) as u32;
            let bottom = (center - peak[0].clamp(-1.0, 1.0) * amplitude)
                .ceil()
                .max(top as f32 + 1.0) as u32;
            line(x, top, bottom, [145, 176, 197, 255]);
        }
    }
    data
}

fn paint_wave(
    pixels: &[u8],
    doc: &Document,
    replay: Option<&replay::ReplayView>,
    candidates: Option<&candidates::CandidateView>,
    audition: Option<i64>,
    width: u32,
    height: u32,
) -> Image {
    let mut data = pixels.to_vec();
    let mut line = |x: u32, top: u32, bottom: u32, color: [u8; 4]| {
        for y in top.min(height)..bottom.min(height) {
            let offset = ((y * width + x.min(width - 1)) * 4) as usize;
            data[offset..offset + 4].copy_from_slice(&color);
        }
    };
    let column = |frame: i64| {
        (((frame - doc.start) as f64 / doc.span as f64 * f64::from(width)).round() as i64)
            .clamp(0, i64::from(width) - 1) as u32
    };
    let mut counts = vec![0_u32; width as usize];
    for anchor in
        doc.editor.anchors().iter().filter(|anchor| {
            (doc.start..=doc.start + doc.span).contains(&anchor.song_time.frames())
        })
    {
        counts[column(anchor.song_time.frames()) as usize] += 1;
    }
    for (x, count) in counts.iter().enumerate().filter(|(_, count)| **count > 0) {
        let top = height * 73 / 100;
        line(
            x as u32,
            top,
            height * 87 / 100,
            if *count == 1 {
                [211, 222, 236, 255]
            } else {
                [230, 187, 112, 255]
            },
        );
    }
    for cue in doc
        .sections
        .iter()
        .filter(|cue| (doc.start..=doc.start + doc.span).contains(&cue.time.frames()))
    {
        line(
            column(cue.time.frames()),
            height * 90 / 100,
            height,
            [149, 167, 226, 255],
        );
    }
    if let Some(replay) = replay {
        for hit in replay
            .hits()
            .filter(|hit| (doc.start..=doc.start + doc.span).contains(&hit.song_time.frames()))
        {
            let player = hit.player.index() as u32;
            line(
                column(hit.song_time.frames()),
                height * (4 + player * 34) / 100,
                height * (33 + player * 34) / 100,
                if player == 0 {
                    [90, 229, 241, 255]
                } else {
                    [244, 169, 91, 255]
                },
            );
        }
        for (player, frame) in replay
            .selected_hits()
            .into_iter()
            .filter(|(_, frame)| (doc.start..=doc.start + doc.span).contains(frame))
        {
            let top = height * (4 + player.index() as u32 * 34) / 100;
            let bottom = height * (33 + player.index() as u32 * 34) / 100;
            for x in column(frame).saturating_sub(1)..=(column(frame) + 1).min(width - 1) {
                line(x, top, bottom, [250, 244, 210, 255]);
            }
        }
    }
    if let Some(candidates) = candidates {
        for (frame, accepted) in candidates
            .points()
            .filter(|(frame, _)| (doc.start..=doc.start + doc.span).contains(frame))
        {
            line(
                column(frame),
                height * 4 / 100,
                height * 67 / 100,
                if accepted {
                    [90, 229, 241, 255]
                } else {
                    [193, 140, 120, 255]
                },
            );
        }
        for (frame, blocker) in candidates
            .selected_points()
            .into_iter()
            .rev()
            .filter(|(frame, _)| (doc.start..=doc.start + doc.span).contains(frame))
        {
            for x in column(frame).saturating_sub(1)..=(column(frame) + 1).min(width - 1) {
                line(
                    x,
                    height * 4 / 100,
                    height * 67 / 100,
                    if blocker {
                        [230, 187, 112, 255]
                    } else {
                        [250, 244, 210, 255]
                    },
                );
            }
        }
    }
    if let Some(anchor) = doc.selected() {
        let frame = doc
            .drag
            .filter(|(id, _)| *id == anchor.id)
            .map_or(anchor.song_time.frames(), |(_, frame)| frame);
        if (doc.start..=doc.start + doc.span).contains(&frame) {
            line(
                column(frame),
                height * 70 / 100,
                height * 89 / 100,
                [90, 229, 241, 255],
            );
        }
    }
    if let Some(frame) = audition
        && (doc.start..=doc.start + doc.span).contains(&frame)
    {
        line(column(frame), 0, height, [255, 191, 91, 255]);
    }
    if (doc.start..=doc.start + doc.span).contains(&doc.cursor) {
        line(column(doc.cursor), 0, height, [242, 243, 246, 255]);
    }
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

pub(super) fn logical_rect(computed: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    let scale = computed.inverse_scale_factor;
    Rect::from_center_size(transform.translation * scale, computed.size * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn waveform_pixels_and_frame_hits_share_borders_and_wide_scaled_rectangles() {
        let mut app = App::new();
        app.init_resource::<Assets<Font>>()
            .init_resource::<Assets<Image>>()
            .insert_resource(super::super::tests::state());
        install_ui_assets(&mut app).unwrap();
        app.world_mut().run_system_once(setup).unwrap();
        let (image, node) = app
            .world_mut()
            .query::<(&ImageNode, &Node, &BoxPart)>()
            .iter(app.world())
            .find_map(|(image, node, part)| {
                matches!(part, BoxPart::Canvas).then_some((image.clone(), node.clone()))
            })
            .unwrap();
        assert_eq!(image.image_mode, NodeImageMode::Stretch);
        assert_eq!(image.visual_box, bevy::ui::VisualBox::BorderBox);
        assert_eq!(node.border, UiRect::all(px(1)));
        let mut state = super::super::tests::state();
        state.document.cursor = 12_000;
        for (logical_width, scale) in [(1256.0_f32, 1.0), (1256.0, 1.25), (8168.0, 2.0)] {
            let mut computed = ComputedNode {
                size: Vec2::new(logical_width, 216.0) * scale,
                inverse_scale_factor: 1.0 / scale,
                ..default()
            };
            computed.border.min_inset = Vec2::splat(scale);
            computed.border.max_inset = Vec2::splat(scale);
            let transform = UiGlobalTransform::from(bevy::math::Affine2::from_translation(
                (Vec2::new(12.0, 113.0) + computed.size / scale * 0.5) * scale,
            ));
            let hit = logical_rect(&computed, &transform);
            let render_box = computed.border_box();
            let displayed = Rect::from_corners(
                (render_box.min + transform.translation) / scale,
                (render_box.max + transform.translation) / scale,
            );
            assert_eq!(hit, displayed);
            let raster_width = logical_width.min(4096.0) as u32;
            let pixels = waveform_pixels(&state.wave, &state.document, raster_width, 216);
            let raster = paint_wave(
                &pixels,
                &state.document,
                None,
                None,
                None,
                raster_width,
                216,
            );
            let column = raster_width / 4;
            let bytes = raster.data.as_ref().unwrap();
            assert_eq!(
                &bytes[(column * 4) as usize..(column * 4 + 4) as usize],
                &[242, 243, 246, 255]
            );
            let displayed_x =
                displayed.min.x + displayed.width() * column as f32 / raster_width as f32;
            assert_eq!(
                state.document.at_pixel(displayed_x, hit.min.x, hit.width()),
                12_000
            );
            assert_eq!(
                state
                    .document
                    .at_pixel(displayed.min.x, hit.min.x, hit.width()),
                0
            );
            assert_eq!(
                state
                    .document
                    .at_pixel(displayed.max.x, hit.min.x, hit.width()),
                48_000
            );
        }
    }

    #[test]
    fn replay_markers_virtual_rows_and_small_window_details_remain_read_only() {
        let mut state = super::super::tests::state();
        state.destination = None;
        state.replay = Some(replay::fixture());
        state.document = Document::from_anchors(
            4_800,
            vec![Anchor {
                id: 7,
                song_time: SongTime::from_frames(1_200),
            }],
            Vec::new(),
        )
        .unwrap();
        state.select(2);
        let pixels = waveform_pixels(&state.wave, &state.document, 480, 100);
        let raster = paint_wave(
            &pixels,
            &state.document,
            state.replay.as_ref(),
            None,
            None,
            480,
            100,
        );
        let bytes = raster.data.as_ref().unwrap();
        let pixel = |x: usize, y: usize| &bytes[(y * 480 + x) * 4..(y * 480 + x) * 4 + 4];
        assert_eq!(pixel(120, 10), &[90, 229, 241, 255]);
        assert_eq!(pixel(120, 40), &[244, 169, 91, 255]);
        assert_eq!(pixel(120, 75), &[211, 222, 236, 255]);
        state.select(9);
        let raster = paint_wave(
            &pixels,
            &state.document,
            state.replay.as_ref(),
            None,
            None,
            480,
            100,
        );
        let bytes = raster.data.as_ref().unwrap();
        for y in [10, 40] {
            assert_eq!(
                &bytes[(y * 480 + 119) * 4..(y * 480 + 119) * 4 + 4],
                &[250, 244, 210, 255]
            );
        }
        state.select(10);
        let mut app = App::new();
        app.init_resource::<Assets<Font>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<input::Controls>()
            .insert_resource(state);
        install_ui_assets(&mut app).unwrap();
        let window = app
            .world_mut()
            .spawn(Window {
                resolution: (640, 480).into(),
                ..default()
            })
            .id();
        app.world_mut().run_system_once(setup).unwrap();
        app.world_mut()
            .resource_mut::<Workbench>()
            .error("Audition failure fixture".into());
        for scale in [1.0, 1.25, 2.0] {
            app.world_mut()
                .get_mut::<Window>(window)
                .unwrap()
                .resolution
                .set_scale_factor_override(Some(scale));
            for details in [false, true] {
                app.world_mut().resource_mut::<Workbench>().details = details;
                app.world_mut().run_system_once(update).unwrap();
                let view = app.world().resource::<View>();
                assert!(view.first_row <= 10 && view.first_row + view.row_count > 10);
                let mut toolbars = 0;
                for (part, node) in app
                    .world_mut()
                    .query::<(&BoxPart, &Node)>()
                    .iter(app.world())
                {
                    match part {
                        BoxPart::Toolbar(_) => {
                            toolbars += 1;
                            assert_eq!(node.display, Display::Flex);
                        }
                        BoxPart::Frame | BoxPart::Apply => assert_eq!(node.display, Display::None),
                        BoxPart::Detail => {
                            assert_eq!(node.display == Display::Flex, details);
                            assert_eq!(node.overflow, Overflow::scroll_y());
                        }
                        _ => {}
                    }
                }
                assert_eq!(toolbars, 6);
                let detail = app
                    .world_mut()
                    .query::<(&Part, &Text)>()
                    .iter(app.world())
                    .find_map(|(part, text)| matches!(part, Part::DetailText).then_some(&text.0))
                    .unwrap();
                assert!(detail.contains("\"input_fact_index\": 4"));
                assert!(detail.contains("\"pending_anchor_count\": 0"));
                assert!(detail.contains("Audition failure fixture"));
                assert!(detail.contains("no device timestamps"));
            }
        }
        let mut state = app.world_mut().resource_mut::<Workbench>();
        for cursor in [i64::MIN, i64::MAX] {
            state.document.cursor = cursor;
            state.document.keep_cursor_visible();
            state.document.zoom(true, 10.0);
            assert_eq!(state.document.cursor, cursor);
            assert!((0..=state.document.end - state.document.span).contains(&state.document.start));
        }
    }

    #[test]
    fn candidate_rows_and_same_pixel_points_remain_distinct_and_read_only() {
        let mut state = super::super::tests::state();
        state.destination = None;
        state.candidates = Some(candidates::fixture());
        state.document = Document::from_anchors(
            4800,
            vec![Anchor {
                id: 99,
                song_time: SongTime::from_frames(333),
            }],
            Vec::new(),
        )
        .unwrap();
        state.select(2);
        let pixels = waveform_pixels(&state.wave, &state.document, 4, 100);
        let image = paint_wave(
            &pixels,
            &state.document,
            None,
            state.candidates.as_ref(),
            None,
            4,
            100,
        );
        assert_eq!(
            &image.data.as_ref().unwrap()[(10 * 4) * 4..(10 * 4) * 4 + 4],
            &[250, 244, 210, 255]
        );
        assert_eq!(state.document.cursor, 1000);
        state.select(3);
        assert_eq!(state.document.cursor, 1200);
        assert_eq!(state.selected_index(), 3);
        assert_eq!(state.document.selected, None);
        assert_eq!(state.document.editor.anchors(), state.document.original);
        let mut app = App::new();
        app.init_resource::<Assets<Font>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<input::Controls>()
            .insert_resource(state);
        install_ui_assets(&mut app).unwrap();
        app.world_mut().spawn(Window {
            resolution: (640, 480).into(),
            ..default()
        });
        app.world_mut().run_system_once(setup).unwrap();
        app.world_mut()
            .resource_mut::<Workbench>()
            .error("Audition failure fixture".into());
        for details in [false, true] {
            app.world_mut().resource_mut::<Workbench>().details = details;
            app.world_mut().run_system_once(update).unwrap();
            let view = app.world().resource::<View>();
            assert!(view.first_row <= 3 && view.first_row + view.row_count > 3);
            let mut toolbars = 0;
            for (part, node) in app
                .world_mut()
                .query::<(&BoxPart, &Node)>()
                .iter(app.world())
            {
                match part {
                    BoxPart::Toolbar(_) => toolbars += 1,
                    BoxPart::Frame | BoxPart::Apply => assert_eq!(node.display, Display::None),
                    BoxPart::Detail => assert_eq!(node.display == Display::Flex, details),
                    _ => {}
                }
            }
            assert_eq!(toolbars, 6);
            let detail = app
                .world_mut()
                .query::<(&Part, &Text)>()
                .iter(app.world())
                .find_map(|(part, text)| matches!(part, Part::DetailText).then_some(&text.0))
                .unwrap();
            assert!(detail.contains("\"onset_index\": 3"));
            assert!(detail.contains("\"id\": 4"));
            assert!(detail.contains("not_assessed"));
            assert!(detail.contains("Audition failure fixture"));
        }
    }

    #[test]
    fn only_details_scroll_is_updated_among_the_real_setup_nodes() {
        let mut app = App::new();
        app.init_resource::<Assets<Font>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<input::Controls>()
            .insert_resource(super::super::tests::state());
        install_ui_assets(&mut app).unwrap();
        app.world_mut().spawn(Window {
            resolution: (640, 480).into(),
            ..default()
        });
        app.world_mut().run_system_once(setup).unwrap();
        let detail = app
            .world_mut()
            .query::<(Entity, &Hit)>()
            .iter(app.world())
            .find_map(|(entity, hit)| matches!(hit, Hit::Details).then_some(entity))
            .unwrap();
        let mut other_nodes = 0;
        for (hit, mut scroll) in app
            .world_mut()
            .query::<(&Hit, &mut ScrollPosition)>()
            .iter_mut(app.world_mut())
        {
            if !matches!(hit, Hit::Details) {
                scroll.0 = Vec2::new(3.0, 17.0);
                other_nodes += 1;
            }
        }
        assert!(
            other_nodes > ROWS,
            "The real buttons and rows have required ScrollPosition components"
        );
        for scale in [1.0_f32, 1.25, 2.0] {
            {
                let mut computed = app.world_mut().get_mut::<ComputedNode>(detail).unwrap();
                computed.size = Vec2::new(616.0, 72.0) * scale;
                computed.content_size = Vec2::new(616.0, 600.0) * scale;
                computed.inverse_scale_factor = 1.0 / scale;
            }
            for (requested, expected) in [(48.0, 48.0), (10_000.0, 528.0), (-10.0, 0.0)] {
                app.world_mut().resource_mut::<Workbench>().detail_scroll = requested;
                app.world_mut().run_system_once(update).unwrap();
                assert_eq!(app.world().resource::<Workbench>().detail_scroll, expected);
                assert_eq!(
                    app.world().get::<ScrollPosition>(detail).unwrap().0.y,
                    expected
                );
                for (hit, scroll) in app
                    .world_mut()
                    .query::<(&Hit, &ScrollPosition)>()
                    .iter(app.world())
                {
                    if !matches!(hit, Hit::Details) {
                        assert_eq!(scroll.0, Vec2::new(3.0, 17.0));
                    }
                }
            }
        }
    }
}
