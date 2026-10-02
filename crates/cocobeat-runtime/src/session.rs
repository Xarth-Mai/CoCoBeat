//! Live input and saved histories share one deterministic rule engine

use crate::{clock::*, dev_song};
use cocobeat_core::DuoEngine;
use cocobeat_replay::{MAX_FACTS, Replay, ReplayIdentity};
use cocobeat_schema::{DuoEvent, DuoInput, DuoRules, Hit, PlayerId, SessionEpoch, SongTime};
use std::path::{Path, PathBuf};

pub const CONTENT_ID: &str = "dev64-pcm16-3390dd080cb536fd4-anchors-v1";
pub const RULES_ID: &str = "duo-watermark-v1";
// An initial software-cursor estimate for experimentation, not measured output latency
pub const CURSOR_UNCERTAINTY_FRAMES: u64 = 2_400;

#[derive(Debug)]
pub struct CaptureDiagnostic {
    pub player: PlayerId,
    pub seq: u64,
    pub observed_ns: u64,
    pub consumed_ns: u64,
    pub song_frames: i64,
    pub uncertainty_frames: u64,
}

pub struct Session {
    pub engine: DuoEngine,
    pub replay: Replay,
    pub clock: ClockBridge,
    pub diagnostics: Vec<CaptureDiagnostic>,
    pub current: SongTime,
    pub uncertainty_frames: u64,
    sequence: [u64; 2],
    watermark: Option<SongTime>,
}

impl Session {
    pub fn new(epoch: SessionEpoch) -> Result<Self, String> {
        Ok(Self {
            engine: DuoEngine::new(epoch, dev_song::anchors(), DuoRules::default())
                .map_err(|error| error.to_string())?,
            replay: Replay::new(
                ReplayIdentity {
                    content_id: CONTENT_ID.into(),
                    rules_id: RULES_ID.into(),
                    build_id: env!("COCOBEAT_BUILD_ID").into(),
                },
                epoch,
            )
            .map_err(|error| error.to_string())?,
            clock: ClockBridge::new(epoch, ClockConfig::default())
                .map_err(|error| format!("Clock configuration: {error:?}"))?,
            diagnostics: Vec::new(),
            current: SongTime::ZERO,
            uncertainty_frames: CURSOR_UNCERTAINTY_FRAMES,
            sequence: [0, 0],
            watermark: None,
        })
    }

    pub fn epoch(&self) -> SessionEpoch {
        self.replay.epoch()
    }

    pub fn observe_audio(&mut self, position: f64, at: MonotonicTime) -> Result<(), String> {
        if !(0.0..=f64::from(dev_song::FRAMES) / 48_000.0).contains(&position) {
            return Err("Audio cursor is outside the development song".into());
        }
        let song_time = SongTime::try_from_seconds_f64(position)
            .ok_or("Audio cursor is outside the song timeline")?;
        // Kira's source cursor must not seek backwards within a session
        // An uncertainty allowance cannot reopen history already confirmed by core
        if self
            .clock
            .last_observation()
            .is_some_and(|last| song_time < last.song_time)
        {
            return Err("Audio cursor moved backwards; restart with a new epoch".into());
        }
        // Re-reading one callback's cursor is not new evidence of audio progress
        // Keep the original anchor so stalled playback reaches the extrapolation limit
        if self.clock.state() == ClockState::Running
            && self
                .clock
                .last_observation()
                .is_some_and(|last| song_time == last.song_time)
        {
            return Ok(());
        }
        self.clock
            .observe(ClockObservation {
                epoch: self.epoch(),
                monotonic: at,
                song_time,
                uncertainty_frames: CURSOR_UNCERTAINTY_FRAMES,
            })
            .map_err(|error| format!("Audio clock observation: {error:?}"))
    }

    pub fn update_position(&mut self, at: MonotonicTime) -> Result<(), String> {
        let estimate = self
            .clock
            .now(at)
            .map_err(|error| format!("Audio clock estimate: {error:?}"))?;
        self.current = estimate.song_time;
        self.uncertainty_frames = estimate.uncertainty_frames;
        Ok(())
    }

    pub fn hit(
        &mut self,
        player: PlayerId,
        observed_ns: u64,
        consumed_ns: u64,
    ) -> Result<Vec<DuoEvent>, String> {
        let estimate = self
            .clock
            .estimate_song_time(MonotonicTime::from_nanos(observed_ns))
            .map_err(|error| format!("Captured input clock: {error:?}"))?;
        if estimate.song_time < SongTime::ZERO
            || estimate.song_time.frames() >= i64::from(dev_song::FRAMES)
        {
            return Ok(Vec::new());
        }
        let seq = self.sequence[player.index()];
        let next = seq.checked_add(1).ok_or("Input sequence overflow")?;
        let events = self.ingest(DuoInput::Hit(Hit {
            epoch: self.epoch(),
            player,
            seq,
            song_time: estimate.song_time,
        }))?;
        self.sequence[player.index()] = next;
        self.diagnostics.push(CaptureDiagnostic {
            player,
            seq,
            observed_ns,
            consumed_ns,
            song_frames: estimate.song_time.frames(),
            uncertainty_frames: estimate.uncertainty_frames,
        });
        Ok(events)
    }

    fn ingest(&mut self, fact: DuoInput) -> Result<Vec<DuoEvent>, String> {
        // Reserve recorder capacity before mutating the rule engine
        if self.replay.facts().len() >= MAX_FACTS {
            return Err("Replay capacity reached; session stopped without dropping input".into());
        }
        let events = self
            .engine
            .ingest(fact)
            .map_err(|error| error.to_string())?;
        self.replay
            .record(fact)
            .map_err(|error| error.to_string())?;
        Ok(events)
    }

    /// Close history only after this frame's captured input queue has been drained
    pub fn advance(&mut self) -> Result<Vec<DuoEvent>, String> {
        let Some(observation) = self.clock.last_observation() else {
            return Ok(Vec::new());
        };
        let through = observation
            .song_time
            .checked_add_frames(-(CURSOR_UNCERTAINTY_FRAMES as i64))
            .ok_or("Watermark overflow")?;
        // Ten-millisecond checkpoints keep files bounded independently of render rate
        if self
            .watermark
            .is_some_and(|previous| through.frames() - previous.frames() < 480)
        {
            return Ok(Vec::new());
        }
        self.close_history(through)
    }

    fn close_history(&mut self, through: SongTime) -> Result<Vec<DuoEvent>, String> {
        if self.replay.facts().len() > MAX_FACTS - 2 {
            return Err("Replay capacity reached before history checkpoint".into());
        }
        let mut events = Vec::new();
        for player in [PlayerId::P1, PlayerId::P2] {
            events.extend(self.ingest(DuoInput::Watermark {
                epoch: self.epoch(),
                player,
                through,
            })?);
        }
        self.watermark = Some(through);
        Ok(events)
    }

    /// The known content end closes the remaining history, without inventing an audio observation
    pub fn finish(&mut self) -> Result<Vec<DuoEvent>, String> {
        let tail = self.engine.confirmation_delay_frames();
        let events = self.close_history(SongTime::from_frames(
            i64::from(dev_song::FRAMES) + tail + 1,
        ))?;
        self.current = SongTime::from_frames(i64::from(dev_song::FRAMES));
        Ok(events)
    }

    pub fn save(&self, directory: &Path) -> Result<PathBuf, String> {
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = directory.join(format!("session-{}-{stamp}.json", self.epoch().0));
        self.replay.save(&path).map_err(|error| error.to_string())?;
        let mut csv =
            String::from("player,seq,observed_ns,consumed_ns,song_frames,uncertainty_frames\n");
        for input in &self.diagnostics {
            use std::fmt::Write;
            writeln!(
                csv,
                "{},{},{},{},{},{}",
                input.player.index() + 1,
                input.seq,
                input.observed_ns,
                input.consumed_ns,
                input.song_frames,
                input.uncertainty_frames
            )
            .expect("Writing to a String cannot fail");
        }
        std::fs::write(path.with_extension("csv"), csv).map_err(|error| {
            format!(
                "Replay saved at {}, but diagnostics failed: {error}",
                path.display()
            )
        })?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_time_survives_late_consumption_and_replays_identically() {
        let mut session = Session::new(SessionEpoch(1)).unwrap();
        session
            .observe_audio(1.0, MonotonicTime::from_nanos(1_000_000_000))
            .unwrap();
        session
            .hit(PlayerId::P1, 1_010_000_000, 1_200_000_000)
            .unwrap();
        session
            .hit(PlayerId::P2, 1_020_000_000, 1_200_000_000)
            .unwrap();
        assert_eq!(session.diagnostics[0].song_frames, 48_480);
        session.finish().unwrap();
        let restored = session
            .replay
            .replay(
                CONTENT_ID,
                RULES_ID,
                dev_song::anchors(),
                DuoRules::default(),
            )
            .unwrap();
        assert_eq!(session.engine.events(), restored.events());
        assert!(
            session
                .engine
                .events()
                .iter()
                .any(|event| matches!(event, DuoEvent::FreeSync(_)))
        );
    }

    #[test]
    fn cursor_rollback_cannot_reopen_confirmed_history() {
        let mut session = Session::new(SessionEpoch(1)).unwrap();
        session
            .observe_audio(1.0, MonotonicTime::from_nanos(1_000_000_000))
            .unwrap();
        session.advance().unwrap();
        let before = session.clock.last_observation();
        assert!(
            session
                .observe_audio(0.9375, MonotonicTime::from_nanos(1_010_000_000))
                .is_err()
        );
        assert_eq!(session.clock.last_observation(), before);
        session
            .hit(PlayerId::P1, 1_010_000_000, 1_020_000_000)
            .unwrap();
    }

    #[test]
    fn repeatedly_reading_a_stalled_cursor_does_not_keep_the_clock_healthy() {
        let mut session = Session::new(SessionEpoch(1)).unwrap();
        session
            .observe_audio(1.0, MonotonicTime::from_nanos(1_000_000_000))
            .unwrap();
        for millis in (16..=240).step_by(16) {
            let at = MonotonicTime::from_nanos(1_000_000_000 + millis * 1_000_000);
            session.observe_audio(1.0, at).unwrap();
            session.update_position(at).unwrap();
        }
        let expired = MonotonicTime::from_nanos(1_256_000_000);
        session.observe_audio(1.0, expired).unwrap();
        assert!(session.update_position(expired).is_err());
        assert_eq!(session.clock.state(), ClockState::Expired);
    }
}
