//! Opt-in sparse GUI reads and original captured-input mapping, never device timing

use std::time::Instant;

use cocobeat_replay::{
    Replay,
    timing::{
        AUDIO_SAMPLING_INTERVAL_NS, AudioHistoryStatus, AudioRead, CallbackPublication,
        CaptureTiming, MAX_AUDIO_READS, SourcePublication, TimingClock, TimingPhase, TimingSidecar,
    },
};

use crate::{
    audio::{AudioOutput, CallbackObservation, SourceObservation},
    clock::ClockConfig,
};

pub(crate) struct TimingDiagnostics {
    pub players: Vec<u8>,
    pub captures: Vec<CaptureTiming>,
    audio_history: Vec<AudioRead>,
    audio_history_status: AudioHistoryStatus,
}

impl TimingDiagnostics {
    pub fn new(players: Vec<u8>) -> Result<Self, String> {
        if !matches!(players.as_slice(), [1] | [2] | [1, 2]) {
            return Err("Timing capture scope must contain the original local seats".into());
        }
        Ok(Self {
            players,
            captures: Vec::new(),
            audio_history: Vec::new(),
            audio_history_status: AudioHistoryStatus::Sampled,
        })
    }

    pub fn audio_read_count(&self) -> usize {
        self.audio_history.len()
    }

    #[cfg(test)]
    pub(crate) fn add_test_read(&mut self, read: AudioRead) {
        self.audio_history.push(read);
    }

    pub fn sample(
        &mut self,
        audio: &AudioOutput,
        origin: Instant,
        phase: TimingPhase,
    ) -> Result<(), String> {
        let before = relative_ns(origin, Instant::now())
            .ok_or("Timing GUI read cannot be represented in the input time domain")?;
        if !self.sample_due(before) {
            return Ok(());
        }
        // These two read-only historical snapshots are independent, not a paired callback
        let source = audio
            .source_observation()
            .and_then(|row| source_row(origin, row));
        let callback = audio
            .callback_observation()
            .and_then(|row| callback_row(origin, row));
        let after = relative_ns(origin, Instant::now())
            .ok_or("Timing GUI read cannot be represented in the input time domain")?;
        self.audio_history.push(AudioRead {
            read_before_ns: before,
            read_after_ns: after,
            phase,
            source,
            callback,
        });
        if self.audio_history.len() == MAX_AUDIO_READS {
            self.audio_history_status = AudioHistoryStatus::LimitReached;
        }
        Ok(())
    }

    fn sample_due(&self, before: u64) -> bool {
        self.audio_history.len() < MAX_AUDIO_READS
            && self.audio_history.last().is_none_or(|last| {
                before
                    .checked_sub(last.read_before_ns)
                    .is_some_and(|gap| gap >= AUDIO_SAMPLING_INTERVAL_NS)
            })
    }

    pub fn sidecar(
        &self,
        replay: &Replay,
        bytes: &[u8],
        frames: u64,
        config: ClockConfig,
    ) -> Result<TimingSidecar, String> {
        let mut sidecar = TimingSidecar::new(
            replay,
            bytes,
            frames,
            self.players.clone(),
            TimingClock {
                max_extrapolation_ns: config.max_extrapolation_ns,
                max_drift_ppm: config.max_drift_ppm,
                history_capacity: u64::try_from(config.history_capacity)
                    .map_err(|_| "Timing clock history capacity overflow")?,
            },
        )
        .map_err(|error| error.to_string())?;
        sidecar.captures.clone_from(&self.captures);
        sidecar.audio_history.clone_from(&self.audio_history);
        sidecar.audio_history_status = self.audio_history_status;
        Ok(sidecar)
    }
}

fn relative_ns(origin: Instant, at: Instant) -> Option<u64> {
    u64::try_from(at.checked_duration_since(origin)?.as_nanos()).ok()
}

fn source_row(origin: Instant, row: SourceObservation) -> Option<SourcePublication> {
    Some(SourcePublication {
        generation: row.generation,
        source_id: row.source_id,
        sequence: row.sequence,
        position_seconds: row.position_seconds,
        published_before_ns: relative_ns(origin, row.published_between[0])?,
        published_after_ns: relative_ns(origin, row.published_between[1])?,
    })
}

fn callback_row(origin: Instant, row: CallbackObservation) -> Option<CallbackPublication> {
    Some(CallbackPublication {
        generation: row.generation,
        sequence: row.sequence,
        observed_ns: relative_ns(origin, row.observed_at)?,
        previous_observed_ns: row
            .previous_observed_at
            .and_then(|at| relative_ns(origin, at)),
        previous_frames: row.previous_frames,
        output_sample_rate: row.output_sample_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn original_snapshot_instants_never_saturate_into_a_fake_origin() {
        let origin = Instant::now();
        let before_origin = origin.checked_sub(Duration::from_millis(1)).unwrap();
        let at = origin.checked_add(Duration::from_millis(5)).unwrap();
        assert_eq!(relative_ns(origin, before_origin), None);
        let source = SourceObservation {
            generation: 3,
            source_id: 7,
            sequence: 9,
            position_seconds: 0.25,
            published_between: [origin, at],
        };
        let converted = source_row(origin, source).unwrap();
        assert_eq!(converted.position_seconds, source.position_seconds);
        assert_eq!(converted.published_after_ns, 5_000_000);
        assert_eq!(
            source_row(
                origin,
                SourceObservation {
                    published_between: [before_origin, at],
                    ..source
                }
            ),
            None
        );
        let callback = callback_row(
            origin,
            CallbackObservation {
                generation: 3,
                sequence: 11,
                observed_at: at,
                previous_observed_at: Some(before_origin),
                previous_frames: Some(512),
                output_sample_rate: 48_000,
            },
        )
        .unwrap();
        assert_eq!(callback.sequence, 11);
        assert_eq!(callback.previous_observed_ns, None);
        assert_eq!(callback.previous_frames, Some(512));
        let replay = Replay::new(
            cocobeat_replay::ReplayIdentity {
                content_id: "callback-origin-fixture".into(),
                rules_id: "fixture".into(),
                build_id: "fixture".into(),
                stage_compiler_version: None,
            },
            cocobeat_schema::SessionEpoch(1),
        )
        .unwrap();
        let bytes = replay.encode().unwrap();
        let mut timing = TimingDiagnostics::new(vec![1]).unwrap();
        timing.audio_history.push(AudioRead {
            read_before_ns: 10_000_000,
            read_after_ns: 11_000_000,
            phase: TimingPhase::Starting,
            source: Some(converted),
            callback: Some(callback),
        });
        let sidecar = timing
            .sidecar(&replay, &bytes, 48_000, ClockConfig::default())
            .unwrap();
        let encoded = serde_json::to_vec(&sidecar).unwrap();
        assert_eq!(
            TimingSidecar::decode(encoded.as_slice(), &replay, &bytes, 48_000).unwrap(),
            sidecar
        );
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cocobeat-callback-origin-{}-{stamp}.timing.json",
            std::process::id()
        ));
        sidecar.save_new(&path, &replay, &bytes, 48_000).unwrap();
        assert_eq!(
            TimingSidecar::load(&path, &replay, &bytes, 48_000).unwrap(),
            sidecar
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sparse_sampling_stops_at_capacity_without_filling_a_delayed_gui_gap() {
        let mut timing = TimingDiagnostics::new(vec![1, 2]).unwrap();
        assert!(timing.sample_due(0));
        timing.audio_history.push(AudioRead {
            read_before_ns: 100_000_000,
            read_after_ns: 100_000_100,
            phase: TimingPhase::Running,
            source: None,
            callback: None,
        });
        assert!(!timing.sample_due(99_000_000));
        assert!(!timing.sample_due(149_999_999));
        assert!(timing.sample_due(150_000_000));
        assert!(timing.sample_due(1_000_000_000));
        assert_eq!(timing.audio_history.len(), 1);
        timing
            .audio_history
            .resize(MAX_AUDIO_READS, timing.audio_history[0].clone());
        assert!(!timing.sample_due(u64::MAX));
    }

    #[test]
    fn actual_mock_source_snapshot_is_kept_verbatim() {
        let origin = Instant::now();
        let (mut manager, sampler) = crate::audio::mock_source_sampler();
        for _ in 0..4 {
            manager.backend_mut().on_start_processing();
            manager.backend_mut().process();
        }
        let (source, _) = sampler.read().unwrap().unwrap();
        let row = source_row(origin, source).unwrap();
        assert_eq!(
            (row.generation, row.source_id, row.sequence),
            (source.generation, source.source_id, source.sequence)
        );
        assert_eq!(row.position_seconds, source.position_seconds);
        assert!(row.published_before_ns <= row.published_after_ns);
    }
}
