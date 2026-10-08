//! Focus and event gate for the two native independent-label text fields

use super::*;
use bevy::{
    input::keyboard::KeyboardInput,
    input_focus::{FocusCause, InputFocus},
    text::{EditableText, TextEdit},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Field {
    Reviewer,
    Kind,
    Frame,
    Start,
    End,
    Decision,
    Reason,
}

impl Field {
    pub(super) fn is_text(self) -> bool {
        matches!(self, Self::Reviewer | Self::Reason)
    }
}

#[derive(Component)]
pub(super) struct LabelText(pub(super) Field);

pub(super) fn pending(text: &EditableText) -> bool {
    text.is_composing() || !text.pending_edits.is_empty() || text.pending_paste.is_some()
}

fn allowed(state: &Workbench, controls: &input::Controls) -> bool {
    state.labels.is_some()
        && controls.owner == Some(InputSource::Keyboard)
        && controls.focused
        && !controls.native_blocked
        && state.saving.is_none()
        && !state.close_confirm
}

// PreUpdate: after capture, before Dispatch and ImeSystems::HandleEvents
pub(super) fn bridge(
    state: Res<Workbench>,
    controls: Res<input::Controls>,
    mut focus: ResMut<InputFocus>,
    mut fields: Query<(Entity, &LabelText, &mut EditableText)>,
    mut keys: ResMut<Messages<KeyboardInput>>,
) {
    let active = if allowed(&state, &controls) {
        match state.focus {
            Focus::Label(field) if field.is_text() => Some(field),
            _ => None,
        }
    } else {
        None
    };
    // InputSystems and Controls have already observed key edges; only the native
    // keyboard dispatch is suppressed, while the original IME stream remains intact
    if !allowed(&state, &controls) || (active.is_some() && controls.ime_busy) {
        keys.clear();
    }
    let mut target = None;
    for (entity, field, mut text) in &mut fields {
        if let Some(labels) = &state.labels {
            let value = match field.0 {
                Field::Reviewer => labels.document.reviewer.as_str(),
                Field::Reason => labels
                    .draft
                    .as_ref()
                    .map_or("", |draft| draft.reason.as_str()),
                _ => continue,
            };
            if !pending(&text) && text.value() != value {
                text.editor_mut().set_text(value);
            }
        }
        if active == Some(field.0) {
            target = Some(entity);
        }
    }
    if target != focus.get() {
        if let Some(entity) = target {
            focus.set(entity, FocusCause::Navigated);
        } else {
            focus.clear();
        }
    }
}

// PostUpdate: after the native EditableTextSystems have applied real edits
pub(super) fn sync(
    mut state: ResMut<Workbench>,
    mut fields: Query<(&LabelText, &mut EditableText)>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(labels) = &mut state.labels else {
        return;
    };
    let busy = fields.iter().any(|(_, text)| pending(text));
    for (field, text) in &fields {
        let value = text.value().to_string();
        match field.0 {
            Field::Reviewer => labels.document.reviewer = value,
            Field::Reason => {
                if let Some(draft) = &mut labels.draft {
                    draft.reason = value;
                }
            }
            _ => {}
        }
    }
    state.refresh_label_dirty();
    if state.pending_label_action == Some(Action::Back) {
        if busy {
            state.focus = Focus::Details;
            for (_, mut text) in &mut fields {
                if text.is_composing() {
                    text.queue_edit(TextEdit::clear_ime_compose());
                }
            }
        } else {
            state.pending_label_action = None;
            state.close(&mut exit);
        }
    } else if !busy {
        state.finish_label_action();
    }
}

#[cfg(test)]
pub(super) fn settled_text(value: &str) -> EditableText {
    let mut text = EditableText::default();
    text.editor_mut().set_text(value);
    text
}

#[cfg(test)]
pub(super) fn fixture() -> labels::LabelView {
    labels::LabelView::new(crate::labels::Source {
        content_id: format!("package-blake3:{}", "07".repeat(32)),
        audio_blake3: "08".repeat(32),
        canonical_frames: 48_000,
        audio_basis: crate::labels::AudioBasis::CanonicalDecoded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::ButtonState,
        input::keyboard::{Key, NativeKey},
        input_focus::{FocusedInput, InputFocusSystems, dispatch_focused_input},
        window::{Ime, PrimaryWindow, WindowCloseRequested},
    };

    #[test]
    fn queued_native_edit_is_busy_before_composition_has_started() {
        let mut text = settled_text("reviewer");
        assert!(!pending(&text));
        text.queue_edit(TextEdit::ImeSetCompose {
            value: "中文".into(),
            cursor: None,
        });
        assert!(!text.is_composing());
        assert!(pending(&text));
        assert_eq!(text.value().to_string(), "reviewer");
    }

    #[test]
    fn apply_waits_for_pending_native_edit_and_uses_the_actual_utf8_value() {
        let mut app = App::new();
        let mut state = super::super::tests::state();
        let mut labels = fixture();
        labels.begin_add(500).unwrap();
        state.labels = Some(labels);
        state.pending_label_action = Some(Action::Apply);
        app.insert_resource(state)
            .add_message::<AppExit>()
            .add_systems(PostUpdate, sync);
        app.world_mut()
            .spawn((LabelText(Field::Reviewer), settled_text("審閱🙂")));
        let mut reason = settled_text("Українська причина 中文");
        reason.queue_edit(TextEdit::ImeSetCompose {
            value: "未提交".into(),
            cursor: None,
        });
        let entity = app
            .world_mut()
            .spawn((LabelText(Field::Reason), reason))
            .id();
        app.update();
        {
            let state = app.world().resource::<Workbench>();
            assert!(state.pending_label_action.is_some());
            assert!(state.labels.as_ref().unwrap().document.labels.is_empty());
            assert_eq!(
                state
                    .labels
                    .as_ref()
                    .unwrap()
                    .draft
                    .as_ref()
                    .unwrap()
                    .reason,
                "Українська причина 中文"
            );
        }
        app.world_mut()
            .get_mut::<EditableText>(entity)
            .unwrap()
            .pending_edits
            .clear();
        app.update();
        let state = app.world().resource::<Workbench>();
        let labels = &state.labels.as_ref().unwrap().document;
        assert!(state.pending_label_action.is_none());
        assert_eq!(labels.reviewer, "審閱🙂");
        assert_eq!(labels.labels.len(), 1);
        assert_eq!(labels.labels[0].reason, "Українська причина 中文");
    }

    fn native_app() -> (App, Entity, Entity, Entity) {
        use bevy::{
            asset::AssetPlugin,
            input_focus::InputFocusPlugin,
            text::{EditableTextSystems, TextPlugin},
            ui::UiScale,
        };
        use bevy_picking::events::{Pointer, Release};
        use bevy_ui_widgets::{EditableTextInputPlugin, ImeSystems};
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TextPlugin,
            InputFocusPlugin,
            EditableTextInputPlugin,
        ))
        .init_resource::<UiScale>()
        .init_resource::<Assets<Image>>()
        .init_resource::<ButtonInput<Key>>()
        .add_message::<KeyboardInput>()
        .add_message::<Ime>()
        .add_message::<Pointer<Release>>()
        .add_message::<AppExit>()
        .add_message::<WindowCloseRequested>()
        .add_message::<bevy::window::WindowFocused>()
        .add_message::<bevy::input::mouse::MouseButtonInput>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_message::<bevy::input::gamepad::GamepadConnectionEvent>()
        .add_message::<bevy::input::gamepad::GamepadEvent>()
        .init_resource::<input::Controls>()
        .insert_resource(ui::View::default())
        .add_systems(
            PreUpdate,
            (input::capture, bridge)
                .chain()
                .before(InputFocusSystems::Dispatch)
                .before(ImeSystems::HandleEvents),
        )
        .add_systems(
            PreUpdate,
            dispatch_focused_input::<KeyboardInput>
                .in_set(InputFocusSystems::Dispatch)
                .after(bridge),
        )
        .add_systems(PostUpdate, sync.after(EditableTextSystems));
        let mut state = super::super::tests::state();
        let mut labels = fixture();
        labels.begin_add(500).unwrap();
        state.labels = Some(labels);
        state.focus = Focus::Label(Field::Reviewer);
        app.insert_resource(state);
        app.world_mut().resource_mut::<input::Controls>().owner = Some(InputSource::Keyboard);
        // Window metadata only: no WindowPlugin, native window, renderer or audio
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let reviewer = app
            .world_mut()
            .spawn((LabelText(Field::Reviewer), EditableText::new("")))
            .id();
        let mut text = EditableText::new("");
        text.allow_newlines = true;
        let reason = app.world_mut().spawn((LabelText(Field::Reason), text)).id();
        (app, window, reviewer, reason)
    }

    #[test]
    fn native_plugin_preedit_and_commit_sync_only_committed_unicode() {
        let (mut app, window, reviewer, reason) = native_app();
        app.update();
        app.world_mut().write_message(Ime::Preedit {
            window,
            value: "審閱🙂".into(),
            cursor: Some((0, "審閱🙂".len())),
        });
        app.update();
        assert!(
            app.world()
                .get::<EditableText>(reviewer)
                .unwrap()
                .is_composing()
        );
        assert_eq!(
            app.world()
                .get::<EditableText>(reviewer)
                .unwrap()
                .value()
                .to_string(),
            ""
        );
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .labels
                .as_ref()
                .unwrap()
                .document
                .reviewer,
            ""
        );
        app.world_mut().write_message(Ime::Commit {
            window,
            value: "審閱🙂".into(),
        });
        app.update();
        assert!(
            !app.world()
                .get::<EditableText>(reviewer)
                .unwrap()
                .is_composing()
        );
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .labels
                .as_ref()
                .unwrap()
                .document
                .reviewer,
            "審閱🙂"
        );
        app.world_mut().resource_mut::<Workbench>().focus = Focus::Label(Field::Reason);
        app.update();
        app.update();
        let committed = "中文 日本語 한국어 Україна 👩🏽‍💻";
        app.world_mut()
            .resource_mut::<Workbench>()
            .pending_label_action = Some(Action::Apply);
        app.world_mut().write_message(Ime::Preedit {
            window,
            value: committed.into(),
            cursor: Some((0, committed.len())),
        });
        app.update();
        {
            let state = app.world().resource::<Workbench>();
            let labels = state.labels.as_ref().unwrap();
            assert!(labels.document.labels.is_empty());
            assert_eq!(labels.draft.as_ref().unwrap().reason, "");
            assert_eq!(state.pending_label_action, Some(Action::Apply));
            assert!(
                app.world()
                    .get::<EditableText>(reason)
                    .unwrap()
                    .is_composing()
            );
        }
        app.world_mut().write_message(Ime::Commit {
            window,
            value: committed.into(),
        });
        app.update();
        let state = app.world().resource::<Workbench>();
        let document = &state.labels.as_ref().unwrap().document;
        assert!(state.pending_label_action.is_none());
        assert_eq!(document.reviewer, "審閱🙂");
        assert_eq!(document.labels.len(), 1);
        assert_eq!(document.labels[0].reason, committed);
        assert_eq!(
            app.world()
                .get::<EditableText>(reason)
                .unwrap()
                .value()
                .to_string(),
            committed
        );
        assert!(
            !app.world()
                .get::<EditableText>(reason)
                .unwrap()
                .is_composing()
        );
    }

    #[test]
    fn native_close_request_finishes_composition_and_keeps_same_batch_text() {
        let (mut app, window, _, reason) = native_app();
        app.update();
        app.world_mut().write_message(Ime::Commit {
            window,
            value: "reviewer".into(),
        });
        app.update();
        app.world_mut().resource_mut::<Workbench>().focus = Focus::Label(Field::Reason);
        app.update();
        app.update();
        app.world_mut().write_message(Ime::Preedit {
            window,
            value: "未提交".into(),
            cursor: None,
        });
        app.world_mut()
            .write_message(WindowCloseRequested { window });
        app.update();
        assert_eq!(
            app.world().resource::<Workbench>().pending_label_action,
            Some(Action::Back)
        );
        assert!(!app.world().resource::<Workbench>().close_confirm);
        assert!(
            app.world()
                .get::<EditableText>(reason)
                .unwrap()
                .is_composing()
        );
        app.world_mut().write_message(KeyboardInput {
            window,
            key_code: KeyCode::Space,
            logical_key: Key::Space,
            state: ButtonState::Pressed,
            text: Some(" ".into()),
            repeat: false,
        });
        for _ in 0..3 {
            app.update();
        }
        {
            let state = app.world().resource::<Workbench>();
            assert!(state.pending_label_action.is_none());
            assert!(state.close_confirm);
            assert!(!state.audition.playing);
            assert_eq!(
                state
                    .labels
                    .as_ref()
                    .unwrap()
                    .draft
                    .as_ref()
                    .unwrap()
                    .reason,
                ""
            );
            assert!(
                !app.world()
                    .get::<EditableText>(reason)
                    .unwrap()
                    .is_composing()
            );
            assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        }
        app.world_mut().write_message(KeyboardInput {
            window,
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
        });
        app.update();
        assert!(!app.world().resource::<Workbench>().close_confirm);
        app.world_mut().resource_mut::<Workbench>().focus = Focus::Label(Field::Reason);
        app.update();
        app.update();
        let committed = "зберегти🙂";
        app.world_mut().write_message(KeyboardInput {
            window,
            key_code: KeyCode::KeyA,
            logical_key: Key::Character(committed.into()),
            state: ButtonState::Pressed,
            text: Some(committed.into()),
            repeat: false,
        });
        app.world_mut()
            .write_message(WindowCloseRequested { window });
        app.update();
        let state = app.world().resource::<Workbench>();
        assert!(state.close_confirm);
        assert!(state.pending_label_action.is_none());
        assert_eq!(
            state
                .labels
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .reason,
            committed
        );
        assert_eq!(
            app.world()
                .get::<EditableText>(reason)
                .unwrap()
                .value()
                .to_string(),
            committed
        );
    }

    #[test]
    fn menu_enter_does_not_insert_a_native_reason_newline() {
        let (mut app, window, _, reason) = native_app();
        app.update();
        {
            let mut state = app.world_mut().resource_mut::<Workbench>();
            let labels = state.labels.as_mut().unwrap();
            labels.document.reviewer = "reviewer".into();
            labels.cancel();
            state.focus = Focus::Toolbar(
                state
                    .toolbar()
                    .iter()
                    .position(|action| *action == Action::Add)
                    .unwrap(),
            );
        }
        app.update();
        app.update();
        let enter = |app: &mut App, pressed: bool| {
            app.world_mut().write_message(KeyboardInput {
                window,
                key_code: KeyCode::Enter,
                logical_key: Key::Enter,
                state: if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                },
                text: None,
                repeat: false,
            });
        };
        enter(&mut app, true);
        app.update();
        assert_eq!(
            app.world()
                .get::<EditableText>(reason)
                .unwrap()
                .value()
                .to_string(),
            ""
        );
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .labels
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .reason,
            ""
        );
        enter(&mut app, false);
        app.update();
        {
            let mut state = app.world_mut().resource_mut::<Workbench>();
            state
                .labels
                .as_mut()
                .unwrap()
                .draft
                .as_mut()
                .unwrap()
                .reason = "precise reason".into();
            state.focus = Focus::Label(Field::Frame);
        }
        app.update();
        app.update();
        enter(&mut app, true);
        app.update();
        assert_eq!(
            app.world()
                .get::<EditableText>(reason)
                .unwrap()
                .value()
                .to_string(),
            "precise reason"
        );
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .labels
                .as_ref()
                .unwrap()
                .document
                .labels[0]
                .reason,
            "precise reason"
        );
        enter(&mut app, false);
        app.update();
        app.world_mut().resource_mut::<Workbench>().focus = Focus::List;
        enter(&mut app, true);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .labels
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .reason,
            "precise reason"
        );
        assert_eq!(
            app.world()
                .get::<EditableText>(reason)
                .unwrap()
                .value()
                .to_string(),
            "precise reason"
        );
        enter(&mut app, false);
        app.update();
        enter(&mut app, true);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Workbench>()
                .labels
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .reason,
            "\nprecise reason"
        );
    }

    #[test]
    fn dispatch_preserves_native_events_and_consumes_a_blocked_batch() {
        let mut app = App::new();
        let mut state = super::super::tests::state();
        state.labels = Some(fixture());
        state.focus = Focus::Label(Field::Reviewer);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let field = app
            .world_mut()
            .spawn((LabelText(Field::Reviewer), settled_text("")))
            .id();
        app.insert_resource(state)
            .init_resource::<input::Controls>()
            .insert_resource(InputFocus::from_entity(field))
            .add_message::<KeyboardInput>()
            .add_message::<Ime>()
            .add_systems(
                PreUpdate,
                (bridge, dispatch_focused_input::<KeyboardInput>).chain(),
            );
        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = observed.clone();
        app.world_mut()
            .add_observer(move |event: On<FocusedInput<KeyboardInput>>| {
                sink.lock().unwrap().push(event.input.clone());
            });
        let event = KeyboardInput {
            window,
            key_code: KeyCode::KeyA,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state: ButtonState::Pressed,
            text: Some("中文🙂".into()),
            repeat: true,
        };
        {
            let mut controls = app.world_mut().resource_mut::<input::Controls>();
            controls.owner = Some(InputSource::Keyboard);
            controls.ime_busy = true;
        }
        app.world_mut().write_message(event.clone());
        app.world_mut().write_message(Ime::Preedit {
            window,
            value: "中文".into(),
            cursor: None,
        });
        app.update();
        assert!(observed.lock().unwrap().is_empty());
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
        assert_eq!(app.world().resource::<Messages<Ime>>().len(), 1);
        app.world_mut().resource_mut::<input::Controls>().ime_busy = false;
        app.update();
        assert!(observed.lock().unwrap().is_empty());
        app.world_mut().write_message(event.clone());
        app.update();
        let captured = observed.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].key_code, event.key_code);
        assert_eq!(captured[0].logical_key, event.logical_key);
        assert_eq!(captured[0].text, event.text);
        assert!(captured[0].repeat);
        assert_eq!(captured[0].window, window);
        drop(captured);
        let pad = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<input::Controls>().owner = Some(InputSource::Pad(pad));
        app.world_mut().write_message(event);
        app.update();
        assert_eq!(observed.lock().unwrap().len(), 1);
    }
}
