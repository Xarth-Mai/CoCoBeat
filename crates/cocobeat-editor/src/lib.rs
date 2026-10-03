//! Exact Anchor edits and bounded undo history, independent of files and UI

use cocobeat_schema::{Anchor, MAX_CANONICAL_FRAMES, MAX_CONTENT_ITEMS, SongTime};
use std::collections::BTreeSet;

const MAX_HISTORY: usize = 1_024;

#[derive(Debug)]
pub struct AnchorEditor {
    end: SongTime,
    anchors: Vec<Anchor>,
    undo: Vec<Change>,
    redo: Vec<Change>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Change {
    before: Option<Anchor>,
    after: Option<Anchor>,
}

impl AnchorEditor {
    pub fn new(end: SongTime, mut anchors: Vec<Anchor>) -> Result<Self, String> {
        if !(1..=MAX_CANONICAL_FRAMES as i64).contains(&end.frames()) {
            return Err("Editor song must cover more than zero and at most ten minutes".into());
        }
        if anchors.len() > MAX_CONTENT_ITEMS {
            return Err("Anchor count exceeds the content item limit".into());
        }
        let mut ids = BTreeSet::new();
        for anchor in &anchors {
            validate_time(end, anchor.song_time)?;
            if !ids.insert(anchor.id) {
                return Err(format!("Duplicate Anchor ID: {}", anchor.id));
            }
        }
        anchors.sort_unstable_by_key(|anchor| (anchor.song_time, anchor.id));
        Ok(Self {
            end,
            anchors,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }

    pub fn add(&mut self, id: u64, time: SongTime) -> Result<(), String> {
        validate_time(self.end, time)?;
        if self.anchors.iter().any(|anchor| anchor.id == id) {
            return Err(format!("Duplicate Anchor ID: {id}"));
        }
        if self.anchors.len() == MAX_CONTENT_ITEMS {
            return Err("Anchor count exceeds the content item limit".into());
        }
        self.commit(Change {
            before: None,
            after: Some(Anchor {
                id,
                song_time: time,
            }),
        })
    }

    pub fn remove(&mut self, id: u64) -> Result<(), String> {
        let before = self.find(id)?;
        self.commit(Change {
            before: Some(before),
            after: None,
        })
    }

    /// Moving to the current frame preserves both history stacks
    pub fn move_to(&mut self, id: u64, time: SongTime) -> Result<(), String> {
        let before = self.find(id)?;
        validate_time(self.end, time)?;
        if before.song_time == time {
            return Ok(());
        }
        self.commit(Change {
            before: Some(before),
            after: Some(Anchor {
                id,
                song_time: time,
            }),
        })
    }

    pub fn undo(&mut self) -> Result<(), String> {
        let change = self.undo.pop().ok_or("No Anchor edit to undo")?;
        self.apply(Change {
            before: change.after,
            after: change.before,
        });
        self.redo.push(change);
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), String> {
        let change = self.redo.pop().ok_or("No Anchor edit to redo")?;
        self.apply(change);
        self.undo.push(change);
        Ok(())
    }

    fn find(&self, id: u64) -> Result<Anchor, String> {
        // ponytail: linear ID lookup is bounded by 100,000 Anchors; index if UI profiling requires it
        self.anchors
            .iter()
            .find(|anchor| anchor.id == id)
            .copied()
            .ok_or_else(|| format!("Missing Anchor ID: {id}"))
    }

    fn commit(&mut self, change: Change) -> Result<(), String> {
        if self.undo.len() == MAX_HISTORY {
            return Err("Anchor edit history is full; undo before making another change".into());
        }
        self.apply(change);
        self.redo.clear();
        self.undo.push(change);
        Ok(())
    }

    fn apply(&mut self, change: Change) {
        if let Some(before) = change.before {
            let index = self
                .anchors
                .binary_search_by_key(&(before.song_time, before.id), |anchor| {
                    (anchor.song_time, anchor.id)
                })
                .expect("Editor history refers to an existing Anchor");
            self.anchors.remove(index);
        }
        if let Some(after) = change.after {
            let index = self.anchors.partition_point(|anchor| {
                (anchor.song_time, anchor.id) < (after.song_time, after.id)
            });
            self.anchors.insert(index, after);
        }
    }
}

fn validate_time(end: SongTime, time: SongTime) -> Result<(), String> {
    if time < SongTime::ZERO || time >= end {
        return Err("Anchor frame must lie within [0, song end)".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(id: u64, frame: i64) -> Anchor {
        Anchor {
            id,
            song_time: SongTime::from_frames(frame),
        }
    }

    #[test]
    fn edits_preserve_exact_frames_order_and_round_trip() {
        let original = vec![
            anchor(0, 0),
            anchor(3, 48),
            anchor(8, 48),
            anchor(u64::MAX, 99),
        ];
        let mut reversed = original.clone();
        reversed.reverse();
        let mut editor = AnchorEditor::new(SongTime::from_frames(100), reversed).unwrap();
        assert_eq!(editor.anchors(), original);
        editor.add(5, SongTime::from_frames(48)).unwrap();
        editor.move_to(8, SongTime::from_frames(1)).unwrap();
        editor.remove(0).unwrap();
        let edited = vec![
            anchor(8, 1),
            anchor(3, 48),
            anchor(5, 48),
            anchor(u64::MAX, 99),
        ];
        assert_eq!(editor.anchors(), edited);
        for _ in 0..3 {
            editor.undo().unwrap();
        }
        assert_eq!(editor.anchors(), original);
        assert!(editor.undo().is_err());
        for _ in 0..3 {
            editor.redo().unwrap();
        }
        assert_eq!(editor.anchors(), edited);
        assert!(editor.redo().is_err());
        editor.undo().unwrap();
        editor.move_to(8, SongTime::from_frames(1)).unwrap();
        editor.redo().unwrap();
        assert_eq!(editor.anchors(), edited);
        editor.undo().unwrap();
        editor.move_to(0, SongTime::from_frames(99)).unwrap();
        assert!(editor.redo().is_err());
        assert_eq!(
            editor.anchors(),
            [
                anchor(8, 1),
                anchor(3, 48),
                anchor(5, 48),
                anchor(0, 99),
                anchor(u64::MAX, 99)
            ]
        );
    }

    #[test]
    fn invalid_edits_keep_state_and_redo_and_content_limits_are_exact() {
        for end in [i64::MIN, -1, 0, MAX_CANONICAL_FRAMES as i64 + 1, i64::MAX] {
            assert!(AnchorEditor::new(SongTime::from_frames(end), vec![]).is_err());
        }
        let end = SongTime::from_frames(MAX_CANONICAL_FRAMES as i64);
        assert!(AnchorEditor::new(end, vec![]).is_ok());
        assert!(AnchorEditor::new(end, vec![anchor(7, 0), anchor(7, 1)]).is_err());
        for frame in [i64::MIN, -1, end.frames(), i64::MAX] {
            assert!(AnchorEditor::new(end, vec![anchor(1, frame)]).is_err());
        }
        let mut editor = AnchorEditor::new(end, vec![anchor(0, 0)]).unwrap();
        editor
            .add(u64::MAX, SongTime::from_frames(end.frames() - 1))
            .unwrap();
        editor.undo().unwrap();
        let before = (
            editor.anchors.clone(),
            editor.undo.clone(),
            editor.redo.clone(),
        );
        for frame in [i64::MIN, -1, end.frames(), i64::MAX] {
            assert!(editor.add(2, SongTime::from_frames(frame)).is_err());
            assert!(editor.move_to(0, SongTime::from_frames(frame)).is_err());
        }
        assert!(editor.add(0, SongTime::ZERO).is_err());
        assert!(editor.remove(2).is_err());
        assert!(editor.move_to(2, SongTime::ZERO).is_err());
        assert!(editor.undo().is_err());
        assert_eq!(
            (&editor.anchors, &editor.undo, &editor.redo),
            (&before.0, &before.1, &before.2)
        );
        editor.redo().unwrap();
        assert_eq!(
            editor.anchors(),
            [anchor(0, 0), anchor(u64::MAX, end.frames() - 1)]
        );

        let maximum: Vec<_> = (0..MAX_CONTENT_ITEMS as u64)
            .map(|id| anchor(id, 0))
            .collect();
        let mut excessive = maximum.clone();
        excessive.push(anchor(u64::MAX, 0));
        assert!(AnchorEditor::new(end, excessive).is_err());
        let mut editor = AnchorEditor::new(SongTime::from_frames(1), maximum.clone()).unwrap();
        assert!(editor.add(u64::MAX, SongTime::ZERO).is_err());
        assert_eq!(editor.anchors(), maximum);
        assert!(editor.undo().is_err());
        editor.remove(0).unwrap();
        editor.add(u64::MAX, SongTime::ZERO).unwrap();
        assert_eq!(editor.anchors().len(), MAX_CONTENT_ITEMS);
        editor.undo().unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.anchors(), maximum);
        editor.redo().unwrap();
        editor.redo().unwrap();
        assert_eq!(editor.anchors().len(), MAX_CONTENT_ITEMS);
        assert_eq!(editor.anchors().last(), Some(&anchor(u64::MAX, 0)));
    }

    #[test]
    fn full_history_rejects_changes_without_losing_undo_or_redo() {
        let mut editor = AnchorEditor::new(SongTime::from_frames(3), vec![anchor(0, 0)]).unwrap();
        for step in 1..=1_024 {
            editor.move_to(0, SongTime::from_frames(step % 2)).unwrap();
        }
        assert!(editor.add(1, SongTime::ZERO).is_err());
        assert!(editor.remove(0).is_err());
        assert!(editor.move_to(0, SongTime::from_frames(1)).is_err());
        editor.move_to(0, SongTime::ZERO).unwrap();
        assert_eq!(editor.anchors(), [anchor(0, 0)]);
        editor.undo().unwrap();
        assert_eq!(editor.anchors(), [anchor(0, 1)]);
        editor.move_to(0, SongTime::from_frames(2)).unwrap();
        assert!(editor.redo().is_err());
        for _ in 0..1_024 {
            editor.undo().unwrap();
        }
        assert_eq!(editor.anchors(), [anchor(0, 0)]);
        assert!(editor.undo().is_err());
        for _ in 0..1_024 {
            editor.redo().unwrap();
        }
        assert_eq!(editor.anchors(), [anchor(0, 2)]);
        assert!(editor.redo().is_err());
        assert!(editor.remove(0).is_err());
    }
}
