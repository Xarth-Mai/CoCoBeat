use cocobeat_schema::SongTime;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command] if command == "time-smoke" => {
            let time = SongTime::from_seconds(64).expect("64 seconds fits in SongTime");
            println!("64 s = {} canonical frames", time.frames());
            println!("Integer timeline smoke check only; no hardware latency measurement.");
            ExitCode::SUCCESS
        }
        [] => {
            println!("Usage: cargo run -p cocobeat-lab -- time-smoke");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Unknown arguments. Available command: time-smoke");
            ExitCode::FAILURE
        }
    }
}
