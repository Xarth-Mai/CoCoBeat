use super::*;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
    },
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    text::TextLayoutInfo,
};
use cocobeat_schema::PlayerId;
use serde_json::{Value, json};
use std::time::Instant;

const CAPTURES: [&str; 5] = [
    "negative-watermark-details",
    "wave-list-tail",
    "pair-wave-list",
    "pair-details-top",
    "pair-details-bottom",
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
    Check(usize),
    Capture(&'static str),
    ScrollBottom,
    Finish,
}

#[derive(Resource)]
pub(super) struct Driver {
    start: Instant,
    output: PathBuf,
    header: Value,
    records: Vec<Value>,
    summary: Value,
    steps: Vec<Step>,
    step: usize,
    checked: Vec<bool>,
    saved: Vec<&'static str>,
    ready: bool,
    pending: bool,
    settled: u8,
    input_count: usize,
}

impl Default for Driver {
    fn default() -> Self {
        let report = std::fs::read_to_string(std::env::var("QA_REPORT").expect("QA_REPORT"))
            .expect("Read the frozen inspect-replay baseline");
        let mut rows: Vec<Value> = report
            .lines()
            .map(|line| serde_json::from_str(line).expect("Diagnostic JSONL row"))
            .collect();
        let header = rows.remove(0);
        let summary = rows.pop().expect("Diagnostic summary");
        assert_eq!(header["type"], "header");
        assert_eq!(summary["type"], "summary");
        assert_eq!(rows.len(), 76, "The recorded native fixture has 76 records");
        assert_eq!(rows[0]["through_frames"], -1632);
        assert_eq!(rows[71]["type"], "free_sync");
        assert_eq!(rows[71]["midpoint_frames"], 26462);
        assert_eq!(rows[71]["p1"]["input_fact_index"], 32);
        assert_eq!(rows[71]["p1"]["input_song_time_frames"], 27099);
        assert_eq!(rows[71]["p2"]["input_fact_index"], 30);
        assert_eq!(rows[71]["p2"]["input_song_time_frames"], 25825);
        let mut steps = vec![
            Step::Key(KeyCode::Enter),
            Step::Key(KeyCode::Tab),
            Step::Check(0),
            Step::Key(KeyCode::Enter),
            Step::Capture(CAPTURES[0]),
        ];
        for _ in 0..8 {
            steps.push(Step::Key(KeyCode::Tab));
        }
        for index in 1..rows.len() {
            steps.extend([Step::Key(KeyCode::ArrowDown), Step::Check(index)]);
        }
        steps.push(Step::Capture(CAPTURES[1]));
        for index in (71..rows.len() - 1).rev() {
            steps.extend([Step::Key(KeyCode::ArrowUp), Step::Check(index)]);
        }
        steps.extend([
            Step::Capture(CAPTURES[2]),
            Step::Key(KeyCode::Enter),
            Step::Capture(CAPTURES[3]),
            Step::ScrollBottom,
            Step::Capture(CAPTURES[4]),
            Step::Finish,
        ]);
        let output = PathBuf::from(std::env::var("QA_CAPTURE_DIR").expect("QA_CAPTURE_DIR"));
        assert!(output.is_dir(), "The runner creates the capture directory");
        assert!(
            CAPTURES
                .iter()
                .all(|name| !output.join(format!("{name}.png")).exists())
        );
        Self {
            start: Instant::now(),
            output,
            header,
            checked: vec![false; rows.len()],
            records: rows,
            summary,
            steps,
            step: 0,
            saved: Vec::new(),
            ready: false,
            pending: false,
            settled: 0,
            input_count: 0,
        }
    }
}

pub(super) fn drive(
    mut driver: ResMut<Driver>,
    windows: Query<Entity, With<Window>>,
    mut keys: MessageWriter<KeyboardInput>,
) {
    assert!(
        driver.start.elapsed().as_secs_f64() < 44.0,
        "QA did not finish within 44 seconds"
    );
    if !driver.ready || driver.pending {
        return;
    }
    let key = match driver.steps[driver.step] {
        Step::Key(key) => {
            driver.step += 1;
            driver.settled = 0;
            key
        }
        Step::ScrollBottom => KeyCode::ArrowDown,
        _ => return,
    };
    let window = windows.single().expect("One QA window");
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
    driver.input_count += 1;
    println!(
        "QA {}",
        json!({"event":"keyboard_message", "index":driver.input_count, "key":format!("{key:?}")})
    );
}

fn expected_frame(record: &Value) -> i64 {
    let field = match record["type"].as_str().expect("Record type") {
        "hit" => "song_time_frames",
        "watermark" => "through_frames",
        "free_sync" => "midpoint_frames",
        "anchor_judged" | "anchor_sync" => "anchor_frame",
        kind => panic!("Unexpected diagnostic record: {kind}"),
    };
    record[field].as_i64().expect("Exact integer frame")
}

fn expected_hits(record: &Value) -> Vec<(PlayerId, i64)> {
    match record["type"].as_str().unwrap() {
        "hit" | "anchor_judged" => {
            let player = match record["player"].as_u64().unwrap() {
                1 => PlayerId::P1,
                2 => PlayerId::P2,
                _ => panic!("Unknown player"),
            };
            let frame = if record["type"] == "hit" {
                record["song_time_frames"].as_i64()
            } else {
                record["input_song_time_frames"].as_i64()
            };
            frame.map(|frame| (player, frame)).into_iter().collect()
        }
        "free_sync" | "anchor_sync" => [("p1", PlayerId::P1), ("p2", PlayerId::P2)]
            .into_iter()
            .filter_map(|(key, player)| {
                record[key]["input_song_time_frames"]
                    .as_i64()
                    .map(|frame| (player, frame))
            })
            .collect(),
        "watermark" => Vec::new(),
        _ => unreachable!(),
    }
}

fn check_record(driver: &Driver, state: &Workbench, text: &str, index: usize) {
    let replay = state.replay.as_ref().expect("Replay mode");
    assert_eq!(replay.len(), driver.records.len());
    assert_eq!(replay.selected, index);
    assert_eq!(
        state.document.cursor,
        expected_frame(&driver.records[index])
    );
    assert_eq!(
        state.document.selected,
        driver.records[index]["anchor_id"].as_u64()
    );
    assert_eq!(
        replay.selected_hits(),
        expected_hits(&driver.records[index])
    );
    let mut values = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    for expected in [&driver.records[index], &driver.header, &driver.summary] {
        assert_eq!(
            &values
                .next()
                .expect("Three diagnostic objects")
                .expect("Valid diagnostic JSON"),
            expected
        );
    }
}

fn check_pair_pixels(state: &Workbench, image: &Image) {
    let hits = state.replay.as_ref().unwrap().selected_hits();
    assert_eq!(hits.len(), 2);
    let width = image.texture_descriptor.size.width;
    let height = image.texture_descriptor.size.height;
    let data = image.data.as_ref().expect("CPU waveform pixels");
    for (player, frame) in hits {
        assert!(
            (state.document.start..=state.document.start + state.document.span).contains(&frame)
        );
        let x = (((frame - state.document.start) as f64 / state.document.span as f64
            * f64::from(width))
        .round() as u32)
            .min(width - 1);
        let lane = player.index() as u32;
        let y = (height * (4 + lane * 34) / 100 + height * (33 + lane * 34) / 100) / 2;
        let offset = ((y * width + x) * 4) as usize;
        assert_eq!(
            &data[offset..offset + 4],
            &[250, 244, 210, 255],
            "Selected {player:?} marker at frame {frame}"
        );
    }
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
    texts: Query<(
        &ui::Part,
        &Text,
        &ComputedNode,
        &UiGlobalTransform,
        &TextLayoutInfo,
    )>,
    images: Res<Assets<Image>>,
    windows: Query<&Window>,
    mut exit: MessageWriter<AppExit>,
) {
    assert!(!state.document.dirty);
    assert!(!state.close_confirm);
    assert!(state.saving.is_none());
    assert!(state.destination.is_none());
    assert!(!state.document.editing_frame);
    assert!(state.document.drag.is_none());
    assert_eq!(state.document.editor.anchors(), state.document.original);
    assert!(state.document.editor.undo().is_err());
    assert!(state.document.editor.redo().is_err());
    let (_, canvas, image, _) = boxes
        .iter()
        .find(|(part, _, _, _)| matches!(part, ui::BoxPart::Canvas))
        .expect("Wave canvas");
    if !driver.ready {
        driver.ready = canvas.size.min_element() > 0.0
            && texts.iter().any(|(part, _, _, _, layout)| {
                matches!(part, ui::Part::Title) && !layout.glyphs.is_empty()
            });
        return;
    }
    if driver.pending {
        return;
    }
    assert_eq!(controls.owner, Some(InputSource::Keyboard));
    let step = driver.steps[driver.step];
    if let Step::Check(index) = step {
        assert_eq!(state.focus, Focus::List);
        check_record(
            &driver,
            &state,
            &state.replay.as_ref().unwrap().details(state.locale),
            index,
        );
        driver.checked[index] = true;
        driver.step += 1;
        println!(
            "QA {}",
            json!({"event":"record_checked", "index":index, "record":driver.records[index], "cursor":state.document.cursor})
        );
        return;
    }
    if matches!(step, Step::Key(_)) {
        return;
    }
    let (_, detail, _, scroll) = boxes
        .iter()
        .find(|(part, _, _, _)| matches!(part, ui::BoxPart::Detail))
        .expect("Details panel");
    let scroll = scroll.expect("Details scroll");
    let maximum = ((detail.content_size.y - detail.size.y) * detail.inverse_scale_factor).max(0.0);
    if matches!(step, Step::ScrollBottom) {
        assert_eq!(state.focus, Focus::Details);
        check_record(
            &driver,
            &state,
            &state.replay.as_ref().unwrap().details(state.locale),
            71,
        );
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
        assert_eq!(driver.saved, CAPTURES);
        println!(
            "QA {}",
            json!({"event":"finished", "seconds":driver.start.elapsed().as_secs_f64(), "records_checked":driver.checked.len(), "input_count":driver.input_count, "screenshots":driver.saved, "dirty":state.document.dirty, "summary":driver.summary})
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
    let (_, text, child, transform, layout) = texts
        .iter()
        .find(|(part, _, _, _, _)| matches!(part, ui::Part::DetailText))
        .expect("Details text");
    let selected = state.replay.as_ref().unwrap().selected;
    check_record(&driver, &state, &text.0, selected);
    if name == "wave-list-tail" || name == "pair-wave-list" {
        assert_eq!(state.focus, Focus::List);
        assert!(!state.details);
        assert!(view.first_row <= 71 && view.first_row + view.row_count > 71);
        assert!(view.first_row <= selected && view.first_row + view.row_count > selected);
        check_pair_pixels(
            &state,
            images
                .get(&image.expect("Canvas image").image)
                .expect("Wave image asset"),
        );
    } else {
        assert_eq!(state.focus, Focus::Details);
        assert!(detail.size.min_element() > 0.0);
        assert!(!layout.glyphs.is_empty());
        if name == "pair-details-bottom" {
            assert!(maximum > 0.0);
            assert!((scroll.0.y - maximum).abs() < 0.5);
            assert!((state.detail_scroll - maximum).abs() < 0.5);
            assert!(text.0.ends_with(state.locale.text("replay.help")));
        } else {
            assert_eq!(state.detail_scroll, 0.0);
            assert_eq!(scroll.0.y, 0.0);
        }
    }
    let path = driver.output.join(format!("{name}.png"));
    assert!(
        !path.exists(),
        "Existing screenshot must not be overwritten"
    );
    println!(
        "QA {}",
        json!({
            "event":"screenshot_request", "name":name, "seconds":driver.start.elapsed().as_secs_f64(),
            "size":windows.single().map(|window|[window.width(),window.height()]).ok(),
            "selected":selected, "cursor":state.document.cursor, "record":driver.records[selected],
            "focus":format!("{:?}",state.focus), "first_row":view.first_row, "row_count":view.row_count,
            "state_scroll":state.detail_scroll, "computed_scroll":scroll.0.y, "maximum_scroll":maximum,
            "detail_size":detail.size.to_array(), "content_size":detail.content_size.to_array(),
            "text_size":child.size.to_array(), "layout_size":layout.size.to_array(),
            "text_center":transform.translation.to_array(), "text":text.0,
        })
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
                .expect("Native screenshot pixels")
                .to_rgb8()
                .save(&path)
                .expect("Save the complete native PNG");
            driver.saved.push(name);
            driver.pending = false;
            driver.settled = 0;
            driver.step += 1;
            println!(
                "QA {}",
                json!({"event":"screenshot_saved", "name":name, "path":path})
            );
        },
    );
}
