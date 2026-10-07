#![allow(dead_code)]

mod anchors;
mod fixture;
mod replay;
mod workbench;

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = if args.len() == 2 && args[0] == "--fixture" {
        fixture::build(std::path::Path::new(&args[1]))
    } else if args.len() == 2 {
        workbench::run(
            std::path::Path::new(&args[0]),
            workbench::Mode::Candidates(std::path::Path::new(&args[1])),
            cocobeat_runtime::Locale::ZhCn,
        )
    } else {
        Err("Expected PACKAGE REPORT or --fixture NEW_DIRECTORY".into())
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
