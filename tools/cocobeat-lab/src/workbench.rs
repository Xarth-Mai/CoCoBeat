//! A package workbench for Anchor editing and read-only evidence diagnostics

use bevy::{prelude::*, window::WindowResizeConstraints};
use cocobeat_editor::AnchorEditor;
use cocobeat_media::ValidatedPackage;
use cocobeat_runtime::{InputSource, Locale, Message, install_ui_assets};
use cocobeat_schema::{Anchor, MAX_CANONICAL_FRAMES, SectionCue, SongTime};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    thread::JoinHandle,
};

mod audition;
mod candidates;
mod input;
mod label_text;
mod labels;
mod replay;
mod ui;

const BIN_FRAMES: i64 = 64;

pub enum Mode<'a> {
    Edit(&'a Path),
    Labels(&'a Path),
    Replay(&'a Path, Option<&'a Path>),
    Candidates(&'a Path),
    CalibratedCandidates(&'a Path, &'a Path, &'a Path, &'a Path),
    InferredCandidates(&'a Path, &'a Path, &'a Path, &'a Path, &'a Path),
    NativeBeats(&'a Path),
    StructureFeatures(usize),
}

pub fn run(source: &Path, mode: Mode<'_>, locale: Locale) -> Result<(), String> {
    let labeling = matches!(mode, Mode::Labels(_));
    let destination = if let Mode::Labels(path) = mode {
        Some(crate::labels_cli::outside_package(source, path)?)
    } else if let Mode::Edit(destination) = mode {
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if std::fs::canonicalize(parent)
            .map_err(|error| error.to_string())?
            .starts_with(std::fs::canonicalize(source).map_err(|error| error.to_string())?)
        {
            return Err("Workbench destination must be outside the source package".into());
        }
        match std::fs::symlink_metadata(destination) {
            Ok(_) => return Err("Workbench destination must be a new package path".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        Some(destination.to_owned())
    } else {
        None
    };
    let mut wave = Waveform::default();
    let mut pcm = Vec::new();
    let mut consume = |frames: &[[f32; 2]]| {
        wave.push(frames)?;
        // ponytail: validated static PCM is capped at 230 MB; stream if songs exceed ten minutes
        pcm.try_reserve(frames.len())
            .map_err(|error| error.to_string())?;
        pcm.extend(
            frames
                .iter()
                .map(|&[left, right]| kira::Frame::new(left, right)),
        );
        Ok(())
    };
    let (package, structure) = if let Mode::StructureFeatures(channel) = mode {
        let (package, evidence) =
            cocobeat_media::read_structure_features_package(source, channel, &mut consume)?;
        (package, Some(evidence))
    } else {
        (cocobeat_media::read_package(source, &mut consume)?, None)
    };
    let mut document = if labeling {
        Document::from_anchors(
            package.manifest.canonical_frames as i64,
            Vec::new(),
            Vec::new(),
        )?
    } else {
        Document::new(&package)?
    };
    let labels =
        labeling.then(|| labels::LabelView::new(crate::labels::Source::from_package(&package)));
    let replay = if let Mode::Replay(path, timing) = mode {
        let mut replay = replay::ReplayView::load(&package, path, timing)?;
        document.cursor = replay.select(0).unwrap_or(0);
        document.selected = replay.selected_anchor();
        Some(replay)
    } else {
        None
    };
    let mut candidates = match mode {
        Mode::Candidates(path) => Some(candidates::CandidateView::load(&package, path)?),
        Mode::CalibratedCandidates(proposal, input, report, choice) => {
            Some(candidates::CandidateView::load_calibrated(
                source, &package, proposal, input, report, choice,
            )?)
        }
        Mode::InferredCandidates(proposal, evidence, input, report, choice) => {
            Some(candidates::CandidateView::load_inferred(
                source, &package, proposal, evidence, input, report, choice,
            )?)
        }
        Mode::NativeBeats(path) => Some(candidates::CandidateView::load_native(&package, path)?),
        Mode::StructureFeatures(_) => Some(candidates::CandidateView::from_structure(
            &package,
            structure.expect("Structure mode returned features from the same package read"),
        )),
        _ => None,
    };
    if let Some(candidates) = &mut candidates {
        document.cursor = candidates.select(0).unwrap_or(0);
        document.selected = None;
    }
    let mut app = App::new();
    let plugins = DefaultPlugins.set(WindowPlugin {
        close_when_requested: false,
        primary_window: Some(Window {
            title: locale
                .text(if labeling {
                    "labels.title"
                } else if replay.is_some() {
                    "replay.title"
                } else if let Some(candidates) = &candidates {
                    if candidates.is_structure() {
                        "structure.title"
                    } else if candidates.is_native() {
                        "native_beats.title"
                    } else {
                        "candidates.title"
                    }
                } else {
                    "workbench.title"
                })
                .into(),
            name: Some("cocobeat-workbench".into()),
            resolution: (1280, 800).into(),
            resize_constraints: WindowResizeConstraints {
                min_width: 640.0,
                min_height: 480.0,
                ..default()
            },
            ..default()
        }),
        ..default()
    });
    app.add_plugins(if labeling {
        plugins.disable::<bevy::input_focus::InputDispatchPlugin>()
    } else {
        plugins
    });
    if labeling {
        app.add_message::<bevy_picking::events::Pointer<bevy_picking::events::Release>>()
            .add_plugins(bevy_ui_widgets::EditableTextInputPlugin);
    }
    install_ui_assets(&mut app)?;
    app.insert_non_send(audition::Output::new(pcm))
        .insert_resource(Workbench {
            source: source.to_owned(),
            destination,
            replay,
            candidates,
            labels,
            pending_label_action: None,
            source_hash: package.manifest.package_hash,
            locale,
            document,
            wave,
            focus: Focus::Timeline,
            details: false,
            detail_scroll: 0.0,
            close_confirm: false,
            discard_selected: false,
            notice: String::new(),
            saving: None,
            audition: audition::Audition::default(),
        })
        .init_resource::<input::Controls>()
        .insert_resource(ClearColor(Color::srgb(0.012, 0.017, 0.042)))
        .add_systems(Startup, ui::setup);
    if labeling {
        app.add_systems(Update, (poll_save, audition::update, ui::update).chain())
            .add_systems(
                PreUpdate,
                (input::capture, label_text::bridge)
                    .chain()
                    .after(bevy::input::InputSystems)
                    .before(bevy::input_focus::InputFocusSystems::Dispatch)
                    .before(bevy_ui_widgets::ImeSystems::HandleEvents),
            )
            .add_systems(
                PreUpdate,
                bevy::input_focus::dispatch_focused_input::<bevy::input::keyboard::KeyboardInput>
                    .in_set(bevy::input_focus::InputFocusSystems::Dispatch)
                    .after(label_text::bridge),
            )
            .add_systems(
                PostUpdate,
                label_text::sync.after(bevy::text::EditableTextSystems),
            )
            .add_systems(
                PostUpdate,
                ui::keep_label_field_visible
                    .after(bevy::ui::UiSystems::Layout)
                    .after(label_text::sync),
            );
    } else {
        app.add_systems(
            Update,
            (poll_save, input::capture, audition::update, ui::update).chain(),
        );
    }
    match app.run() {
        AppExit::Success => Ok(()),
        error => Err(format!("Workbench exited: {error:?}")),
    }
}

#[derive(Default)]
struct Waveform {
    bins: Vec<[[f32; 2]; 2]>,
    frames: i64,
}

impl Waveform {
    fn push(&mut self, frames: &[[f32; 2]]) -> Result<(), String> {
        if frames.len() as u64 > MAX_CANONICAL_FRAMES - self.frames as u64 {
            return Err("Waveform exceeds the canonical song limit".into());
        }
        for frame in frames {
            if self.frames % BIN_FRAMES == 0 {
                self.bins
                    .try_reserve(1)
                    .map_err(|error| error.to_string())?;
                self.bins.push([[f32::INFINITY, f32::NEG_INFINITY]; 2]);
            }
            let bin = self.bins.last_mut().expect("A frame has a peak bin");
            for (range, sample) in bin.iter_mut().zip(frame) {
                range[0] = range[0].min(*sample);
                range[1] = range[1].max(*sample);
            }
            self.frames += 1;
        }
        Ok(())
    }

    fn peaks(&self, start: i64, end: i64) -> [[f32; 2]; 2] {
        if end <= start || start >= self.frames {
            return [[0.0; 2]; 2];
        }
        let first = (start.clamp(0, self.frames) / BIN_FRAMES) as usize;
        let last = ((end.clamp(0, self.frames) + BIN_FRAMES - 1) / BIN_FRAMES) as usize;
        if first == last {
            return [[0.0; 2]; 2];
        }
        let mut result = [[f32::INFINITY, f32::NEG_INFINITY]; 2];
        for bin in &self.bins[first.min(self.bins.len())..last.min(self.bins.len())] {
            for (range, samples) in result.iter_mut().zip(bin) {
                range[0] = range[0].min(samples[0]);
                range[1] = range[1].max(samples[1]);
            }
        }
        result
    }
}

struct Document {
    editor: AnchorEditor,
    original: Vec<Anchor>,
    sections: Vec<SectionCue>,
    end: i64,
    cursor: i64,
    start: i64,
    span: i64,
    selected: Option<u64>,
    drag: Option<(u64, i64)>,
    frame: String,
    caret: usize,
    editing_frame: bool,
    dirty: bool,
    revision: u64,
}

impl Document {
    fn new(package: &ValidatedPackage) -> Result<Self, String> {
        Self::from_anchors(
            package.manifest.canonical_frames as i64,
            package.chart.anchors.clone(),
            package.chart.sections.clone(),
        )
    }

    fn from_anchors(
        end: i64,
        original: Vec<Anchor>,
        sections: Vec<SectionCue>,
    ) -> Result<Self, String> {
        Ok(Self {
            editor: AnchorEditor::new(SongTime::from_frames(end), original.clone())?,
            selected: original.first().map(|anchor| anchor.id),
            original,
            sections,
            end,
            cursor: 0,
            start: 0,
            span: end,
            drag: None,
            frame: String::new(),
            caret: 0,
            editing_frame: false,
            dirty: false,
            revision: 0,
        })
    }

    fn selected(&self) -> Option<Anchor> {
        self.editor
            .anchors()
            .iter()
            .find(|a| Some(a.id) == self.selected)
            .copied()
    }

    fn select(&mut self, index: usize) {
        if let Some(anchor) = self.editor.anchors().get(index).copied() {
            self.selected = Some(anchor.id);
            self.cursor = anchor.song_time.frames();
            self.editing_frame = false;
            self.keep_cursor_visible();
        }
    }

    fn browse(&mut self, step: i32) {
        let anchors = self.editor.anchors();
        if anchors.is_empty() {
            return;
        }
        let index = anchors
            .iter()
            .position(|a| Some(a.id) == self.selected)
            .unwrap_or(0);
        self.select((index as i64 + i64::from(step)).clamp(0, anchors.len() as i64 - 1) as usize);
    }

    fn changed(&mut self) {
        self.dirty = self.editor.anchors() != self.original;
        if self.selected().is_none() {
            self.selected = self.editor.anchors().first().map(|anchor| anchor.id);
        }
        self.editing_frame = false;
        self.revision += 1;
    }

    fn add(&mut self) -> Result<(), String> {
        let ids: BTreeSet<_> = self
            .editor
            .anchors()
            .iter()
            .map(|anchor| anchor.id)
            .collect();
        let id = (0..)
            .find(|id| !ids.contains(id))
            .expect("Bounded Anchor IDs leave a free ID");
        self.editor.add(id, SongTime::from_frames(self.cursor))?;
        self.selected = Some(id);
        self.changed();
        Ok(())
    }

    fn move_selected(&mut self, frame: i64) -> Result<(), String> {
        let id = self.selected.ok_or("No Anchor selected")?;
        self.editor.move_to(id, SongTime::from_frames(frame))?;
        self.cursor = frame;
        self.changed();
        self.keep_cursor_visible();
        Ok(())
    }

    fn finish_drag(&mut self) -> Result<(), String> {
        if let Some((id, frame)) = self.drag.take() {
            self.editor.move_to(id, SongTime::from_frames(frame))?;
            self.changed();
        }
        Ok(())
    }

    fn begin_frame(&mut self) {
        if let Some(anchor) = self.selected() {
            self.frame = anchor.song_time.frames().to_string();
            self.caret = self.frame.len();
            self.editing_frame = true;
        }
    }

    fn cancel(&mut self) {
        self.drag = None;
        self.editing_frame = false;
    }

    fn keep_cursor_visible(&mut self) {
        if self.cursor < self.start || self.cursor > self.start + self.span {
            self.start = self
                .cursor
                .saturating_sub(self.span / 2)
                .clamp(0, self.end - self.span);
        }
    }

    fn zoom(&mut self, inward: bool, width: f32) {
        let minimum = ((width.ceil() as i64).max(1) * BIN_FRAMES).min(self.end);
        let span = if inward {
            self.span / 2
        } else {
            self.span.saturating_mul(2)
        };
        self.span = span.clamp(minimum, self.end);
        self.start = self
            .cursor
            .saturating_sub(self.span / 2)
            .clamp(0, self.end - self.span);
    }

    fn pan(&mut self, direction: i64) {
        self.start =
            (self.start + direction * (self.span / 8).max(1)).clamp(0, self.end - self.span);
    }

    fn at_pixel(&self, x: f32, left: f32, width: f32) -> i64 {
        let ratio = (f64::from(x - left) / f64::from(width.max(1.0))).clamp(0.0, 1.0);
        (self.start + (ratio * self.span as f64).round() as i64).clamp(0, self.end)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Focus {
    Toolbar(usize),
    Timeline,
    List,
    Frame,
    Details,
    Label(label_text::Field),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    PlayPause,
    Stop,
    Seek,
    Undo,
    Redo,
    Add,
    Remove,
    Export,
    Back,
    ZoomIn,
    ZoomOut,
    Previous,
    Next,
    List,
    Details,
    Frame,
    Apply,
    Keep,
    Discard,
    LabelEdit,
    LabelCancel,
}

const TOOLBAR: [Action; 11] = [
    Action::PlayPause,
    Action::Stop,
    Action::Seek,
    Action::Undo,
    Action::Redo,
    Action::Add,
    Action::Remove,
    Action::ZoomIn,
    Action::ZoomOut,
    Action::Export,
    Action::Back,
];

enum SaveOutcome {
    Package { hash: [u8; 32], count: usize },
    Labels { destination: PathBuf, count: usize },
}
struct SaveWorker {
    receiver: Mutex<mpsc::Receiver<Result<SaveOutcome, String>>>,
    worker: JoinHandle<()>,
}
#[derive(Resource)]
struct Workbench {
    source: PathBuf,
    destination: Option<PathBuf>,
    replay: Option<replay::ReplayView>,
    candidates: Option<candidates::CandidateView>,
    labels: Option<labels::LabelView>,
    pending_label_action: Option<Action>,
    source_hash: [u8; 32],
    locale: Locale,
    document: Document,
    wave: Waveform,
    focus: Focus,
    details: bool,
    detail_scroll: f32,
    close_confirm: bool,
    discard_selected: bool,
    notice: String,
    saving: Option<SaveWorker>,
    audition: audition::Audition,
}

impl Workbench {
    fn is_read_only(&self) -> bool {
        self.replay.is_some() || self.candidates.is_some()
    }

    fn record_count(&self) -> usize {
        if let Some(labels) = &self.labels {
            labels.document.labels.len()
        } else if let Some(replay) = &self.replay {
            replay.len()
        } else if let Some(candidates) = &self.candidates {
            candidates.len()
        } else {
            self.document.editor.anchors().len()
        }
    }

    fn selected_index(&self) -> usize {
        if let Some(labels) = &self.labels {
            labels.selected
        } else if let Some(replay) = &self.replay {
            replay.selected
        } else if let Some(candidates) = &self.candidates {
            candidates.selected
        } else {
            self.document
                .editor
                .anchors()
                .iter()
                .position(|anchor| Some(anchor.id) == self.document.selected)
                .unwrap_or(0)
        }
    }

    fn toolbar(&self) -> &[Action] {
        if self.labels.is_some() {
            &[
                Action::PlayPause,
                Action::Stop,
                Action::Seek,
                Action::Add,
                Action::LabelEdit,
                Action::Remove,
                Action::ZoomIn,
                Action::ZoomOut,
                Action::Export,
                Action::Back,
            ]
        } else if self.is_read_only() {
            &[
                Action::PlayPause,
                Action::Stop,
                Action::Seek,
                Action::ZoomIn,
                Action::ZoomOut,
                Action::Back,
            ]
        } else {
            &TOOLBAR
        }
    }

    fn select(&mut self, index: usize) {
        if let Some(labels) = &mut self.labels {
            labels.select(index);
            if let Some(label) = labels.document.labels.get(labels.selected) {
                self.document.cursor = match label.location {
                    crate::labels::Location::Point { frame } => frame,
                    crate::labels::Location::Interval { start_frame, .. } => start_frame,
                };
                self.document.keep_cursor_visible();
            }
            self.detail_scroll = 0.0;
        } else if let Some(replay) = &mut self.replay {
            if let Some(frame) = replay.select(index) {
                self.document.cursor = frame;
                self.document.keep_cursor_visible();
            }
            self.document.selected = replay.selected_anchor();
            self.detail_scroll = 0.0;
        } else if let Some(candidates) = &mut self.candidates {
            if let Some(frame) = candidates.select(index) {
                self.document.cursor = frame;
                self.document.keep_cursor_visible();
            }
            self.document.selected = None;
            self.detail_scroll = 0.0;
        } else {
            self.document.select(index);
        }
    }

    fn browse(&mut self, step: i32) {
        if let Some(labels) = &mut self.labels {
            labels.browse(step);
            let selected = labels.selected;
            self.select(selected);
        } else if self.is_read_only() {
            self.select(
                (self.selected_index() as i64 + i64::from(step))
                    .clamp(0, self.record_count().saturating_sub(1) as i64)
                    as usize,
            );
        } else {
            self.document.browse(step);
        }
    }

    fn text(
        &self,
        key: &'static str,
        args: impl IntoIterator<Item = (&'static str, String)>,
    ) -> String {
        Message::with(key, args).render(self.locale)
    }

    fn error(&mut self, reason: String) {
        self.notice = self.text("workbench.error", [("reason", reason)]);
        self.details = true;
        self.detail_scroll = 0.0;
        self.focus = Focus::Details;
    }

    fn refresh_label_dirty(&mut self) {
        if let Some(labels) = &self.labels {
            self.document.dirty = labels.dirty();
        }
    }
    fn finish_label_action(&mut self) {
        match self.pending_label_action.take() {
            Some(Action::Apply) => {
                let result = self.labels.as_mut().unwrap().apply();
                match result {
                    Ok(index) => {
                        self.select(index);
                        self.document.revision += 1;
                        self.notice.clear();
                        self.focus = Focus::Details;
                    }
                    Err(reason) => self.error(reason),
                }
            }
            Some(Action::Export) => self.save(),
            _ => {}
        }
        self.refresh_label_dirty();
    }
    fn close(&mut self, exit: &mut MessageWriter<AppExit>) {
        self.refresh_label_dirty();
        if self.saving.is_some() {
            return;
        }
        self.audition.stop();
        self.document.cancel();
        if self.document.dirty {
            self.close_confirm = true;
            self.discard_selected = false;
        } else {
            exit.write(AppExit::Success);
        }
    }

    fn action(
        &mut self,
        action: Action,
        keyboard: bool,
        width: f32,
        exit: &mut MessageWriter<AppExit>,
    ) {
        if self.saving.is_some() {
            return;
        }
        if (self.is_read_only() || !keyboard)
            && matches!(
                action,
                Action::Undo
                    | Action::Redo
                    | Action::Add
                    | Action::Remove
                    | Action::Export
                    | Action::Frame
                    | Action::Apply
            )
        {
            if !self.is_read_only() {
                self.notice = self.locale.text("workbench.keyboard_required").into();
                self.details = true;
                self.detail_scroll = 0.0;
            }
            return;
        }
        if self.labels.is_some()
            && matches!(
                action,
                Action::Add
                    | Action::LabelEdit
                    | Action::LabelCancel
                    | Action::Remove
                    | Action::Apply
                    | Action::Export
            )
        {
            if !keyboard {
                self.error(self.locale.text("workbench.keyboard_required").into());
                return;
            }
            let result = match action {
                Action::Add => self
                    .labels
                    .as_mut()
                    .unwrap()
                    .begin_add(self.document.cursor),
                Action::LabelEdit => self.labels.as_mut().unwrap().begin_edit(),
                Action::Remove => self.labels.as_mut().unwrap().remove_selected(),
                Action::LabelCancel => {
                    self.labels.as_mut().unwrap().cancel();
                    Ok(())
                }
                Action::Apply | Action::Export => {
                    self.pending_label_action = Some(action);
                    Ok(())
                }
                _ => unreachable!(),
            };
            if let Err(reason) = result {
                self.error(reason);
            } else {
                self.details = true;
                self.focus = if self.labels.as_ref().unwrap().draft.is_some() {
                    Focus::Label(label_text::Field::Reason)
                } else {
                    Focus::Details
                };
            }
            self.document.revision += 1;
            self.refresh_label_dirty();
            return;
        }
        let result = match action {
            Action::PlayPause => self
                .audition
                .toggle(self.document.cursor, self.document.end),
            Action::Stop => {
                self.audition.stop();
                Ok(())
            }
            Action::Seek => self.audition.seek(self.document.cursor, self.document.end),
            Action::Undo => self
                .document
                .editor
                .undo()
                .map(|()| self.document.changed()),
            Action::Redo => self
                .document
                .editor
                .redo()
                .map(|()| self.document.changed()),
            Action::Add if self.document.cursor == self.document.end => {
                Err(self.locale.text("workbench.at_end").into())
            }
            Action::Add => self.document.add(),
            Action::Remove => self
                .document
                .selected
                .ok_or_else(|| self.locale.text("workbench.no_anchor").to_owned())
                .and_then(|id| self.document.editor.remove(id))
                .map(|()| self.document.changed()),
            Action::Frame => {
                if !self.document.editing_frame {
                    self.document.begin_frame();
                }
                self.focus = Focus::Frame;
                self.details = true;
                Ok(())
            }
            Action::Apply => {
                if !self.document.editing_frame {
                    self.document.begin_frame();
                }
                let frame = self
                    .document
                    .frame
                    .parse::<i64>()
                    .ok()
                    .filter(|frame| (0..self.document.end).contains(frame));
                match frame {
                    Some(frame) => self.document.move_selected(frame),
                    None => Err(self.text(
                        "workbench.invalid_frame",
                        [("last", (self.document.end - 1).to_string())],
                    )),
                }
            }
            Action::ZoomIn | Action::ZoomOut => {
                self.document.zoom(action == Action::ZoomIn, width);
                Ok(())
            }
            Action::Previous | Action::Next => {
                self.browse(if action == Action::Previous { -1 } else { 1 });
                Ok(())
            }
            Action::List => {
                self.details = false;
                self.focus = Focus::List;
                Ok(())
            }
            Action::Details => {
                self.details = true;
                self.focus = Focus::Details;
                Ok(())
            }
            Action::Back => {
                self.close(exit);
                Ok(())
            }
            Action::Keep => {
                self.close_confirm = false;
                Ok(())
            }
            Action::Discard => {
                self.audition.stop();
                exit.write(AppExit::Success);
                Ok(())
            }
            Action::Export => {
                self.save();
                Ok(())
            }
            Action::LabelEdit | Action::LabelCancel => Ok(()),
        };
        if let Err(reason) = result {
            self.error(reason);
        } else if matches!(
            action,
            Action::PlayPause
                | Action::Stop
                | Action::Seek
                | Action::Undo
                | Action::Redo
                | Action::Add
                | Action::Remove
                | Action::Apply
        ) {
            self.notice.clear();
        }
    }

    fn save(&mut self) {
        let Some(destination) = self.destination.clone() else {
            return;
        };
        if self.saving.is_some() {
            return;
        }
        if self
            .labels
            .as_ref()
            .is_some_and(|labels| labels.draft.is_some())
        {
            self.error(self.locale.text("labels.draft").into());
            return;
        }
        self.audition.pause();
        self.document.cancel();
        let source = self.source.clone();
        let hash = self.source_hash;
        let anchors = self.document.editor.anchors().to_vec();
        let original_label_source = self.labels.as_ref().map(|labels| labels.source().clone());
        let labels = self.labels.as_ref().map(|labels| labels.document.clone());
        let (send, receive) = mpsc::channel();
        match std::thread::Builder::new()
            .name("workbench-save".into())
            .spawn(move || {
                let result = if let Some(document) = labels {
                    (|| {
                        let destination =
                            crate::labels_cli::outside_package(&source, &destination)?;
                        let expected = original_label_source.unwrap();
                        crate::labels_cli::require_fresh_source(&source, &expected)?;
                        crate::labels::save(&destination, &expected, &document)?;
                        Ok(SaveOutcome::Labels {
                            destination,
                            count: document.labels.len(),
                        })
                    })()
                } else {
                    cocobeat_media::export_anchors(source, hash, &anchors, destination).map(
                        |package| SaveOutcome::Package {
                            hash: package.manifest.package_hash,
                            count: package.chart.anchors.len(),
                        },
                    )
                };
                let _ = send.send(result);
            }) {
            Ok(worker) => {
                self.saving = Some(SaveWorker {
                    receiver: Mutex::new(receive),
                    worker,
                });
                self.notice = self.locale.text("workbench.saving").into();
                self.details = true;
                self.focus = Focus::Details;
                self.detail_scroll = 0.0;
            }
            Err(error) => self.error(error.to_string()),
        }
    }
}

fn identity(hash: [u8; 32]) -> String {
    format!(
        "package-blake3:{}",
        hash.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn poll_save(mut workbench: ResMut<Workbench>, mut exit: MessageWriter<AppExit>) {
    let Some(saving) = &workbench.saving else {
        return;
    };
    let result = saving
        .receiver
        .lock()
        .expect("Save receiver is accessed on the main thread")
        .try_recv();
    let result = match result {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Disconnected) => Err("Save worker stopped without a result".into()),
        Err(mpsc::TryRecvError::Empty) => return,
    };
    let saving = workbench.saving.take().unwrap();
    if saving.worker.join().is_err() {
        workbench.error("Save worker panicked".into());
        return;
    }
    match result {
        Ok(SaveOutcome::Package { hash, count }) => {
            println!(
                "{}",
                serde_json::json!({"source_content_id":identity(workbench.source_hash),"content_id":identity(hash),"destination":workbench.destination,"anchor_count":count})
            );
            workbench.document.dirty = false;
            exit.write(AppExit::Success);
        }
        Ok(SaveOutcome::Labels { destination, count }) => {
            println!(
                "{}",
                serde_json::json!({"source":workbench.labels.as_ref().unwrap().document.source,"label_count":count,"destination":destination,"scope":"manual_records_only"})
            );
            workbench.document.dirty = false;
            exit.write(AppExit::Success);
        }
        Err(reason) => workbench.error(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn state() -> Workbench {
        Workbench {
            source: PathBuf::from("source"),
            destination: Some(PathBuf::from("new-package")),
            replay: None,
            candidates: None,
            labels: None,
            pending_label_action: None,
            source_hash: [7; 32],
            locale: Locale::EnUs,
            document: Document::from_anchors(
                48_000,
                vec![Anchor {
                    id: u64::MAX,
                    song_time: SongTime::from_frames(500),
                }],
                Vec::new(),
            )
            .unwrap(),
            wave: Waveform::default(),
            focus: Focus::Timeline,
            details: false,
            detail_scroll: 0.0,
            close_confirm: false,
            discard_selected: false,
            notice: String::new(),
            saving: None,
            audition: audition::Audition::default(),
        }
    }

    #[test]
    fn calibrated_candidates_reuse_read_only_actions_and_selection_requires_explicit_seek() {
        use bevy::ecs::system::SystemState;
        let mut app = App::new();
        app.add_message::<AppExit>();
        let mut state = state();
        state.candidates = Some(candidates::calibrated_fixture());
        let original = state.document.editor.anchors().to_vec();
        let destination = state.destination.clone();
        state.audition.playing = true;
        state.audition.target = Some(99);
        state.select(3);
        assert!(state.is_read_only());
        assert_eq!(state.document.cursor, 1200);
        assert_eq!(state.document.selected, None);
        assert_eq!(state.audition.target, Some(99));
        assert!(state.audition.playing);
        let mut system = SystemState::<MessageWriter<AppExit>>::new(app.world_mut());
        let mut exit = system.get_mut(app.world_mut()).unwrap();
        for action in [
            Action::Add,
            Action::Remove,
            Action::Undo,
            Action::Redo,
            Action::Export,
            Action::Frame,
            Action::Apply,
        ] {
            assert!(!state.toolbar().contains(&action));
            state.action(action, true, 1280.0, &mut exit);
        }
        assert_eq!(state.document.editor.anchors(), original);
        assert!(!state.document.dirty);
        assert!(state.saving.is_none());
        assert_eq!(state.destination, destination);
        state.action(Action::Seek, true, 1280.0, &mut exit);
        assert_eq!(state.audition.target, Some(1200));
    }

    #[test]
    fn waveform_tracks_pcm_across_blocks_and_keeps_the_partial_tail() {
        let frames: Vec<_> = (0..130)
            .map(|i| [i as f32 / 200.0, -(i as f32) / 200.0])
            .collect();
        let mut wave = Waveform::default();
        for chunk in frames.chunks(17) {
            wave.push(chunk).unwrap();
        }
        assert_eq!(wave.frames, 130);
        assert_eq!(wave.bins.len(), 3);
        assert_eq!(wave.bins[0], [[0.0, 0.315], [-0.315, 0.0]]);
        assert_eq!(wave.bins[2], [[0.64, 0.645], [-0.645, -0.64]]);
        assert_eq!(wave.peaks(128, 130), wave.bins[2]);
        assert_eq!(wave.peaks(130, 130), [[0.0; 2]; 2]);
        wave.frames = MAX_CANONICAL_FRAMES as i64;
        assert!(wave.push(&[[0.0, 0.0]]).is_err());
        assert_eq!(wave.bins.len(), 3);
    }

    #[test]
    fn exact_editing_commits_one_drag_and_round_trips_without_rewriting_ids() {
        let mut state = state();
        let doc = &mut state.document;
        for target in 600..640 {
            doc.drag = Some((u64::MAX, target));
        }
        assert_eq!(doc.selected().unwrap().song_time.frames(), 500);
        doc.finish_drag().unwrap();
        assert_eq!(doc.selected().unwrap().song_time.frames(), 639);
        doc.editor.undo().unwrap();
        doc.changed();
        assert!(!doc.dirty);
        doc.editor.redo().unwrap();
        doc.changed();
        assert_eq!(doc.selected().unwrap().song_time.frames(), 639);
        doc.drag = Some((u64::MAX, 700));
        doc.cancel();
        doc.finish_drag().unwrap();
        assert_eq!(doc.selected().unwrap().song_time.frames(), 639);
        doc.cursor = 48_000;
        assert!(doc.add().is_err());
        doc.cursor = 640;
        doc.add().unwrap();
        assert_eq!(doc.selected, Some(0));
        assert_eq!(doc.editor.anchors()[0].id, u64::MAX);
        assert_eq!(doc.editor.anchors()[1].id, 0);
        assert_eq!(doc.at_pixel(110.0, 10.0, 100.0), 48_000);
        assert_eq!(doc.at_pixel(10.5, 10.0, 100.0), 240);
        doc.start = 200;
        doc.span = 1_000;
        assert_eq!(doc.at_pixel(10.5, 10.0, 100.0), 205);
    }
    #[test]
    fn save_worker_error_joins_and_keeps_dirty_manual_draft() {
        use bevy::ecs::system::RunSystemOnce;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let mut state = state();
        state.labels = Some(labels::LabelView::new(crate::labels::Source {
            content_id: format!("package-blake3:{}", "07".repeat(32)),
            audio_blake3: "08".repeat(32),
            canonical_frames: 48_000,
            audio_basis: crate::labels::AudioBasis::CanonicalDecoded,
        }));
        let labels = state.labels.as_mut().unwrap();
        labels.document.reviewer = "reviewer".into();
        labels.begin_add(123).unwrap();
        labels.draft.as_mut().unwrap().reason = "keep this draft".into();
        state.refresh_label_dirty();
        let joined = Arc::new(AtomicBool::new(false));
        let completed = joined.clone();
        let (sender, receiver) = mpsc::channel();
        let (ready, wait) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            sender.send(Err("fresh source changed".into())).unwrap();
            ready.send(()).unwrap();
            completed.store(true, Ordering::SeqCst);
        });
        state.saving = Some(SaveWorker {
            receiver: Mutex::new(receiver),
            worker,
        });
        wait.recv().unwrap();
        let mut app = App::new();
        app.add_message::<AppExit>().insert_resource(state);
        app.world_mut().run_system_once(poll_save).unwrap();
        assert!(joined.load(Ordering::SeqCst));
        let state = app.world().resource::<Workbench>();
        assert!(state.saving.is_none());
        assert!(state.document.dirty);
        assert_eq!(
            state
                .labels
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .reason,
            "keep this draft"
        );
        assert!(state.notice.contains("fresh source changed"));
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
    }
}
