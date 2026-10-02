//! Explicitly invoked, audible software-cursor experiments; physical latency needs loopback

use crate::{
    audio::{AudioOutput, probe_duration_frames},
    dev_song,
};
use cocobeat_schema::SongTime;
use kira::sound::PlaybackState;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const START_TIMEOUT_NS: u64 = 2_000_000_000;
const STALL_TIMEOUT_NS: u64 = 250_000_000;

/// Plays audible clicks and writes new evidence files without overwriting earlier experiments
/// No loopback is captured; all reported timing statistics describe software observations
pub fn audio_probe(duration_seconds: u32, output: &Path) -> Result<(), String> {
    let total_frames = probe_duration_frames(duration_seconds)?;
    fs::create_dir_all(output).map_err(|error| error.to_string())?;
    let mut metadata = BufWriter::new(create(output, "metadata.txt")?);
    writeln!(
        metadata,
        "kind=REAL_AUDIO_SOFTWARE_OBSERVATION\nphysical_output_latency=NOT MEASURED\nloopback_recording=NOT CAPTURED\ncompletion=INCOMPLETE unless a final result is appended\nbuild={}\nos={}\narch={}\nunix_start_ns={}\nduration_seconds={duration_seconds}\nsource_rate={}\nsource_channels=2\nsource_frames={total_frames}\nsource_pcm_bytes={}\nsource_signal=CC0-1.0, 10 ms decaying 1 kHz bipolar pulses, peak 0.16\nexpected_click_onsets=0.5 seconds + whole seconds\nrequested_poll_interval_ns=1000000\ncursor_timestamp=midpoint of monotonic before/after reads\nsoftware_drift=cursor_delta minus nominal 48000 Hz monotonic_delta since first advancing cursor\nstart_confirmation=first strictly positive source cursor, not initial Playing state\nstart_timeout_ns={START_TIMEOUT_NS}\nno_cursor_progress_timeout_ns={STALL_TIMEOUT_NS}\noutput_buffer_latency=UNKNOWN\n",
        env!("COCOBEAT_BUILD_ID"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos(),
        dev_song::SAMPLE_RATE,
        u64::from(total_frames) * 8,
    )
    .map_err(|error| error.to_string())?;
    metadata.flush().map_err(|error| error.to_string())?;

    let mut expected = BufWriter::new(create(output, "expected-clicks.csv")?);
    writeln!(expected, "click_index,source_frame").map_err(|error| error.to_string())?;
    for second in 0..duration_seconds {
        writeln!(expected, "{second},{}", second * 48_000 + 24_000)
            .map_err(|error| error.to_string())?;
    }
    expected.flush().map_err(|error| error.to_string())?;
    let mut csv = BufWriter::with_capacity(64 * 1024, create(output, "cursor.csv")?);
    writeln!(
        csv,
        "before_ns,after_ns,cursor_frames,state,changed,poll_interval_ns,cursor_update_interval_ns,software_drift_frames,backend_error_detected"
    )
    .map_err(|error| error.to_string())?;

    let mut stats = ProbeStats::default();
    let result = (|| {
        let mut audio = AudioOutput::new()?;
        metadata
            .write_all(audio.output_info().as_bytes())
            .and_then(|()| metadata.flush())
            .map_err(|error| error.to_string())?;
        println!(
            "Audio probe will play clicks for {duration_seconds} seconds; physical latency is NOT MEASURED"
        );
        let origin = audio.start_click_probe(duration_seconds)?;
        let sampled = sample(&mut audio, origin, duration_seconds, &mut csv, &mut stats);
        audio.stop();
        sampled
    })();

    // Flush accumulated rows even when initialization, playback, or a later write failed
    let flushed = csv
        .flush()
        .map_err(|error| format!("Flush cursor CSV: {error}"));
    let result = match (result, flushed) {
        (Err(error), Err(flush_error)) => Err(format!("{error}; {flush_error}")),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    };
    let summary = write_summary(&mut metadata, &mut stats, &result)
        .and_then(|()| metadata.flush())
        .map_err(|error| format!("Write probe summary: {error}"));
    match (result, summary) {
        (Err(error), Err(summary_error)) => Err(format!("{error}; {summary_error}")),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => {
            println!("Audio cursor evidence saved to {}", output.display());
            Ok(())
        }
    }
}

fn create(directory: &Path, name: &str) -> Result<File, String> {
    let path = directory.join(name);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("Create {}: {error}", path.display()))
}

fn elapsed_ns(origin: Instant) -> Result<u64, String> {
    u64::try_from(origin.elapsed().as_nanos()).map_err(|error| error.to_string())
}

fn sample(
    audio: &mut AudioOutput,
    origin: Instant,
    seconds: u32,
    csv: &mut impl Write,
    stats: &mut ProbeStats,
) -> Result<(), String> {
    let deadline_ns = (u64::from(seconds) + 2) * 1_000_000_000;
    let max_observations = (u64::from(seconds) + 2) * 2_000;
    let mut last_flush_ns = 0;
    loop {
        let before = elapsed_ns(origin)?;
        let position = audio
            .position()
            .ok_or("Audio probe lost its playback handle")?;
        let state = audio.state().ok_or("Audio probe lost its playback state")?;
        let after = elapsed_ns(origin)?;
        let at = before + (after - before) / 2;
        let context = || {
            format!(
                "before_ns={before}, after_ns={after}, position_seconds={position:?}, state={state:?}"
            )
        };
        let frames = SongTime::try_from_seconds_f64(position)
            .ok_or_else(|| {
                format!(
                    "Audio probe returned a non-finite or overflowing cursor; {}",
                    context()
                )
            })?
            .frames();
        if frames < 0 || frames > i64::from(seconds) * 48_000 {
            return Err(format!(
                "Audio probe cursor is outside the source signal; {}",
                context()
            ));
        }
        let poll_interval_ns = at.checked_sub(stats.last_poll_ns).ok_or_else(|| {
            format!(
                "Audio probe monotonic timestamp moved backwards; {}",
                context()
            )
        })?;
        let (changed, update_interval, drift) = stats
            .observe(at, frames)
            .map_err(|error| format!("{error}; {}", context()))?;
        let backend_error = audio.take_error();
        writeln!(
            csv,
            "{before},{after},{frames},{state:?},{changed},{poll_interval_ns},{},{},{}",
            update_interval
                .map(|value| value.to_string())
                .unwrap_or_default(),
            drift.map(|value| value.to_string()).unwrap_or_default(),
            backend_error.is_some(),
        )
        .map_err(|error| format!("Write cursor CSV: {error}"))?;
        if after - last_flush_ns >= 1_000_000_000 {
            csv.flush()
                .map_err(|error| format!("Flush cursor CSV: {error}"))?;
            last_flush_ns = after;
        }
        if let Some(error) = backend_error {
            return Err(error);
        }
        if after > deadline_ns || stats.observations > max_observations {
            return Err("Audio probe exceeded its duration or observation bound".into());
        }
        if state == PlaybackState::Stopped {
            return if stats.first_progress.is_some() {
                Ok(())
            } else {
                Err("Audio probe stopped before any source progress was observed".into())
            };
        }
        if state != PlaybackState::Playing {
            return Err(format!("Audio probe entered unexpected state {state:?}"));
        }
        stats.check_progress(at)?;
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[derive(Default)]
struct ProbeStats {
    observations: u64,
    first_progress: Option<(u64, i64)>,
    last_poll_ns: u64,
    last_cursor: i64,
    last_change_ns: u64,
    update_intervals_ns: Vec<u64>,
    last_drift_frames: i64,
    max_abs_drift_frames: u64,
}

impl ProbeStats {
    fn observe(
        &mut self,
        at: u64,
        frames: i64,
    ) -> Result<(bool, Option<u64>, Option<i64>), String> {
        if at < self.last_poll_ns || frames < self.last_cursor {
            return Err("Audio probe cursor or monotonic timestamp moved backwards".into());
        }
        self.observations += 1;
        self.last_poll_ns = at;
        let changed = frames > self.last_cursor;
        let mut interval = None;
        if changed {
            if self.first_progress.is_some() {
                interval = Some(at - self.last_change_ns);
                self.update_intervals_ns.push(at - self.last_change_ns);
            } else {
                self.first_progress = Some((at, frames));
            }
            self.last_change_ns = at;
            self.last_cursor = frames;
        }
        let drift = self.first_progress.map(|(first_ns, first_frames)| {
            let nominal = i128::from(at - first_ns) * 48_000 / 1_000_000_000;
            (i128::from(frames - first_frames) - nominal) as i64
        });
        if let Some(drift) = drift {
            self.last_drift_frames = drift;
            self.max_abs_drift_frames = self.max_abs_drift_frames.max(drift.unsigned_abs());
        }
        Ok((changed, interval, drift))
    }

    fn check_progress(&self, at: u64) -> Result<(), String> {
        if self.first_progress.map_or(at, |(first_ns, _)| first_ns) > START_TIMEOUT_NS {
            return Err("Audio probe did not begin advancing within two seconds".into());
        }
        if self.first_progress.is_some() && at - self.last_change_ns > STALL_TIMEOUT_NS {
            return Err("Audio probe cursor stopped advancing for over 250 ms".into());
        }
        Ok(())
    }
}

fn write_summary(
    metadata: &mut impl Write,
    stats: &mut ProbeStats,
    result: &Result<(), String>,
) -> std::io::Result<()> {
    stats.update_intervals_ns.sort_unstable();
    writeln!(
        metadata,
        "\nresult={}\nerror={:?}\nobservations={}\nfirst_cursor_progress_ns={:?}\nlast_observation_ns={}\nlast_cursor_frames={}\nlast_software_drift_frames={}\nmax_abs_software_drift_frames={}\nobserved_cursor_update_intervals={}\n",
        if result.is_ok() {
            "COMPLETED"
        } else {
            "FAILED"
        },
        result.as_ref().err(),
        stats.observations,
        stats.first_progress.map(|(at, _)| at),
        stats.last_poll_ns,
        stats.last_cursor,
        stats.last_drift_frames,
        stats.max_abs_drift_frames,
        stats.update_intervals_ns.len(),
    )?;
    for percent in [50, 95, 99] {
        writeln!(
            metadata,
            "software_cursor_update_interval_p{percent}_ns={:?}",
            percentile(&stats.update_intervals_ns, percent),
        )?;
    }
    Ok(())
}

fn percentile(sorted: &[u64], percent: usize) -> Option<u64> {
    (!sorted.is_empty()).then(|| sorted[(sorted.len() * percent).div_ceil(100) - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_statistics_keep_stale_reads_and_output_latency_distinct() {
        let mut stats = ProbeStats::default();
        assert_eq!(stats.observe(1_000_000, 0).unwrap(), (false, None, None));
        assert_eq!(stats.observe(2_000_000, 48).unwrap(), (true, None, Some(0)));
        assert_eq!(
            stats.observe(3_000_000, 48).unwrap(),
            (false, None, Some(-48)),
        );
        assert_eq!(
            stats.observe(4_000_000, 144).unwrap(),
            (true, Some(2_000_000), Some(0)),
        );
        assert!(stats.check_progress(254_000_000).is_ok());
        assert!(stats.check_progress(254_000_001).is_err());
        assert!(stats.observe(5_000_000, 100).is_err());
        assert!(ProbeStats::default().check_progress(2_000_000_001).is_err());
        let mut summary = Vec::new();
        write_summary(&mut summary, &mut stats, &Ok(())).unwrap();
        let summary = String::from_utf8(summary).unwrap();
        assert!(summary.contains("software_cursor_update_interval_p95_ns=Some(2000000)"));
        assert!(summary.contains("max_abs_software_drift_frames=48"));
        assert_eq!(percentile(&[], 99), None);
        assert_eq!(percentile(&[1, 2, 3, 4], 50), Some(2));
        assert_eq!(percentile(&[1, 2, 3, 4], 95), Some(4));
    }
}
