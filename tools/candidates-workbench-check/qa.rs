use super::*;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
    },
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    text::TextLayoutInfo,
};
use serde_json::{Value, json};
use std::time::Instant;

const RICH_CAPTURES: [&str; 5] = [
    "unknown-details",
    "list-tail",
    "dense-blocker-details",
    "accepted-wave-list",
    "details-bottom",
];

pub(super) fn size() -> (u32, u32) {
    match std::env::var("QA_SIZE").expect("QA_SIZE").as_str() {
        "640" => (640, 480),
        "1280" => (1280, 800),
        _ => panic!("QA_SIZE must be 640 or 1280"),
    }
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Key(KeyCode),
    Shortcut(KeyCode),
    Check(usize),
    Capture(&'static str),
    ScrollBottom,
    Finish,
}

#[derive(Resource)]
pub(super) struct Driver {
    start: Instant,
    output: PathBuf,
    report: Value,
    package: ValidatedPackage,
    steps: Vec<Step>,
    step: usize,
    checked: Vec<bool>,
    saved: Vec<&'static str>,
    ready: bool,
    pending: bool,
    settled: u8,
    message_count: usize,
}

impl Default for Driver {
    fn default() -> Self {
        let package = cocobeat_media::validate_package(
            std::env::args().nth(1).expect("Source package argument"),
        )
        .expect("Validated source");
        let report_path = std::env::var("QA_REPORT").expect("QA_REPORT");
        // Compare the report's declared f32 domain rather than JSON parser f64 guesses
        let report = serde_json::to_value(
            crate::anchors::load_report(&package, Path::new(&report_path))
                .expect("Validated actual proposal"),
        )
        .expect("Typed proposal JSON");
        let count = report["evidence"].as_array().expect("Evidence array").len();
        assert!(count == 0 || count == 40, "Controlled rich/empty fixture");
        assert_eq!(report["production_admission"], "not_assessed");
        assert!(
            package
                .analysis
                .diagnostics
                .contains("constructed uncalibrated")
        );
        let mut steps = vec![
            Step::Key(KeyCode::Enter),
            Step::Key(KeyCode::Tab),
            Step::Shortcut(KeyCode::KeyZ),
            Step::Shortcut(KeyCode::KeyS),
            Step::Key(KeyCode::Delete),
            Step::Key(KeyCode::Digit9),
        ];
        if count == 0 {
            steps.extend([Step::Key(KeyCode::Enter), Step::Capture("zero-candidates")]);
        } else {
            assert_eq!(report["evidence"][2]["frame"], 1000);
            assert_eq!(report["evidence"][2]["decision"]["blocking_onset_index"], 3);
            assert_eq!(report["evidence"][3]["frame"], 1200);
            assert_eq!(report["evidence"][4]["frame"], 1201);
            assert_eq!(report["evidence"][4]["decision"]["blocking_onset_index"], 3);
            steps.extend([
                Step::Check(0),
                Step::Key(KeyCode::Enter),
                Step::Capture(RICH_CAPTURES[0]),
            ]);
            steps.extend([Step::Key(KeyCode::Tab); 5]);
            for index in 1..count {
                steps.extend([Step::Key(KeyCode::ArrowDown), Step::Check(index)]);
            }
            steps.push(Step::Capture(RICH_CAPTURES[1]));
            for index in (2..count - 1).rev() {
                steps.extend([Step::Key(KeyCode::ArrowUp), Step::Check(index)]);
            }
            steps.extend([Step::Key(KeyCode::Enter), Step::Capture(RICH_CAPTURES[2])]);
            steps.extend([Step::Key(KeyCode::Tab); 5]);
            steps.extend([
                Step::Key(KeyCode::ArrowDown),
                Step::Check(3),
                Step::Capture(RICH_CAPTURES[3]),
                Step::Key(KeyCode::Enter),
                Step::ScrollBottom,
                Step::Capture(RICH_CAPTURES[4]),
            ]);
        }
        steps.push(Step::Finish);
        Self {
            start: Instant::now(),
            output: std::env::var("QA_CAPTURE_DIR")
                .expect("QA_CAPTURE_DIR")
                .into(),
            report,
            package,
            steps,
            step: 0,
            checked: vec![false; count],
            saved: Vec::new(),
            ready: false,
            pending: false,
            settled: 0,
            message_count: 0,
        }
    }
}

pub(super) fn drive(
    mut driver: ResMut<Driver>,
    windows: Query<Entity, With<Window>>,
    mut keys: MessageWriter<KeyboardInput>,
) {
    assert!(driver.start.elapsed().as_secs_f64() < 44.0, "QA timeout");
    if !driver.ready || driver.pending {
        return;
    }
    let (key, chord) = match driver.steps[driver.step] {
        Step::Key(key) => {
            driver.step += 1;
            (key, false)
        }
        Step::Shortcut(key) => {
            driver.step += 1;
            (key, true)
        }
        Step::ScrollBottom => (KeyCode::ArrowDown, false),
        _ => return,
    };
    driver.settled = 0;
    let window = windows.single().expect("One QA window");
    let mut send = |key_code, state| {
        keys.write(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        });
        driver.message_count += 1;
    };
    if chord {
        send(KeyCode::ControlLeft, ButtonState::Pressed);
    }
    send(key, ButtonState::Pressed);
    send(key, ButtonState::Released);
    if chord {
        send(KeyCode::ControlLeft, ButtonState::Released);
    }
    println!(
        "QA {}",
        json!({"event":"keyboard_message", "message_count":driver.message_count,
        "key":format!("{key:?}"), "control":chord})
    );
}

fn check_record(driver: &Driver, state: &Workbench, index: usize, details: &str) {
    let view = state.candidates.as_ref().expect("Candidate mode");
    assert_eq!(view.len(), driver.checked.len());
    assert_eq!(view.selected, index);
    assert_eq!(
        state.document.cursor,
        driver.report["evidence"][index]["frame"].as_i64().unwrap()
    );
    assert_eq!(
        state.document.selected, None,
        "Proposal is distinct from the source chart"
    );
    let mut values = serde_json::Deserializer::from_str(details).into_iter::<Value>();
    let selected: crate::anchors::Evidence =
        serde_json::from_value(values.next().unwrap().unwrap())
            .expect("Selected evidence keeps the proposal f32 domain");
    let selected = serde_json::to_value(selected).unwrap();
    let relations = values.next().unwrap().unwrap();
    let context = values.next().unwrap().unwrap();
    let header = values.next().unwrap().unwrap();
    let expected = &driver.report["evidence"][index];
    assert_eq!(&selected, expected);
    let expected_anchor = expected["decision"]["anchor_id"]
        .as_u64()
        .map(|id| json!({"id":id,"frame":expected["frame"]}));
    assert_eq!(
        relations["proposed_anchor"],
        expected_anchor.unwrap_or(Value::Null)
    );
    let blocker = expected["decision"]["blocking_onset_index"]
        .as_u64()
        .map(|index| driver.report["evidence"][index as usize].clone());
    assert_eq!(
        relations["blocking_candidate"],
        blocker.unwrap_or(Value::Null)
    );
    for key in [
        "report_version",
        "compiler_version",
        "source",
        "policy",
        "production_admission",
    ] {
        assert_eq!(header[key], driver.report[key], "{key}");
    }
    assert_eq!(header["source_chart_anchor_count"], 1);
    assert_eq!(header["candidate_count"], driver.checked.len());
    assert_eq!(
        header["proposed_anchor_count"],
        driver.report["anchors"].as_array().unwrap().len()
    );
    assert_eq!(
        header["analysis_diagnostics"],
        driver.package.analysis.diagnostics
    );
    let energy = driver.package.analysis.energy[0];
    assert_eq!(
        context["energy"],
        json!({"start_frame":0,"frames":4800,
        "rms":energy.rms,"peak":energy.peak})
    );
    if index == 0 {
        assert_eq!(selected["confidence"], Value::Null);
        assert_eq!(context["beat_at_or_before"], Value::Null);
        assert_eq!(context["beat_after"]["frame"], 300);
        assert_eq!(context["section"], Value::Null);
    }
    if index == 2 || index == 3 || index == 4 {
        assert_eq!(context["beat_at_or_before"]["frame"], 300);
        assert_eq!(context["beat_after"]["frame"], 1400);
        assert_eq!(context["section"]["start_frame"], 900);
        assert_eq!(context["section"]["confidence"], Value::Null);
    }
}

fn check_pixels(state: &Workbench, image: &Image, index: usize) {
    let view = state.candidates.as_ref().unwrap();
    let width = image.texture_descriptor.size.width;
    let height = image.texture_descriptor.size.height;
    let column = |frame: i64| {
        (((frame - state.document.start) as f64 / state.document.span as f64 * f64::from(width))
            .round() as u32)
            .min(width - 1)
    };
    let points = view.selected_points();
    let color = |x: u32| {
        let offset = ((height / 10 * width + x) * 4) as usize;
        &image.data.as_ref().unwrap()[offset..offset + 4]
    };
    assert_eq!(
        color(column(points[0].0)),
        &[242, 243, 246, 255],
        "Exact cursor column"
    );
    if index == 2 {
        assert_eq!(points, [(1000, false), (1200, true)]);
        assert_eq!(
            color(column(1200)),
            &[230, 187, 112, 255],
            "Exact blocker column"
        );
    }
    assert_eq!(
        column(1200),
        column(1201),
        "Adjacent frames share a pixel in the full viewport"
    );
}

pub(super) fn capture(
    mut commands: Commands,
    mut driver: ResMut<Driver>,
    mut state: ResMut<Workbench>,
    controls: Res<input::Controls>,
    view: Res<ui::View>,
    boxes: Query<(
        &ui::BoxPart,
        &ComputedNode,
        Option<&ImageNode>,
        Option<&ScrollPosition>,
    )>,
    texts: Query<(&ui::Part, &Text, &TextLayoutInfo)>,
    images: Res<Assets<Image>>,
    mut exit: MessageWriter<AppExit>,
) {
    assert!(!state.document.dirty && !state.close_confirm && state.saving.is_none());
    assert!(
        state.destination.is_none()
            && !state.document.editing_frame
            && state.document.drag.is_none()
    );
    assert!(state.document.frame.is_empty());
    assert_eq!(state.document.editor.anchors(), state.document.original);
    assert_eq!(state.document.original, driver.package.chart.anchors);
    assert_eq!(state.document.sections, driver.package.chart.sections);
    assert!(state.document.editor.undo().is_err() && state.document.editor.redo().is_err());
    assert_eq!(
        state.toolbar(),
        &[Action::ZoomIn, Action::ZoomOut, Action::Back]
    );
    let (_, canvas, image, _) = boxes
        .iter()
        .find(|(part, _, _, _)| matches!(part, ui::BoxPart::Canvas))
        .unwrap();
    if !driver.ready {
        driver.ready = canvas.size.min_element() > 0.0
            && texts.iter().any(|(part, _, layout)| {
                matches!(part, ui::Part::Title) && !layout.glyphs.is_empty()
            });
        return;
    }
    if driver.pending {
        return;
    }
    for (part, computed, _, _) in &boxes {
        if matches!(part, ui::BoxPart::Frame | ui::BoxPart::Apply) {
            assert_eq!(computed.size, Vec2::ZERO, "Hidden frame editing controls");
        }
    }
    assert_eq!(controls.owner, Some(InputSource::Keyboard));
    let (_, text, layout) = texts
        .iter()
        .find(|(part, _, _)| matches!(part, ui::Part::DetailText))
        .unwrap();
    let step = driver.steps[driver.step];
    if let Step::Check(index) = step {
        assert_eq!(state.focus, Focus::List);
        check_record(&driver, &state, index, &text.0);
        driver.checked[index] = true;
        driver.step += 1;
        println!(
            "QA {}",
            json!({"event":"record_checked","index":index,"cursor":state.document.cursor})
        );
        return;
    }
    if matches!(step, Step::Key(_) | Step::Shortcut(_)) {
        return;
    }
    let (_, detail, _, scroll) = boxes
        .iter()
        .find(|(part, _, _, _)| matches!(part, ui::BoxPart::Detail))
        .unwrap();
    let scroll = scroll.unwrap();
    let maximum = ((detail.content_size.y - detail.size.y) * detail.inverse_scale_factor).max(0.0);
    if matches!(step, Step::ScrollBottom) {
        assert_eq!(state.focus, Focus::Details);
        if maximum > 0.0
            && (state.detail_scroll - maximum).abs() < 0.5
            && (scroll.0.y - maximum).abs() < 0.5
        {
            driver.step += 1;
            driver.settled = 0;
        }
        return;
    }
    if matches!(step, Step::Finish) {
        assert!(driver.checked.iter().all(|checked| *checked));
        assert_eq!(
            driver.saved,
            if driver.checked.is_empty() {
                vec!["zero-candidates"]
            } else {
                RICH_CAPTURES.to_vec()
            }
        );
        println!(
            "QA {}",
            json!({"event":"finished","records_checked":driver.checked.len(),
            "message_count":driver.message_count,"screenshots":driver.saved,"dirty":state.document.dirty,
            "readonly":true,"saving":false,"editing":false,"drag":false,
            "source_chart_anchors":state.document.editor.anchors().len(),"production_admission":"not_assessed"})
        );
        exit.write(AppExit::Success);
        return;
    }
    let Step::Capture(name) = step else {
        unreachable!()
    };
    driver.settled += 1;
    if driver.settled < 3 {
        return;
    }

    let selected = state.candidates.as_ref().unwrap().selected;
    if driver.checked.is_empty() {
        assert_eq!(state.document.cursor, 0);
        assert_eq!(driver.report["anchors"], json!([]));
        assert!(text.0.contains("\"candidate_count\": 0"));
        assert_eq!(state.document.editor.anchors()[0].id, 99);
    } else {
        check_record(&driver, &state, selected, &text.0);
    }
    if name == "list-tail" || name == "accepted-wave-list" {
        assert_eq!(state.focus, Focus::List);
        assert!(!state.details);
        assert!(view.first_row <= selected && view.first_row + view.row_count > selected);
        if name == "list-tail" {
            assert!(view.first_row > 0);
        }
        check_pixels(&state, images.get(&image.unwrap().image).unwrap(), selected);
    } else {
        assert_eq!(state.focus, Focus::Details);
        assert!(detail.size.min_element() > 0.0 && !layout.glyphs.is_empty());
        if name == "details-bottom" {
            assert!(maximum > 0.0 && (scroll.0.y - maximum).abs() < 0.5);
            assert!(text.0.ends_with(state.locale.text("candidates.help")));
        } else {
            assert_eq!(state.detail_scroll, 0.0);
            assert_eq!(scroll.0.y, 0.0);
        }
        if name == "dense-blocker-details" {
            check_pixels(&state, images.get(&image.unwrap().image).unwrap(), selected);
        }
    }
    let path = driver.output.join(format!("{name}.png"));
    assert!(!path.exists());
    println!(
        "QA {}",
        json!({"event":"screenshot_request","name":name,"selected":selected,
        "cursor":state.document.cursor,"focus":format!("{:?}",state.focus),
        "first_row":view.first_row,"row_count":view.row_count,"state_scroll":state.detail_scroll,
        "computed_scroll":scroll.0.y,"maximum_scroll":maximum,"text":text.0})
    );
    driver.pending = true;
    commands.spawn(Screenshot::primary_window()).observe(
        move |capture: On<ScreenshotCaptured>, mut driver: ResMut<Driver>| {
            let dimensions = capture.image.texture_descriptor.size;
            assert_eq!((dimensions.width, dimensions.height), size());
            capture
                .image
                .clone()
                .try_into_dynamic()
                .unwrap()
                .to_rgb8()
                .save(&path)
                .unwrap();
            driver.saved.push(name);
            driver.pending = false;
            driver.settled = 0;
            driver.step += 1;
            println!(
                "QA {}",
                json!({"event":"screenshot_saved","name":name,"path":path})
            );
        },
    );
}
