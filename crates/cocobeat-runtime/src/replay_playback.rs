//! Read-only playback of validated facts through the existing core
//!
//! Replay contains no historical arrival or callback timestamps
//! A fact becomes visible when the audio source cursor reaches its song frame
//! Original file order is retained, so an earlier future fact holds back its prefix
//! EOF drains only the recorded facts, including original final watermarks

use cocobeat_core::DuoEngine;
use cocobeat_schema::{DuoEvent, DuoInput, Hit, SongTime, content::MAX_CANONICAL_FRAMES};
use std::ops::Range;

pub(crate) struct ReplayPlayback {
    end: SongTime,
    next: usize,
    through: SongTime,
    presentation_time: SongTime,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PlaybackPresentation {
    Hit(Hit),
    Event { event: DuoEvent, observed: SongTime },
}

pub(crate) struct PlaybackBatch {
    pub facts: Range<usize>,
    pub hits: [usize; 2],
    pub events: Vec<DuoEvent>,
    pub presentation: Vec<PlaybackPresentation>,
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
            presentation_time: SongTime::ZERO,
        })
    }

    pub fn consumed(&self) -> usize {
        self.next
    }

    /// Call only with a freshly constructed engine using the original Replay epoch
    pub fn reset(&mut self) {
        self.next = 0;
        self.through = SongTime::ZERO;
        self.presentation_time = SongTime::ZERO;
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
        let mut presentation = Vec::new();
        while let Some(&fact) = facts.get(self.next) {
            let at = match fact {
                DuoInput::Hit(hit) => hit.song_time,
                DuoInput::Watermark { through, .. } => through,
            }
            .clamp(SongTime::ZERO, self.end);
            if at > through {
                break;
            }
            let emitted = engine.ingest(fact).map_err(|error| error.to_string())?;
            self.presentation_time = self.presentation_time.max(at);
            if let DuoInput::Hit(hit) = fact {
                hits[hit.player.index()] += 1;
                presentation.push(PlaybackPresentation::Hit(hit));
            }
            presentation.extend(
                emitted
                    .iter()
                    .cloned()
                    .map(|event| PlaybackPresentation::Event {
                        event,
                        observed: self.presentation_time,
                    }),
            );
            events.extend(emitted);
            self.next += 1;
        }
        self.through = through;
        Ok(PlaybackBatch {
            facts: first..self.next,
            hits,
            events,
            presentation,
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

    #[test]
    fn presentation_order_and_observation_times_are_independent_of_advance_chunking() {
        let end = SongTime::from_frames(240_000);
        let later_hit = |player| {
            DuoInput::Hit(Hit {
                epoch: SessionEpoch(9),
                player,
                seq: 1,
                song_time: SongTime::from_frames(210_000),
            })
        };
        let facts = [
            hit(PlayerId::P1),
            watermark(PlayerId::P1, 200_000),
            hit(PlayerId::P2),
            watermark(PlayerId::P2, 200_000),
            later_hit(PlayerId::P1),
            later_hit(PlayerId::P2),
            watermark(PlayerId::P1, 260_000),
            watermark(PlayerId::P2, 260_000),
        ];
        let mut playback = ReplayPlayback::new(end).unwrap();
        let whole = playback.advance(&facts, &mut engine(), end).unwrap();
        assert!(matches!(
            whole.presentation.first(),
            Some(PlaybackPresentation::Hit(_))
        ));
        assert!(
            matches!(whole.presentation.get(1), Some(PlaybackPresentation::Hit(hit)) if hit.player == PlayerId::P2)
        );
        let observations: Vec<_> = whole
            .presentation
            .iter()
            .filter_map(|item| match item {
                PlaybackPresentation::Event { observed, .. } => Some(*observed),
                PlaybackPresentation::Hit(_) => None,
            })
            .collect();
        assert!(!observations.is_empty());
        assert_eq!(observations.first(), Some(&SongTime::from_frames(200_000)));
        assert_eq!(observations.last(), Some(&end));
        let first_event = whole
            .presentation
            .iter()
            .position(|item| matches!(item, PlaybackPresentation::Event { .. }))
            .unwrap();
        assert!(
            whole.presentation[first_event + 1..]
                .iter()
                .any(|item| matches!(item, PlaybackPresentation::Hit(_)))
        );
        for cursors in [
            vec![0, 100, 200_000, 220_000, 240_000],
            vec![200_001, 240_000],
        ] {
            playback.reset();
            let mut actual = engine();
            let mut split = Vec::new();
            for frame in cursors {
                split.extend(
                    playback
                        .advance(&facts, &mut actual, SongTime::from_frames(frame))
                        .unwrap()
                        .presentation,
                );
            }
            assert_eq!(split, whole.presentation);
        }
    }
}
