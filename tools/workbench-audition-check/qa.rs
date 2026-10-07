use super::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::text::TextLayoutInfo;
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
}

impl Default for Driver {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            frames: 0,
            step: 0,
            saved: 0,
            pending: false,
        }
    }
}

pub(super) fn drive(mut driver: ResMut<Driver>, mut state: ResMut<Workbench>) {
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
