//! 输入时间是 Bevy 中首次读取消息的软件观察时刻，不是硬件按键时间

use std::{collections::HashSet, time::Instant};

use bevy::{
    input::{
        ButtonState, InputSystems,
        gamepad::{GamepadConnectionEvent, GamepadEvent},
        keyboard::KeyboardInput,
    },
    prelude::*,
    window::WindowFocused,
};
use cocobeat_replay::timing::InputKind;
use cocobeat_schema::PlayerId;

use crate::i18n::{Locale, Message};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    Hit(PlayerId),
    Start,
    TogglePause(InputSource),
    Restart,
    MainMenu,
    Settings(SettingsAction),
    Library(LibraryAction),
    SaveReplay,
    Quit,
    FocusLost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputSource {
    Keyboard,
    Pad(Entity),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAccess {
    Ignored,
    Claimed,
    Action,
}

/// A claim consumes the action; callers retain their own release and transition barriers
pub fn menu_access(
    owner: &mut Option<InputSource>,
    source: InputSource,
    explicit_claim: bool,
) -> MenuAccess {
    if *owner == Some(source) {
        MenuAccess::Action
    } else if owner.is_none() || explicit_claim {
        *owner = Some(source);
        MenuAccess::Claimed
    } else {
        MenuAccess::Ignored
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryAction {
    Open,
    Imports,
    ImportConfirm,
    Refresh,
    Select(usize),
    Back,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum LibraryMenu {
    #[default]
    Packages,
    Sources,
    Confirm,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MenuKind {
    #[default]
    Game,
    Settings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MenuRowRole {
    #[default]
    Action,
    Primary,
    Information,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MenuPresentation {
    pub title: String,
    pub rows: Vec<MenuRow>,
    pub kind: MenuKind,
    pub players: Option<[String; 2]>,
    pub owner_hint: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MenuRow {
    pub role: MenuRowRole,
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
    pub input_kind: InputKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Binding {
    Keyboard(PlayerId),
    JoinPad(PlayerId),
    PadButton(PlayerId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuAction {
    Start,
    Resume,
    Players,
    Settings,
    Restart,
    SaveReplay,
    MainMenu,
    Quit,
    Bind(Binding),
    UnassignPad(PlayerId),
    Library,
    LibraryRefresh,
    LibraryImports,
    LibraryImportConfirm,
    LibraryImportBack,
    Song(usize),
    LibraryBack,
    Back,
}

impl MenuAction {
    fn label(
        self,
        locale: Locale,
        network_player: Option<PlayerId>,
        network_spent: bool,
    ) -> String {
        let (key, player) = match self {
            Self::Start => ("menu.start", None),
            Self::Resume => ("menu.resume", None),
            Self::Players => ("menu.players", None),
            Self::Settings => ("menu.settings", None),
            Self::Library => ("library.open", None),
            Self::LibraryRefresh => ("library.refresh", None),
            Self::LibraryImports => ("import.open", None),
            Self::LibraryImportConfirm => ("import.confirm", None),
            Self::LibraryImportBack => ("import.background_back", None),
            Self::LibraryBack => ("settings.back", None),
            Self::Song(_) => ("library.development", None),
            Self::Restart if network_player.is_some() || network_spent => {
                ("network.next_round", None)
            }
            Self::Restart => ("menu.restart", None),
            Self::SaveReplay => ("menu.save_replay", None),
            Self::MainMenu => ("menu.main", None),
            Self::Quit => ("menu.quit", None),
            Self::Back => ("settings.back", None),
            Self::Bind(Binding::Keyboard(player)) => ("menu.bind_keyboard", Some(player)),
            Self::Bind(Binding::JoinPad(player)) => ("menu.join_controller", Some(player)),
            Self::Bind(Binding::PadButton(player)) => ("menu.bind_controller", Some(player)),
            Self::UnassignPad(player) => ("menu.unassign_controller", Some(player)),
        };
        Message::with(
            key,
            player.map(|player| ("player", format!("{:?}", network_player.unwrap_or(player)))),
        )
        .render(locale)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum MenuPhase {
    #[default]
    Ready,
    Paused,
    Finished,
    Fault,
    Transition,
}

const MENU: [MenuAction; 5] = [
    MenuAction::Start,
    MenuAction::Players,
    MenuAction::Settings,
    MenuAction::Library,
    MenuAction::Quit,
];

const PLAYERS_MENU: [MenuAction; 9] = [
    MenuAction::Bind(Binding::JoinPad(PlayerId::P1)),
    MenuAction::UnassignPad(PlayerId::P1),
    MenuAction::Bind(Binding::JoinPad(PlayerId::P2)),
    MenuAction::UnassignPad(PlayerId::P2),
    MenuAction::Bind(Binding::Keyboard(PlayerId::P1)),
    MenuAction::Bind(Binding::Keyboard(PlayerId::P2)),
    MenuAction::Bind(Binding::PadButton(PlayerId::P1)),
    MenuAction::Bind(Binding::PadButton(PlayerId::P2)),
    MenuAction::Back,
];

#[derive(Resource)]
pub struct InputState {
    pub origin: Instant,
    pub queued: Vec<CapturedControl>,
    pub menu_open: bool,
    pub status: Message,
    settings_open: bool,
    menu_phase: MenuPhase,
    replay_unsaved: bool,
    watch_replay: bool,
    menu_transitioning: bool,
    recovery_cancel: bool,
    players_open: bool,
    library_available: bool,
    library_rows: Option<Vec<String>>,
    library_busy: bool,
    library_menu: LibraryMenu,
    source_import_busy: bool,
    network_player: Option<PlayerId>,
    network_spent: bool,
    network_next_round: bool,
    menu_owner: Option<InputSource>,
    controller_order: Vec<Entity>,
    keys: [KeyCode; 2],
    pads: [Option<Entity>; 2],
    pad_buttons: [GamepadButton; 2],
    held_keys: HashSet<KeyCode>,
    held_pad_buttons: HashSet<(Entity, GamepadButton)>,
    held_pad_axes: HashSet<(Entity, GamepadAxis)>,
    focused: bool,
    controls_enabled: bool,
    capture_transitioned: bool,
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
            menu_phase: MenuPhase::Ready,
            replay_unsaved: false,
            watch_replay: false,
            menu_transitioning: false,
            recovery_cancel: false,
            players_open: false,
            library_available: false,
            library_rows: None,
            library_busy: false,
            library_menu: LibraryMenu::Packages,
            source_import_busy: false,
            network_player: None,
            network_spent: false,
            network_next_round: false,
            menu_owner: None,
            controller_order: Vec::new(),
            keys: [KeyCode::KeyF, KeyCode::KeyJ],
            pads: [None, None],
            pad_buttons: [GamepadButton::South; 2],
            held_keys: HashSet::new(),
            held_pad_buttons: HashSet::new(),
            held_pad_axes: HashSet::new(),
            focused: true,
            controls_enabled: true,
            capture_transitioned: false,
            selection: 0,
            menu_row_count: MENU.len() - 1,
            binding: None,
        }
    }
}

impl InputState {
    /// 网络身份只映射本地 P1 槽，菜单所有权与玩家身份独立
    pub fn set_network_player(&mut self, player: Option<PlayerId>) {
        if self.network_player != player {
            self.network_player = player;
            self.queued
                .retain(|event| event.control == Control::FocusLost);
            self.selection = 0;
            self.menu_row_count = self.menu_actions().len().max(1);
            self.reset_edges();
        }
    }

    pub fn set_network_spent(&mut self, spent: bool) {
        if self.network_spent != spent {
            self.network_spent = spent;
            self.queued
                .retain(|event| event.control == Control::FocusLost);
            self.selection = 0;
            self.menu_row_count = self.menu_actions().len().max(1);
            self.reset_edges();
        }
    }

    pub fn set_network_next_round(&mut self, available: bool) {
        if self.network_next_round != available {
            self.network_next_round = available;
            self.queued
                .retain(|event| event.control == Control::FocusLost);
            self.selection = 0;
            self.menu_row_count = self.menu_actions().len().max(1);
            self.reset_edges();
        }
    }

    pub(crate) fn can_start_next_round(&self) -> bool {
        self.network_spent
            && self.menu_open
            && !self.settings_open
            && !self.players_open
            && self.menu_actions().contains(&MenuAction::Restart)
    }

    fn displayed_player(&self, player: PlayerId) -> PlayerId {
        if player == PlayerId::P1 {
            self.network_player.unwrap_or(player)
        } else {
            player
        }
    }

    fn hit_player(&self, slot: PlayerId) -> Option<PlayerId> {
        match self.network_player {
            Some(player) if slot == PlayerId::P1 => Some(player),
            Some(_) => None,
            None => Some(slot),
        }
    }

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
            if !open && self.players_open {
                self.players_open = false;
                self.selection = 0;
                self.menu_row_count = MENU.len();
            }
            self.reset_edges();
        }
    }

    pub(crate) fn set_library_available(&mut self, available: bool) {
        if self.library_available != available {
            self.library_available = available;
            self.menu_row_count = self.menu_actions().len().max(1);
            self.selection = self.selection.min(self.menu_row_count - 1);
            self.queued
                .retain(|event| event.control == Control::FocusLost);
            self.reset_edges();
        }
    }

    pub(crate) fn library_open(&self) -> bool {
        self.library_rows.is_some()
    }

    pub(crate) fn show_library(&mut self, rows: Vec<String>, busy: bool) {
        self.show_library_page(rows, busy, LibraryMenu::Packages);
    }

    pub(crate) fn set_source_import_busy(&mut self, busy: bool) {
        if self.source_import_busy != busy {
            self.source_import_busy = busy;
            self.menu_row_count = self.menu_actions().len().max(1);
            self.selection = self.selection.min(self.menu_row_count - 1);
            self.queued
                .retain(|event| event.control == Control::FocusLost);
            self.reset_edges();
        }
    }

    pub(crate) fn show_library_page(&mut self, rows: Vec<String>, busy: bool, page: LibraryMenu) {
        if self.library_rows.as_ref() == Some(&rows)
            && self.library_busy == busy
            && self.library_menu == page
        {
            return;
        }
        self.library_menu = page;
        self.library_rows = Some(rows);
        self.library_busy = busy;
        self.players_open = false;
        self.settings_open = false;
        self.menu_open = true;
        self.selection = 0;
        self.menu_row_count = self.menu_actions().len().max(1);
        self.queued
            .retain(|event| event.control == Control::FocusLost);
        self.reset_edges();
    }

    pub(crate) fn library_selection(&self) -> Option<usize> {
        match self.menu_actions().get(self.selection) {
            Some(MenuAction::Song(index)) => Some(*index),
            _ => None,
        }
    }

    pub fn open_main_menu(&mut self) {
        self.menu_phase = MenuPhase::Ready;
        self.replay_unsaved = false;
        self.menu_open = true;
        self.settings_open = false;
        self.players_open = false;
        self.library_rows = None;
        self.library_busy = false;
        self.menu_row_count = self.menu_actions().len().max(1);
        self.selection = 0;
        self.queued.clear();
        self.reset_edges();
    }

    pub(crate) fn set_watch_replay(&mut self, watching: bool) {
        self.watch_replay = watching;
        if watching {
            self.status = Message::new("replay.watching");
        }
    }

    pub(crate) fn set_menu_phase(&mut self, phase: MenuPhase, replay_unsaved: bool) {
        if self.menu_phase != phase || self.replay_unsaved != replay_unsaved {
            if self.menu_phase != phase && matches!(phase, MenuPhase::Finished | MenuPhase::Fault) {
                self.players_open = false;
                self.library_rows = None;
                self.selection = 0;
                self.reset_edges();
            }
            self.menu_phase = phase;
            self.replay_unsaved = replay_unsaved;
            if !self.players_open && !self.settings_open {
                self.selection = 0;
                self.menu_row_count = self.menu_actions().len().max(1);
            }
        }
    }

    pub(crate) fn set_menu_transitioning(&mut self, transitioning: bool) {
        if self.menu_transitioning != transitioning {
            self.menu_transitioning = transitioning;
            self.reset_edges();
        }
    }

    pub(crate) fn set_recovery_cancel(&mut self, enabled: bool) {
        if self.recovery_cancel != enabled {
            self.recovery_cancel = enabled;
            self.reset_edges();
        }
    }

    pub fn set_settings_open(&mut self, open: bool) {
        self.settings_open = open;
        self.queued
            .retain(|event| event.control == Control::FocusLost);
        self.reset_edges();
    }

    /// 保留释放屏障，并停止本帧后续逻辑操作，直到新界面有机会显示
    pub fn reset_edges(&mut self) {
        self.queued
            .retain(|event| !matches!(event.control, Control::Hit(_)));
        self.binding = None;
        self.capture_transitioned = true;
    }

    pub(crate) fn claim_menu(&mut self, source: InputSource) {
        self.menu_owner = Some(source);
        if let InputSource::Pad(pad) = source {
            self.register_controller(pad);
        }
        self.queued
            .retain(|event| event.control == Control::FocusLost);
        self.capture_transitioned = true;
    }

    fn register_controller(&mut self, pad: Entity) {
        if !self.controller_order.contains(&pad) {
            self.controller_order.push(pad);
        }
    }

    pub(crate) fn menu_owner_hint(&self, locale: Locale) -> String {
        let device = match self.menu_owner {
            None => return locale.text("menu.claim_control").into(),
            Some(InputSource::Keyboard) => locale.text("menu.keyboard").into(),
            Some(InputSource::Pad(pad)) => {
                if let Some(player) = [PlayerId::P1, PlayerId::P2].into_iter().find(|player| {
                    self.pads[player.index()] == Some(pad)
                        && (self.network_player.is_none() || *player == PlayerId::P1)
                }) {
                    Message::with(
                        "menu.player_controller",
                        [("player", format!("{:?}", self.displayed_player(player)))],
                    )
                    .render(locale)
                } else {
                    let number = self
                        .controller_order
                        .iter()
                        .position(|known| *known == pad)
                        .unwrap()
                        + 1;
                    Message::with("menu.numbered_controller", [("number", number.to_string())])
                        .render(locale)
                }
            }
        };
        Message::with("menu.owner", [("device", device)]).render(locale)
    }

    fn menu_control(&mut self, source: InputSource, explicit_claim: bool) -> bool {
        match menu_access(&mut self.menu_owner, source, explicit_claim) {
            MenuAccess::Action => true,
            MenuAccess::Claimed => {
                self.claim_menu(source);
                false
            }
            MenuAccess::Ignored => false,
        }
    }

    fn binding_lines(&self, locale: Locale) -> [String; 2] {
        if let Some(player) = self.network_player {
            let binding = if self.pads[0].is_some() {
                format!("{:?} / {:?}", self.keys[0], self.pad_buttons[0])
            } else {
                format!("{:?}", self.keys[0])
            };
            let remote = if player == PlayerId::P1 {
                PlayerId::P2
            } else {
                PlayerId::P1
            };
            let mut lines = [String::new(), String::new()];
            lines[player.index()] = Message::with(
                "network.local_binding",
                [("player", format!("{player:?}")), ("binding", binding)],
            )
            .render(locale);
            lines[remote.index()] =
                Message::with("network.remote_player", [("player", format!("{remote:?}"))])
                    .render(locale);
            return lines;
        }
        [PlayerId::P1, PlayerId::P2].map(|player| {
            let index = player.index();
            let mut args = vec![
                ("player", format!("{:?}", self.displayed_player(player))),
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

    pub fn menu_presentation(
        &mut self,
        locale: Locale,
        title: String,
        information: Vec<String>,
    ) -> Option<MenuPresentation> {
        if !self.menu_open || self.settings_open {
            return None;
        }
        let title = if self.players_open {
            format!("{} · {title}", locale.text("menu.players"))
        } else {
            title
        };
        if let Some(binding) = self.binding {
            let (key, player) = match binding {
                Binding::Keyboard(player) => ("input.bind_keyboard", player),
                Binding::JoinPad(player) => ("input.join_controller", player),
                Binding::PadButton(player) => ("input.bind_controller", player),
            };
            return Some(MenuPresentation {
                kind: MenuKind::Game,
                players: (!self.watch_replay).then(|| self.binding_lines(locale)),
                owner_hint: Some(self.menu_owner_hint(locale)),
                title,
                rows: vec![MenuRow {
                    text: [Message::with(
                        key,
                        [("player", format!("{:?}", self.displayed_player(player)))],
                    )
                    .render(locale)]
                    .into_iter()
                    .chain(self.binding_lines(locale))
                    .chain([self.status.render(locale)])
                    .chain(information)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                    role: MenuRowRole::Information,
                    language: None,
                    selected: true,
                }],
            });
        }
        let mut rows = self
            .menu_actions()
            .iter()
            .enumerate()
            .map(|(index, action)| MenuRow {
                text: if let MenuAction::Song(song) = action {
                    self.library_rows
                        .as_ref()
                        .and_then(|rows| rows.get(*song))
                        .cloned()
                        .unwrap_or_default()
                } else {
                    action.label(locale, self.network_player, self.network_spent)
                },
                role: if index == 0
                    && matches!(
                        action,
                        MenuAction::Start | MenuAction::Resume | MenuAction::Restart
                    ) {
                    MenuRowRole::Primary
                } else {
                    MenuRowRole::Action
                },
                ..default()
            })
            .collect::<Vec<_>>();
        for text in information
            .into_iter()
            .chain((!self.watch_replay).then(|| locale.text("menu.controls").into()))
            .chain(
                (!self.watch_replay)
                    .then(|| self.binding_lines(locale))
                    .into_iter()
                    .flatten(),
            )
            .chain([
                if self.watch_replay && self.status.key == "replay.watching" {
                    String::new()
                } else {
                    self.status.render(locale)
                },
            ])
        {
            rows.extend(
                text.lines()
                    .filter(|line| !line.is_empty())
                    .map(|line| MenuRow {
                        role: MenuRowRole::Information,
                        text: line.into(),
                        ..default()
                    }),
            );
        }
        self.menu_row_count = rows.len();
        self.selection = self.selection.min(self.menu_row_count - 1);
        rows[self.selection].selected = true;
        Some(MenuPresentation {
            title,
            rows,
            kind: MenuKind::Game,
            players: (!self.watch_replay).then(|| self.binding_lines(locale)),
            owner_hint: Some(self.menu_owner_hint(locale)),
        })
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
        self.emit_from(control, monotonic_ns, InputKind::Internal);
    }

    fn emit_from(&mut self, control: Control, monotonic_ns: u64, input_kind: InputKind) {
        if matches!(
            control,
            Control::Start
                | Control::TogglePause(_)
                | Control::Restart
                | Control::MainMenu
                | Control::FocusLost
                | Control::Library(_)
        ) || (control == Control::SaveReplay && self.menu_open)
        {
            self.capture_transitioned = true;
        }
        self.queued.push(CapturedControl {
            control,
            monotonic_ns,
            input_kind,
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
        // ponytail: 单场联网进程保持此绑定，支持切回本地双人时先消除两槽键位重叠
        if self.network_player.is_none() && self.keys[1 - player.index()] == key {
            return Err("input.key_assigned");
        }
        self.keys[player.index()] = key;
        Ok(())
    }

    fn join_pad(&mut self, player: PlayerId, pad: Entity) {
        let other = if player == PlayerId::P1 {
            PlayerId::P2
        } else {
            PlayerId::P1
        };
        if self.pads[other.index()] == Some(pad) && self.network_player.is_some() {
            self.pads[other.index()] = None;
        }
        if self.pads[other.index()] == Some(pad) {
            self.status = Message::with(
                "input.controller_assigned",
                [("player", format!("{other:?}"))],
            );
            return;
        }
        self.pads[player.index()] = Some(pad);
        self.status = Message::with(
            "input.controller_joined",
            [
                ("player", format!("{:?}", self.displayed_player(player))),
                ("button", format!("{:?}", self.pad_buttons[player.index()])),
            ],
        );
        self.reset_edges();
    }

    fn disconnect_pad(&mut self, pad: Entity) {
        let selected_action = self
            .players_open
            .then(|| self.menu_actions().get(self.selection).copied())
            .flatten();
        let was_owner = self.menu_owner == Some(InputSource::Pad(pad));
        if was_owner {
            self.menu_owner = None;
        }
        if was_owner || self.pads.contains(&Some(pad)) {
            // Open already changed the capture route; the app must finish that transition
            self.queued.retain(|event| {
                matches!(
                    event.control,
                    Control::FocusLost | Control::Settings(SettingsAction::Open)
                )
            });

            self.capture_transitioned = true;
            self.emit(Control::FocusLost, self.now_ns());
        }
        for player in [PlayerId::P1, PlayerId::P2] {
            if self.pads[player.index()] == Some(pad) {
                self.pads[player.index()] = None;
                self.status = Message::with(
                    "input.controller_disconnected",
                    [("player", format!("{:?}", self.displayed_player(player)))],
                );
                if self.binding == Some(Binding::PadButton(player)) {
                    self.reset_edges();
                }
            }
        }
        if let Some(action) = selected_action {
            let action = match action {
                MenuAction::UnassignPad(player) if self.pads[player.index()].is_none() => {
                    MenuAction::Bind(Binding::JoinPad(player))
                }
                _ => action,
            };
            self.selection = self
                .menu_actions()
                .iter()
                .position(|candidate| *candidate == action)
                .unwrap();
        }
        self.held_pad_buttons.retain(|(entity, _)| *entity != pad);
        self.held_pad_axes.retain(|(entity, _)| *entity != pad);
    }

    fn menu_actions(&self) -> Vec<MenuAction> {
        let actions = if let Some(rows) = &self.library_rows {
            if self.library_busy {
                vec![if self.source_import_busy {
                    MenuAction::LibraryImportBack
                } else {
                    MenuAction::LibraryBack
                }]
            } else if self.library_menu == LibraryMenu::Confirm {
                vec![MenuAction::LibraryImportConfirm, MenuAction::LibraryBack]
            } else {
                let mut actions: Vec<_> = (0..rows.len()).map(MenuAction::Song).collect();
                if self.library_menu == LibraryMenu::Packages {
                    actions.push(MenuAction::LibraryImports);
                }
                actions.extend([MenuAction::LibraryRefresh, MenuAction::LibraryBack]);
                actions
            }
        } else if self.players_open {
            PLAYERS_MENU.to_vec()
        } else {
            match self.menu_phase {
                MenuPhase::Ready => {
                    let mut actions = MENU.to_vec();
                    if self.network_spent {
                        actions[0] = MenuAction::Restart;
                    }
                    actions
                }
                MenuPhase::Paused => vec![
                    MenuAction::Resume,
                    MenuAction::Players,
                    MenuAction::Settings,
                    MenuAction::Restart,
                    MenuAction::SaveReplay,
                    MenuAction::MainMenu,
                    MenuAction::Quit,
                ],
                MenuPhase::Finished | MenuPhase::Fault => vec![
                    MenuAction::Restart,
                    MenuAction::SaveReplay,
                    MenuAction::Settings,
                    MenuAction::MainMenu,
                    MenuAction::Quit,
                ],
                MenuPhase::Transition => vec![],
            }
        };
        actions
            .into_iter()
            .filter(|action| match action {
                MenuAction::Library => {
                    self.library_available
                        && !self.watch_replay
                        && self.network_player.is_none()
                        && !self.network_spent
                }
                MenuAction::Start => !self.network_spent && !self.source_import_busy,
                MenuAction::Resume => !self.network_spent,
                MenuAction::Restart if self.source_import_busy => false,
                MenuAction::Restart if self.network_player.is_some() || self.network_spent => {
                    self.network_spent
                        && self.network_next_round
                        && matches!(
                            self.menu_phase,
                            MenuPhase::Ready | MenuPhase::Finished | MenuPhase::Fault
                        )
                }
                _ => true,
            })
            .filter(|action| {
                self.network_player.is_none()
                    || !matches!(
                        action,
                        MenuAction::Bind(
                            Binding::Keyboard(PlayerId::P2)
                                | Binding::JoinPad(PlayerId::P2)
                                | Binding::PadButton(PlayerId::P2)
                        ) | MenuAction::UnassignPad(PlayerId::P2)
                    )
            })
            .filter(|action| {
                !self.watch_replay
                    || !matches!(action, MenuAction::Players | MenuAction::SaveReplay)
            })
            .filter(|action| *action != MenuAction::SaveReplay || self.replay_unsaved)
            .filter(|action| match action {
                MenuAction::UnassignPad(player) => self.pads[player.index()].is_some(),
                _ => true,
            })
            .collect()
    }

    fn players_page(&mut self, open: bool, scroll: &mut MenuScroll) {
        self.queued
            .retain(|event| event.control == Control::FocusLost);
        self.reset_edges();
        self.players_open = open;
        self.menu_row_count = self.menu_actions().len();
        self.selection = if open {
            0
        } else {
            self.menu_actions()
                .iter()
                .position(|action| *action == MenuAction::Players)
                .unwrap_or(0)
        };
        scroll.reset();
    }

    pub(crate) fn activate(&mut self, pad: Option<Entity>, now: u64, scroll: &mut MenuScroll) {
        let Some(&action) = self.menu_actions().get(self.selection) else {
            return;
        };
        scroll.reset();
        match action {
            MenuAction::Start | MenuAction::Resume => self.emit(Control::Start, now),
            MenuAction::Restart => self.emit(Control::Restart, now),
            MenuAction::SaveReplay => self.emit(Control::SaveReplay, now),
            MenuAction::MainMenu => self.emit(Control::MainMenu, now),
            MenuAction::Quit => self.emit(Control::Quit, now),
            MenuAction::Players => self.players_page(true, scroll),
            MenuAction::Library => self.emit(Control::Library(LibraryAction::Open), now),
            MenuAction::LibraryRefresh => self.emit(Control::Library(LibraryAction::Refresh), now),
            MenuAction::LibraryImports => self.emit(Control::Library(LibraryAction::Imports), now),
            MenuAction::LibraryImportConfirm => {
                self.emit(Control::Library(LibraryAction::ImportConfirm), now)
            }
            MenuAction::Song(index) => {
                self.emit(Control::Library(LibraryAction::Select(index)), now)
            }
            MenuAction::LibraryBack | MenuAction::LibraryImportBack => {
                self.emit(Control::Library(LibraryAction::Back), now)
            }
            MenuAction::Back => self.players_page(false, scroll),
            MenuAction::Settings => {
                // Route the rest of this capture batch through the settings gate
                self.set_settings_open(true);
                self.emit(Control::Settings(SettingsAction::Open), now);
            }
            MenuAction::UnassignPad(player) => {
                self.pads[player.index()] = None;
                self.status = Message::with(
                    "input.controller_unassigned",
                    [("player", format!("{:?}", self.displayed_player(player)))],
                );
                self.players_page(true, scroll);
                self.selection = self
                    .menu_actions()
                    .iter()
                    .position(|action| *action == MenuAction::Bind(Binding::JoinPad(player)))
                    .unwrap();
            }
            MenuAction::Bind(binding) => {
                if let Binding::PadButton(player) = binding
                    && self.pads[player.index()].is_none()
                {
                    self.status = Message::with(
                        "input.join_first",
                        [("player", format!("{:?}", self.displayed_player(player)))],
                    );
                    return;
                }
                self.reset_edges();
                self.binding = Some(binding);
                if let Binding::JoinPad(player) = binding
                    && let Some(pad) = pad
                {
                    self.join_pad(player, pad);
                }
            }
        }
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
        if !self.held_keys.insert(key)
            || repeat
            || !self.focused
            || !self.controls_enabled
            || self.capture_transitioned
            || (self.menu_transitioning
                && !(self.recovery_cancel && matches!(key, KeyCode::Escape | KeyCode::Enter)))
        {
            return;
        }
        if self.menu_open || self.settings_open {
            let source = InputSource::Keyboard;
            if key == KeyCode::Enter && !self.menu_control(source, true) {
                return;
            }
            let binding_response = matches!(self.binding, Some(Binding::Keyboard(_)))
                && !matches!(key, KeyCode::Escape | KeyCode::ArrowUp | KeyCode::ArrowDown);
            if !binding_response
                && (!matches!(
                    key,
                    KeyCode::Enter
                        | KeyCode::Escape
                        | KeyCode::ArrowUp
                        | KeyCode::ArrowDown
                        | KeyCode::ArrowLeft
                        | KeyCode::ArrowRight
                        | KeyCode::F5
                        | KeyCode::F6
                ) || !self.menu_control(source, false))
            {
                return;
            }
        }
        if self.menu_transitioning && self.recovery_cancel {
            if key == KeyCode::Escape {
                self.emit(Control::TogglePause(InputSource::Keyboard), now);
            }
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
                self.reset_edges();
            } else if let Binding::Keyboard(player) = binding {
                match self.bind_key(player, key) {
                    Ok(()) => {
                        self.status = Message::with(
                            "input.keyboard_bound",
                            [
                                ("player", format!("{:?}", self.displayed_player(player))),
                                ("key", format!("{key:?}")),
                            ],
                        );
                        self.reset_edges();
                    }
                    Err(key) => self.status = Message::new(key),
                }
            }
            return;
        }
        match key {
            KeyCode::Escape if self.menu_open && self.library_open() => {
                self.emit(Control::Library(LibraryAction::Back), now)
            }
            KeyCode::Escape if self.menu_open && self.players_open => {
                self.players_page(false, scroll)
            }
            KeyCode::Escape => self.emit(Control::TogglePause(InputSource::Keyboard), now),
            KeyCode::F5
                if !self.players_open
                    && !self.library_open()
                    && if self.menu_open {
                        self.menu_actions().contains(&MenuAction::Restart)
                    } else {
                        self.network_player.is_none() && !self.network_spent
                    } =>
            {
                self.emit(Control::Restart, now)
            }
            KeyCode::F6 if !self.players_open && !self.library_open() && self.replay_unsaved => {
                self.emit(Control::SaveReplay, now)
            }
            KeyCode::Enter if self.menu_open => self.activate(None, now, scroll),
            _ if !self.menu_open => {
                for player in [PlayerId::P1, PlayerId::P2] {
                    if self.keys[player.index()] == key
                        && let Some(player) = self.hit_player(player)
                    {
                        self.emit_from(Control::Hit(player), now, InputKind::KeyboardMessage);
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
        if !self.held_pad_buttons.insert((pad, button))
            || !self.focused
            || !self.controls_enabled
            || self.capture_transitioned
            || (self.menu_transitioning
                && !(self.recovery_cancel
                    && matches!(button, GamepadButton::East | GamepadButton::Start)))
        {
            return;
        }
        if self.menu_open || self.settings_open {
            let source = InputSource::Pad(pad);
            if button == GamepadButton::Start && !self.menu_control(source, true) {
                return;
            }
            let binding_response = match self.binding {
                Some(Binding::JoinPad(_)) => button == GamepadButton::South,
                Some(Binding::PadButton(player)) => {
                    self.pads[player.index()] == Some(pad)
                        && !matches!(
                            button,
                            GamepadButton::East
                                | GamepadButton::Start
                                | GamepadButton::Select
                                | GamepadButton::DPadUp
                                | GamepadButton::DPadDown
                        )
                }
                _ => false,
            };
            if !binding_response
                && (!matches!(
                    button,
                    GamepadButton::DPadUp
                        | GamepadButton::DPadDown
                        | GamepadButton::DPadLeft
                        | GamepadButton::DPadRight
                        | GamepadButton::South
                        | GamepadButton::East
                        | GamepadButton::Start
                        | GamepadButton::Select
                ) || !self.menu_control(source, false))
            {
                return;
            }
        }
        if self.menu_transitioning && self.recovery_cancel {
            self.emit(Control::TogglePause(InputSource::Pad(pad)), now);
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
                self.reset_edges();
            } else {
                match binding {
                    Binding::JoinPad(player) if button == GamepadButton::South => {
                        self.join_pad(player, pad)
                    }
                    Binding::PadButton(player) => {
                        if self.pads[player.index()] != Some(pad) {
                            self.status = Message::with(
                                "input.use_assigned",
                                [("player", format!("{:?}", self.displayed_player(player)))],
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
                                    ("player", format!("{:?}", self.displayed_player(player))),
                                    ("button", format!("{button:?}")),
                                ],
                            );
                            self.reset_edges();
                        }
                    }
                    _ => {}
                }
            }
            return;
        }
        match button {
            GamepadButton::Start
                if !self.players_open
                    && ((!self.menu_open
                        && (self.pads.contains(&Some(pad))
                            || (self.watch_replay
                                && self.menu_owner == Some(InputSource::Pad(pad)))))
                        || (self.menu_open
                            && self.menu_actions().iter().any(|action| {
                                matches!(action, MenuAction::Start | MenuAction::Resume)
                            }))) =>
            {
                self.emit(
                    if self.menu_open {
                        Control::Start
                    } else {
                        Control::TogglePause(InputSource::Pad(pad))
                    },
                    now,
                )
            }
            GamepadButton::Select
                if !self.players_open
                    && !self.library_open()
                    && self.replay_unsaved
                    && (self.menu_open || self.pads.contains(&Some(pad))) =>
            {
                self.emit(Control::SaveReplay, now)
            }
            GamepadButton::East | GamepadButton::Start if self.menu_open && self.library_open() => {
                self.emit(Control::Library(LibraryAction::Back), now)
            }
            GamepadButton::East if self.menu_open && self.players_open => {
                self.players_page(false, scroll)
            }
            GamepadButton::East if self.menu_open => {
                self.emit(Control::TogglePause(InputSource::Pad(pad)), now)
            }
            GamepadButton::South if self.menu_open => self.activate(Some(pad), now, scroll),
            _ if !self.menu_open => {
                for player in [PlayerId::P1, PlayerId::P2] {
                    let index = player.index();
                    if self.pads[index] == Some(pad)
                        && self.pad_buttons[index] == button
                        && let Some(player) = self.hit_player(player)
                    {
                        self.emit_from(Control::Hit(player), now, InputKind::GamepadMessage);
                    }
                }
            }
            _ => {}
        }
    }

    fn pad_axis(
        &mut self,
        pad: Entity,
        axis: GamepadAxis,
        value: f32,
        now: u64,
        scroll: &mut MenuScroll,
    ) {
        if !matches!(axis, GamepadAxis::LeftStickX | GamepadAxis::LeftStickY) || !value.is_finite()
        {
            return;
        }
        if value.abs() <= 0.3 {
            self.held_pad_axes.remove(&(pad, axis));
            return;
        }
        if value.abs() < 0.6
            || !self.held_pad_axes.insert((pad, axis))
            || !self.focused
            || !self.controls_enabled
            || self.capture_transitioned
            || self.menu_transitioning
        {
            return;
        }
        if self.menu_owner != Some(InputSource::Pad(pad)) {
            return;
        }
        let action = match axis {
            GamepadAxis::LeftStickY if value > 0.0 => SettingsAction::Up,
            GamepadAxis::LeftStickY => SettingsAction::Down,
            GamepadAxis::LeftStickX if value > 0.0 => SettingsAction::Next,
            _ => SettingsAction::Previous,
        };
        if self.settings_open {
            self.emit(Control::Settings(action), now);
        } else if self.menu_open && axis == GamepadAxis::LeftStickY {
            self.navigate_menu(action, scroll);
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
    state.capture_transitioned = false;
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
    mut events: MessageReader<GamepadEvent>,
) {
    let mut changed_connections = HashSet::new();
    let mut connected = HashSet::new();
    for event in connections.read() {
        changed_connections.insert(event.gamepad);
        if event.disconnected() {
            connected.remove(&event.gamepad);
            scroll.reset();
            state.disconnect_pad(event.gamepad);
        } else {
            state.register_controller(event.gamepad);
            connected.insert(event.gamepad);
        }
    }
    for event in events.read() {
        let pad = match event {
            GamepadEvent::Button(event) => event.entity,
            GamepadEvent::Axis(event) => event.entity,
            GamepadEvent::Connection(_) => continue,
        };
        // 新连接的初始状态与断连前的尾部消息均不代表明确的新按下
        if changed_connections.contains(&pad) {
            // 连接时已按住的输入仍须先释放，模拟按钮的后续数值变化不算新按下
            if connected.contains(&pad) {
                match event {
                    GamepadEvent::Button(event) if event.state == ButtonState::Pressed => {
                        state.held_pad_buttons.insert((pad, event.button));
                    }
                    GamepadEvent::Button(event) => {
                        state.held_pad_buttons.remove(&(pad, event.button));
                    }
                    GamepadEvent::Axis(event)
                        if matches!(
                            event.axis,
                            GamepadAxis::LeftStickX | GamepadAxis::LeftStickY
                        ) =>
                    {
                        if event.value.abs() > 0.3 {
                            state.held_pad_axes.insert((pad, event.axis));
                        } else {
                            state.held_pad_axes.remove(&(pad, event.axis));
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        let now = state.now_ns();
        match event {
            GamepadEvent::Button(event) => state.pad_button(
                pad,
                event.button,
                event.state == ButtonState::Pressed,
                now,
                &mut scroll,
            ),
            GamepadEvent::Axis(event) => {
                state.pad_axis(pad, event.axis, event.value, now, &mut scroll)
            }
            GamepadEvent::Connection(_) => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_owner_pad_can_pause_without_player_binding_and_second_pad_cannot() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let mut input = InputState::default();
        let mut scroll = MenuScroll::default();
        input.set_watch_replay(true);
        input.claim_menu(InputSource::Pad(first));
        input.set_menu_open(false);
        next_frame(&mut input);
        input.pad_button(second, GamepadButton::Start, true, 0, &mut scroll);
        assert!(input.queued.is_empty());
        input.pad_button(first, GamepadButton::Start, true, 1, &mut scroll);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(
            input.queued[0].control,
            Control::TogglePause(InputSource::Pad(first))
        );
        input.pad_button(first, GamepadButton::Start, true, 2, &mut scroll);
        assert_eq!(input.queued.len(), 1);
        input.pad_button(first, GamepadButton::Start, false, 3, &mut scroll);
        next_frame(&mut input);
        input.pad_button(first, GamepadButton::Start, true, 4, &mut scroll);
        assert_eq!(input.queued.len(), 2);
        input.queued.clear();
        input.disconnect_pad(first);
        assert_eq!(input.queued[0].control, Control::FocusLost);
    }

    #[test]
    fn watcher_menu_retains_device_ownership_controls_without_binding_or_save_actions() {
        let mut input = InputState::default();
        input.set_watch_replay(true);
        for phase in [
            MenuPhase::Ready,
            MenuPhase::Paused,
            MenuPhase::Finished,
            MenuPhase::Fault,
        ] {
            input.set_menu_phase(phase, true);
            let actions = input.menu_actions();
            assert!(!actions.contains(&MenuAction::Players));
            assert!(!actions.contains(&MenuAction::SaveReplay));
            assert!(actions.contains(&MenuAction::Settings));
            assert!(actions.contains(&MenuAction::Quit));
            let menu = input
                .menu_presentation(Locale::EnUs, "Watch".into(), vec![])
                .unwrap();
            assert!(menu.players.is_none());
        }
    }

    #[test]
    fn network_identity_maps_only_the_local_slot_and_keeps_release_and_menu_ownership() {
        let mut world = World::new();
        let local_pad = world.spawn_empty().id();
        let other_pad = world.spawn_empty().id();
        let mut input = controlled_input();
        let mut scroll = MenuScroll::default();
        input.register_controller(local_pad);
        input.register_controller(other_pad);
        input.join_pad(PlayerId::P1, local_pad);
        input.join_pad(PlayerId::P2, other_pad);
        input.set_menu_open(false);
        next_frame(&mut input);
        input.key(KeyCode::KeyF, true, false, 0, &mut scroll);
        input.pad_button(local_pad, GamepadButton::South, true, 1, &mut scroll);
        assert_eq!(input.queued.len(), 2);
        input.set_network_player(Some(PlayerId::P2));
        assert!(input.queued.is_empty());
        assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
        next_frame(&mut input);
        input.key(KeyCode::KeyF, true, false, 2, &mut scroll);
        input.pad_button(local_pad, GamepadButton::South, true, 3, &mut scroll);
        input.key(KeyCode::KeyJ, true, false, 4, &mut scroll);
        input.pad_button(other_pad, GamepadButton::South, true, 5, &mut scroll);
        assert!(input.queued.is_empty());
        input.key(KeyCode::KeyF, false, false, 6, &mut scroll);
        input.pad_button(local_pad, GamepadButton::South, false, 7, &mut scroll);
        input.key(KeyCode::KeyF, true, false, 8, &mut scroll);
        input.pad_button(local_pad, GamepadButton::South, true, 9, &mut scroll);
        assert!(
            input
                .queued
                .iter()
                .all(|event| event.control == Control::Hit(PlayerId::P2))
        );
        assert_eq!(input.queued.len(), 2);
        input.set_menu_open(true);
        next_frame(&mut input);
        input.pad_button(other_pad, GamepadButton::Start, true, 10, &mut scroll);
        assert_eq!(input.menu_owner, Some(InputSource::Pad(other_pad)));
        assert!(input.queued.is_empty());
        assert_eq!(input.pads, [Some(local_pad), Some(other_pad)]);
        assert!(input.menu_owner_hint(Locale::EnUs).contains("Controller 2"));
        let lines = input.binding_lines(Locale::EnUs);
        assert!(lines[0].contains("P1") && !lines[0].contains("KeyJ"));
        assert!(lines[1].contains("P2") && lines[1].contains("KeyF"));
        input.players_page(true, &mut scroll);
        assert!(input.menu_actions().iter().all(|action| !matches!(
            action,
            MenuAction::Bind(
                Binding::Keyboard(PlayerId::P2)
                    | Binding::JoinPad(PlayerId::P2)
                    | Binding::PadButton(PlayerId::P2)
            ) | MenuAction::UnassignPad(PlayerId::P2)
        )));
        let menu = input
            .menu_presentation(Locale::EnUs, String::new(), vec![])
            .unwrap();
        assert!(
            menu.rows
                .iter()
                .find(|row| row.role == MenuRowRole::Action)
                .unwrap()
                .text
                .contains("P2")
        );
        input.set_network_player(None);
        input.set_menu_open(false);
        next_frame(&mut input);
        input.key(KeyCode::KeyJ, false, false, 11, &mut scroll);
        input.pad_button(other_pad, GamepadButton::South, false, 12, &mut scroll);
        input.key(KeyCode::KeyJ, true, false, 13, &mut scroll);
        input.pad_button(other_pad, GamepadButton::South, true, 14, &mut scroll);
        assert_eq!(input.queued.len(), 2);
        assert!(
            input
                .queued
                .iter()
                .all(|event| event.control == Control::Hit(PlayerId::P2))
        );
    }

    #[test]
    fn network_local_key_can_use_the_dormant_slot_without_creating_a_remote_hit() {
        for role in [PlayerId::P1, PlayerId::P2] {
            let mut input = controlled_input();
            let mut scroll = MenuScroll::default();
            assert_eq!(
                input.bind_key(PlayerId::P1, KeyCode::KeyJ),
                Err("input.key_assigned")
            );
            input.set_network_player(Some(role));
            input.bind_key(PlayerId::P1, KeyCode::KeyJ).unwrap();
            input.set_menu_open(false);
            next_frame(&mut input);
            input.key(KeyCode::KeyJ, true, false, 1, &mut scroll);
            assert_eq!(input.queued.len(), 1);
            assert_eq!(input.queued[0].control, Control::Hit(role));
            assert_eq!(input.hit_player(PlayerId::P2), None);
            assert_eq!(
                input.bind_key(PlayerId::P1, KeyCode::Enter),
                Err("input.key_reserved")
            );
        }
    }

    #[test]
    fn spent_network_invitation_keeps_settings_save_and_quit_without_start_shortcuts() {
        let mut input = controlled_input();
        let mut scroll = MenuScroll::default();
        input.set_network_player(Some(PlayerId::P1));
        input.set_network_spent(true);
        for phase in [
            MenuPhase::Ready,
            MenuPhase::Paused,
            MenuPhase::Finished,
            MenuPhase::Fault,
        ] {
            input.set_menu_phase(phase, true);
            let actions = input.menu_actions();
            assert!(!actions.iter().any(|action| matches!(
                action,
                MenuAction::Start | MenuAction::Resume | MenuAction::Restart
            )));
            assert!(actions.contains(&MenuAction::Settings));
            assert!(actions.contains(&MenuAction::Quit));
            if phase != MenuPhase::Ready {
                assert!(actions.contains(&MenuAction::SaveReplay));
            }
            next_frame(&mut input);
            input.key(KeyCode::F5, false, false, 0, &mut scroll);
            input.key(KeyCode::F5, true, false, 1, &mut scroll);
            assert!(input.queued.is_empty());
        }
        input.set_menu_open(false);
        next_frame(&mut input);
        input.key(KeyCode::F5, false, false, 2, &mut scroll);
        input.key(KeyCode::F5, true, false, 3, &mut scroll);
        assert!(input.queued.is_empty());
        input.open_main_menu();
        assert!(!input.menu_actions().contains(&MenuAction::Start));
        input.set_network_spent(false);
        assert!(input.menu_actions().contains(&MenuAction::Start));
        next_frame(&mut input);
        input.key(KeyCode::Enter, true, false, 5, &mut scroll);
        assert_eq!(input.queued[0].control, Control::Start);
    }

    #[test]
    fn network_next_round_requires_terminal_or_ready_spent_and_available_for_menu_and_f5() {
        for phase in [
            MenuPhase::Ready,
            MenuPhase::Paused,
            MenuPhase::Finished,
            MenuPhase::Fault,
        ] {
            for spent in [false, true] {
                for available in [false, true] {
                    let mut input = controlled_input();
                    let mut scroll = MenuScroll::default();
                    input.set_network_player(Some(PlayerId::P2));
                    input.set_menu_phase(phase, true);
                    input.set_network_spent(spent);
                    input.set_network_next_round(available);
                    let expected = spent
                        && available
                        && matches!(
                            phase,
                            MenuPhase::Ready | MenuPhase::Finished | MenuPhase::Fault
                        );
                    let actions = input.menu_actions();
                    assert_eq!(actions.contains(&MenuAction::Restart), expected);
                    assert_eq!(input.can_start_next_round(), expected);
                    if spent {
                        assert!(!actions.contains(&MenuAction::Start));
                        assert!(!actions.contains(&MenuAction::Resume));
                    }
                    next_frame(&mut input);
                    input.key(KeyCode::F5, true, false, 0, &mut scroll);
                    assert_eq!(input.queued.len(), usize::from(expected));
                    if expected {
                        assert_eq!(input.queued[0].control, Control::Restart);
                        assert_eq!(
                            actions[0].label(
                                Locale::EnUs,
                                input.network_player,
                                input.network_spent
                            ),
                            Locale::EnUs.text("network.next_round")
                        );
                    }
                    input.queued.clear();
                    input.set_menu_open(false);
                    next_frame(&mut input);
                    input.key(KeyCode::F5, false, false, 1, &mut scroll);
                    input.key(KeyCode::F5, true, false, 2, &mut scroll);
                    assert!(input.queued.is_empty());
                    assert!(!input.can_start_next_round());
                }
            }
        }
        let mut offline = controlled_input();
        let mut scroll = MenuScroll::default();
        offline.set_menu_open(false);
        next_frame(&mut offline);
        offline.key(KeyCode::F5, true, false, 0, &mut scroll);
        assert_eq!(offline.queued[0].control, Control::Restart);
        assert_eq!(
            MenuAction::Restart.label(Locale::EnUs, None, false),
            "Restart song"
        );
    }

    #[test]
    fn next_round_menu_changes_keep_held_barriers_and_consume_one_confirmation() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        for (phase, use_pad) in [MenuPhase::Ready, MenuPhase::Finished, MenuPhase::Fault]
            .into_iter()
            .flat_map(|phase| [false, true].map(|pad| (phase, pad)))
        {
            let mut input = controlled_input();
            let mut scroll = MenuScroll::default();
            input.set_network_player(Some(PlayerId::P1));
            input.set_network_spent(true);
            input.set_menu_phase(phase, true);
            if use_pad {
                input.claim_menu(InputSource::Pad(pad));
                input.pad_button(pad, GamepadButton::South, true, 0, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, false, 0, &mut scroll);
            }
            input.selection = input.menu_row_count - 1;
            input.emit(Control::FocusLost, 1);
            input.emit(Control::Restart, 2);
            input.set_network_next_round(true);
            assert_eq!(input.selection, 0);
            assert_eq!(input.menu_row_count, input.menu_actions().len());
            assert_eq!(input.queued.len(), 1);
            assert_eq!(input.queued[0].control, Control::FocusLost);
            input.queued.clear();
            next_frame(&mut input);
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 3, &mut scroll);
                assert!(input.queued.is_empty());
                input.pad_button(pad, GamepadButton::South, false, 4, &mut scroll);
                input.pad_button(pad, GamepadButton::South, true, 5, &mut scroll);
                input.pad_button(pad, GamepadButton::South, false, 6, &mut scroll);
                input.pad_button(pad, GamepadButton::South, true, 7, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, false, 3, &mut scroll);
                assert!(input.queued.is_empty());
                input.key(KeyCode::Enter, false, false, 4, &mut scroll);
                input.key(KeyCode::Enter, true, false, 5, &mut scroll);
                input.key(KeyCode::Enter, false, false, 6, &mut scroll);
                input.key(KeyCode::Enter, true, false, 7, &mut scroll);
            }
            assert_eq!(input.queued.len(), 1);
            assert_eq!(input.queued[0].control, Control::Restart);
            input.set_network_next_round(true);
            assert_eq!(input.queued.len(), 1);
            input.set_network_next_round(false);
            assert!(input.queued.is_empty());
            assert!(!input.can_start_next_round());
            assert_eq!(input.selection, 0);
            assert_eq!(input.menu_row_count, input.menu_actions().len());
        }
    }

    #[test]
    fn shared_menu_access_consumes_claims_and_requires_explicit_takeover() {
        let mut world = World::new();
        let first = InputSource::Pad(world.spawn_empty().id());
        let second = InputSource::Pad(world.spawn_empty().id());
        let mut owner = None;
        for (source, explicit, expected, expected_owner) in [
            (first, false, MenuAccess::Claimed, first),
            (first, false, MenuAccess::Action, first),
            (second, false, MenuAccess::Ignored, first),
            (second, true, MenuAccess::Claimed, second),
            (second, true, MenuAccess::Action, second),
            (InputSource::Keyboard, false, MenuAccess::Ignored, second),
            (
                InputSource::Keyboard,
                true,
                MenuAccess::Claimed,
                InputSource::Keyboard,
            ),
            (
                InputSource::Keyboard,
                false,
                MenuAccess::Action,
                InputSource::Keyboard,
            ),
            (first, false, MenuAccess::Ignored, InputSource::Keyboard),
            (first, true, MenuAccess::Claimed, first),
        ] {
            assert_eq!(menu_access(&mut owner, source, explicit), expected);
            assert_eq!(owner, Some(expected_owner));
        }
    }

    fn controlled_input() -> InputState {
        InputState {
            menu_owner: Some(InputSource::Keyboard),
            ..default()
        }
    }

    fn next_frame(input: &mut InputState) {
        input.capture_transitioned = false;
    }

    #[test]
    fn recovery_cancel_uses_the_owner_and_preserves_held_input_barriers() {
        let mut input = controlled_input();
        let mut scroll = MenuScroll::default();
        let pad = World::new().spawn_empty().id();
        input.set_menu_phase(MenuPhase::Transition, false);
        input.set_menu_transitioning(true);
        input.set_recovery_cancel(true);
        next_frame(&mut input);
        let hit_key = input.keys[0];
        input.key(hit_key, true, false, 1, &mut scroll);
        input.key(KeyCode::Enter, true, false, 2, &mut scroll);
        input.pad_button(pad, GamepadButton::East, true, 3, &mut scroll);
        assert!(input.queued.is_empty());
        input.pad_button(pad, GamepadButton::Start, true, 4, &mut scroll);
        assert_eq!(input.menu_owner, Some(InputSource::Pad(pad)));
        assert!(input.queued.is_empty());
        next_frame(&mut input);
        input.pad_button(pad, GamepadButton::Start, false, 5, &mut scroll);
        input.pad_button(pad, GamepadButton::Start, true, 6, &mut scroll);
        assert_eq!(
            input.queued[0].control,
            Control::TogglePause(InputSource::Pad(pad))
        );
        input.queued.clear();
        next_frame(&mut input);
        input.key(KeyCode::Enter, false, false, 7, &mut scroll);
        input.key(KeyCode::Enter, true, false, 8, &mut scroll);
        assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
        assert!(input.queued.is_empty());
        next_frame(&mut input);
        input.key(KeyCode::Escape, true, false, 9, &mut scroll);
        assert_eq!(
            input.queued[0].control,
            Control::TogglePause(InputSource::Keyboard)
        );
        input.queued.clear();
        input.set_recovery_cancel(false);
        input.set_menu_transitioning(false);
        input.set_menu_open(false);
        next_frame(&mut input);
        input.key(hit_key, true, false, 10, &mut scroll);
        assert!(input.queued.is_empty());
        input.key(hit_key, false, false, 11, &mut scroll);
        input.key(hit_key, true, false, 12, &mut scroll);
        assert_eq!(input.queued[0].control, Control::Hit(PlayerId::P1));
    }

    fn capture_app() -> App {
        let mut app = App::new();
        install(&mut app);
        app.add_message::<WindowFocused>()
            .add_message::<KeyboardInput>()
            .add_message::<GamepadConnectionEvent>()
            .add_message::<GamepadEvent>();
        app
    }

    fn select(input: &mut InputState, action: MenuAction) {
        input.selection = input
            .menu_actions()
            .iter()
            .position(|candidate| *candidate == action)
            .unwrap();
    }

    #[test]
    fn library_navigation_is_stable_and_selection_needs_a_new_press_to_start() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        for use_pad in [false, true] {
            let mut input = controlled_input();
            let mut scroll = MenuScroll::default();
            input.set_library_available(true);
            input.join_pad(PlayerId::P1, first);
            input.join_pad(PlayerId::P2, second);
            if use_pad {
                input.claim_menu(InputSource::Pad(first));
            }
            let rows = vec!["dev".into(), "candidate".into()];
            input.show_library(rows.clone(), false);
            next_frame(&mut input);
            input.navigate_menu(SettingsAction::Down, &mut scroll);
            assert_eq!(input.library_selection(), Some(1));
            if use_pad {
                input.pad_button(first, GamepadButton::South, true, 1, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, false, 1, &mut scroll);
            }
            input.show_library(rows.clone(), false);
            assert_eq!(input.library_selection(), Some(1));
            assert_eq!(input.queued.len(), 1);
            assert_eq!(
                input.queued[0].control,
                Control::Library(LibraryAction::Select(1))
            );
            input.pad_button(second, GamepadButton::South, true, 2, &mut scroll);
            assert_eq!(input.queued.len(), 1);
            input.open_main_menu();
            next_frame(&mut input);
            if use_pad {
                input.pad_button(first, GamepadButton::South, true, 3, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, false, 3, &mut scroll);
            }
            assert!(input.queued.is_empty());
            if use_pad {
                input.pad_button(first, GamepadButton::South, false, 4, &mut scroll);
                input.pad_button(first, GamepadButton::South, true, 5, &mut scroll);
            } else {
                input.key(KeyCode::Enter, false, false, 4, &mut scroll);
                input.key(KeyCode::Enter, true, false, 5, &mut scroll);
            }
            assert_eq!(input.queued[0].control, Control::Start);
            input.show_library(rows, true);
            next_frame(&mut input);
            input.pad_button(second, GamepadButton::Start, true, 6, &mut scroll);
            assert_eq!(input.menu_owner, Some(InputSource::Pad(second)));
            assert!(input.queued.is_empty());
            next_frame(&mut input);
            input.pad_button(second, GamepadButton::East, true, 7, &mut scroll);
            assert_eq!(
                input.queued[0].control,
                Control::Library(LibraryAction::Back)
            );
            assert_eq!(input.menu_actions(), vec![MenuAction::LibraryBack]);
        }
    }

    #[test]
    fn library_entry_is_local_ready_only_and_busy_page_does_not_restart_or_save() {
        let mut input = controlled_input();
        let mut scroll = MenuScroll::default();
        assert!(!input.menu_actions().contains(&MenuAction::Library));
        input.set_library_available(true);
        assert!(input.menu_actions().contains(&MenuAction::Library));
        for phase in [
            MenuPhase::Paused,
            MenuPhase::Finished,
            MenuPhase::Fault,
            MenuPhase::Transition,
        ] {
            input.set_menu_phase(phase, true);
            assert!(!input.menu_actions().contains(&MenuAction::Library));
        }
        input.set_menu_phase(MenuPhase::Ready, true);
        input.set_watch_replay(true);
        assert!(!input.menu_actions().contains(&MenuAction::Library));
        input.set_watch_replay(false);
        input.set_network_player(Some(PlayerId::P1));
        assert!(!input.menu_actions().contains(&MenuAction::Library));
        input.set_network_player(None);
        input.show_library(vec!["candidate".into()], true);
        next_frame(&mut input);
        input.key(KeyCode::F5, true, false, 0, &mut scroll);
        input.key(KeyCode::F6, true, false, 1, &mut scroll);
        input.key(KeyCode::KeyF, true, false, 2, &mut scroll);
        assert!(input.queued.is_empty());
        input.key(KeyCode::Escape, true, false, 3, &mut scroll);
        assert_eq!(
            input.queued[0].control,
            Control::Library(LibraryAction::Back)
        );
    }

    #[test]
    fn ending_under_settings_keeps_the_draft_and_returns_to_the_primary_game_action() {
        use crate::{
            display::DisplayState, settings::DisplaySettings, settings_menu::SettingsMenu,
        };
        for phase in [MenuPhase::Finished, MenuPhase::Fault] {
            let mut input = controlled_input();
            let mut scroll = MenuScroll::default();
            let mut display = DisplayState::new(DisplaySettings::default());
            display.set_headless_surface([1920, 1080]);
            let mut settings = SettingsMenu::default();
            select(&mut input, MenuAction::Settings);
            input.activate(None, 0, &mut scroll);
            assert!(input.settings_open);
            assert_eq!(
                input.queued[0].control,
                Control::Settings(SettingsAction::Open)
            );
            settings.begin(&display);
            settings.handle(SettingsAction::Down, 0.0, &mut display);
            settings.handle(SettingsAction::Confirm, 0.0, &mut display);
            let draft = settings.presentation(0.0, &display, Locale::EnUs).unwrap();
            input.set_menu_phase(phase, false);
            assert!(input.settings_open);
            assert_eq!(
                settings.presentation(0.0, &display, Locale::EnUs).unwrap(),
                draft
            );
            assert!(settings.handle(SettingsAction::Back, 0.0, &mut display));
            input.set_settings_open(false);
            let menu = input
                .menu_presentation(Locale::EnUs, "Stopped".into(), vec![])
                .unwrap();
            assert!(menu.rows[0].selected);
            assert_eq!(menu.rows[0].role, MenuRowRole::Primary);
            assert_eq!(menu.rows[0].text, "Restart song");
            assert!(menu.rows.iter().skip(1).all(|row| !row.selected));
            assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
        }
    }

    #[test]
    fn saving_while_playing_keeps_later_hits_in_the_same_capture_batch() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let stranger = world.spawn_empty().id();
        for use_pad in [false, true] {
            let mut input = controlled_input();
            let mut scroll = MenuScroll::default();
            input.join_pad(PlayerId::P1, pad);
            input.set_menu_phase(MenuPhase::Paused, true);
            input.set_menu_open(false);
            next_frame(&mut input);
            input.pad_button(stranger, GamepadButton::Select, true, 0, &mut scroll);
            assert!(input.queued.is_empty());
            if use_pad {
                input.pad_button(pad, GamepadButton::Select, true, 1, &mut scroll);
                input.pad_button(pad, GamepadButton::South, true, 2, &mut scroll);
            } else {
                input.key(KeyCode::F6, true, false, 1, &mut scroll);
                input.key(KeyCode::KeyF, true, false, 2, &mut scroll);
            }
            assert_eq!(
                input
                    .queued
                    .iter()
                    .map(|event| event.control)
                    .collect::<Vec<_>>(),
                [Control::SaveReplay, Control::Hit(PlayerId::P1)]
            );
            input.set_menu_open(true);
            input.claim_menu(InputSource::Pad(stranger));
            next_frame(&mut input);
            input.pad_button(stranger, GamepadButton::Select, true, 3, &mut scroll);
            assert!(input.queued.is_empty());
            input.pad_button(stranger, GamepadButton::Select, false, 4, &mut scroll);
            input.pad_button(stranger, GamepadButton::Select, true, 5, &mut scroll);
            input.pad_button(stranger, GamepadButton::South, true, 6, &mut scroll);
            assert_eq!(input.queued.len(), 1);
            assert_eq!(input.queued[0].control, Control::SaveReplay);
        }
    }

    #[test]
    fn phase_actions_match_available_work_and_do_not_save_empty_or_already_saved_history() {
        let mut input = controlled_input();
        let mut scroll = MenuScroll::default();
        for (phase, primary, has_main) in [
            (MenuPhase::Ready, MenuAction::Start, false),
            (MenuPhase::Paused, MenuAction::Resume, true),
            (MenuPhase::Finished, MenuAction::Restart, true),
            (MenuPhase::Fault, MenuAction::Restart, true),
        ] {
            for unsaved in [false, true] {
                input.set_menu_phase(phase, unsaved);
                let actions = input.menu_actions();
                assert_eq!(actions[0], primary);
                assert_eq!(actions.contains(&MenuAction::MainMenu), has_main);
                assert_eq!(
                    actions.contains(&MenuAction::SaveReplay),
                    unsaved && phase != MenuPhase::Ready
                );
                assert_eq!(
                    actions.contains(&MenuAction::Start),
                    phase == MenuPhase::Ready
                );
                assert_eq!(
                    actions.contains(&MenuAction::Resume),
                    phase == MenuPhase::Paused
                );
                assert_eq!(
                    actions
                        .iter()
                        .filter(|action| **action == MenuAction::Restart)
                        .count(),
                    usize::from(phase != MenuPhase::Ready)
                );
                input.selection = 0;
                input.queued.clear();
                next_frame(&mut input);
                input.activate(None, 0, &mut scroll);
                assert_eq!(
                    input.queued[0].control,
                    if matches!(phase, MenuPhase::Ready | MenuPhase::Paused) {
                        Control::Start
                    } else {
                        Control::Restart
                    }
                );
            }
        }
        input.set_menu_phase(MenuPhase::Ready, false);
        input.queued.clear();
        next_frame(&mut input);
        input.key(KeyCode::F5, true, false, 1, &mut scroll);
        input.key(KeyCode::F6, true, false, 2, &mut scroll);
        assert!(input.queued.is_empty());
    }

    #[test]
    fn existing_notice_and_bindings_follow_the_selected_locale() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut input = controlled_input();
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
            input.binding_lines(Locale::ZhCn),
            input.binding_lines(Locale::EnUs)
        );
        assert!(input.binding_lines(Locale::ZhCn)[1].contains("KeyJ"));
        assert_eq!(input.status, notice);
    }

    #[test]
    fn settings_route_shortcuts_and_require_release_after_return() {
        let mut scroll = MenuScroll::default();
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        for use_pad in [false, true] {
            let mut input = controlled_input();
            input.join_pad(PlayerId::P1, pad);
            input.claim_menu(if use_pad {
                InputSource::Pad(pad)
            } else {
                InputSource::Keyboard
            });
            next_frame(&mut input);
            select(&mut input, MenuAction::Players);
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
                [Control::Settings(SettingsAction::Open)]
            );
            assert_eq!(
                input
                    .queued
                    .iter()
                    .map(|event| event.monotonic_ns)
                    .collect::<Vec<_>>(),
                [1]
            );
            input.set_settings_open(false);
            select(&mut input, MenuAction::Start);
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 6, &mut scroll);
            } else {
                next_frame(&mut input);
                input.key(KeyCode::Enter, true, false, 6, &mut scroll);
            }
            assert!(input.queued.is_empty());
            next_frame(&mut input);
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
            let mut input = controlled_input();
            input.bind_key(PlayerId::P1, KeyCode::KeyD).unwrap();
            input.join_pad(PlayerId::P2, pad);
            input.pad_buttons[1] = GamepadButton::West;
            input.claim_menu(if use_pad {
                InputSource::Pad(pad)
            } else {
                InputSource::Keyboard
            });
            next_frame(&mut input);
            input.key(KeyCode::KeyD, true, false, 1, &mut scroll);
            input.pad_button(pad, GamepadButton::West, true, 1, &mut scroll);
            input.set_menu_phase(MenuPhase::Paused, true);
            for _ in 0..input
                .menu_actions()
                .iter()
                .position(|action| *action == MenuAction::MainMenu)
                .unwrap()
            {
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
            let bindings = input.binding_lines(Locale::EnUs);
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
                "Start"
            );
            assert!(input.queued.is_empty());
            assert_eq!(input.binding_lines(Locale::EnUs), bindings);
            assert_eq!(input.pads, [None, Some(pad)]);
            assert_eq!(input.held_keys, held_keys);
            assert_eq!(input.held_pad_buttons, held_pad_buttons);
            if use_pad {
                input.pad_button(pad, GamepadButton::South, true, 6, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, true, 6, &mut scroll);
                next_frame(&mut input);
                input.key(KeyCode::Enter, true, false, 6, &mut scroll);
            }
            assert!(input.queued.is_empty());
            next_frame(&mut input);
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
        let mut input = controlled_input();
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
        select(&mut input, MenuAction::Start);
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
        next_frame(&mut input);
        input.key(KeyCode::Enter, false, false, 9, &mut scroll);
        input.key(KeyCode::Enter, true, false, 10, &mut scroll);
        for button in [GamepadButton::Start, GamepadButton::South] {
            input.pad_button(pad, button, false, 9, &mut scroll);
            input.pad_button(pad, button, true, 10, &mut scroll);
        }
        input.set_controls_enabled(true);
        assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::Start);
    }

    #[test]
    fn controls_gate_keeps_release_focus_and_disconnect_processing() {
        let mut scroll = MenuScroll::default();
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut input = controlled_input();
        input.join_pad(PlayerId::P1, pad);
        next_frame(&mut input);
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
        assert!(
            input
                .queued
                .iter()
                .all(|event| event.control == Control::FocusLost)
        );
        let focus_events = input.queued.len();
        next_frame(&mut input);
        input.key(KeyCode::Enter, true, false, 6, &mut scroll);
        input.pad_button(pad, GamepadButton::Start, true, 6, &mut scroll);
        assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
        assert_eq!(input.queued.len(), focus_events + 1);
        assert_eq!(input.queued.last().unwrap().control, Control::Start);
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
        next_frame(&mut input);
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
        next_frame(&mut input);
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
        assert_eq!(input.pads, [Some(first), None]);
        assert_eq!(input.status.key, "input.controller_assigned");
        input.join_pad(PlayerId::P2, second);
        next_frame(&mut input);
        input.queued.clear();
        input.pad_button(first, GamepadButton::South, true, 13, &mut scroll);
        input.pad_button(first, GamepadButton::South, true, 14, &mut scroll);
        assert_eq!(input.queued.len(), 1);
        assert_eq!(input.queued[0].control, Control::Hit(PlayerId::P1));
        input.disconnect_pad(first);
        input.queued.clear();
        input.pad_button(first, GamepadButton::South, true, 15, &mut scroll);
        assert!(input.queued.is_empty());
        assert_eq!(input.pads, [None, Some(second)]);
    }

    #[test]
    fn ready_information_is_read_only_and_long_rows_page_before_selection_changes() {
        let mut input = controlled_input();
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut scroll = MenuScroll::default();
        let action_count = input.menu_actions().len();
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
            assert_eq!(
                presentation.rows[action_count - 1].text,
                locale.text("menu.quit")
            );
            assert_eq!(
                presentation.rows[action_count + 2].text,
                locale.text("menu.controls")
            );
            assert!(presentation.rows[action_count + 3].text.contains("P1"));
            assert!(presentation.rows[action_count + 4].text.contains("P2"));
            assert_eq!(presentation.rows[action_count + 1].text, "notice");
            assert_eq!(
                presentation.rows.iter().filter(|row| row.selected).count(),
                1
            );
        }
        input.selection = action_count;
        input.key(KeyCode::Enter, true, false, 1, &mut scroll);
        input.pad_button(pad, GamepadButton::South, true, 2, &mut scroll);
        assert!(input.queued.is_empty());
        assert!(input.binding.is_none());
        input.pad_button(pad, GamepadButton::Start, true, 3, &mut scroll);
        assert!(input.queued.is_empty());
        assert_eq!(input.menu_owner, Some(InputSource::Pad(pad)));
        input.claim_menu(InputSource::Keyboard);
        next_frame(&mut input);
        scroll.can_down = true;
        input.key(KeyCode::ArrowDown, true, false, 4, &mut scroll);
        assert_eq!(input.selection, action_count);
        assert_eq!(scroll.request, 1);
        input.key(KeyCode::ArrowDown, false, false, 5, &mut scroll);
        scroll.can_down = false;
        scroll.can_up = true;
        input.key(KeyCode::ArrowDown, true, false, 6, &mut scroll);
        assert_eq!(input.selection, action_count + 1);
        assert!(!scroll.can_up && !scroll.can_down);
        input.key(KeyCode::ArrowUp, true, false, 7, &mut scroll);
        assert_eq!(input.selection, action_count);
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
        assert!(input.selection >= action_count);
    }

    #[test]
    fn capture_batch_keeps_settings_and_binding_gates_immediate() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let mut input = controlled_input();
        let mut scroll = MenuScroll::default();
        select(&mut input, MenuAction::Players);
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
            [(Control::Settings(SettingsAction::Open), 2)]
        );
        input.set_settings_open(false);
        select(&mut input, MenuAction::Start);
        input.key(KeyCode::Enter, true, false, 5, &mut scroll);
        input.pad_button(pad, GamepadButton::South, true, 6, &mut scroll);
        assert!(input.queued.is_empty());

        let mut input = controlled_input();
        input.players_page(true, &mut scroll);
        select(
            &mut input,
            MenuAction::Bind(Binding::Keyboard(PlayerId::P1)),
        );
        let binding_selection = input.selection;
        next_frame(&mut input);
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
        next_frame(&mut input);
        input.key(KeyCode::ArrowDown, true, false, 11, &mut scroll);
        assert_eq!(scroll.request, 1);
        assert_eq!(input.selection, binding_selection);
        assert_eq!(input.status, notice);
        assert!(input.binding.is_some());
        input.key(KeyCode::KeyD, true, false, 12, &mut scroll);
        assert_eq!(input.keys, [KeyCode::KeyD, KeyCode::KeyJ]);
        assert!(input.binding.is_none());
        assert!(!scroll.can_down && scroll.request == 0);
        assert!(input.queued.is_empty());
        input.set_menu_open(false);
        input.key(KeyCode::KeyD, true, false, 13, &mut scroll);
        assert!(input.queued.is_empty());
        next_frame(&mut input);
        input.key(KeyCode::KeyD, false, false, 14, &mut scroll);
        input.key(KeyCode::KeyD, true, false, 15, &mut scroll);
        assert_eq!(input.queued[0].control, Control::Hit(PlayerId::P1));
        assert_eq!(input.queued[0].monotonic_ns, 15);
    }

    #[test]
    fn two_controllers_join_without_stealing_and_keep_independent_hits() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let mut input = controlled_input();
        let mut scroll = MenuScroll::default();
        input.players_page(true, &mut scroll);
        select(&mut input, MenuAction::Bind(Binding::JoinPad(PlayerId::P1)));
        input.activate(None, 0, &mut scroll);
        next_frame(&mut input);
        input.pad_button(first, GamepadButton::South, true, 1, &mut scroll);
        assert_eq!(input.pads, [Some(first), None]);
        assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
        select(&mut input, MenuAction::Bind(Binding::JoinPad(PlayerId::P2)));
        input.activate(None, 2, &mut scroll);
        next_frame(&mut input);
        input.pad_button(first, GamepadButton::South, false, 3, &mut scroll);
        input.pad_button(first, GamepadButton::South, true, 4, &mut scroll);
        assert_eq!(input.status.key, "input.controller_assigned");
        assert_eq!(input.binding, Some(Binding::JoinPad(PlayerId::P2)));
        input.pad_button(second, GamepadButton::South, true, 5, &mut scroll);
        assert_eq!(input.pads, [Some(first), Some(second)]);
        select(
            &mut input,
            MenuAction::Bind(Binding::PadButton(PlayerId::P2)),
        );
        input.activate(None, 6, &mut scroll);
        next_frame(&mut input);
        input.pad_button(first, GamepadButton::West, true, 7, &mut scroll);
        assert_eq!(input.binding, Some(Binding::PadButton(PlayerId::P2)));
        input.pad_button(second, GamepadButton::West, true, 8, &mut scroll);
        assert_eq!(
            input.pad_buttons,
            [GamepadButton::South, GamepadButton::West]
        );
        assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
        input.set_menu_open(false);
        next_frame(&mut input);
        input.pad_button(first, GamepadButton::South, true, 9, &mut scroll);
        input.pad_button(second, GamepadButton::West, true, 9, &mut scroll);
        assert!(input.queued.is_empty());
        for (pad, button) in [(first, GamepadButton::South), (second, GamepadButton::West)] {
            input.pad_button(pad, button, false, 10, &mut scroll);
            input.pad_button(pad, button, true, 11, &mut scroll);
        }
        assert_eq!(
            input
                .queued
                .iter()
                .map(|event| event.control)
                .collect::<Vec<_>>(),
            [Control::Hit(PlayerId::P1), Control::Hit(PlayerId::P2)]
        );
        assert!(
            input
                .queued
                .iter()
                .all(|event| event.input_kind == InputKind::GamepadMessage)
        );
        input.disconnect_pad(second);
        assert_eq!(input.pads, [Some(first), None]);
        assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
        assert_eq!(input.queued.last().unwrap().control, Control::FocusLost);
        assert!(input.held_pad_buttons.iter().all(|(pad, _)| *pad == first));
    }

    #[test]
    fn unassigning_a_controller_keeps_menu_ownership_and_allows_either_mixed_arrangement() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let other_pad = world.spawn_empty().id();
        for from in [PlayerId::P1, PlayerId::P2] {
            let to = if from == PlayerId::P1 {
                PlayerId::P2
            } else {
                PlayerId::P1
            };
            for other_assigned in [false, true] {
                let mut input = controlled_input();
                let mut scroll = MenuScroll::default();
                input.bind_key(PlayerId::P1, KeyCode::KeyD).unwrap();
                input.bind_key(PlayerId::P2, KeyCode::KeyK).unwrap();
                input.join_pad(from, pad);
                if other_assigned {
                    input.join_pad(to, other_pad);
                    input.pad_buttons = [GamepadButton::West, GamepadButton::North];
                }
                let buttons = input.pad_buttons;
                input.claim_menu(InputSource::Pad(pad));
                input.players_page(true, &mut scroll);
                select(&mut input, MenuAction::UnassignPad(from));
                next_frame(&mut input);
                input.pad_button(pad, GamepadButton::South, true, 1, &mut scroll);
                input.pad_button(other_pad, GamepadButton::Start, true, 2, &mut scroll);
                assert_eq!(input.pads[from.index()], None);
                assert_eq!(input.pads[to.index()], other_assigned.then_some(other_pad));
                assert_eq!(input.menu_owner, Some(InputSource::Pad(pad)));
                assert_eq!(input.keys, [KeyCode::KeyD, KeyCode::KeyK]);
                assert_eq!(input.pad_buttons, buttons);
                assert!(input.queued.is_empty());
                assert!(
                    input
                        .held_pad_buttons
                        .contains(&(pad, GamepadButton::South))
                );
                let actions = input.menu_actions();
                assert!(!actions.contains(&MenuAction::UnassignPad(from)));
                assert_eq!(
                    actions[input.selection],
                    MenuAction::Bind(Binding::JoinPad(from))
                );
                let presentation = input
                    .menu_presentation(Locale::EnUs, String::new(), vec![])
                    .unwrap();
                assert_eq!(
                    presentation.rows.iter().filter(|row| row.selected).count(),
                    1
                );
                if other_assigned {
                    continue;
                }
                assert!(
                    actions
                        .iter()
                        .all(|action| !matches!(action, MenuAction::UnassignPad(_)))
                );
                select(&mut input, MenuAction::Bind(Binding::JoinPad(to)));
                next_frame(&mut input);
                input.pad_button(pad, GamepadButton::South, true, 3, &mut scroll);
                assert_eq!(input.pads, [None, None]);
                input.pad_button(pad, GamepadButton::South, false, 4, &mut scroll);
                input.pad_button(pad, GamepadButton::South, true, 5, &mut scroll);
                input.key(KeyCode::Enter, true, false, 6, &mut scroll);
                assert_eq!(input.pads[from.index()], None);
                assert_eq!(input.pads[to.index()], Some(pad));
                assert_eq!(input.menu_owner, Some(InputSource::Pad(pad)));
                assert!(input.queued.is_empty());
                input.set_menu_open(false);
                next_frame(&mut input);
                input.pad_button(pad, GamepadButton::South, true, 7, &mut scroll);
                assert!(input.queued.is_empty());
                input.pad_button(pad, GamepadButton::South, false, 8, &mut scroll);
                input.pad_button(pad, GamepadButton::South, true, 9, &mut scroll);
                input.key(input.keys[from.index()], true, false, 10, &mut scroll);
                assert_eq!(
                    input
                        .queued
                        .iter()
                        .map(|event| event.control)
                        .collect::<Vec<_>>(),
                    [Control::Hit(to), Control::Hit(from)]
                );
            }
        }
    }

    #[test]
    fn disconnecting_a_controller_preserves_the_selected_player_action() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        for (selected, after) in [
            (
                MenuAction::Bind(Binding::JoinPad(PlayerId::P2)),
                MenuAction::Bind(Binding::JoinPad(PlayerId::P2)),
            ),
            (
                MenuAction::UnassignPad(PlayerId::P1),
                MenuAction::Bind(Binding::JoinPad(PlayerId::P1)),
            ),
            (MenuAction::Back, MenuAction::Back),
        ] {
            let mut input = controlled_input();
            let mut scroll = MenuScroll::default();
            input.join_pad(PlayerId::P1, first);
            input.join_pad(PlayerId::P2, second);
            input.players_page(true, &mut scroll);
            select(&mut input, selected);
            input.disconnect_pad(first);
            let presentation = input
                .menu_presentation(Locale::EnUs, String::new(), vec![])
                .unwrap();
            assert_eq!(input.pads, [None, Some(second)]);
            assert_eq!(input.menu_actions()[input.selection], after);
            assert_eq!(
                presentation.rows[input.selection].text,
                after.label(Locale::EnUs, None, false)
            );
            assert_eq!(
                presentation.rows.iter().filter(|row| row.selected).count(),
                1
            );
        }
    }

    #[test]
    fn two_sticks_navigate_once_per_centering_and_keep_mode_barriers() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let mut input = InputState::default();
        let mut scroll = MenuScroll::default();
        let y = GamepadAxis::LeftStickY;
        let x = GamepadAxis::LeftStickX;
        input.pad_axis(first, y, -0.9, 1, &mut scroll);
        assert!(input.menu_owner.is_none());
        assert_eq!(input.selection, 0);
        input.pad_button(first, GamepadButton::Start, true, 2, &mut scroll);
        next_frame(&mut input);
        input.pad_axis(first, y, -0.8, 3, &mut scroll);
        assert_eq!(input.selection, 0);
        input.pad_axis(first, y, 0.0, 4, &mut scroll);
        input.pad_axis(first, y, -0.59, 5, &mut scroll);
        assert_eq!(input.selection, 0);
        input.pad_axis(first, y, -0.8, 6, &mut scroll);
        input.pad_axis(first, y, -0.4, 7, &mut scroll);
        input.pad_axis(first, y, 0.9, 8, &mut scroll);
        input.pad_axis(second, y, -0.9, 9, &mut scroll);
        assert_eq!(input.menu_actions()[input.selection], MenuAction::Players);
        input.pad_button(second, GamepadButton::Start, true, 10, &mut scroll);
        assert_eq!(input.menu_owner, Some(InputSource::Pad(second)));
        next_frame(&mut input);
        input.pad_axis(second, y, -0.9, 11, &mut scroll);
        assert_eq!(input.menu_actions()[input.selection], MenuAction::Players);
        input.pad_axis(second, y, 0.0, 12, &mut scroll);
        input.pad_axis(second, y, -0.9, 13, &mut scroll);
        assert_eq!(input.menu_actions()[input.selection], MenuAction::Settings);
        input.set_settings_open(true);
        next_frame(&mut input);
        input.pad_axis(second, x, 0.9, 14, &mut scroll);
        input.pad_axis(second, x, 0.2, 15, &mut scroll);
        input.pad_axis(second, x, -0.9, 16, &mut scroll);
        assert_eq!(
            input
                .queued
                .iter()
                .map(|event| event.control)
                .collect::<Vec<_>>(),
            [
                Control::Settings(SettingsAction::Next),
                Control::Settings(SettingsAction::Previous)
            ]
        );
        input.set_settings_open(false);
        input.set_controls_enabled(false);
        input.pad_axis(second, y, 0.0, 17, &mut scroll);
        input.pad_axis(second, y, -0.9, 18, &mut scroll);
        input.set_controls_enabled(true);
        next_frame(&mut input);
        input.pad_axis(second, y, -0.9, 19, &mut scroll);
        assert_eq!(input.menu_actions()[input.selection], MenuAction::Settings);
        input.disconnect_pad(first);
        assert_eq!(input.menu_owner, Some(InputSource::Pad(second)));
        input.disconnect_pad(second);
        assert!(input.menu_owner.is_none());
        assert!(input.held_pad_axes.is_empty());
    }

    #[test]
    fn capture_preserves_stick_button_order_and_ignores_connection_edges() {
        use bevy::input::gamepad::{
            GamepadAxisChangedEvent, GamepadButtonChangedEvent, GamepadConnection,
        };
        let mut app = capture_app();
        let first = app.world_mut().spawn_empty().id();
        let second = app.world_mut().spawn_empty().id();
        app.world_mut()
            .resource_mut::<InputState>()
            .claim_menu(InputSource::Pad(first));
        let button = |pad, state, value| {
            GamepadEvent::Button(GamepadButtonChangedEvent::new(
                pad,
                GamepadButton::South,
                state,
                value,
            ))
        };
        app.world_mut()
            .write_message(GamepadEvent::Axis(GamepadAxisChangedEvent::new(
                first,
                GamepadAxis::LeftStickY,
                -0.8,
            )));
        app.world_mut()
            .write_message(button(first, ButtonState::Pressed, 1.0));
        app.update();
        assert!(app.world().resource::<InputState>().players_open);
        assert!(app.world().resource::<InputState>().queued.is_empty());
        app.world_mut()
            .resource_scope(|world, mut input: Mut<InputState>| {
                input.activate(None, 0, &mut world.resource_mut::<MenuScroll>());
            });
        app.world_mut().write_message(GamepadConnectionEvent::new(
            second,
            GamepadConnection::Connected {
                name: "P2 test controller".into(),
                vendor_id: None,
                product_id: None,
            },
        ));
        app.world_mut()
            .write_message(button(second, ButtonState::Pressed, 1.0));
        app.update();
        assert_eq!(app.world().resource::<InputState>().pads, [None, None]);
        assert!(
            app.world()
                .resource::<InputState>()
                .held_pad_buttons
                .contains(&(second, GamepadButton::South))
        );
        app.world_mut()
            .write_message(button(second, ButtonState::Pressed, 0.9));
        app.update();
        assert_eq!(app.world().resource::<InputState>().pads, [None, None]);
        app.world_mut()
            .write_message(button(second, ButtonState::Released, 0.0));
        app.world_mut()
            .write_message(button(second, ButtonState::Pressed, 1.0));
        app.update();
        assert_eq!(
            app.world().resource::<InputState>().pads,
            [Some(second), None]
        );
        app.world_mut().write_message(GamepadConnectionEvent::new(
            second,
            GamepadConnection::Disconnected,
        ));
        app.world_mut()
            .write_message(button(second, ButtonState::Released, 0.0));
        app.world_mut()
            .write_message(button(second, ButtonState::Pressed, 1.0));
        app.world_mut()
            .write_message(GamepadEvent::Axis(GamepadAxisChangedEvent::new(
                second,
                GamepadAxis::LeftStickY,
                0.8,
            )));
        app.update();
        let input = app.world().resource::<InputState>();
        assert_eq!(input.pads, [None, None]);
        assert_eq!(input.status.key, "input.controller_disconnected");
        assert!(
            !input
                .held_pad_buttons
                .contains(&(second, GamepadButton::South))
        );
        assert!(
            !input
                .held_pad_axes
                .contains(&(second, GamepadAxis::LeftStickY))
        );
        assert!(
            input
                .queued
                .iter()
                .all(|event| event.control == Control::FocusLost)
        );
    }

    #[test]
    fn menu_ownership_consumes_claims_and_preserves_binding_transactions() {
        use bevy::input::{
            gamepad::GamepadButtonChangedEvent,
            keyboard::{Key, NativeKey},
        };
        let mut app = capture_app();
        let first = app.world_mut().spawn_empty().id();
        let second = app.world_mut().spawn_empty().id();
        let button = |pad, button, state| {
            GamepadEvent::Button(GamepadButtonChangedEvent::new(
                pad,
                button,
                state,
                if state == ButtonState::Pressed {
                    1.0
                } else {
                    0.0
                },
            ))
        };
        let key = |key_code, state| KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: first,
        };
        let press = ButtonState::Pressed;
        let release = ButtonState::Released;
        let south = GamepadButton::South;
        let east = GamepadButton::East;
        select(
            &mut app.world_mut().resource_mut::<InputState>(),
            MenuAction::Players,
        );
        app.world_mut().write_message(button(first, south, press));
        app.world_mut().write_message(button(second, south, press));
        app.world_mut()
            .write_message(button(second, south, release));
        app.update();
        let input = app.world().resource::<InputState>();
        assert_eq!(input.menu_owner, Some(InputSource::Pad(first)));
        assert!(!input.players_open && input.queued.is_empty());
        assert!(input.menu_owner_hint(Locale::EnUs).contains("Controller 1"));
        assert!(!input.held_pad_buttons.contains(&(second, south)));
        app.world_mut().write_message(button(first, south, release));
        app.world_mut().write_message(button(first, south, press));
        app.world_mut().write_message(button(second, south, press));
        app.update();
        let input = app.world().resource::<InputState>();
        assert!(input.players_open && input.binding.is_none());
        assert_eq!(input.pads, [None, None]);
        app.world_mut()
            .write_message(button(second, GamepadButton::DPadDown, press));
        app.world_mut().write_message(button(second, east, press));
        app.update();
        assert_eq!(app.world().resource::<InputState>().selection, 0);
        assert!(app.world().resource::<InputState>().players_open);

        // The owner requests a target keyboard binding; Enter takes over without submitting it
        select(
            &mut app.world_mut().resource_mut::<InputState>(),
            MenuAction::Bind(Binding::Keyboard(PlayerId::P2)),
        );
        app.world_mut().write_message(button(first, south, release));
        app.world_mut().write_message(button(first, south, press));
        app.update();
        assert_eq!(
            app.world().resource::<InputState>().binding,
            Some(Binding::Keyboard(PlayerId::P2))
        );
        app.world_mut().write_message(key(KeyCode::Enter, press));
        app.world_mut().write_message(button(first, east, press));
        app.update();
        let input = app.world().resource::<InputState>();
        assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
        assert_eq!(input.binding, Some(Binding::Keyboard(PlayerId::P2)));
        app.world_mut()
            .write_message(button(first, GamepadButton::Start, press));
        app.update();
        assert_eq!(
            app.world().resource::<InputState>().menu_owner,
            Some(InputSource::Pad(first))
        );
        app.world_mut().write_message(key(KeyCode::Enter, release));
        app.world_mut().write_message(key(KeyCode::KeyD, press));
        app.world_mut().write_message(button(first, south, release));
        app.world_mut().write_message(button(first, south, press));
        app.update();
        let input = app.world().resource::<InputState>();
        assert_eq!(input.keys[1], KeyCode::KeyD);
        assert_eq!(input.menu_owner, Some(InputSource::Pad(first)));
        assert!(input.binding.is_none());
        let title = app
            .world_mut()
            .resource_mut::<InputState>()
            .menu_presentation(Locale::ZhCn, "已暂停".into(), vec![])
            .unwrap()
            .title;
        assert_eq!(title, "双人输入 · 已暂停");

        // Loss of the menu device does not cancel a different target's pending binding
        app.world_mut().write_message(button(first, south, release));
        app.world_mut().write_message(button(first, south, press));
        app.update();
        assert_eq!(
            app.world().resource::<InputState>().binding,
            Some(Binding::Keyboard(PlayerId::P2))
        );
        app.world_mut().write_message(GamepadConnectionEvent::new(
            first,
            bevy::input::gamepad::GamepadConnection::Disconnected,
        ));
        app.world_mut()
            .write_message(button(second, GamepadButton::Start, press));
        app.update();
        let input = app.world().resource::<InputState>();
        assert!(input.menu_owner.is_none());
        assert_eq!(input.binding, Some(Binding::Keyboard(PlayerId::P2)));
        app.world_mut().write_message(key(KeyCode::KeyD, release));
        app.world_mut().write_message(key(KeyCode::KeyK, press));
        app.update();
        let input = app.world().resource::<InputState>();
        assert!(input.menu_owner.is_none() && input.binding.is_none());
        assert_eq!(input.keys[1], KeyCode::KeyK);
        app.world_mut().write_message(key(KeyCode::Enter, press));
        app.update();
        assert_eq!(
            app.world().resource::<InputState>().menu_owner,
            Some(InputSource::Keyboard)
        );
        assert!(app.world().resource::<InputState>().binding.is_none());
        app.world_mut()
            .resource_mut::<InputState>()
            .set_settings_open(true);
        app.world_mut()
            .write_message(button(second, GamepadButton::Start, release));
        app.world_mut()
            .write_message(button(second, GamepadButton::Start, press));
        app.world_mut()
            .write_message(button(second, south, release));
        app.world_mut().write_message(button(second, south, press));
        app.update();
        let input = app.world().resource::<InputState>();
        assert!(input.settings_open);
        assert_eq!(input.menu_owner, Some(InputSource::Pad(second)));
        assert!(
            input
                .queued
                .iter()
                .all(|event| event.control == Control::FocusLost)
        );
    }

    #[test]
    fn two_keyboard_seats_retain_message_kind_without_inventing_device_identity() {
        let mut input = controlled_input();
        let mut scroll = MenuScroll::default();
        input.set_menu_open(false);
        next_frame(&mut input);
        input.key(KeyCode::KeyF, true, false, 1, &mut scroll);
        input.key(KeyCode::KeyJ, true, false, 2, &mut scroll);
        assert_eq!(
            input
                .queued
                .iter()
                .map(|event| (event.control, event.input_kind))
                .collect::<Vec<_>>(),
            [
                (Control::Hit(PlayerId::P1), InputKind::KeyboardMessage),
                (Control::Hit(PlayerId::P2), InputKind::KeyboardMessage)
            ]
        );
    }

    #[test]
    fn mixed_keyboard_and_controller_players_keep_hits_and_pause_sources() {
        let mut world = World::new();
        let pad = world.spawn_empty().id();
        let stranger = world.spawn_empty().id();
        for pad_player in [PlayerId::P1, PlayerId::P2] {
            let keyboard_player = if pad_player == PlayerId::P1 {
                PlayerId::P2
            } else {
                PlayerId::P1
            };
            let mut input = controlled_input();
            let mut scroll = MenuScroll::default();
            input.join_pad(pad_player, pad);
            input.set_menu_open(false);
            next_frame(&mut input);
            let key = input.keys[keyboard_player.index()];
            input.key(key, true, false, 1, &mut scroll);
            input.pad_button(pad, GamepadButton::South, true, 2, &mut scroll);
            assert_eq!(
                input
                    .queued
                    .iter()
                    .map(|event| event.control)
                    .collect::<Vec<_>>(),
                [Control::Hit(keyboard_player), Control::Hit(pad_player)]
            );
            assert_eq!(
                input
                    .queued
                    .iter()
                    .map(|event| event.input_kind)
                    .collect::<Vec<_>>(),
                [InputKind::KeyboardMessage, InputKind::GamepadMessage]
            );
            input.pad_button(stranger, GamepadButton::Start, true, 3, &mut scroll);
            assert_eq!(input.queued.len(), 2);
            input.pad_button(pad, GamepadButton::Start, true, 4, &mut scroll);
            input.key(KeyCode::Escape, true, false, 5, &mut scroll);
            assert_eq!(
                input.queued.last().unwrap().control,
                Control::TogglePause(InputSource::Pad(pad))
            );
            assert_eq!(input.queued.len(), 3);
            assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
            input.claim_menu(InputSource::Pad(pad));
            input.set_menu_open(true);
            assert!(
                input
                    .menu_owner_hint(Locale::EnUs)
                    .contains(&format!("{pad_player:?}"))
            );
            next_frame(&mut input);
            input.key(KeyCode::Enter, true, false, 6, &mut scroll);
            assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
            assert_eq!(input.pads[pad_player.index()], Some(pad));
            assert!(input.queued.is_empty());
            input.set_menu_open(false);
            next_frame(&mut input);
            input.key(KeyCode::Escape, false, false, 7, &mut scroll);
            input.key(KeyCode::Escape, true, false, 8, &mut scroll);
            input.pad_button(pad, GamepadButton::Start, false, 9, &mut scroll);
            input.pad_button(pad, GamepadButton::Start, true, 10, &mut scroll);
            assert_eq!(
                input.queued[0].control,
                Control::TogglePause(InputSource::Keyboard)
            );
            assert_eq!(input.queued.len(), 1);
            input.disconnect_pad(stranger);
            assert_eq!(input.queued.len(), 1);
            input.disconnect_pad(pad);
            assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
            assert_eq!(input.queued[0].control, Control::FocusLost);
        }
    }

    #[test]
    fn asynchronous_transitions_block_capture_side_menu_mutations() {
        use bevy::input::keyboard::{Key, NativeKey};
        let mut app = capture_app();
        let window = app.world_mut().spawn_empty().id();
        let key = |key_code, state| KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        };
        app.world_mut()
            .resource_mut::<InputState>()
            .claim_menu(InputSource::Keyboard);
        select(
            &mut app.world_mut().resource_mut::<InputState>(),
            MenuAction::Settings,
        );
        app.world_mut()
            .resource_mut::<InputState>()
            .set_menu_phase(MenuPhase::Paused, false);
        select(
            &mut app.world_mut().resource_mut::<InputState>(),
            MenuAction::Settings,
        );
        app.world_mut()
            .write_message(key(KeyCode::F5, ButtonState::Pressed));
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Pressed));
        app.update();
        assert!(!app.world().resource::<InputState>().settings_open);
        assert_eq!(
            app.world().resource::<InputState>().queued[0].control,
            Control::Restart
        );
        app.world_mut()
            .resource_mut::<InputState>()
            .set_menu_transitioning(true);
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Released));
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Pressed));
        app.update();
        assert!(!app.world().resource::<InputState>().settings_open);
        app.world_mut()
            .resource_mut::<InputState>()
            .set_menu_transitioning(false);
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Pressed));
        app.update();
        assert!(!app.world().resource::<InputState>().settings_open);
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Released));
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Pressed));
        app.update();
        assert!(app.world().resource::<InputState>().settings_open);
        // Opening settings in First must survive a joined controller disconnect in PreUpdate
        let pad = app.world_mut().spawn_empty().id();
        {
            let mut input = app.world_mut().resource_mut::<InputState>();
            input.open_main_menu();
            input.join_pad(PlayerId::P2, pad);
            select(&mut input, MenuAction::Settings);
        }
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Released));
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Pressed));
        app.world_mut().write_message(GamepadConnectionEvent::new(
            pad,
            bevy::input::gamepad::GamepadConnection::Disconnected,
        ));
        app.update();
        let input = app.world().resource::<InputState>();
        assert!(input.settings_open);
        assert_eq!(
            input
                .queued
                .iter()
                .map(|event| event.control)
                .collect::<Vec<_>>(),
            [Control::Settings(SettingsAction::Open), Control::FocusLost]
        );
        let other = app.world_mut().spawn_empty().id();
        {
            let mut input = app.world_mut().resource_mut::<InputState>();
            input.queued.clear();
            input.join_pad(PlayerId::P1, other);
        }
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Released));
        app.world_mut()
            .write_message(key(KeyCode::Enter, ButtonState::Pressed));
        app.world_mut().write_message(GamepadConnectionEvent::new(
            other,
            bevy::input::gamepad::GamepadConnection::Disconnected,
        ));
        app.update();
        let input = app.world().resource::<InputState>();
        assert!(input.settings_open);
        assert_eq!(
            input
                .queued
                .iter()
                .map(|event| event.control)
                .collect::<Vec<_>>(),
            [Control::FocusLost]
        );
    }

    #[test]
    fn source_import_start_gate_survives_return_and_needs_release_after_completion() {
        let mut input = controlled_input();
        let mut scroll = MenuScroll::default();
        input.set_source_import_busy(true);
        input.show_library_page(vec![], true, LibraryMenu::Confirm);
        assert_eq!(input.menu_actions(), vec![MenuAction::LibraryImportBack]);
        input.open_main_menu();
        next_frame(&mut input);
        assert!(!input.menu_actions().contains(&MenuAction::Start));
        input.key(KeyCode::Enter, true, false, 1, &mut scroll);
        assert!(input.queued.is_empty());
        input.open_main_menu();
        input.set_source_import_busy(false);
        next_frame(&mut input);
        assert!(input.menu_actions().contains(&MenuAction::Start));
        input.key(KeyCode::Enter, true, false, 2, &mut scroll);
        assert!(input.queued.is_empty());
        input.key(KeyCode::Enter, false, false, 3, &mut scroll);
        input.key(KeyCode::Enter, true, false, 4, &mut scroll);
        assert_eq!(input.queued[0].control, Control::Start);
    }

    #[test]
    fn paired_source_pages_keep_mixed_and_dual_controller_ownership_and_fresh_confirmation() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        for mode in 0..3 {
            let mut input = controlled_input();
            let mut scroll = MenuScroll::default();
            let use_pad = mode != 0;
            if mode != 0 {
                input.join_pad(PlayerId::P1, first);
            }
            if mode != 1 {
                input.join_pad(PlayerId::P2, second);
            }
            input.claim_menu(if use_pad {
                InputSource::Pad(first)
            } else {
                InputSource::Keyboard
            });
            input.show_library_page(vec!["song.wav".into()], false, LibraryMenu::Sources);
            next_frame(&mut input);
            if use_pad {
                input.pad_button(first, GamepadButton::South, true, 1, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, false, 1, &mut scroll);
            }
            assert_eq!(
                input.queued[0].control,
                Control::Library(LibraryAction::Select(0))
            );
            input.show_library_page(vec![], false, LibraryMenu::Confirm);
            next_frame(&mut input);
            if use_pad {
                input.pad_button(first, GamepadButton::South, true, 2, &mut scroll);
            } else {
                input.key(KeyCode::Enter, true, false, 2, &mut scroll);
            }
            assert!(input.queued.is_empty());
            if mode == 1 {
                input.key(KeyCode::Enter, true, false, 3, &mut scroll);
                assert_eq!(input.menu_owner, Some(InputSource::Keyboard));
            } else {
                input.pad_button(second, GamepadButton::Start, true, 3, &mut scroll);
                assert_eq!(input.menu_owner, Some(InputSource::Pad(second)));
            }
            assert!(input.queued.is_empty());
            next_frame(&mut input);
            if mode == 1 {
                input.key(KeyCode::Enter, false, false, 4, &mut scroll);
                input.key(KeyCode::Enter, true, false, 5, &mut scroll);
            } else {
                input.pad_button(second, GamepadButton::South, true, 5, &mut scroll);
            }
            assert_eq!(input.queued.len(), 1);
            assert_eq!(
                input.queued[0].control,
                Control::Library(LibraryAction::ImportConfirm)
            );
            assert_eq!(input.pads[0], (mode != 0).then_some(first));
            assert_eq!(input.pads[1], (mode != 1).then_some(second));
        }
    }
}
