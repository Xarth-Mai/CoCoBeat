//! Standalone renderer for the actual brand modules; never registers game systems

#[path = "../../../crates/cocobeat-runtime/src/brand_audio.rs"]
mod brand_audio;
#[path = "../../../crates/cocobeat-runtime/src/brand_intro.rs"]
mod brand_intro;

use std::{fs, io::Write, path::PathBuf, time::Duration};

use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    camera::{ImageRenderTarget, RenderTarget},
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::TextureFormat,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use brand_intro::{
    BrandImpact, BrandIntroControl, BrandIntroPhase, BrandIntroStatus, BrandIntroSystems, END,
    IDLE_PERIOD,
};

#[derive(Resource)]
struct Capture {
    directory: PathBuf,
    times: Vec<f64>,
    size: UVec2,
    scale: f32,
    index: usize,
    settled_frames: usize,
    total_frames: usize,
    waiting: bool,
    target: Handle<Image>,
}

fn main() -> AppExit {
    let mut capture = Capture {
        directory: PathBuf::from("target/brand-validation"),
        times: [
            0.0, 0.60, 0.90, 1.12, 1.50, 2.10, 2.32, 3.35, 3.80, 4.00, 4.12, 4.42, 4.95, 6.15,
            6.474, END,
        ]
        .into_iter()
        .chain(
            [
                2.88,
                2.97,
                6.0 + 0.28,
                6.58,
                8.02,
                10.58,
                10.92,
                15.28,
                15.60,
                17.29,
                19.77,
                IDLE_PERIOD,
            ]
            .map(|time| END + time),
        )
        .collect(),
        size: UVec2::new(1280, 800),
        scale: 1.0,
        index: 0,
        settled_frames: 0,
        total_frames: 0,
        waiting: false,
        target: default(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                capture.directory = args.next().expect("--output requires a directory").into()
            }
            "--size" => {
                let size = args.next().expect("--size requires WIDTHxHEIGHT");
                let (width, height) = size.split_once('x').expect("--size requires WIDTHxHEIGHT");
                capture.size = UVec2::new(
                    width.parse().expect("invalid width"),
                    height.parse().expect("invalid height"),
                );
            }
            "--scale" => {
                capture.scale = args
                    .next()
                    .expect("--scale requires a factor")
                    .parse()
                    .expect("invalid scale")
            }
            "--sequence" | "--menu-sequence" => {
                let end = END
                    + if arg == "--menu-sequence" {
                        IDLE_PERIOD
                    } else {
                        0.0
                    };
                capture.times = (0..=(end * 30.0).round() as u32)
                    .map(|frame| (f64::from(frame) / 30.0).min(end))
                    .collect()
            }
            _ => panic!(
                "unknown argument {arg}; use --output DIR --size WIDTHxHEIGHT --scale FACTOR --sequence or --menu-sequence"
            ),
        }
    }
    assert!(
        capture.scale.is_finite() && (0.5..=4.0).contains(&capture.scale),
        "scale must be 0.5..=4"
    );
    assert!(
        capture.size.min_element() >= 64 && capture.size.max_element() <= 8192,
        "physical dimensions must be 64..=8192"
    );
    fs::create_dir_all(&capture.directory).expect("cannot create output directory");
    let frame_list = capture
        .times
        .iter()
        .enumerate()
        .map(|(index, time)| format!("frame_{index:04}.png,{time:.6}\n"))
        .collect::<String>();
    fs::write(
        capture.directory.join("frames.csv"),
        format!("file,seconds\n{frame_list}"),
    )
    .expect("cannot write frame list");
    for (name, impact) in [
        ("pon-blue.wav", BrandImpact::Co1),
        ("pon-pink.wav", BrandImpact::Co2),
        ("whoom.wav", BrandImpact::Beat),
    ] {
        write_sound(capture.directory.join(name), impact);
    }

    let mut app = App::new();
    app.insert_resource(capture)
        .insert_resource(ClearColor(Color::srgb(0.04, 0.05, 0.08)))
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .add_systems(Startup, setup)
        .add_systems(Update, select_frame.before(BrandIntroSystems::Advance))
        .add_systems(Update, capture_frame.after(BrandIntroSystems::Advance));
    brand_intro::install(&mut app);
    app.run()
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut capture: ResMut<Capture>) {
    capture.target = images.add(Image::new_target_texture(
        capture.size.x,
        capture.size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    commands.spawn((
        Camera2d,
        IsDefaultUiCamera,
        RenderTarget::Image(ImageRenderTarget {
            handle: capture.target.clone(),
            scale_factor: capture.scale,
        }),
    ));
}

fn select_frame(
    mut capture: ResMut<Capture>,
    mut status: ResMut<BrandIntroStatus>,
    mut control: ResMut<BrandIntroControl>,
    mut exit: MessageWriter<AppExit>,
) {
    capture.total_frames += 1;
    if status.phase == BrandIntroPhase::Failed
        || capture.total_frames > 1000 + capture.times.len() * 20
    {
        eprintln!("Brand capture failed or timed out: {:?}", *status);
        exit.write(AppExit::error());
        return;
    }
    if capture.index == capture.times.len() {
        println!(
            "Captured {} frames at {}x{} scale {} in {}",
            capture.index,
            capture.size.x,
            capture.size.y,
            capture.scale,
            capture.directory.display()
        );
        exit.write(AppExit::Success);
        return;
    }
    if status.phase == BrandIntroPhase::Loading {
        return;
    }
    control.suspended = true;
    let time = capture.times[capture.index];
    control.idle_enabled = time >= END;
    status.elapsed_seconds = time.min(END);
    status.idle_seconds = (time - END).max(0.0).rem_euclid(IDLE_PERIOD);
    status.phase = brand_intro::phase_at(time);
    status.reveal_progress = brand_intro::reveal_at(time);
    assert_eq!(status.is_complete(), time >= END);
    capture.settled_frames += 1;
}

fn capture_frame(
    mut commands: Commands,
    mut capture: ResMut<Capture>,
    status: Res<BrandIntroStatus>,
) {
    if capture.waiting
        || capture.index == capture.times.len()
        || capture.settled_frames < 4
        || matches!(
            status.phase,
            BrandIntroPhase::Loading | BrandIntroPhase::Failed
        )
    {
        return;
    }
    capture.waiting = true;
    commands
        .spawn(Screenshot(RenderTarget::Image(ImageRenderTarget {
            handle: capture.target.clone(),
            scale_factor: capture.scale,
        })))
        .observe(save_frame);
}

fn save_frame(
    event: On<ScreenshotCaptured>,
    mut capture: ResMut<Capture>,
    mut exit: MessageWriter<AppExit>,
) {
    let path = capture
        .directory
        .join(format!("frame_{:04}.png", capture.index));
    let result = event
        .image
        .clone()
        .try_into_dynamic()
        .map_err(|error| error.to_string())
        .and_then(|image| {
            image
                .to_rgb8()
                .save(&path)
                .map_err(|error| error.to_string())
        });
    if let Err(error) = result {
        eprintln!("Cannot save {}: {error}", path.display());
        exit.write(AppExit::error());
        return;
    }
    capture.index += 1;
    capture.settled_frames = 0;
    capture.waiting = false;
}

fn write_sound(path: PathBuf, impact: BrandImpact) {
    let sound = brand_audio::sound(impact);
    let data_size = sound.frames.len() as u32 * 4;
    let mut wav = fs::File::create(path).expect("cannot create WAV");
    wav.write_all(b"RIFF").unwrap();
    wav.write_all(&(36 + data_size).to_le_bytes()).unwrap();
    wav.write_all(b"WAVEfmt \x10\0\0\0\x01\0\x02\0").unwrap();
    wav.write_all(&sound.sample_rate.to_le_bytes()).unwrap();
    wav.write_all(&(sound.sample_rate * 4).to_le_bytes())
        .unwrap();
    wav.write_all(b"\x04\0\x10\0data").unwrap();
    wav.write_all(&data_size.to_le_bytes()).unwrap();
    for frame in sound.frames.iter() {
        for sample in [frame.left, frame.right] {
            wav.write_all(&((sample * f32::from(i16::MAX)).round() as i16).to_le_bytes())
                .unwrap();
        }
    }
}
