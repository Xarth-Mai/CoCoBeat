mod replay;
mod workbench;

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 2, "SOURCE_PACKAGE REPLAY");
    match workbench::run(
        std::path::Path::new(&args[0]),
        workbench::Mode::Replay(std::path::Path::new(&args[1])),
        cocobeat_runtime::Locale::ZhCn,
    ) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
