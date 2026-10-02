use cocobeat_runtime::clock::{ClockBridge, ClockConfig, ClockObservation, MonotonicTime};
use cocobeat_schema::{SessionEpoch, SongTime};
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

const OFFSET_FRAMES: i64 = -96_000;

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    drift_ppm: i64,
    noisy: bool,
    render_hitch: bool,
}

const SCENARIOS: [Scenario; 4] = [
    Scenario {
        name: "nominal",
        drift_ppm: 0,
        noisy: false,
        render_hitch: false,
    },
    Scenario {
        name: "fast_device",
        drift_ppm: 100,
        noisy: false,
        render_hitch: false,
    },
    Scenario {
        name: "slow_device",
        drift_ppm: -100,
        noisy: false,
        render_hitch: false,
    },
    Scenario {
        name: "sampling_and_hitch",
        drift_ppm: 100,
        noisy: true,
        render_hitch: true,
    },
];

// Independent oscillator oracle: rational elapsed seconds × physical sample rate
// No ClockBridge or SongTime conversion participates in the expected frame count
fn physical_frame(nanos: u64, drift_ppm: i64) -> i64 {
    (i128::from(nanos) * 48 * i128::from(1_000_000 + drift_ppm) / 1_000_000_000_000_i128) as i64
        + OFFSET_FRAMES
}

#[derive(Default)]
struct Measurements {
    errors: Vec<u64>,
    render_errors: Vec<u64>,
    first_error: i64,
    last_error: i64,
    max_uncertainty: u64,
    uncovered: usize,
}

fn run_case(
    seconds: u64,
    scenario: Scenario,
    writer: &mut impl Write,
) -> Result<Measurements, Box<dyn Error>> {
    let epoch = SessionEpoch(1);
    let mut bridge = ClockBridge::new(
        epoch,
        ClockConfig {
            max_extrapolation_ns: 250_000_000,
            max_drift_ppm: 1_000,
            history_capacity: 256,
        },
    )?;
    let mut measurements = Measurements::default();
    for tick in 0..=seconds * 100 {
        let nanos = tick * 10_000_000;
        let truth = physical_frame(nanos, scenario.drift_ppm);
        if tick % 5 == 0 {
            let noise = if scenario.noisy {
                [0, 32, -32][(tick / 5 % 3) as usize]
            } else {
                0
            };
            bridge.observe(ClockObservation {
                epoch,
                monotonic: MonotonicTime::from_nanos(nanos),
                song_time: SongTime::from_frames(truth + noise),
                uncertainty_frames: if scenario.noisy { 96 } else { 1 },
            })?;
        }
        if tick % 2 != 0 {
            continue;
        }
        let estimate = bridge.estimate_song_time(MonotonicTime::from_nanos(nanos))?;
        let error = estimate.song_time.frames() - truth;
        let delay = if scenario.render_hitch && tick % 300 < 12 {
            (12 - tick % 300) * 10_000_000
        } else {
            0
        };
        let consumed_nanos = nanos + delay;
        let render_error = physical_frame(consumed_nanos, scenario.drift_ppm) - truth;
        if measurements.errors.is_empty() {
            measurements.first_error = error;
        }
        measurements.last_error = error;
        measurements.errors.push(error.unsigned_abs());
        measurements.render_errors.push(render_error.unsigned_abs());
        measurements.max_uncertainty = measurements
            .max_uncertainty
            .max(estimate.uncertainty_frames);
        measurements.uncovered += usize::from(error.unsigned_abs() > estimate.uncertainty_frames);
        writeln!(
            writer,
            "SIMULATED,{seconds},{},{},{nanos},{consumed_nanos},{truth},{},{error},{},{render_error}",
            scenario.name,
            scenario.drift_ppm,
            estimate.song_time.frames(),
            estimate.uncertainty_frames,
        )?;
    }
    Ok(measurements)
}

fn percentile(sorted: &[u64], percent: usize) -> u64 {
    sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)]
}

pub fn run(output: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(output)?;
    let mut rows = BufWriter::new(File::create(output.join("samples.csv"))?);
    writeln!(
        rows,
        "kind,duration_seconds,scenario,device_drift_ppm,input_monotonic_ns,consume_monotonic_ns,expected_frame,mapped_frame,error_frames,uncertainty_frames,render_clock_error_frames"
    )?;
    let mut summary = BufWriter::new(File::create(output.join("summary.csv"))?);
    writeln!(
        summary,
        "kind,duration_seconds,scenario,samples,abs_p50_frames,abs_p95_frames,abs_p99_frames,max_abs_frames,max_uncertainty_frames,outside_uncertainty,residual_drift_ppm,render_p99_frames,render_max_frames"
    )?;
    let mut uncovered = 0;
    for seconds in [30, 64, 300, 600] {
        for scenario in SCENARIOS {
            let mut result = run_case(seconds, scenario, &mut rows)?;
            result.errors.sort_unstable();
            result.render_errors.sort_unstable();
            let drift = (result.last_error - result.first_error) as f64 * 1_000_000.0
                / (seconds as f64 * 48_000.0);
            uncovered += result.uncovered;
            writeln!(
                summary,
                "SIMULATED,{seconds},{},{},{},{},{},{},{},{},{drift:.6},{},{}",
                scenario.name,
                result.errors.len(),
                percentile(&result.errors, 50),
                percentile(&result.errors, 95),
                percentile(&result.errors, 99),
                result.errors.last().unwrap(),
                result.max_uncertainty,
                result.uncovered,
                percentile(&result.render_errors, 99),
                result.render_errors.last().unwrap(),
            )?;
        }
    }
    rows.flush()?;
    summary.flush()?;
    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable".into());
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable".into());
    fs::write(
        output.join("README.txt"),
        format!(
            "SIMULATED — no physical input, device, Kira playback or loopback measured\n\
         package={}\nhost={}-{}\nrustc={rustc}\nrevision={revision}\n\
         revision identifies HEAD only; uncommitted working-tree changes may exist\n\
         durations_seconds=30,64,300,600\ncanonical_sample_rate=48000\n\
         input_interval_ms=20\nobservation_interval_ms=50\ninitial_offset_frames=-96000\n\
         device_drift_ppm=0,+100,-100\nsampling_noise_frames=0,+32,-32\n\
         declared_sampling_uncertainty_frames=96\nrender_hitch_ms=120 every 3000 ms\n\
         bridge_extrapolation_limit_ms=250\nbridge_drift_bound_ppm=1000\n\
         oracle=independent integer oscillator driven by synthetic nanoseconds\n\
         percentiles=nearest rank over absolute mapping error in canonical frames\n\
         residual_drift_ppm=(last signed error-first signed error)/duration_frames*1e6\n\
         render columns show error from substituting deferred consumption for input time\n\
         outside_uncertainty={uncovered}\n\
         NOT RUN: physical output offset, keyboard/gamepad latency, Windows/Linux audio acceptance\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
        ),
    )?;
    println!("SIMULATED timing matrix written to {}", output.display());
    if uncovered != 0 {
        return Err(
            format!("{uncovered} simulated measurements exceed declared uncertainty").into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_oracle_and_hitch_preserve_early_input_mapping() {
        assert_eq!(physical_frame(64_000_000_000, 0), 2_976_000);
        assert_eq!(physical_frame(600_000_000_000, 100), 28_706_880);
        assert_eq!(physical_frame(600_000_000_000, -100), 28_701_120);
        assert_eq!(percentile(&[1, 2, 3, 4], 50), 2);
        for scenario in SCENARIOS {
            let result = run_case(3, scenario, &mut std::io::sink()).unwrap();
            assert_eq!(result.uncovered, 0);
            assert_eq!(result.errors.len(), 151);
            if scenario.render_hitch {
                assert!(result.render_errors.iter().max().unwrap() >= &5_760);
                assert!(result.errors.iter().max().unwrap() < &100);
            }
        }
    }
}
