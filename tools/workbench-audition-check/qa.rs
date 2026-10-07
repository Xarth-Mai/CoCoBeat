use super::*;
use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput, NativeKey},
};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::text::TextLayoutInfo;
use bevy::window::WindowFocused;
use serde_json::json;
use std::time::Instant;

pub(super) fn size() -> (u32, u32) {
    match std::env::var("QA_SIZE").expect("QA_SIZE").as_str() {
        "640" => (640, 480),
        "1280" => (1280, 800),
        _ => panic!("Unsupported QA_SIZE"),
    }
}

#[derive(Resource)]
pub(super) struct Driver {
    start: Instant,
    frames: u32,
    step: usize,
    saved: usize,
    pending: bool,
    audio: bool,
    stable: Option<(Instant, i64)>,
    initial_cursor: Option<i64>,
    initial_selected: Option<usize>,
    requested_capture: Option<&'static str>,
}

impl Default for Driver {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            frames: 0,
            step: 0,
            saved: 0,
            pending: false,
            audio: std::env::var_os("QA_REAL_AUDIO").is_some(),
            stable: None,
            initial_cursor: None,
            initial_selected: None,
            requested_capture: None,
        }
    }
}

pub(super) fn display(mut driver: ResMut<Driver>, mut state: ResMut<Workbench>) {
    if driver.audio {
        return;
    }
    assert!(driver.start.elapsed().as_secs() < 25, "GUI QA timed out");
    driver.frames += 1;
    if driver.step == 1 {
        // This is an explicit visual control, not an audio-device observation
        state.audition.position = Some(state.document.end / 2);
        state.audition.target = Some(state.document.end * 3 / 4);
        state.audition.status = "audition.paused";
    }
}

pub(super) fn capture(
    mut commands: Commands,
    mut driver: ResMut<Driver>,
    state: Res<Workbench>,
    boxes: Query<(
        &ui::BoxPart,
        &ComputedNode,
        &UiGlobalTransform,
        Option<&ImageNode>,
    )>,
    texts: Query<(&ui::Part, &Text, &TextLayoutInfo)>,
    images: Res<Assets<Image>>,
    mut exit: MessageWriter<AppExit>,
) {
    if driver.audio {
        if let Some(name) = driver.requested_capture.take() {
            let path =
                PathBuf::from(std::env::var("QA_CAPTURE_DIR").unwrap()).join(format!("{name}.png"));
            assert!(!path.exists());
            driver.pending = true;
            commands.spawn(Screenshot::primary_window()).observe(
                move |captured: On<ScreenshotCaptured>,
                      mut driver: ResMut<Driver>,
                      state: Res<Workbench>| {
                    captured
                        .image
                        .clone()
                        .try_into_dynamic()
                        .unwrap()
                        .save(&path)
                        .unwrap();
                    driver.saved += 1;
                    driver.pending = false;
                    record(name, &state);
                },
            );
        }
        return;
    }
    assert_eq!(
        state.document.cursor, 0,
        "Audio must not change the edit cursor"
    );
    assert_eq!(state.document.editor.anchors(), state.document.original);
    assert!(!state.document.dirty && state.saving.is_none());
    if driver.pending
        || driver.frames < 12
        || !texts
            .iter()
            .any(|(part, _, layout)| matches!(part, ui::Part::Title) && !layout.glyphs.is_empty())
    {
        return;
    }
    if driver.step == 2 {
        assert_eq!(driver.saved, 2);
        println!(
            "QA {}",
            json!({"event":"finished","captures":2,"input_audio_device":"NOT RUN","visual_control":"Explicit synthetic callback-cursor display; actual Kira callback behavior is checked separately with MockBackend"})
        );
        exit.write(AppExit::Success);
        return;
    }
    let mut toolbar_count = 0;
    let (width, height) = size();
    for (part, node, transform, _) in &boxes {
        if matches!(part, ui::BoxPart::Toolbar(_)) {
            let rect = ui::logical_rect(node, transform);
            assert!(
                rect.min.x >= 0.0
                    && rect.min.y >= 0.0
                    && rect.max.x <= width as f32
                    && rect.max.y <= height as f32
            );
            toolbar_count += 1;
        }
    }
    assert_eq!(toolbar_count, 11);
    let audition_text = texts
        .iter()
        .find_map(|(part, text, _)| matches!(part, ui::Part::Audition).then_some(&text.0))
        .unwrap();
    assert!(audition_text.contains(state.locale.text(state.audition.status_key())));
    if driver.step == 1 {
        assert!(audition_text.contains(&(state.document.end / 2).to_string()));
        assert!(audition_text.contains(&(state.document.end * 3 / 4).to_string()));
        let (_, _, _, image) = boxes
            .iter()
            .find(|(part, _, _, _)| matches!(part, ui::BoxPart::Canvas))
            .unwrap();
        let image = images.get(&image.unwrap().image).unwrap();
        let width = image.texture_descriptor.size.width;
        let x = width / 2;
        let offset = x as usize * 4;
        assert_eq!(
            &image.data.as_ref().unwrap()[offset..offset + 4],
            &[255, 191, 91, 255]
        );
    }
    let name = if driver.step == 0 {
        "idle-toolbar"
    } else {
        "independent-audio-cursor"
    };
    let path = PathBuf::from(std::env::var("QA_CAPTURE_DIR").unwrap()).join(format!("{name}.png"));
    assert!(!path.exists());
    driver.pending = true;
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, mut driver: ResMut<Driver>| {
            captured
                .image
                .clone()
                .try_into_dynamic()
                .unwrap()
                .save(&path)
                .unwrap();
            driver.saved += 1;
            driver.step += 1;
            driver.pending = false;
            driver.frames = 0;
            println!("QA {}", json!({"event":"capture","name":name}));
        },
    );
}

// Real mode writes only input messages; all positions and playback states come from production
pub(super) fn drive(
    mut driver: ResMut<Driver>,
    state: Res<Workbench>,
    windows: Query<Entity, With<Window>>,
    mut keys: MessageWriter<KeyboardInput>,
    mut focus: MessageWriter<WindowFocused>,
    mut exit: MessageWriter<AppExit>,
) {
    if !driver.audio {
        return;
    }
    assert!(
        driver.start.elapsed().as_secs() < 35,
        "Real audio QA timed out"
    );
    driver.frames += 1;
    if driver.pending || driver.frames < 20 {
        return;
    }
    assert!(state.is_read_only());
    assert_eq!(state.document.editor.anchors(), state.document.original);
    assert!(!state.document.dirty && state.saving.is_none());
    let window = windows.single().unwrap();
    let step = driver.step;
    let unavailable = std::env::var_os("QA_UNAVAILABLE_AUDIO").is_some();
    let key = match step {
        0 => {
            driver.initial_cursor = Some(state.document.cursor);
            driver.initial_selected = Some(state.selected_index());
            assert_eq!(state.document.cursor, -123);
            Some(KeyCode::Enter)
        }
        1 => Some(KeyCode::Space),
        2 => {
            assert_eq!(state.document.cursor, -123);
            assert!(state.notice.contains("Audition selection"));
            record("selection_error_preserved", &state);
            Some(KeyCode::Home)
        }
        3 => Some(KeyCode::Space),
        4 if unavailable => {
            if state.audition.status != "audition.failed" {
                return;
            }
            assert!(!state.notice.is_empty());
            assert!(state.audition.position.is_none());
            assert!(!state.audition.playing);
            assert_eq!(state.document.cursor, 0);
            assert_eq!(state.selected_index(), driver.initial_selected.unwrap());
            if driver.saved == 0 {
                driver.requested_capture = Some("output-error-preserved");
                return;
            }
            record("output_error_preserved", &state);
            record("finished", &state);
            exit.write(AppExit::Success);
            return;
        }
        4 => {
            if state.audition.status != "audition.playing"
                || state.audition.position.is_none_or(|p| p < 4800)
            {
                return;
            }
            assert_eq!(state.document.cursor, 0);
            assert_eq!(state.selected_index(), driver.initial_selected.unwrap());
            record("callback_advancing", &state);
            Some(KeyCode::Space)
        }
        5 => {
            if state.audition.status != "audition.paused" {
                return;
            }
            driver.stable = Some((Instant::now(), state.audition.position.unwrap()));
            record("pause_acknowledged", &state);
            None
        }
        6 => {
            let (at, position) = driver.stable.unwrap();
            assert_eq!(state.audition.position, Some(position));
            if at.elapsed().as_millis() < 300 {
                return;
            }
            if state.focus != Focus::Timeline {
                for (key_code, state) in [
                    (KeyCode::ShiftLeft, ButtonState::Pressed),
                    (KeyCode::Tab, ButtonState::Pressed),
                    (KeyCode::Tab, ButtonState::Released),
                    (KeyCode::ShiftLeft, ButtonState::Released),
                ] {
                    keys.write(KeyboardInput {
                        key_code,
                        logical_key: Key::Unidentified(NativeKey::Unidentified),
                        state,
                        text: None,
                        repeat: false,
                        window,
                    });
                }
                println!(
                    "QA {}",
                    json!({"event":"keyboard_message","step":step,"key":"Shift+Tab","from_focus":format!("{:?}", state.focus)})
                );
                return;
            }
            record("paused_cursor_stable", &state);
            Some(KeyCode::ArrowRight)
        }
        7..=9 => Some(KeyCode::Tab),
        10..=11 => Some(KeyCode::ArrowRight),
        12 => {
            assert_eq!(state.focus, Focus::Toolbar(2));
            driver.stable = Some((Instant::now(), driver.stable.unwrap().1));
            Some(KeyCode::Enter)
        }
        13 => {
            assert_eq!(state.audition.target, Some(1));
            assert_eq!(state.document.cursor, 1);
            assert_eq!(state.audition.status, "audition.paused");
            assert_eq!(state.audition.position, Some(driver.stable.unwrap().1));
            // Three hundred milliseconds after selection proves no new callback progression
            if driver.stable.unwrap().0.elapsed().as_millis() < 300 {
                return;
            }
            if driver.saved == 0 {
                driver.requested_capture = Some("paused-seek-no-playback");
                return;
            }
            record("paused_seek_no_playback", &state);
            Some(KeyCode::Space)
        }
        14 => {
            if state.audition.status != "audition.playing"
                || state.audition.position.is_none_or(|p| p < 1000)
            {
                return;
            }
            if driver.saved == 1 {
                assert!(state.audition.position.unwrap() < driver.stable.unwrap().1);
            }
            assert_eq!(state.audition.target, Some(1));
            assert_eq!(state.document.cursor, 1);
            assert_eq!(state.selected_index(), driver.initial_selected.unwrap());
            if driver.saved == 1 {
                driver.requested_capture = Some("resumed-new-source-start");
                return;
            }
            record("resumed_new_source_start", &state);
            focus.write(WindowFocused {
                window,
                focused: false,
            });
            None
        }
        15 => {
            if state.audition.status != "audition.paused" {
                return;
            }
            driver.stable = Some((Instant::now(), state.audition.position.unwrap()));
            record("focus_loss_pause_acknowledged", &state);
            None
        }
        16 => {
            let (at, position) = driver.stable.unwrap();
            assert_eq!(state.audition.position, Some(position));
            if at.elapsed().as_millis() < 300 {
                return;
            }
            record("focus_loss_cursor_stable", &state);
            focus.write(WindowFocused {
                window,
                focused: true,
            });
            None
        }
        17 => Some(KeyCode::ArrowLeft),
        18 => {
            assert_eq!(state.focus, Focus::Toolbar(1));
            Some(KeyCode::Enter)
        }
        19 => {
            assert_eq!(state.audition.status_key(), "audition.stopped");
            assert!(state.audition.position.is_none() && state.audition.target.is_none());
            assert!(!state.audition.playing);
            assert_eq!(state.document.cursor, 1);
            assert_eq!(state.selected_index(), driver.initial_selected.unwrap());
            if driver.saved == 2 {
                driver.requested_capture = Some("stopped-selection-preserved");
                return;
            }
            record("stopped_selection_preserved", &state);
            record("finished", &state);
            exit.write(AppExit::Success);
            return;
        }
        _ => unreachable!(),
    };
    if let Some(key) = key {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            keys.write(KeyboardInput {
                key_code: key,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state,
                text: None,
                repeat: false,
                window,
            });
        }
        println!(
            "QA {}",
            json!({"event":"keyboard_message","step":step,"key":format!("{key:?}")})
        );
    }
    driver.step += 1;
    driver.frames = 20;
}

fn record(event: &str, state: &Workbench) {
    println!(
        "QA {}",
        json!({"event":event,"status":state.audition.status_key(),"position_frames":state.audition.position,"target_frames":state.audition.target,"document_cursor":state.document.cursor,"replay_selected":state.selected_index(),"notice":state.notice,"playing_intent":state.audition.playing,"focus":format!("{:?}", state.focus)})
    );
}
