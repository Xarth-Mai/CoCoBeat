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

    pub fn bindings_text(&self, locale: Locale) -> String {
        [PlayerId::P1, PlayerId::P2]
            .map(|player| {
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
            .join("    ")
    }

    pub fn menu_text(&self, locale: Locale) -> String {
        if !self.menu_open {
            return locale.text("input.game_controls").into();
        }
        if let Some(binding) = self.binding {
            let (key, player) = match binding {
                Binding::Keyboard(player) => ("input.bind_keyboard", player),
                Binding::JoinPad(player) => ("input.join_controller", player),
                Binding::PadButton(player) => ("input.bind_controller", player),
            };
            return Message::with(key, [("player", format!("{player:?}"))]).render(locale);
        }
        let (key, player) = MENU[self.selection];
        let item = Message::with(key, player.map(|player| ("player", format!("{player:?}"))))
            .render(locale);
        format!(
            "{}\n{}",
            Message::with(
                "menu.selection",
                [
                    ("selected", (self.selection + 1).to_string()),
                    ("total", MENU.len().to_string()),
                    ("item", item),
                ],
            )
            .render(locale),
            locale.text("menu.controls"),
        )
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

    fn activate(&mut self, pad: Option<Entity>, now: u64) {
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
            _ => {
                self.emit(Control::Quit, now);
                None
            }
        };
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

    fn key(&mut self, key: KeyCode, pressed: bool, repeat: bool, now: u64) {
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
        if let Some(binding) = self.binding {
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
            KeyCode::ArrowUp if self.menu_open => {
                self.selection = (self.selection + MENU.len() - 1) % MENU.len()
            }
            KeyCode::ArrowDown if self.menu_open => {
                self.selection = (self.selection + 1) % MENU.len()
            }
            KeyCode::Enter if self.menu_open => self.activate(None, now),
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

    fn pad_button(&mut self, pad: Entity, button: GamepadButton, pressed: bool, now: u64) {
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
        if let Some(binding) = self.binding {
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
            GamepadButton::DPadUp if self.menu_open => {
                self.selection = (self.selection + MENU.len() - 1) % MENU.len()
            }
            GamepadButton::DPadDown if self.menu_open => {
                self.selection = (self.selection + 1) % MENU.len()
            }
            GamepadButton::South if self.menu_open => self.activate(Some(pad), now),
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
        .add_systems(
            First,
            capture_keyboard.after(crate::display::DisplaySystems::Pace),
        )
        .add_systems(PreUpdate, capture_gamepad.after(InputSystems));
}

fn capture_keyboard(
    mut state: ResMut<InputState>,
    mut focus: MessageReader<WindowFocused>,
    mut keys: MessageReader<KeyboardInput>,
) {
    state.suppress_hits = false;
    for event in focus.read() {
        let now = state.now_ns();
        state.focus(event.focused, now);
    }
    for event in keys.read() {
        let now = state.now_ns();
        state.key(
            event.key_code,
            event.state == ButtonState::Pressed,
            event.repeat,
            now,
        );
    }
}

fn capture_gamepad(
    mut state: ResMut<InputState>,
    mut connections: MessageReader<GamepadConnectionEvent>,
    mut buttons: MessageReader<GamepadButtonStateChangedEvent>,
) {
    let mut changed_connections = HashSet::new();
    for event in connections.read() {
        changed_connections.insert(event.gamepad);
        if event.disconnected() {
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
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        for use_pad in [false, true] {
            let mut input = InputState::default();
            input.join_pad(PlayerId::P1, pad);
            input.selection = 10;
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 1);
                input.pad_button(pad, GamepadButton::Start, true, 2);
                input.pad_button(pad, GamepadButton::Select, true, 3);
            } else {
                input.key(KeyCode::Enter, true, false, 1);
                input.key(KeyCode::Escape, true, false, 2);
                input.key(KeyCode::F5, true, false, 3);
                input.key(KeyCode::F6, true, false, 4);
                input.key(KeyCode::KeyF, true, false, 5);
            }
            assert!(input.settings_open);
            assert_eq!(
                input.queued.iter().map(|e| e.control).collect::<Vec<_>>(),
                [
                    Control::Settings(SettingsAction::Open),
                    Control::Settings(SettingsAction::Back)
                ]
            );
            input.set_settings_open(false);
            input.selection = 0;
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 6);
            } else {
                input.key(KeyCode::Enter, true, false, 6);
            }
            assert!(input.queued.is_empty());
            if use_pad {
                input.pad_button(pad, GamepadButton::South, false, 7);
                input.pad_button(pad, GamepadButton::South, true, 8);
            } else {
                input.key(KeyCode::Enter, false, false, 7);
                input.key(KeyCode::Enter, true, false, 8);
            }
            assert_eq!(input.queued[0].control, Control::Start);
        }
    }

    #[test]
    fn main_menu_keeps_bindings_and_requires_a_fresh_confirmation() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        for use_pad in [false, true] {
            let mut input = InputState::default();
            input.bind_key(PlayerId::P1, KeyCode::KeyD).unwrap();
            input.join_pad(PlayerId::P2, pad);
            input.pad_buttons[1] = GamepadButton::West;
            input.key(KeyCode::KeyD, true, false, 1);
            input.pad_button(pad, GamepadButton::West, true, 1);
            for _ in 0..9 {
                if use_pad {
                    input.pad_button(pad, GamepadButton::DPadDown, true, 2);
                    input.pad_button(pad, GamepadButton::DPadDown, false, 3);
                } else {
                    input.key(KeyCode::ArrowDown, true, false, 2);
                    input.key(KeyCode::ArrowDown, false, false, 3);
                }
            }
            assert!(input.menu_text(Locale::EnUs).contains("> Main menu"));
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 4);
            } else {
                input.key(KeyCode::Enter, true, false, 4);
            }
            assert_eq!(input.queued.len(), 1);
            assert_eq!(input.queued[0].control, Control::MainMenu);
            input.key(KeyCode::F5, true, false, 5);
            let bindings = input.bindings_text(Locale::EnUs);
            let held_keys = input.held_keys.clone();
            let held_pad_buttons = input.held_pad_buttons.clone();

            input.open_main_menu();
            assert!(input.menu_open);
            assert!(input.menu_text(Locale::EnUs).contains("> Start / Resume"));
            assert!(input.queued.is_empty());
            assert_eq!(input.bindings_text(Locale::EnUs), bindings);
            assert_eq!(input.pads, [None, Some(pad)]);
            assert_eq!(input.held_keys, held_keys);
            assert_eq!(input.held_pad_buttons, held_pad_buttons);
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 6);
            } else {
                input.key(KeyCode::Enter, true, true, 6);
                input.key(KeyCode::Enter, true, false, 6);
            }
            assert!(input.queued.is_empty());
            if use_pad {
                input.pad_button(pad, GamepadButton::South, false, 7);
                input.pad_button(pad, GamepadButton::South, true, 8);
            } else {
                input.key(KeyCode::Enter, false, false, 7);
                input.key(KeyCode::Enter, true, false, 8);
            }
            assert_eq!(input.queued.len(), 1);
            assert_eq!(input.queued[0].control, Control::Start);
        }
    }

    #[test]
    fn controls_gate_blocks_menu_gameplay_and_held_confirmations() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut input = InputState::default();
        input.key(KeyCode::Enter, true, false, 1);
        assert_eq!(input.queued.len(), 1);
        input.binding = Some(Binding::Keyboard(PlayerId::P1));
        input.set_controls_enabled(false);
        assert!(input.queued.is_empty());
        assert!(input.binding.is_none());
        input.key(KeyCode::Enter, false, false, 2);

        for selection in 0..MENU.len() {
            input.selection = selection;
            for key in [KeyCode::Enter, KeyCode::ArrowUp, KeyCode::ArrowDown] {
                input.key(key, true, false, 3);
                input.key(key, false, false, 4);
            }
            for button in [
                GamepadButton::South,
                GamepadButton::DPadUp,
                GamepadButton::DPadDown,
            ] {
                input.pad_button(pad, button, true, 3);
                input.pad_button(pad, button, false, 4);
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
            input.key(key, true, false, 5);
            input.key(key, false, false, 6);
        }
        input.pad_button(pad, GamepadButton::Select, true, 5);
        input.pad_button(pad, GamepadButton::Select, false, 6);
        assert!(input.queued.is_empty());
        assert_eq!(input.keys, [KeyCode::KeyF, KeyCode::KeyJ]);

        input.set_menu_open(true);
        input.selection = 0;
        input.key(KeyCode::Enter, true, false, 7);
        for button in [GamepadButton::Start, GamepadButton::South] {
            input.pad_button(pad, button, true, 7);
        }
        input.set_controls_enabled(true);
        input.key(KeyCode::Enter, true, true, 8);
        input.key(KeyCode::Enter, true, false, 8);
        for button in [GamepadButton::Start, GamepadButton::South] {
            input.pad_button(pad, button, true, 8);
        }
        assert!(input.queued.is_empty());
        input.key(KeyCode::Enter, false, false, 9);
        input.key(KeyCode::Enter, true, false, 10);
        for button in [GamepadButton::Start, GamepadButton::South] {
            input.pad_button(pad, button, false, 9);
            input.pad_button(pad, button, true, 10);
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
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut input = InputState::default();
        input.join_pad(PlayerId::P1, pad);
        input.key(KeyCode::Enter, true, false, 1);
        input.pad_button(pad, GamepadButton::Start, true, 1);
        input.set_controls_enabled(false);
        input.key(KeyCode::Enter, false, false, 2);
        input.pad_button(pad, GamepadButton::Start, false, 2);
        assert!(input.held_keys.is_empty());
        assert!(input.held_pad_buttons.is_empty());

        input.focus(false, 3);
        assert!(!input.is_focused());
        input.pad_button(pad, GamepadButton::South, true, 4);
        input.disconnect_pad(pad);
        assert_eq!(input.pads, [None, None]);
        assert!(input.held_pad_buttons.is_empty());
        input.focus(true, 5);
        assert!(input.is_focused());
        input.set_controls_enabled(true);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::FocusLost);
        input.key(KeyCode::Enter, true, false, 6);
        input.pad_button(pad, GamepadButton::Start, true, 6);
        assert_eq!(input.queued.len(), 3);
        assert!(
            input.queued[1..]
                .iter()
                .all(|event| event.control == Control::Start)
        );
    }

    #[test]
    fn edges_bindings_and_device_identity_survive_mode_changes() {
        let mut input = InputState {
            menu_open: false,
            ..default()
        };
        input.key(KeyCode::KeyF, true, false, 1);
        input.key(KeyCode::KeyF, true, true, 2);
        input.key(KeyCode::KeyF, true, false, 3);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::Hit(PlayerId::P1));
        assert_eq!(input.queued[0].monotonic_ns, 1);
        input.set_menu_open(true);
        input.set_menu_open(false);
        input.suppress_hits = false;
        input.key(KeyCode::KeyF, true, false, 4);
        assert!(input.queued.is_empty());
        input.key(KeyCode::KeyF, false, false, 5);
        input.key(KeyCode::KeyF, true, false, 6);
        assert_eq!(input.queued.len(), 1);
        assert!(input.bind_key(PlayerId::P1, KeyCode::KeyJ).is_err());
        assert!(input.bind_key(PlayerId::P1, KeyCode::Escape).is_err());
        assert!(input.bind_key(PlayerId::P1, KeyCode::KeyD).is_ok());

        input.focus(false, 7);
        input.key(KeyCode::KeyD, true, false, 8);
        input.focus(true, 9);
        input.suppress_hits = false;
        input.key(KeyCode::KeyD, true, false, 10);
        assert!(
            input
                .queued
                .iter()
                .all(|event| event.control == Control::FocusLost)
        );
        input.key(KeyCode::KeyD, false, false, 11);
        input.key(KeyCode::KeyD, true, false, 12);
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
        input.pad_button(first, GamepadButton::South, true, 13);
        input.pad_button(first, GamepadButton::South, true, 14);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::Hit(PlayerId::P2));
        input.disconnect_pad(first);
        input.queued.clear();
        input.pad_button(first, GamepadButton::South, true, 15);
        assert!(input.queued.is_empty());
        assert_eq!(input.pads, [Some(second), None]);
    }
}
