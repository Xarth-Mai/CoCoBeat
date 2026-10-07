#![allow(dead_code)]

mod anchors;
mod replay;
mod workbench;

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [package, destination] => workbench::run(
            std::path::Path::new(package),
            workbench::Mode::Edit(std::path::Path::new(destination)),
            cocobeat_runtime::Locale::ZhCn,
        ),
        _ => Err("Expected PACKAGE NEW_DESTINATION".into()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
