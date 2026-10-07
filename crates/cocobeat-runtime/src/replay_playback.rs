//! Read-only playback of validated facts through the existing core
//!
//! Replay contains no historical arrival or callback timestamps
//! A fact becomes visible when the audio source cursor reaches its song frame
//! Original file order is retained, so an earlier future fact holds back its prefix
//! EOF drains only the recorded facts, including original final watermarks

use cocobeat_core::DuoEngine;
use cocobeat_schema::{DuoEvent, DuoInput, SongTime, content::MAX_CANONICAL_FRAMES};
use std::ops::Range;

pub(crate) struct ReplayPlayback {
    end: SongTime,
    next: usize,
    through: SongTime,
}

pub(crate) struct PlaybackBatch {
    pub facts: Range<usize>,
    pub hits: [usize; 2],
    pub events: Vec<DuoEvent>,
}

impl ReplayPlayback {
    /// The caller validates the full package and core history before opening a window
    pub fn new(end: SongTime) -> Result<Self, String> {
        if !(1..=MAX_CANONICAL_FRAMES as i64).contains(&end.frames()) {
            return Err("Replay duration must cover more than zero and at most ten minutes".into());
        }
        Ok(Self {
            end,
            next: 0,
            through: SongTime::ZERO,
        })
    }

    pub fn consumed(&self) -> usize {
        self.next
    }

    /// Call only with a freshly constructed engine using the original Replay epoch
    pub fn reset(&mut self) {
        self.next = 0;
        self.through = SongTime::ZERO;
    }

    pub fn advance(
        &mut self,
        facts: &[DuoInput],
        engine: &mut DuoEngine,
        through: SongTime,
    ) -> Result<PlaybackBatch, String> {
        if through < self.through || through > self.end {
            return Err("Replay source cursor must be monotonic and inside the song".into());
        }
        let first = self.next;
        let mut hits = [0; 2];
        let mut events = Vec::new();
        while let Some(&fact) = facts.get(self.next) {
            let at = match fact {
                DuoInput::Hit(hit) => hit.song_time,
                DuoInput::Watermark { through, .. } => through,
            }
            .clamp(SongTime::ZERO, self.end);
            if at > through {
                break;
            }
            events.extend(engine.ingest(fact).map_err(|error| error.to_string())?);
            if let DuoInput::Hit(hit) = fact {
                hits[hit.player.index()] += 1;
            }
            self.next += 1;
        }
        self.through = through;
        Ok(PlaybackBatch {
            facts: first..self.next,
            hits,
            events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_schema::{Anchor, DuoRules, Hit, PlayerId, SessionEpoch};

    fn engine() -> DuoEngine {
        DuoEngine::new(
            SessionEpoch(9),
            vec![Anchor {
                id: 1,
                song_time: SongTime::from_frames(100),
            }],
            DuoRules::default(),
        )
        .unwrap()
    }

    fn hit(player: PlayerId) -> DuoInput {
        DuoInput::Hit(Hit {
            epoch: SessionEpoch(9),
            player,
            seq: 0,
            song_time: SongTime::from_frames(100),
        })
    }

    fn watermark(player: PlayerId, through: i64) -> DuoInput {
        DuoInput::Watermark {
            epoch: SessionEpoch(9),
            player,
            through: SongTime::from_frames(through),
        }
    }

    #[test]
    fn original_prefix_eof_and_restart_reuse_core_without_changing_facts() {
        let end = SongTime::from_frames(240_000);
        let original = vec![
            hit(PlayerId::P1),
            watermark(PlayerId::P1, 200_000),
            hit(PlayerId::P2),
            watermark(PlayerId::P2, 200_000),
            watermark(PlayerId::P1, 260_000),
            watermark(PlayerId::P2, 260_000),
        ];
        let facts = original.clone();
        let mut expected = engine();
        for fact in &facts {
            expected.ingest(*fact).unwrap();
        }
        let mut playback = ReplayPlayback::new(end).unwrap();
        for _ in 0..2 {
            let mut actual = engine();
            let first = playback
                .advance(&facts, &mut actual, SongTime::from_frames(100))
                .unwrap();
            assert_eq!(first.facts, 0..1);
            assert_eq!(first.hits, [1, 0]);
            assert!(first.events.is_empty());
            assert!(
                playback
                    .advance(&facts, &mut actual, SongTime::from_frames(199_999))
                    .unwrap()
                    .events
                    .is_empty()
            );
            let joined = playback
                .advance(&facts, &mut actual, SongTime::from_frames(200_000))
                .unwrap();
            assert_eq!(joined.facts, 1..4);
            assert_eq!(joined.hits, [0, 1]);
            assert!(!joined.events.is_empty());
            assert_eq!(
                playback.advance(&facts, &mut actual, end).unwrap().facts,
                4..6
            );
            assert_eq!(actual.events(), expected.events());
            assert_eq!(
                playback.advance(&facts, &mut actual, end).unwrap().facts,
                6..6
            );
            assert_eq!(facts, original);
            playback.reset();
        }
    }

    #[test]
    fn partial_history_eof_creates_no_watermark_or_miss_and_invalid_cursor_is_rejected() {
        let end = SongTime::from_frames(4_800);
        let mut playback = ReplayPlayback::new(end).unwrap();
        let mut actual = engine();
        let facts = [hit(PlayerId::P1)];
        assert!(
            playback
                .advance(&facts, &mut actual, SongTime::from_frames(-1))
                .is_err()
        );
        assert_eq!(
            playback.advance(&facts, &mut actual, end).unwrap().facts,
            0..1
        );
        assert!(actual.events().is_empty());
        assert!(
            playback
                .advance(&facts, &mut actual, SongTime::ZERO)
                .is_err()
        );
        assert!(
            playback
                .advance(&facts, &mut actual, SongTime::from_frames(4_801))
                .is_err()
        );
        assert_eq!(
            playback.advance(&facts, &mut actual, end).unwrap().facts,
            1..1
        );
        assert!(actual.events().is_empty());
        assert!(ReplayPlayback::new(SongTime::ZERO).is_err());
    }
}
