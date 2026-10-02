//! 输入时间是 Bevy 中首次读取消息的软件观察时刻，不是硬件按键时间

use std::{collections::HashSet, time::Instant};

use bevy::{
    input::{
        ButtonState, InputSystems,
        gamepad::{GamepadButtonStateChangedEvent, GamepadConnectionEvent},
        keyboard::KeyboardInput,
    },
    prelude::*,
    window::WindowFocused,
};
use cocobeat_schema::PlayerId;

use crate::i18n::{Locale, Message};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Hit(PlayerId),
    Start,
    TogglePause,
    Restart,
    MainMenu,
    Settings(SettingsAction),
    SaveReplay,
    Quit,
    FocusLost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsAction {
    Open,
    Up,
    Down,
    Previous,
    Next,
    Confirm,
    Back,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MenuPresentation {
    pub title: String,
    pub rows: Vec<MenuRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MenuRow {
    pub text: String,
    pub language: Option<Locale>,
    pub selected: bool,
}

#[derive(Resource, Default)]
pub(crate) struct MenuScroll {
    pub(crate) can_up: bool,
    pub(crate) can_down: bool,
    pub(crate) request: i8,
    pub(crate) recenter: bool,
}

impl MenuScroll {
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

#[derive(Clone, Copy, Debug)]
pub struct CapturedControl {
    pub control: Control,
    pub monotonic_ns: u64,
}

#[derive(Clone, Copy)]
enum Binding {
    Keyboard(PlayerId),
    JoinPad(PlayerId),
    PadButton(PlayerId),
}

const MENU: [(&str, Option<PlayerId>); 12] = [
    ("menu.start", None),
    ("menu.restart", None),
    ("menu.save_replay", None),
    ("menu.bind_keyboard", Some(PlayerId::P1)),
    ("menu.bind_keyboard", Some(PlayerId::P2)),
    ("menu.join_controller", Some(PlayerId::P1)),
    ("menu.join_controller", Some(PlayerId::P2)),
    ("menu.bind_controller", Some(PlayerId::P1)),
    ("menu.bind_controller", Some(PlayerId::P2)),
    ("menu.main", None),
    ("menu.settings", None),
    ("menu.quit", None),
];

#[derive(Resource)]
pub struct InputState {
    pub origin: Instant,
    pub queued: Vec<CapturedControl>,
    pub menu_open: bool,
    pub status: Message,
    settings_open: bool,
    keys: [KeyCode; 2],
    pads: [Option<Entity>; 2],
    pad_buttons: [GamepadButton; 2],
    held_keys: HashSet<KeyCode>,
    held_pad_buttons: HashSet<(Entity, GamepadButton)>,
    focused: bool,
    controls_enabled: bool,
    suppress_hits: bool,
    selection: usize,
    menu_row_count: usize,
    binding: Option<Binding>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            queued: Vec::new(),
            menu_open: true,
            status: Message::new("input.ready"),
            settings_open: false,
            keys: [KeyCode::KeyF, KeyCode::KeyJ],
            pads: [None, None],
            pad_buttons: [GamepadButton::South; 2],
            held_keys: HashSet::new(),
            held_pad_buttons: HashSet::new(),
            focused: true,
            controls_enabled: true,
            suppress_hits: false,
            selection: 0,
            menu_row_count: MENU.len(),
            binding: None,
        }
    }
}

impl InputState {
    /// 切换门控时清理逻辑操作，保留焦点事件与释放前的 held 屏障
    pub fn set_controls_enabled(&mut self, enabled: bool) {
        if self.controls_enabled != enabled {
            self.controls_enabled = enabled;
            self.queued
                .retain(|event| event.control == Control::FocusLost);
            self.reset_edges();
        }
    }

    pub fn controls_enabled(&self) -> bool {
        self.controls_enabled
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn set_menu_open(&mut self, open: bool) {
        if self.menu_open != open {
            self.menu_open = open;
            self.reset_edges();
        }
    }

    pub fn open_main_menu(&mut self) {
        self.menu_open = true;
        self.settings_open = false;
        self.selection = 0;
        self.queued.clear();
        self.reset_edges();
    }

    pub fn set_settings_open(&mut self, open: bool) {
        self.settings_open = open;
        self.queued
            .retain(|event| event.control == Control::FocusLost);
        self.reset_edges();
    }

    /// 保留已按住按钮作为释放屏障，避免切换模式后将它误认成新按下
    pub fn reset_edges(&mut self) {
        self.queued
            .retain(|event| !matches!(event.control, Control::Hit(_)));
        self.binding = None;
        self.suppress_hits = true;
    }

    fn binding_lines(&self, locale: Locale) -> [String; 2] {
        [PlayerId::P1, PlayerId::P2].map(|player| {
            let index = player.index();
            let mut args = vec![
                ("player", format!("{player:?}")),
                ("key", format!("{:?}", self.keys[index])),
            ];
            let key = if self.pads[index].is_some() {
                args.push(("button", format!("{:?}", self.pad_buttons[index])));
                "input.binding"
            } else {
                "input.binding_unassigned"
            };
            Message::with(key, args).render(locale)
        })
    }

    pub fn bindings_text(&self, locale: Locale) -> String {
        self.binding_lines(locale).join("    ")
    }

    pub fn menu_presentation(
        &mut self,
        locale: Locale,
        title: String,
        information: Vec<String>,
    ) -> Option<MenuPresentation> {
        if !self.menu_open || self.settings_open {
            return None;
        }
        if let Some(binding) = self.binding {
            let (key, player) = match binding {
                Binding::Keyboard(player) => ("input.bind_keyboard", player),
                Binding::JoinPad(player) => ("input.join_controller", player),
                Binding::PadButton(player) => ("input.bind_controller", player),
            };
            return Some(MenuPresentation {
                title,
                rows: vec![MenuRow {
                    text: [Message::with(key, [("player", format!("{player:?}"))]).render(locale)]
                        .into_iter()
                        .chain(self.binding_lines(locale))
                        .chain([self.status.render(locale)])
                        .chain(information)
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    language: None,
                    selected: true,
                }],
            });
        }
        let mut rows = MENU
            .into_iter()
            .map(|(key, player)| MenuRow {
                text: Message::with(key, player.map(|player| ("player", format!("{player:?}"))))
                    .render(locale),
                ..default()
            })
            .collect::<Vec<_>>();
        for text in [locale.text("menu.controls").into()]
            .into_iter()
            .chain(self.binding_lines(locale))
            .chain([self.status.render(locale)])
            .chain(information)
        {
            rows.extend(
                text.lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| MenuRow {
                        text: line.into(),
                        ..default()
                    }),
            );
        }
        self.menu_row_count = rows.len();
        self.selection = self.selection.min(self.menu_row_count - 1);
        rows[self.selection].selected = true;
        Some(MenuPresentation { title, rows })
    }

    pub(crate) fn navigate_menu(&mut self, action: SettingsAction, scroll: &mut MenuScroll) {
        if !scroll.handle(action) && self.binding.is_none() {
            self.selection = match action {
                SettingsAction::Up => {
                    (self.selection + self.menu_row_count - 1) % self.menu_row_count
                }
                SettingsAction::Down => (self.selection + 1) % self.menu_row_count,
                _ => return,
            };
            scroll.reset();
        }
    }

    fn now_ns(&self) -> u64 {
        self.origin.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }

    fn emit(&mut self, control: Control, monotonic_ns: u64) {
        if matches!(
            control,
            Control::Start
                | Control::TogglePause
                | Control::Restart
                | Control::MainMenu
                | Control::FocusLost
        ) {
            self.suppress_hits = true;
        }
        self.queued.push(CapturedControl {
            control,
            monotonic_ns,
        });
    }

    fn bind_key(&mut self, player: PlayerId, key: KeyCode) -> Result<(), &'static str> {
        if matches!(
            key,
            KeyCode::Enter
                | KeyCode::Escape
                | KeyCode::ArrowUp
                | KeyCode::ArrowDown
                | KeyCode::F5
                | KeyCode::F6
        ) {
            return Err("input.key_reserved");
        }
        if self.keys[1 - player.index()] == key {
            return Err("input.key_assigned");
        }
        self.keys[player.index()] = key;
        Ok(())
    }

    fn join_pad(&mut self, player: PlayerId, pad: Entity) {
        for assigned in &mut self.pads {
            if *assigned == Some(pad) {
                *assigned = None;
            }
        }
        self.pads[player.index()] = Some(pad);
        self.status = Message::with(
            "input.controller_joined",
            [
                ("player", format!("{player:?}")),
                ("button", format!("{:?}", self.pad_buttons[player.index()])),
            ],
        );
        self.binding = None;
    }

    fn disconnect_pad(&mut self, pad: Entity) {
        for player in [PlayerId::P1, PlayerId::P2] {
            if self.pads[player.index()] == Some(pad) {
                self.pads[player.index()] = None;
                self.status = Message::with(
                    "input.controller_disconnected",
                    [("player", format!("{player:?}"))],
                );
                self.binding = None;
            }
        }
        self.held_pad_buttons.retain(|(entity, _)| *entity != pad);
    }

    fn activate(&mut self, pad: Option<Entity>, now: u64, scroll: &mut MenuScroll) {
        let binding = match self.selection {
            0 => {
                self.emit(Control::Start, now);
                None
            }
            1 => {
                self.emit(Control::Restart, now);
                None
            }
            2 => {
                self.emit(Control::SaveReplay, now);
                None
            }
            3 => Some(Binding::Keyboard(PlayerId::P1)),
            4 => Some(Binding::Keyboard(PlayerId::P2)),
            5 => Some(Binding::JoinPad(PlayerId::P1)),
            6 => Some(Binding::JoinPad(PlayerId::P2)),
            7 => Some(Binding::PadButton(PlayerId::P1)),
            8 => Some(Binding::PadButton(PlayerId::P2)),
            9 => {
                self.emit(Control::MainMenu, now);
                None
            }
            10 => {
                // Route the rest of this capture batch through the settings gate
                self.set_settings_open(true);
                self.emit(Control::Settings(SettingsAction::Open), now);
                None
            }
            11 => {
                self.emit(Control::Quit, now);
                None
            }
            _ => return,
        };
        scroll.reset();
        if let Some(Binding::JoinPad(player)) = binding
            && let Some(pad) = pad
        {
            self.join_pad(player, pad);
            return;
        }
        if let Some(Binding::PadButton(player)) = binding
            && self.pads[player.index()].is_none()
        {
            self.status = Message::with("input.join_first", [("player", format!("{player:?}"))]);
            return;
        }
        self.binding = binding;
    }

    fn key(
        &mut self,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
        now: u64,
        scroll: &mut MenuScroll,
    ) {
        if !pressed {
            self.held_keys.remove(&key);
            return;
        }
        if !self.held_keys.insert(key) || repeat || !self.focused || !self.controls_enabled {
            return;
        }
        if self.settings_open {
            let action = match key {
                KeyCode::ArrowUp => Some(SettingsAction::Up),
                KeyCode::ArrowDown => Some(SettingsAction::Down),
                KeyCode::ArrowLeft => Some(SettingsAction::Previous),
                KeyCode::ArrowRight => Some(SettingsAction::Next),
                KeyCode::Enter => Some(SettingsAction::Confirm),
                KeyCode::Escape => Some(SettingsAction::Back),
                _ => None,
            };
            if let Some(action) = action {
                self.emit(Control::Settings(action), now);
            }
            return;
        }
        if self.menu_open && matches!(key, KeyCode::ArrowUp | KeyCode::ArrowDown) {
            self.navigate_menu(
                if key == KeyCode::ArrowUp {
                    SettingsAction::Up
                } else {
                    SettingsAction::Down
                },
                scroll,
            );
            return;
        }
        if let Some(binding) = self.binding {
            scroll.reset();
            if key == KeyCode::Escape {
                self.binding = None;
            } else if let Binding::Keyboard(player) = binding {
                match self.bind_key(player, key) {
                    Ok(()) => {
                        self.status = Message::with(
                            "input.keyboard_bound",
                            [
                                ("player", format!("{player:?}")),
                                ("key", format!("{key:?}")),
                            ],
                        );
                        self.binding = None;
                    }
                    Err(key) => self.status = Message::new(key),
                }
            }
            return;
        }
        match key {
            KeyCode::Escape => self.emit(Control::TogglePause, now),
            KeyCode::F5 => self.emit(Control::Restart, now),
            KeyCode::F6 => self.emit(Control::SaveReplay, now),
            KeyCode::Enter if self.menu_open => self.activate(None, now, scroll),
            _ if !self.menu_open && !self.suppress_hits => {
                for player in [PlayerId::P1, PlayerId::P2] {
                    if self.keys[player.index()] == key {
                        self.emit(Control::Hit(player), now);
                    }
                }
            }
            _ => {}
        }
    }

    fn pad_button(
        &mut self,
        pad: Entity,
        button: GamepadButton,
        pressed: bool,
        now: u64,
        scroll: &mut MenuScroll,
    ) {
        if !pressed {
            self.held_pad_buttons.remove(&(pad, button));
            return;
        }
        if !self.held_pad_buttons.insert((pad, button)) || !self.focused || !self.controls_enabled {
            return;
        }
        if self.settings_open {
            let action = match button {
                GamepadButton::DPadUp => Some(SettingsAction::Up),
                GamepadButton::DPadDown => Some(SettingsAction::Down),
                GamepadButton::DPadLeft => Some(SettingsAction::Previous),
                GamepadButton::DPadRight => Some(SettingsAction::Next),
                GamepadButton::South => Some(SettingsAction::Confirm),
                GamepadButton::East | GamepadButton::Start => Some(SettingsAction::Back),
                _ => None,
            };
            if let Some(action) = action {
                self.emit(Control::Settings(action), now);
            }
            return;
        }
        if self.menu_open && matches!(button, GamepadButton::DPadUp | GamepadButton::DPadDown) {
            self.navigate_menu(
                if button == GamepadButton::DPadUp {
                    SettingsAction::Up
                } else {
                    SettingsAction::Down
                },
                scroll,
            );
            return;
        }
        if let Some(binding) = self.binding {
            scroll.reset();
            if button == GamepadButton::East {
                self.binding = None;
            } else {
                match binding {
                    Binding::JoinPad(player) if button == GamepadButton::South => {
                        self.join_pad(player, pad)
                    }
                    Binding::PadButton(player) => {
                        if self.pads[player.index()] != Some(pad) {
                            self.status = Message::with(
                                "input.use_assigned",
                                [("player", format!("{player:?}"))],
                            );
                        } else if matches!(
                            button,
                            GamepadButton::Start
                                | GamepadButton::Select
                                | GamepadButton::DPadUp
                                | GamepadButton::DPadDown
                        ) {
                            self.status = Message::new("input.button_reserved");
                        } else {
                            self.pad_buttons[player.index()] = button;
                            self.status = Message::with(
                                "input.controller_bound",
                                [
                                    ("player", format!("{player:?}")),
                                    ("button", format!("{button:?}")),
                                ],
                            );
                            self.binding = None;
                        }
                    }
                    _ => {}
                }
            }
            return;
        }
        match button {
            GamepadButton::Start => self.emit(
                if self.menu_open {
                    Control::Start
                } else {
                    Control::TogglePause
                },
                now,
            ),
            GamepadButton::Select => self.emit(Control::SaveReplay, now),
            GamepadButton::East if self.menu_open => self.emit(Control::TogglePause, now),
            GamepadButton::South if self.menu_open => self.activate(Some(pad), now, scroll),
            _ if !self.menu_open && !self.suppress_hits => {
                for player in [PlayerId::P1, PlayerId::P2] {
                    let index = player.index();
                    if self.pads[index] == Some(pad) && self.pad_buttons[index] == button {
                        self.emit(Control::Hit(player), now);
                    }
                }
            }
            _ => {}
        }
    }

    fn focus(&mut self, focused: bool, now: u64) {
        if self.focused != focused {
            self.focused = focused;
            self.reset_edges();
            if focused {
                self.status = Message::new("input.focus_restored");
            } else {
                self.status = Message::new("input.focus_lost");
                self.emit(Control::FocusLost, now);
            }
        }
    }
}

pub fn install(app: &mut App) {
    app.init_resource::<InputState>()
        .init_resource::<MenuScroll>()
        .add_systems(
            First,
            capture_keyboard.after(crate::display::DisplaySystems::Pace),
        )
        .add_systems(PreUpdate, capture_gamepad.after(InputSystems));
}

fn capture_keyboard(
    mut state: ResMut<InputState>,
    mut scroll: ResMut<MenuScroll>,
    mut focus: MessageReader<WindowFocused>,
    mut keys: MessageReader<KeyboardInput>,
) {
    state.suppress_hits = false;
    for event in focus.read() {
        let now = state.now_ns();
        if state.focused != event.focused {
            scroll.reset();
        }
        state.focus(event.focused, now);
    }
    for event in keys.read() {
        let now = state.now_ns();
        state.key(
            event.key_code,
            event.state == ButtonState::Pressed,
            event.repeat,
            now,
            &mut scroll,
        );
    }
}

fn capture_gamepad(
    mut state: ResMut<InputState>,
    mut scroll: ResMut<MenuScroll>,
    mut connections: MessageReader<GamepadConnectionEvent>,
    mut buttons: MessageReader<GamepadButtonStateChangedEvent>,
) {
    let mut changed_connections = HashSet::new();
    for event in connections.read() {
        changed_connections.insert(event.gamepad);
        if event.disconnected() {
            scroll.reset();
            state.disconnect_pad(event.gamepad);
        }
    }
    for event in buttons.read() {
        // 新连接的初始状态与断连前的尾部消息均不代表明确的新按下
        if changed_connections.contains(&event.entity) {
            continue;
        }
        let now = state.now_ns();
        state.pad_button(
            event.entity,
            event.button,
            event.state == ButtonState::Pressed,
            now,
            &mut scroll,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_notice_and_bindings_follow_the_selected_locale() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut input = InputState::default();
        input.join_pad(PlayerId::P2, pad);
        let notice = input.status.clone();
        assert_eq!(notice.key, "input.controller_joined");
        assert_eq!(
            input.status.render(Locale::EnUs),
            "Controller joined P2; Hit = South"
        );
        let translated = input.status.render(Locale::ZhCn);
        assert_ne!(translated, input.status.render(Locale::EnUs));
        assert!(translated.contains("P2") && translated.contains("South"));
        assert_ne!(
            input.bindings_text(Locale::ZhCn),
            input.bindings_text(Locale::EnUs)
        );
        assert!(input.bindings_text(Locale::ZhCn).contains("KeyJ"));
        assert_eq!(input.status, notice);
    }

    #[test]
    fn settings_route_shortcuts_and_require_release_after_return() {
        let mut scroll = MenuScroll::default();
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        for use_pad in [false, true] {
            let mut input = InputState::default();
            input.join_pad(PlayerId::P1, pad);
            input.selection = 9;
            if use_pad {
                input.pad_button(pad, GamepadButton::DPadDown, true, 0, &mut scroll);
                input.pad_button(pad, GamepadButton::DPadDown, false, 0, &mut scroll);
                input.pad_button(pad, GamepadButton::South, true, 1, &mut scroll);
                input.pad_button(pad, GamepadButton::Start, true, 2, &mut scroll);
                input.pad_button(pad, GamepadButton::Select, true, 3, &mut scroll);
            } else {
                input.key(KeyCode::ArrowDown, true, false, 0, &mut scroll);
                input.key(KeyCode::ArrowDown, false, false, 0, &mut scroll);
                input.key(KeyCode::Enter, true, false, 1, &mut scroll);
                input.key(KeyCode::Escape, true, false, 2, &mut scroll);
                input.key(KeyCode::F5, true, false, 3, &mut scroll);
                input.key(KeyCode::F6, true, false, 4, &mut scroll);
                input.key(KeyCode::KeyF, true, false, 5, &mut scroll);
            }
            assert!(input.settings_open);
            assert_eq!(
                input.queued.iter().map(|e| e.control).collect::<Vec<_>>(),
                [
                    Control::Settings(SettingsAction::Open),
                    Control::Settings(SettingsAction::Back)
                ]
            );
            assert_eq!(
                input
                    .queued
                    .iter()
                    .map(|event| event.monotonic_ns)
                    .collect::<Vec<_>>(),
                [1, 2]
            );
            input.set_settings_open(false);
            input.selection = 0;
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 6, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, false, 6, &mut scroll);
            }
            assert!(input.queued.is_empty());
            if use_pad {
                input.pad_button(pad, GamepadButton::South, false, 7, &mut scroll);
                input.pad_button(pad, GamepadButton::South, true, 8, &mut scroll);
            } else {
                input.key(KeyCode::Enter, false, false, 7, &mut scroll);
                input.key(KeyCode::Enter, true, false, 8, &mut scroll);
            }
            assert_eq!(input.queued[0].control, Control::Start);
        }
    }

    #[test]
    fn main_menu_keeps_bindings_and_requires_a_fresh_confirmation() {
        let mut scroll = MenuScroll::default();
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        for use_pad in [false, true] {
            let mut input = InputState::default();
            input.bind_key(PlayerId::P1, KeyCode::KeyD).unwrap();
            input.join_pad(PlayerId::P2, pad);
            input.pad_buttons[1] = GamepadButton::West;
            input.key(KeyCode::KeyD, true, false, 1, &mut scroll);
            input.pad_button(pad, GamepadButton::West, true, 1, &mut scroll);
            for _ in 0..9 {
                if use_pad {
                    input.pad_button(pad, GamepadButton::DPadDown, true, 2, &mut scroll);
                    input.pad_button(pad, GamepadButton::DPadDown, false, 3, &mut scroll);
                } else {
                    input.key(KeyCode::ArrowDown, true, false, 2, &mut scroll);
                    input.key(KeyCode::ArrowDown, false, false, 3, &mut scroll);
                }
            }
            assert_eq!(
                input
                    .menu_presentation(Locale::EnUs, String::new(), vec![])
                    .unwrap()
                    .rows
                    .iter()
                    .find(|row| row.selected)
                    .unwrap()
                    .text,
                "Main menu"
            );
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 4, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, false, 4, &mut scroll);
            }
            assert_eq!(input.queued.len(), 1);
            assert_eq!(input.queued[0].control, Control::MainMenu);
            input.key(KeyCode::F5, true, false, 5, &mut scroll);
            let bindings = input.bindings_text(Locale::EnUs);
            let held_keys = input.held_keys.clone();
            let held_pad_buttons = input.held_pad_buttons.clone();

            input.open_main_menu();
            assert!(input.menu_open);
            assert_eq!(
                input
                    .menu_presentation(Locale::EnUs, String::new(), vec![])
                    .unwrap()
                    .rows
                    .iter()
                    .find(|row| row.selected)
                    .unwrap()
                    .text,
                "Start / Resume"
            );
            assert!(input.queued.is_empty());
            assert_eq!(input.bindings_text(Locale::EnUs), bindings);
            assert_eq!(input.pads, [None, Some(pad)]);
            assert_eq!(input.held_keys, held_keys);
            assert_eq!(input.held_pad_buttons, held_pad_buttons);
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 6, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, true, 6, &mut scroll);
                input.key(KeyCode::Enter, true, false, 6, &mut scroll);
            }
            assert!(input.queued.is_empty());
            if use_pad {
                input.pad_button(pad, GamepadButton::South, false, 7, &mut scroll);
                input.pad_button(pad, GamepadButton::South, true, 8, &mut scroll);
            } else {
                input.key(KeyCode::Enter, false, false, 7, &mut scroll);
                input.key(KeyCode::Enter, true, false, 8, &mut scroll);
            }
            assert_eq!(input.queued.len(), 1);
            assert_eq!(input.queued[0].control, Control::Start);
        }
    }

    #[test]
    fn controls_gate_blocks_menu_gameplay_and_held_confirmations() {
        let mut scroll = MenuScroll::default();
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut input = InputState::default();
        input.key(KeyCode::Enter, true, false, 1, &mut scroll);
        assert_eq!(input.queued.len(), 1);
        input.binding = Some(Binding::Keyboard(PlayerId::P1));
        input.set_controls_enabled(false);
        assert!(input.queued.is_empty());
        assert!(input.binding.is_none());
        input.key(KeyCode::Enter, false, false, 2, &mut scroll);

        for selection in 0..MENU.len() {
            input.selection = selection;
            for key in [KeyCode::Enter, KeyCode::ArrowUp, KeyCode::ArrowDown] {
                input.key(key, true, false, 3, &mut scroll);
                input.key(key, false, false, 4, &mut scroll);
            }
            for button in [
                GamepadButton::South,
                GamepadButton::DPadUp,
                GamepadButton::DPadDown,
            ] {
                input.pad_button(pad, button, true, 3, &mut scroll);
                input.pad_button(pad, button, false, 4, &mut scroll);
            }
            assert_eq!(input.selection, selection);
            assert!(input.binding.is_none());
            assert_eq!(input.pads, [None, None]);
            assert!(input.queued.is_empty());
        }
        input.set_menu_open(false);
        input.suppress_hits = false;
        for key in [
            KeyCode::Escape,
            KeyCode::F5,
            KeyCode::F6,
            KeyCode::KeyF,
            KeyCode::KeyJ,
        ] {
            input.key(key, true, false, 5, &mut scroll);
            input.key(key, false, false, 6, &mut scroll);
        }
        input.pad_button(pad, GamepadButton::Select, true, 5, &mut scroll);
        input.pad_button(pad, GamepadButton::Select, false, 6, &mut scroll);
        assert!(input.queued.is_empty());
        assert_eq!(input.keys, [KeyCode::KeyF, KeyCode::KeyJ]);

        input.set_menu_open(true);
        input.selection = 0;
        input.key(KeyCode::Enter, true, false, 7, &mut scroll);
        for button in [GamepadButton::Start, GamepadButton::South] {
            input.pad_button(pad, button, true, 7, &mut scroll);
        }
        input.set_controls_enabled(true);
        input.key(KeyCode::Enter, true, true, 8, &mut scroll);
        input.key(KeyCode::Enter, true, false, 8, &mut scroll);
        for button in [GamepadButton::Start, GamepadButton::South] {
            input.pad_button(pad, button, true, 8, &mut scroll);
        }
        assert!(input.queued.is_empty());
        input.key(KeyCode::Enter, false, false, 9, &mut scroll);
        input.key(KeyCode::Enter, true, false, 10, &mut scroll);
        for button in [GamepadButton::Start, GamepadButton::South] {
            input.pad_button(pad, button, false, 9, &mut scroll);
            input.pad_button(pad, button, true, 10, &mut scroll);
        }
        input.set_controls_enabled(true);
        assert_eq!(input.queued.len(), 3);
        assert!(
            input
                .queued
                .iter()
                .all(|event| event.control == Control::Start)
        );
    }

    #[test]
    fn controls_gate_keeps_release_focus_and_disconnect_processing() {
        let mut scroll = MenuScroll::default();
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut input = InputState::default();
        input.join_pad(PlayerId::P1, pad);
        input.key(KeyCode::Enter, true, false, 1, &mut scroll);
        input.pad_button(pad, GamepadButton::Start, true, 1, &mut scroll);
        input.set_controls_enabled(false);
        input.key(KeyCode::Enter, false, false, 2, &mut scroll);
        input.pad_button(pad, GamepadButton::Start, false, 2, &mut scroll);
        assert!(input.held_keys.is_empty());
        assert!(input.held_pad_buttons.is_empty());

        input.focus(false, 3);
        assert!(!input.is_focused());
        input.pad_button(pad, GamepadButton::South, true, 4, &mut scroll);
        input.disconnect_pad(pad);
        assert_eq!(input.pads, [None, None]);
        assert!(input.held_pad_buttons.is_empty());
        input.focus(true, 5);
        assert!(input.is_focused());
        input.set_controls_enabled(true);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::FocusLost);
        input.key(KeyCode::Enter, true, false, 6, &mut scroll);
        input.pad_button(pad, GamepadButton::Start, true, 6, &mut scroll);
        assert_eq!(input.queued.len(), 3);
        assert!(
            input.queued[1..]
                .iter()
                .all(|event| event.control == Control::Start)
        );
    }

    #[test]
    fn edges_bindings_and_device_identity_survive_mode_changes() {
        let mut scroll = MenuScroll::default();
        let mut input = InputState {
            menu_open: false,
            ..default()
        };
        input.key(KeyCode::KeyF, true, false, 1, &mut scroll);
        input.key(KeyCode::KeyF, true, true, 2, &mut scroll);
        input.key(KeyCode::KeyF, true, false, 3, &mut scroll);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::Hit(PlayerId::P1));
        assert_eq!(input.queued[0].monotonic_ns, 1);
        input.set_menu_open(true);
        input.set_menu_open(false);
        input.suppress_hits = false;
        input.key(KeyCode::KeyF, true, false, 4, &mut scroll);
        assert!(input.queued.is_empty());
        input.key(KeyCode::KeyF, false, false, 5, &mut scroll);
        input.key(KeyCode::KeyF, true, false, 6, &mut scroll);
        assert_eq!(input.queued.len(), 1);
        assert!(input.bind_key(PlayerId::P1, KeyCode::KeyJ).is_err());
        assert!(input.bind_key(PlayerId::P1, KeyCode::Escape).is_err());
        assert!(input.bind_key(PlayerId::P1, KeyCode::KeyD).is_ok());

        input.focus(false, 7);
        input.key(KeyCode::KeyD, true, false, 8, &mut scroll);
        input.focus(true, 9);
        input.suppress_hits = false;
        input.key(KeyCode::KeyD, true, false, 10, &mut scroll);
        assert!(
            input
                .queued
                .iter()
                .all(|event| event.control == Control::FocusLost)
        );
        input.key(KeyCode::KeyD, false, false, 11, &mut scroll);
        input.key(KeyCode::KeyD, true, false, 12, &mut scroll);
        assert_eq!(
            input.queued.last().unwrap().control,
            Control::Hit(PlayerId::P1)
        );

        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        input.join_pad(PlayerId::P1, first);
        input.join_pad(PlayerId::P2, first);
        assert_eq!(input.pads, [None, Some(first)]);
        input.join_pad(PlayerId::P1, second);
        input.queued.clear();
        input.pad_button(first, GamepadButton::South, true, 13, &mut scroll);
        input.pad_button(first, GamepadButton::South, true, 14, &mut scroll);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::Hit(PlayerId::P2));
        input.disconnect_pad(first);
        input.queued.clear();
        input.pad_button(first, GamepadButton::South, true, 15, &mut scroll);
        assert!(input.queued.is_empty());
        assert_eq!(input.pads, [Some(second), None]);
    }

    #[test]
    fn ready_information_is_read_only_and_long_rows_page_before_selection_changes() {
        let mut input = InputState::default();
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut scroll = MenuScroll::default();
        for locale in Locale::ALL {
            let presentation = input
                .menu_presentation(
                    locale,
                    "Ready".into(),
                    vec!["timing\nnotice".into(), String::new()],
                )
                .unwrap();
            assert_eq!(presentation.title, "Ready");
            assert_eq!(presentation.rows[0].text, locale.text("menu.start"));
            assert_eq!(presentation.rows[11].text, locale.text("menu.quit"));
            assert_eq!(presentation.rows[12].text, locale.text("menu.controls"));
            assert!(presentation.rows[13].text.contains("P1"));
            assert!(presentation.rows[14].text.contains("P2"));
            assert_eq!(presentation.rows.last().unwrap().text, "notice");
            assert_eq!(
                presentation.rows.iter().filter(|row| row.selected).count(),
                1
            );
        }
        input.selection = 12;
        input.key(KeyCode::Enter, true, false, 1, &mut scroll);
        input.pad_button(pad, GamepadButton::South, true, 2, &mut scroll);
        assert!(input.queued.is_empty());
        assert!(input.binding.is_none());
        input.pad_button(pad, GamepadButton::Start, true, 3, &mut scroll);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::Start);
        assert_eq!(input.queued[0].monotonic_ns, 3);
        input.queued.clear();
        scroll.can_down = true;
        input.key(KeyCode::ArrowDown, true, false, 4, &mut scroll);
        assert_eq!(input.selection, 12);
        assert_eq!(scroll.request, 1);
        input.key(KeyCode::ArrowDown, false, false, 5, &mut scroll);
        scroll.can_down = false;
        scroll.can_up = true;
        input.key(KeyCode::ArrowDown, true, false, 6, &mut scroll);
        assert_eq!(input.selection, 13);
        assert!(!scroll.can_up && !scroll.can_down);
        input.pad_button(pad, GamepadButton::DPadUp, true, 7, &mut scroll);
        assert_eq!(input.selection, 12);
        assert!(input.queued.is_empty());
        input.selection = input.menu_row_count - 1;
        let before = input.selection;
        input.set_menu_open(false);
        assert!(
            input
                .menu_presentation(Locale::EnUs, String::new(), vec![])
                .is_none()
        );
        assert_eq!(input.selection, before);
        input.set_menu_open(true);
        let presentation = input
            .menu_presentation(Locale::EnUs, String::new(), vec![])
            .unwrap();
        assert_eq!(input.selection, presentation.rows.len() - 1);
        assert!(input.selection >= MENU.len());
    }

    #[test]
    fn capture_batch_keeps_settings_and_binding_gates_immediate() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut input = InputState::default();
        let mut scroll = MenuScroll::default();
        input.selection = 9;
        input.key(KeyCode::ArrowDown, true, false, 1, &mut scroll);
        input.key(KeyCode::Enter, true, false, 2, &mut scroll);
        input.pad_button(pad, GamepadButton::South, true, 3, &mut scroll);
        input.pad_button(pad, GamepadButton::Start, true, 4, &mut scroll);
        assert_eq!(
            input
                .queued
                .iter()
                .map(|event| (event.control, event.monotonic_ns))
                .collect::<Vec<_>>(),
            [
                (Control::Settings(SettingsAction::Open), 2),
                (Control::Settings(SettingsAction::Confirm), 3),
                (Control::Settings(SettingsAction::Back), 4),
            ]
        );
        input.set_settings_open(false);
        input.selection = 0;
        input.key(KeyCode::Enter, true, false, 5, &mut scroll);
        input.pad_button(pad, GamepadButton::South, true, 6, &mut scroll);
        assert!(input.queued.is_empty());

        let mut input = InputState {
            selection: 3,
            ..default()
        };
        scroll.can_down = true;
        input.key(KeyCode::Enter, true, false, 10, &mut scroll);
        input.set_menu_open(true);
        let information = vec![
            Locale::EnUs.text("game.audio_failed").into(),
            Locale::EnUs.text("settings_notice.save_failed").into(),
            Message::with("hud.timing", [("milliseconds", "1.5".into())]).render(Locale::EnUs),
        ];
        let presentation = input
            .menu_presentation(
                Locale::EnUs,
                Locale::EnUs.text("phase.fault").into(),
                information.clone(),
            )
            .unwrap();
        assert_eq!(presentation.rows.len(), 1);
        for text in information
            .into_iter()
            .chain(input.binding_lines(Locale::EnUs))
            .chain([input.status.render(Locale::EnUs)])
        {
            assert!(presentation.rows[0].text.contains(&text));
        }
        assert!(presentation.rows[0].selected);
        assert!(presentation.rows[0].text.contains("P1"));
        assert!(!scroll.can_down);
        scroll.can_down = true;
        let notice = input.status.clone();
        input.key(KeyCode::ArrowDown, true, false, 11, &mut scroll);
        assert_eq!(scroll.request, 1);
        assert_eq!(input.selection, 3);
        assert_eq!(input.status, notice);
        assert!(input.binding.is_some());
        input.key(KeyCode::KeyD, true, false, 12, &mut scroll);
        assert_eq!(input.keys, [KeyCode::KeyD, KeyCode::KeyJ]);
        assert!(input.binding.is_none());
        assert!(!scroll.can_down && scroll.request == 0);
        assert!(input.queued.is_empty());
        input.set_menu_open(false);
        input.suppress_hits = false;
        input.key(KeyCode::KeyD, true, false, 13, &mut scroll);
        assert!(input.queued.is_empty());
        input.key(KeyCode::KeyD, false, false, 14, &mut scroll);
        input.key(KeyCode::KeyD, true, false, 15, &mut scroll);
        assert_eq!(input.queued[0].control, Control::Hit(PlayerId::P1));
        assert_eq!(input.queued[0].monotonic_ns, 15);
    }
}
