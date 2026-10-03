//! Live input and saved histories share one deterministic rule engine

use crate::{
    clock::*,
    content::{RULES_ID, SongContent},
};
use cocobeat_core::DuoEngine;
use cocobeat_replay::{MAX_FACTS, Replay, ReplayIdentity};
use cocobeat_schema::{
    AnchorGrade, DuoEvent, DuoInput, DuoRules, Hit, PlayerId, SessionEpoch, SongTime,
    content::MAX_CANONICAL_FRAMES,
};
use std::path::{Path, PathBuf};

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

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SessionResults {
    pub hits: [u64; 2],
    pub anchors: [[u64; 4]; 2],
    pub free_sync: u64,
    pub anchor_sync: u64,
}

pub struct Session {
    pub engine: DuoEngine,
    pub replay: Replay,
    pub clock: ClockBridge,
    pub diagnostics: Vec<CaptureDiagnostic>,
    pub current: SongTime,
    pub uncertainty_frames: u64,
    end: SongTime,
    sequence: [u64; 2],
    watermark: Option<SongTime>,
}

impl Session {
    #[cfg(test)]
    pub fn new(epoch: SessionEpoch) -> Result<Self, String> {
        Self::for_content(epoch, &SongContent::development())
    }

    pub fn for_content(epoch: SessionEpoch, content: &SongContent) -> Result<Self, String> {
        if !(1..=MAX_CANONICAL_FRAMES as i64).contains(&content.end.frames()) {
            return Err("Session content must cover more than zero and at most ten minutes".into());
        }
        if content
            .anchors
            .iter()
            .any(|anchor| anchor.song_time < SongTime::ZERO || anchor.song_time >= content.end)
        {
            return Err("Session Anchor lies outside the song timeline".into());
        }
        Ok(Self {
            engine: DuoEngine::new(epoch, content.anchors.clone(), DuoRules::default())
                .map_err(|error| error.to_string())?,
            replay: Replay::new(
                ReplayIdentity {
                    content_id: content.content_id.clone(),
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
            end: content.end,
            sequence: [0, 0],
            watermark: None,
        })
    }

    pub fn epoch(&self) -> SessionEpoch {
        self.replay.epoch()
    }

    pub fn observe_audio(&mut self, position: f64, at: MonotonicTime) -> Result<(), String> {
        if !(0.0..=self.end.as_seconds_f64()).contains(&position) {
            return Err("Audio cursor is outside the song timeline".into());
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
        if estimate.song_time < SongTime::ZERO || estimate.song_time >= self.end {
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
        let through = self
            .end
            .checked_add_frames(tail)
            .and_then(|time| time.checked_add_frames(1))
            .ok_or("Final history watermark overflow")?;
        let events = self.close_history(through)?;
        self.current = self.end;
        Ok(events)
    }

    pub fn summary(&self) -> SessionResults {
        let mut results = SessionResults::default();
        for fact in self.replay.facts() {
            if let DuoInput::Hit(hit) = fact {
                results.hits[hit.player.index()] += 1;
            }
        }
        for event in self.engine.events() {
            match event {
                DuoEvent::AnchorJudged(judgement) => {
                    let grade = match judgement.grade {
                        AnchorGrade::Precise => 0,
                        AnchorGrade::Good => 1,
                        AnchorGrade::LateOrEarly => 2,
                        AnchorGrade::Miss => 3,
                    };
                    results.anchors[judgement.player.index()][grade] += 1;
                }
                DuoEvent::FreeSync(_) => results.free_sync += 1,
                DuoEvent::AnchorSync(_) => results.anchor_sync += 1,
            }
        }
        results
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
    use crate::{content::CONTENT_ID, dev_song};
    use cocobeat_schema::Anchor;

    fn fixture_content(frames: i64) -> SongContent {
        SongContent {
            content_id: format!("session-fixture-{frames}"),
            end: SongTime::from_frames(frames),
            anchors: vec![Anchor {
                id: 1,
                song_time: SongTime::from_frames(frames - 1),
            }],
            sections: vec![],
        }
    }

    #[test]
    fn content_end_controls_eof_anchor_confirmation_and_replay_identity() {
        for frames in [1, 4_801, 65 * 48_000 + 17, MAX_CANONICAL_FRAMES as i64] {
            let content = fixture_content(frames);
            let mut session = Session::for_content(SessionEpoch(7), &content).unwrap();
            assert_eq!(session.replay.identity().content_id, content.content_id);
            let last = SongTime::from_frames(frames - 1);
            let captured = 1_000_000_000;
            session
                .observe_audio(last.as_seconds_f64(), MonotonicTime::from_nanos(captured))
                .unwrap();
            session
                .update_position(MonotonicTime::from_nanos(captured))
                .unwrap();
            assert_eq!(session.current, last);
            for player in [PlayerId::P1, PlayerId::P2] {
                session
                    .hit(player, captured, captured + 500_000_000)
                    .unwrap();
            }
            assert!(
                session
                    .diagnostics
                    .iter()
                    .all(|hit| hit.song_frames == frames - 1)
            );
            session.advance().unwrap();
            assert_eq!(session.summary().anchors, [[0; 4]; 2]);

            let eof_at = MonotonicTime::from_nanos(captured + 20_834);
            session
                .observe_audio(content.end.as_seconds_f64(), eof_at)
                .unwrap();
            session.update_position(eof_at).unwrap();
            assert_eq!(session.current, content.end);
            let before = session.replay.facts().len();
            for at in [eof_at.nanos(), eof_at.nanos() + 1_000_000] {
                assert!(session.hit(PlayerId::P1, at, at + 1).unwrap().is_empty());
            }
            assert_eq!(session.replay.facts().len(), before);
            assert_eq!(session.sequence, [1, 1]);
            assert_eq!(session.diagnostics.len(), 2);
            let observation = session.clock.last_observation();
            for position in [
                -0.1,
                f64::NAN,
                f64::INFINITY,
                content.end.as_seconds_f64() + 1.0 / 48_000.0,
            ] {
                assert!(
                    session
                        .observe_audio(position, MonotonicTime::from_nanos(captured + 2_000_000))
                        .is_err()
                );
                assert_eq!(session.clock.last_observation(), observation);
            }
            session.finish().unwrap();
            assert_eq!(session.current, content.end);
            assert_eq!(session.clock.last_observation(), observation);
            assert_eq!(session.summary().hits, [1, 1]);
            assert_eq!(session.summary().anchors, [[1, 0, 0, 0]; 2]);
            assert_eq!(session.summary().anchor_sync, 1);
            let recorded = Replay::decode(session.replay.encode().unwrap().as_slice()).unwrap();
            let restored = recorded
                .replay(
                    &content.content_id,
                    RULES_ID,
                    content.anchors.clone(),
                    DuoRules::default(),
                )
                .unwrap();
            assert_eq!(session.engine.events(), restored.events());
            assert_eq!(session.engine.resonance(), restored.resonance());
            let mut other_chart = content.clone();
            other_chart.content_id.push_str("-other-chart");
            other_chart.anchors[0].id += 1;
            assert!(
                recorded
                    .replay(
                        &other_chart.content_id,
                        RULES_ID,
                        other_chart.anchors,
                        DuoRules::default()
                    )
                    .unwrap_err()
                    .to_string()
                    .contains("identity mismatch")
            );
        }
    }

    #[test]
    fn content_metadata_rejects_invalid_timelines_and_preserves_the_development_wrapper() {
        let original = fixture_content(100);
        for end in [-1, 0, MAX_CANONICAL_FRAMES as i64 + 1, i64::MAX] {
            let mut content = original.clone();
            content.end = SongTime::from_frames(end);
            assert!(Session::for_content(SessionEpoch(1), &content).is_err());
        }
        for frame in [-1, 100, i64::MAX] {
            let mut content = original.clone();
            content.anchors[0].song_time = SongTime::from_frames(frame);
            assert!(Session::for_content(SessionEpoch(1), &content).is_err());
        }
        let mut content = original.clone();
        content.anchors.push(content.anchors[0]);
        assert!(Session::for_content(SessionEpoch(1), &content).is_err());
        for identity in [String::new(), "x".repeat(257)] {
            content = original.clone();
            content.content_id = identity;
            assert!(Session::for_content(SessionEpoch(1), &content).is_err());
        }
        let mut old = Session::new(SessionEpoch(1)).unwrap();
        let mut explicit =
            Session::for_content(SessionEpoch(1), &SongContent::development()).unwrap();
        assert_eq!(old.replay, explicit.replay);
        assert_eq!(old.end.frames(), i64::from(dev_song::FRAMES));
        old.finish().unwrap();
        explicit.finish().unwrap();
        assert_eq!(old.replay, explicit.replay);
        assert_eq!(old.engine.events(), explicit.engine.events());
    }

    #[test]
    fn summary_counts_recorded_hits_and_only_confirmed_grades_for_the_whole_session() {
        let mut session = Session::new(SessionEpoch(7)).unwrap();
        for (seq, frame) in [
            48_000,
            dev_song::ANCHOR_FRAMES[0],
            dev_song::ANCHOR_FRAMES[1] + 3_000,
            dev_song::ANCHOR_FRAMES[2] + 6_000,
        ]
        .into_iter()
        .enumerate()
        {
            for player in [PlayerId::P1, PlayerId::P2] {
                session
                    .ingest(DuoInput::Hit(Hit {
                        epoch: session.epoch(),
                        player,
                        seq: seq as u64,
                        song_time: SongTime::from_frames(frame),
                    }))
                    .unwrap();
            }
        }
        assert_eq!(session.summary().hits, [4, 4]);
        assert_eq!(session.summary().anchors, [[0; 4]; 2]);
        session.finish().unwrap();
        let results = session.summary();
        assert_eq!(results.hits, [4, 4]);
        assert_eq!(results.anchors, [[1, 1, 1, 4]; 2]);
        assert_eq!(results.anchor_sync, 2);
        assert_eq!(results.free_sync, 2);
        assert_eq!(session.engine.resonance().inputs, [0, 0]);
        assert_eq!(session.summary(), results);
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
        assert_eq!(
            Session::new(SessionEpoch(8)).unwrap().summary(),
            SessionResults::default()
        );
    }

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

    #[test]
    fn recorder_capacity_failure_keeps_rules_and_capture_state_unchanged() {
        let mut session = Session::new(SessionEpoch(1)).unwrap();
        session
            .observe_audio(1.0, MonotonicTime::from_nanos(1_000_000_000))
            .unwrap();
        session
            .hit(PlayerId::P1, 1_010_000_000, 1_020_000_000)
            .unwrap();
        let fact = DuoInput::Watermark {
            epoch: session.epoch(),
            player: PlayerId::P1,
            through: SongTime::ZERO,
        };
        // Fill the recorder directly to isolate capacity preflight from rule processing
        while session.replay.facts().len() < MAX_FACTS - 1 {
            session.replay.record(fact).unwrap();
        }
        let before = format!("{:?}", session.engine);
        assert!(session.finish().unwrap_err().contains("capacity"));
        assert_eq!(format!("{:?}", session.engine), before);
        assert_eq!(session.watermark, None);
        assert_eq!(session.current, SongTime::ZERO);
        assert_eq!(session.replay.facts().len(), MAX_FACTS - 1);

        session.replay.record(fact).unwrap();
        assert!(
            session
                .hit(PlayerId::P2, 1_015_000_000, 1_020_000_000)
                .unwrap_err()
                .contains("capacity")
        );
        assert_eq!(format!("{:?}", session.engine), before);
        assert_eq!(session.sequence, [1, 0]);
        assert_eq!(session.diagnostics.len(), 1);
        assert_eq!(session.diagnostics[0].player, PlayerId::P1);
        assert_eq!(session.replay.facts().len(), MAX_FACTS);
    }
}
