use super::*;
use bevy::{
    ecs::system::SystemParam,
    input::{
        ButtonState,
        gamepad::{GamepadConnectionEvent, GamepadEvent},
        keyboard::KeyboardInput,
        mouse::{MouseButtonInput, MouseWheel},
    },
    window::{WindowCloseRequested, WindowFocused},
};
use cocobeat_runtime::{MenuAccess, menu_access};
use std::collections::HashSet;

#[derive(Resource)]
pub(super) struct Controls {
    pub owner: Option<InputSource>,
    pub pads: Vec<Entity>,
    keys: HashSet<KeyCode>,
    buttons: HashSet<(Entity, GamepadButton)>,
    axes: HashSet<(Entity, GamepadAxis)>,
    mouse: bool,
    drag_offset: i64,
    focused: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            owner: None,
            pads: Vec::new(),
            keys: HashSet::new(),
            buttons: HashSet::new(),
            axes: HashSet::new(),
            mouse: false,
            drag_offset: 0,
            focused: true,
        }
    }
}

impl Controls {
    fn shift(&self) -> bool {
        self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight)
    }
    fn control(&self) -> bool {
        self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight)
    }

    fn access(&mut self, source: InputSource, claim: bool, state: &mut Workbench) -> bool {
        match menu_access(&mut self.owner, source, claim) {
            MenuAccess::Action => true,
            MenuAccess::Claimed => {
                state.document.cancel();
                false
            }
            MenuAccess::Ignored => false,
        }
    }

    fn key_edge(&mut self, key: KeyCode, pressed: bool, repeat: bool) -> bool {
        if pressed {
            self.keys.insert(key) && !repeat
        } else {
            self.keys.remove(&key);
            false
        }
    }

    fn button_edge(&mut self, pad: Entity, button: GamepadButton, pressed: bool) -> bool {
        if pressed {
            self.buttons.insert((pad, button))
        } else {
            self.buttons.remove(&(pad, button));
            false
        }
    }

    fn axis_edge(&mut self, pad: Entity, axis: GamepadAxis, value: f32) -> bool {
        if value.abs() <= 0.3 {
            self.axes.remove(&(pad, axis));
            false
        } else if value.abs() >= 0.6 {
            self.axes.insert((pad, axis))
        } else {
            false
        }
    }
}

#[derive(SystemParam)]
pub(super) struct Events<'w, 's> {
    keys: MessageReader<'w, 's, KeyboardInput>,
    mouse: MessageReader<'w, 's, MouseButtonInput>,
    wheel: MessageReader<'w, 's, MouseWheel>,
    focus: MessageReader<'w, 's, WindowFocused>,
    close: MessageReader<'w, 's, WindowCloseRequested>,
    connections: MessageReader<'w, 's, GamepadConnectionEvent>,
    pads: MessageReader<'w, 's, GamepadEvent>,
}

pub(super) fn capture(
    mut state: ResMut<Workbench>,
    mut controls: ResMut<Controls>,
    view: Res<ui::View>,
    windows: Query<(Entity, &Window)>,
    targets: Query<(&ui::Hit, &ComputedNode, &UiGlobalTransform)>,
    mut events: Events,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok((window_id, window)) = windows.single() else {
        return;
    };
    let canvas = targets
        .iter()
        .find(|(hit, node, _)| matches!(hit, ui::Hit::Timeline) && node.size.min_element() > 0.0)
        .map_or(view.canvas, |(_, node, transform)| {
            ui::logical_rect(node, transform)
        });
    let mut used = state.saving.is_some();
    for event in events
        .focus
        .read()
        .filter(|event| event.window == window_id)
    {
        controls.focused = event.focused;
        state.document.cancel();
        used = true;
    }
    if events.close.read().any(|event| event.window == window_id) {
        state.close(&mut exit);
        used = true;
    }
    let changed: HashSet<_> = events
        .connections
        .read()
        .map(|event| {
            if event.disconnected() {
                if controls.owner == Some(InputSource::Pad(event.gamepad)) {
                    controls.owner = None;
                    state.document.cancel();
                    used = true;
                }
                controls.buttons.retain(|(pad, _)| *pad != event.gamepad);
                controls.axes.retain(|(pad, _)| *pad != event.gamepad);
            } else if !controls.pads.contains(&event.gamepad) {
                controls.pads.push(event.gamepad);
            }
            event.gamepad
        })
        .collect();

    for event in events.keys.read().filter(|event| event.window == window_id) {
        let edge = controls.key_edge(
            event.key_code,
            event.state == ButtonState::Pressed,
            event.repeat,
        );
        if !edge || used || !controls.focused || !handled_key(event.key_code) {
            continue;
        }
        let before = controls.owner;
        if !controls.access(
            InputSource::Keyboard,
            event.key_code == KeyCode::Enter,
            &mut state,
        ) {
            used |= before != controls.owner;
            continue;
        }
        used = key(
            &mut state,
            &controls,
            event.key_code,
            canvas.width(),
            &mut exit,
        );
    }
    for event in events.pads.read() {
        let (pad, button, axis) = match event {
            GamepadEvent::Button(event) => (
                event.entity,
                Some((event.button, event.state == ButtonState::Pressed)),
                None,
            ),
            GamepadEvent::Axis(event) => (event.entity, None, Some((event.axis, event.value))),
            GamepadEvent::Connection(_) => continue,
        };
        let command = if let Some((button, pressed)) = button {
            controls
                .button_edge(pad, button, pressed)
                .then_some(PadCommand::Button(button))
        } else if let Some((axis, value)) = axis {
            if !matches!(axis, GamepadAxis::LeftStickX | GamepadAxis::LeftStickY) {
                continue;
            }
            if changed.contains(&pad) || used || !controls.focused {
                if value.abs() > 0.3 {
                    controls.axes.insert((pad, axis));
                } else {
                    controls.axes.remove(&(pad, axis));
                }
                None
            } else {
                controls
                    .axis_edge(pad, axis, value)
                    .then_some(PadCommand::Axis(axis, value))
            }
        } else {
            None
        };
        let Some(command) = command else {
            continue;
        };
        if changed.contains(&pad) || used || !controls.focused {
            continue;
        }
        if !command.handled() {
            continue;
        }
        let before = controls.owner;
        if !controls.access(
            InputSource::Pad(pad),
            matches!(command, PadCommand::Button(GamepadButton::Start)),
            &mut state,
        ) {
            used |= before != controls.owner;
            continue;
        }
        pad_command(&mut state, command, canvas.width(), &mut exit);
        used = true;
    }
    let cursor = window.cursor_position();
    for event in events
        .mouse
        .read()
        .filter(|event| event.window == window_id && event.button == MouseButton::Left)
    {
        let pressed = event.state == ButtonState::Pressed;
        let fresh = pressed && !controls.mouse;
        controls.mouse = pressed;
        if !pressed {
            if !used && controls.focused && controls.owner == Some(InputSource::Keyboard) {
                let committed = state.document.drag.is_some();
                update_drag(&mut state.document, cursor, canvas, controls.drag_offset);
                if let Err(error) = state.document.finish_drag() {
                    state.error(error);
                }
                used |= committed;
            } else {
                state.document.drag = None;
            }
            continue;
        }
        if !fresh || used || !controls.focused {
            continue;
        }
        let Some(cursor) = cursor else {
            continue;
        };
        let before = controls.owner;
        if !controls.access(InputSource::Keyboard, true, &mut state) {
            used |= before != controls.owner;
            continue;
        }
        let hit = targets
            .iter()
            .filter(|(_, node, _)| node.size.min_element() > 0.0)
            .find(|(hit, node, transform)| {
                (!state.close_confirm
                    || matches!(hit, ui::Hit::Action(Action::Keep | Action::Discard)))
                    && ui::logical_rect(node, transform).contains(cursor)
            })
            .map(|(hit, node, transform)| (*hit, ui::logical_rect(node, transform)));
        if let Some((hit, bounds)) = hit {
            click(
                &mut state,
                &view,
                hit,
                bounds,
                cursor,
                canvas.width(),
                &mut exit,
            );
            if let Some((_, frame)) = state.document.drag {
                controls.drag_offset = frame
                    - state
                        .document
                        .at_pixel(cursor.x, bounds.min.x, bounds.width());
            }
            used = true;
        }
    }
    if controls.mouse
        && controls.focused
        && controls.owner == Some(InputSource::Keyboard)
        && !state.close_confirm
        && state.saving.is_none()
    {
        update_drag(&mut state.document, cursor, canvas, controls.drag_offset);
    }
    for event in events
        .wheel
        .read()
        .filter(|event| event.window == window_id)
    {
        if used || !controls.focused || state.close_confirm || state.saving.is_some() {
            continue;
        }
        let Some(cursor) = cursor else {
            continue;
        };
        if event.y == 0.0 {
            continue;
        }
        let target = targets
            .iter()
            .filter(|(_, node, _)| node.size.min_element() > 0.0)
            .find(|(hit, node, transform)| {
                matches!(hit, ui::Hit::Timeline | ui::Hit::Row(_) | ui::Hit::Details)
                    && ui::logical_rect(node, transform).contains(cursor)
            })
            .map(|(hit, _, _)| *hit);
        let Some(target) = target else {
            continue;
        };
        let before = controls.owner;
        if !controls.access(InputSource::Keyboard, false, &mut state) {
            used |= before != controls.owner;
            continue;
        }
        if matches!(target, ui::Hit::Timeline) {
            if controls.shift() {
                state.document.pan(if event.y > 0.0 { -1 } else { 1 });
            } else {
                state.document.zoom(event.y > 0.0, canvas.width());
            }
        } else if matches!(target, ui::Hit::Details) {
            state.detail_scroll = (state.detail_scroll - event.y.signum() * 48.0).max(0.0);
        } else {
            state.document.browse(if event.y > 0.0 { -1 } else { 1 });
        }
        used = true;
    }
}

fn update_drag(doc: &mut Document, cursor: Option<Vec2>, bounds: Rect, offset: i64) {
    if let (Some(cursor), Some((id, _))) = (cursor, doc.drag) {
        let frame =
            (doc.at_pixel(cursor.x, bounds.min.x, bounds.width()) + offset).clamp(0, doc.end - 1);
        doc.drag = Some((id, frame));
        doc.cursor = frame;
    }
}

fn handled_key(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::Enter
            | KeyCode::Escape
            | KeyCode::Tab
            | KeyCode::ArrowLeft
            | KeyCode::ArrowRight
            | KeyCode::ArrowUp
            | KeyCode::ArrowDown
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Equal
            | KeyCode::Minus
            | KeyCode::NumpadAdd
            | KeyCode::NumpadSubtract
            | KeyCode::KeyZ
            | KeyCode::KeyS
            | KeyCode::Delete
            | KeyCode::Backspace
    ) || digit(key).is_some()
}

fn digit(key: KeyCode) -> Option<char> {
    Some(match key {
        KeyCode::Digit0 | KeyCode::Numpad0 => '0',
        KeyCode::Digit1 | KeyCode::Numpad1 => '1',
        KeyCode::Digit2 | KeyCode::Numpad2 => '2',
        KeyCode::Digit3 | KeyCode::Numpad3 => '3',
        KeyCode::Digit4 | KeyCode::Numpad4 => '4',
        KeyCode::Digit5 | KeyCode::Numpad5 => '5',
        KeyCode::Digit6 | KeyCode::Numpad6 => '6',
        KeyCode::Digit7 | KeyCode::Numpad7 => '7',
        KeyCode::Digit8 | KeyCode::Numpad8 => '8',
        KeyCode::Digit9 | KeyCode::Numpad9 => '9',
        _ => return None,
    })
}

fn cycle_focus(state: &mut Workbench, backwards: bool) {
    let index = match state.focus {
        Focus::Toolbar(i) => i,
        Focus::Timeline => 8,
        Focus::List => 9,
        Focus::Frame => 10,
        Focus::Details => 11,
    };
    let index = if backwards {
        (index + 11) % 12
    } else {
        (index + 1) % 12
    };
    state.document.cancel();
    state.focus = match index {
        0..=7 => Focus::Toolbar(index),
        8 => Focus::Timeline,
        9 => Focus::List,
        10 => Focus::Frame,
        _ => Focus::Details,
    };
    if matches!(state.focus, Focus::Frame | Focus::Details) {
        state.details = true;
    }
    if state.focus == Focus::List {
        state.details = false;
    }
}

fn key(
    state: &mut Workbench,
    controls: &Controls,
    key: KeyCode,
    width: f32,
    exit: &mut MessageWriter<AppExit>,
) -> bool {
    if state.close_confirm {
        match key {
            KeyCode::Escape => state.action(Action::Keep, true, width, exit),
            KeyCode::Tab | KeyCode::ArrowLeft | KeyCode::ArrowRight => {
                state.discard_selected = !state.discard_selected
            }
            KeyCode::Enter => state.action(
                if state.discard_selected {
                    Action::Discard
                } else {
                    Action::Keep
                },
                true,
                width,
                exit,
            ),
            _ => {}
        }
        return true;
    }
    if key == KeyCode::Escape {
        if state.document.drag.is_some() || state.document.editing_frame {
            state.document.cancel();
        } else {
            state.close(exit);
        }
        return true;
    }
    if key == KeyCode::Tab {
        cycle_focus(state, controls.shift());
        return true;
    }
    if controls.control() {
        match key {
            KeyCode::KeyZ => state.action(
                if controls.shift() {
                    Action::Redo
                } else {
                    Action::Undo
                },
                true,
                width,
                exit,
            ),
            KeyCode::KeyS => state.action(Action::Export, true, width, exit),
            _ => {}
        }
        return true;
    }
    if state.focus == Focus::Frame {
        if !state.document.editing_frame {
            state.document.begin_frame();
        }
        let doc = &mut state.document;
        match key {
            KeyCode::Enter => state.action(Action::Apply, true, width, exit),
            KeyCode::ArrowLeft => doc.caret = doc.caret.saturating_sub(1),
            KeyCode::ArrowRight => doc.caret = (doc.caret + 1).min(doc.frame.len()),
            KeyCode::Home => doc.caret = 0,
            KeyCode::End => doc.caret = doc.frame.len(),
            KeyCode::Backspace if doc.caret > 0 => {
                doc.caret -= 1;
                doc.frame.remove(doc.caret);
            }
            KeyCode::Delete if doc.caret < doc.frame.len() => {
                doc.frame.remove(doc.caret);
            }
            _ => {
                if let Some(digit) = digit(key)
                    && doc.frame.len() < 20
                {
                    doc.frame.insert(doc.caret, digit);
                    doc.caret += 1;
                }
            }
        }
        return key == KeyCode::Enter;
    }
    match key {
        KeyCode::Equal | KeyCode::NumpadAdd => state.document.zoom(true, width),
        KeyCode::Minus | KeyCode::NumpadSubtract => state.document.zoom(false, width),
        KeyCode::Delete => state.action(Action::Remove, true, width, exit),
        KeyCode::Enter => match state.focus {
            Focus::Toolbar(i) => state.action(TOOLBAR[i], true, width, exit),
            Focus::List => state.action(Action::Details, true, width, exit),
            _ => {}
        },
        KeyCode::Home => {
            state.document.cursor = 0;
            state.document.keep_cursor_visible();
        }
        KeyCode::End => {
            state.document.cursor = state.document.end;
            state.document.keep_cursor_visible();
        }
        KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::ArrowUp | KeyCode::ArrowDown => {
            let direction = if matches!(key, KeyCode::ArrowLeft | KeyCode::ArrowUp) {
                -1
            } else {
                1
            };
            navigate(state, direction, controls.shift());
        }
        _ => return false,
    }
    true
}

fn navigate(state: &mut Workbench, direction: i32, fast: bool) {
    match state.focus {
        Focus::Toolbar(i) => {
            state.focus = Focus::Toolbar((i as i32 + direction).rem_euclid(8) as usize)
        }
        Focus::Timeline => {
            state.document.cursor = (state.document.cursor
                + i64::from(direction) * if fast { 48 } else { 1 })
            .clamp(0, state.document.end);
            state.document.keep_cursor_visible();
        }
        Focus::List => state.document.browse(direction),
        Focus::Details => {
            state.detail_scroll = (state.detail_scroll + direction as f32 * 48.0).max(0.0)
        }
        Focus::Frame => {}
    }
}

#[derive(Clone, Copy)]
enum PadCommand {
    Button(GamepadButton),
    Axis(GamepadAxis, f32),
}

impl PadCommand {
    fn handled(self) -> bool {
        match self {
            Self::Axis(_, _) => true,
            Self::Button(button) => matches!(
                button,
                GamepadButton::Start
                    | GamepadButton::South
                    | GamepadButton::East
                    | GamepadButton::DPadLeft
                    | GamepadButton::DPadRight
                    | GamepadButton::DPadUp
                    | GamepadButton::DPadDown
                    | GamepadButton::LeftTrigger
                    | GamepadButton::RightTrigger
            ),
        }
    }
}

fn pad_command(
    state: &mut Workbench,
    command: PadCommand,
    width: f32,
    exit: &mut MessageWriter<AppExit>,
) {
    let button = match command {
        PadCommand::Button(button) => button,
        PadCommand::Axis(GamepadAxis::LeftStickX, value) => {
            if value > 0.0 {
                GamepadButton::DPadRight
            } else {
                GamepadButton::DPadLeft
            }
        }
        PadCommand::Axis(_, value) => {
            if value > 0.0 {
                GamepadButton::DPadUp
            } else {
                GamepadButton::DPadDown
            }
        }
    };
    if state.close_confirm {
        match button {
            GamepadButton::DPadLeft
            | GamepadButton::DPadRight
            | GamepadButton::LeftTrigger
            | GamepadButton::RightTrigger => state.discard_selected = !state.discard_selected,
            GamepadButton::East | GamepadButton::Start => {
                state.action(Action::Keep, false, width, exit)
            }
            GamepadButton::South => state.action(
                if state.discard_selected {
                    Action::Discard
                } else {
                    Action::Keep
                },
                false,
                width,
                exit,
            ),
            _ => {}
        }
        return;
    }
    match button {
        GamepadButton::LeftTrigger | GamepadButton::RightTrigger => {
            cycle_focus(state, button == GamepadButton::LeftTrigger)
        }
        GamepadButton::East | GamepadButton::Start => state.close(exit),
        GamepadButton::South => match state.focus {
            Focus::Toolbar(i) => state.action(TOOLBAR[i], false, width, exit),
            Focus::List => state.action(Action::Details, false, width, exit),
            Focus::Frame => state.action(Action::Frame, false, width, exit),
            _ => {}
        },
        GamepadButton::DPadLeft | GamepadButton::DPadRight => {
            let direction = if button == GamepadButton::DPadLeft {
                -1
            } else {
                1
            };
            if state.focus == Focus::Timeline {
                state.document.pan(i64::from(direction));
            } else {
                navigate(state, direction, false);
            }
        }
        GamepadButton::DPadUp | GamepadButton::DPadDown => {
            let direction = if button == GamepadButton::DPadUp {
                -1
            } else {
                1
            };
            if state.focus == Focus::Timeline {
                state.document.zoom(direction < 0, width);
            } else {
                navigate(state, direction, false);
            }
        }
        _ => {}
    }
}

fn click(
    state: &mut Workbench,
    view: &ui::View,
    hit: ui::Hit,
    bounds: Rect,
    cursor: Vec2,
    canvas_width: f32,
    exit: &mut MessageWriter<AppExit>,
) {
    match hit {
        ui::Hit::Action(action) => {
            if let Some(index) = TOOLBAR.iter().position(|candidate| *candidate == action) {
                state.focus = Focus::Toolbar(index);
            }
            state.action(action, true, canvas_width, exit);
        }
        ui::Hit::Row(row) => {
            state.document.select(view.first_row + row);
            state.focus = Focus::List;
        }
        ui::Hit::Details => {
            state.focus = Focus::Details;
            state.document.cancel();
        }
        ui::Hit::Timeline => {
            state.document.cancel();
            state.focus = Focus::Timeline;
            state.document.cursor = state
                .document
                .at_pixel(cursor.x, bounds.min.x, bounds.width());
            if cursor.y >= bounds.min.y + bounds.height() * 0.7
                && cursor.y < bounds.min.y + bounds.height() * 0.9
            {
                let doc = &mut state.document;
                let tolerance = (doc.span as f64 * 7.0 / f64::from(bounds.width())).ceil() as i64;
                if let Some(anchor) = doc
                    .editor
                    .anchors()
                    .iter()
                    .min_by_key(|a| ((a.song_time.frames() - doc.cursor).abs(), a.id))
                    .copied()
                    .filter(|a| (a.song_time.frames() - doc.cursor).abs() <= tolerance)
                {
                    doc.selected = Some(anchor.id);
                    doc.drag = Some((anchor.id, anchor.song_time.frames()));
                }
            } else if cursor.y >= bounds.min.y + bounds.height() * 0.9 {
                let doc = &mut state.document;
                let tolerance = (doc.span as f64 * 7.0 / f64::from(bounds.width())).ceil() as i64;
                if let Some(cue) = doc
                    .sections
                    .iter()
                    .min_by_key(|cue| ((cue.time.frames() - doc.cursor).abs(), cue.id))
                    .filter(|cue| (cue.time.frames() - doc.cursor).abs() <= tolerance)
                {
                    doc.cursor = cue.time.frames();
                    state.details = true;
                    state.detail_scroll = 0.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{
        gamepad::{GamepadAxisChangedEvent, GamepadButtonChangedEvent, GamepadConnection},
        keyboard::{Key, NativeKey},
    };

    fn app() -> (App, Entity, Entity, Entity) {
        let mut app = App::new();
        let mut view = ui::View::default();
        view.canvas = Rect::from_corners(Vec2::ZERO, Vec2::new(600.0, 200.0));
        app.insert_resource(super::super::tests::state())
            .insert_resource(view)
            .init_resource::<Controls>()
            .add_message::<AppExit>()
            .add_message::<KeyboardInput>()
            .add_message::<MouseButtonInput>()
            .add_message::<MouseWheel>()
            .add_message::<WindowFocused>()
            .add_message::<WindowCloseRequested>()
            .add_message::<GamepadConnectionEvent>()
            .add_message::<GamepadEvent>()
            .add_systems(Update, capture);
        let window = app.world_mut().spawn(Window::default()).id();
        let first = app.world_mut().spawn_empty().id();
        let second = app.world_mut().spawn_empty().id();
        (app, window, first, second)
    }

    fn key(app: &mut App, window: Entity, key: KeyCode, pressed: bool) {
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state: if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
            text: None,
            repeat: false,
            window,
        });
    }

    fn pad(app: &mut App, pad: Entity, button: GamepadButton, pressed: bool) {
        app.world_mut()
            .write_message(GamepadEvent::Button(GamepadButtonChangedEvent::new(
                pad,
                button,
                if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                },
                if pressed { 1.0 } else { 0.0 },
            )));
    }

    #[test]
    fn mixed_owners_consume_claims_and_block_same_batch_secondary_actions() {
        let (mut app, window, first, second) = app();
        app.world_mut().resource_mut::<Workbench>().focus = Focus::Toolbar(2);
        key(&mut app, window, KeyCode::Enter, true);
        pad(&mut app, first, GamepadButton::Start, true);
        pad(&mut app, second, GamepadButton::Start, true);
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().owner,
            Some(InputSource::Keyboard)
        );
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .document
                .editor
                .anchors()
                .len(),
            1
        );
        // Both pad presses were observed while the keyboard claim consumed the batch
        pad(&mut app, first, GamepadButton::Start, true);
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().owner,
            Some(InputSource::Keyboard)
        );
        pad(&mut app, first, GamepadButton::Start, false);
        pad(&mut app, first, GamepadButton::Start, true);
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().owner,
            Some(InputSource::Pad(first))
        );
        pad(&mut app, first, GamepadButton::South, true);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .document
                .editor
                .anchors()
                .len(),
            1
        );
        assert!(
            app.world()
                .resource::<Workbench>()
                .notice
                .contains("keyboard")
        );
        pad(&mut app, second, GamepadButton::DPadRight, true);
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().owner,
            Some(InputSource::Pad(first))
        );
        pad(&mut app, second, GamepadButton::Start, false);
        pad(&mut app, second, GamepadButton::Start, true);
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().owner,
            Some(InputSource::Pad(second))
        );
        key(&mut app, window, KeyCode::Enter, false);
        key(&mut app, window, KeyCode::Enter, true);
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().owner,
            Some(InputSource::Keyboard)
        );
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .document
                .editor
                .anchors()
                .len(),
            1
        );
        key(&mut app, window, KeyCode::Enter, false);
        key(&mut app, window, KeyCode::Enter, true);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .document
                .editor
                .anchors()
                .len(),
            2
        );
        key(&mut app, window, KeyCode::ControlLeft, true);
        key(&mut app, window, KeyCode::KeyZ, true);
        app.update();
        assert!(!app.world().resource::<Workbench>().document.dirty);
        key(&mut app, window, KeyCode::KeyZ, false);
        key(&mut app, window, KeyCode::ShiftLeft, true);
        key(&mut app, window, KeyCode::KeyZ, true);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .document
                .editor
                .anchors()
                .len(),
            2
        );
    }

    #[test]
    fn focus_connection_and_axis_barriers_preserve_edits_and_require_release() {
        let (mut app, window, first, _) = app();
        pad(&mut app, first, GamepadButton::Start, true);
        app.update();
        app.world_mut().resource_mut::<Workbench>().document.drag = Some((u64::MAX, 700));
        app.world_mut().write_message(WindowFocused {
            window,
            focused: false,
        });
        app.world_mut()
            .write_message(GamepadEvent::Axis(GamepadAxisChangedEvent::new(
                first,
                GamepadAxis::LeftStickY,
                0.4,
            )));
        app.update();
        assert!(app.world().resource::<Workbench>().document.drag.is_none());
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .document
                .selected()
                .unwrap()
                .song_time
                .frames(),
            500
        );
        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        app.update();
        app.world_mut().resource_mut::<Workbench>().focus = Focus::Toolbar(0);
        app.world_mut()
            .write_message(GamepadEvent::Axis(GamepadAxisChangedEvent::new(
                first,
                GamepadAxis::LeftStickY,
                0.8,
            )));
        app.update();
        assert_eq!(app.world().resource::<Workbench>().focus, Focus::Toolbar(0));
        app.world_mut()
            .write_message(GamepadEvent::Axis(GamepadAxisChangedEvent::new(
                first,
                GamepadAxis::LeftStickY,
                0.2,
            )));
        app.update();
        app.world_mut()
            .write_message(GamepadEvent::Axis(GamepadAxisChangedEvent::new(
                first,
                GamepadAxis::LeftStickY,
                -0.8,
            )));
        app.update();
        assert_eq!(app.world().resource::<Workbench>().focus, Focus::Toolbar(1));
        app.world_mut().write_message(GamepadConnectionEvent::new(
            first,
            GamepadConnection::Disconnected,
        ));
        pad(&mut app, first, GamepadButton::Start, true);
        app.update();
        assert!(app.world().resource::<Controls>().owner.is_none());
        app.world_mut().write_message(GamepadConnectionEvent::new(
            first,
            GamepadConnection::Connected {
                name: "test".into(),
                vendor_id: None,
                product_id: None,
            },
        ));
        pad(&mut app, first, GamepadButton::Start, true);
        app.update();
        pad(&mut app, first, GamepadButton::Start, true);
        app.update();
        assert!(app.world().resource::<Controls>().owner.is_none());
        pad(&mut app, first, GamepadButton::Start, false);
        pad(&mut app, first, GamepadButton::Start, true);
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().owner,
            Some(InputSource::Pad(first))
        );
    }

    #[test]
    fn invalid_frame_and_close_confirmation_keep_the_original_draft() {
        let (mut app, window, _, _) = app();
        app.world_mut().resource_mut::<Controls>().owner = Some(InputSource::Keyboard);
        {
            let mut state = app.world_mut().resource_mut::<Workbench>();
            state.document.move_selected(601).unwrap();
            state.document.begin_frame();
            state.document.frame = "48000".into();
            state.document.caret = 5;
            state.focus = Focus::Frame;
        }
        key(&mut app, window, KeyCode::Enter, true);
        app.update();
        let state = app.world().resource::<Workbench>();
        assert_eq!(state.document.selected().unwrap().song_time.frames(), 601);
        assert_eq!(state.document.frame, "48000");
        assert!(state.document.editing_frame);
        assert!(state.notice.contains("47999"));
        app.world_mut()
            .write_message(WindowCloseRequested { window });
        app.update();
        assert!(app.world().resource::<Workbench>().close_confirm);
        assert!(!app.world().resource::<Workbench>().discard_selected);
        key(&mut app, window, KeyCode::Escape, true);
        app.update();
        assert!(!app.world().resource::<Workbench>().close_confirm);
        assert!(app.world().resource::<Workbench>().document.dirty);
        key(&mut app, window, KeyCode::ControlLeft, true);
        key(&mut app, window, KeyCode::KeyZ, true);
        app.update();
        assert!(!app.world().resource::<Workbench>().document.dirty);
    }

    #[test]
    fn scaled_hit_geometry_and_drag_offset_do_not_move_on_selection() {
        let node = ComputedNode {
            size: Vec2::new(1200.0, 400.0),
            inverse_scale_factor: 0.5,
            ..default()
        };
        let transform = UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(
            600.0, 200.0,
        )));
        let bounds = ui::logical_rect(&node, &transform);
        assert_eq!(
            bounds,
            Rect::from_corners(Vec2::ZERO, Vec2::new(600.0, 200.0))
        );
        let mut state = super::super::tests::state();
        let doc = &mut state.document;
        doc.drag = Some((u64::MAX, 500));
        let cursor = Vec2::new(7.0, 160.0);
        let offset = 500 - doc.at_pixel(cursor.x, bounds.min.x, bounds.width());
        update_drag(doc, Some(cursor), bounds, offset);
        doc.finish_drag().unwrap();
        assert!(!doc.dirty);
        assert!(doc.editor.undo().is_err());
        doc.drag = Some((u64::MAX, 500));
        update_drag(doc, Some(Vec2::new(8.0, 160.0)), bounds, offset);
        doc.finish_drag().unwrap();
        assert_eq!(doc.selected().unwrap().song_time.frames(), 580);
        doc.editor.undo().unwrap();
        assert_eq!(doc.selected().unwrap().song_time.frames(), 500);
    }

    #[test]
    fn mouse_claim_and_scaled_scroll_hit_the_intended_panel() {
        let (mut app, window, pad, _) = app();
        {
            let mut state = app.world_mut().resource_mut::<Workbench>();
            state.document.cursor = 1000;
            state.document.add().unwrap();
            state.document.cursor = 2000;
            state.document.add().unwrap();
            state.document.select(0);
        }
        app.world_mut().resource_mut::<Controls>().owner = Some(InputSource::Pad(pad));
        for (hit, bounds) in [
            (
                ui::Hit::Row(1),
                Rect::from_corners(Vec2::new(0.0, 300.0), Vec2::new(300.0, 330.0)),
            ),
            (
                ui::Hit::Details,
                Rect::from_corners(Vec2::new(350.0, 300.0), Vec2::new(600.0, 450.0)),
            ),
        ] {
            app.world_mut().spawn((
                hit,
                ComputedNode {
                    size: bounds.size() * 2.0,
                    inverse_scale_factor: 0.5,
                    ..default()
                },
                UiGlobalTransform::from(bevy::math::Affine2::from_translation(
                    bounds.center() * 2.0,
                )),
            ));
        }
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(100.0, 315.0)));
        let mouse = |pressed| MouseButtonInput {
            button: MouseButton::Left,
            state: if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
            window,
        };
        app.world_mut().write_message(mouse(true));
        app.update();
        assert_eq!(
            app.world().resource::<Controls>().owner,
            Some(InputSource::Keyboard)
        );
        assert_eq!(
            app.world().resource::<Workbench>().document.selected,
            Some(u64::MAX)
        );
        app.world_mut().write_message(mouse(true));
        app.update();
        assert_eq!(
            app.world().resource::<Workbench>().document.selected,
            Some(u64::MAX)
        );
        app.world_mut().write_message(mouse(false));
        app.world_mut().write_message(mouse(true));
        app.update();
        assert_eq!(
            app.world().resource::<Workbench>().document.selected,
            Some(0)
        );
        app.world_mut().write_message(mouse(false));
        app.update();
        let wheel = || MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Line,
            phase: bevy::input::touch::TouchPhase::Moved,
            x: 0.0,
            y: -1.0,
            window,
        };
        app.world_mut().write_message(wheel());
        app.update();
        assert_eq!(
            app.world().resource::<Workbench>().document.selected,
            Some(1)
        );
        assert_eq!(app.world().resource::<Workbench>().detail_scroll, 0.0);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(400.0, 350.0)));
        app.world_mut().write_message(wheel());
        app.update();
        assert_eq!(app.world().resource::<Workbench>().detail_scroll, 48.0);
        assert_eq!(
            app.world().resource::<Workbench>().document.selected,
            Some(1)
        );
    }
}
