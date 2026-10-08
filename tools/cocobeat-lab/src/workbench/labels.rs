//! Manual label forms over the existing independent-label schema

use crate::labels::{self, AnchorDecision, Label, Location, Playback, Source};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LocationKind {
    Point,
    Interval,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LabelDraft {
    pub(super) item_id: u64,
    pub(super) kind: LocationKind,
    pub(super) frame: String,
    pub(super) start_frame: String,
    pub(super) end_frame: String,
    pub(super) decision: AnchorDecision,
    pub(super) reason: String,
}

impl LabelDraft {
    fn point(item_id: u64, frame: i64) -> Self {
        Self {
            item_id,
            kind: LocationKind::Point,
            frame: frame.to_string(),
            start_frame: frame.to_string(),
            end_frame: frame.saturating_add(1).to_string(),
            decision: AnchorDecision::Uncertain,
            reason: String::new(),
        }
    }

    fn from_label(label: &Label) -> Self {
        let mut draft = match label.location {
            Location::Point { frame } => Self::point(label.item_id, frame),
            Location::Interval {
                start_frame,
                end_frame,
            } => Self {
                kind: LocationKind::Interval,
                start_frame: start_frame.to_string(),
                end_frame: end_frame.to_string(),
                ..Self::point(label.item_id, start_frame)
            },
        };
        draft.decision = label.anchor_decision;
        draft.reason.clone_from(&label.reason);
        draft
    }

    fn label(&self) -> Result<Label, String> {
        let location = match self.kind {
            LocationKind::Point => Location::Point {
                frame: parse_frame(&self.frame)?,
            },
            LocationKind::Interval => Location::Interval {
                start_frame: parse_frame(&self.start_frame)?,
                end_frame: parse_frame(&self.end_frame)?,
            },
        };
        Ok(Label {
            item_id: self.item_id,
            location,
            anchor_decision: self.decision,
            reason: self.reason.clone(),
            playback: Playback::Stereo,
        })
    }
}

pub(super) struct LabelView {
    pub(super) document: labels::Document,
    pub(super) selected: usize,
    pub(super) draft: Option<LabelDraft>,
    initial: labels::Document,
    initial_draft: Option<LabelDraft>,
}

impl LabelView {
    pub(super) fn new(source: Source) -> Self {
        let document = labels::Document {
            schema_version: 1,
            source,
            reviewer: String::new(),
            labels: Vec::new(),
        };
        Self {
            initial: document.clone(),
            document,
            selected: 0,
            draft: None,
            initial_draft: None,
        }
    }

    pub(super) fn dirty(&self) -> bool {
        self.document != self.initial || self.draft_dirty()
    }

    pub(super) fn source(&self) -> &Source {
        &self.initial.source
    }

    pub(super) fn draft_dirty(&self) -> bool {
        self.draft != self.initial_draft
    }

    pub(super) fn begin_add(&mut self, frame: i64) -> Result<(), String> {
        self.require_no_draft()?;
        let ids: BTreeSet<_> = self
            .document
            .labels
            .iter()
            .map(|label| label.item_id)
            .collect();
        let item_id = (0..)
            .find(|id| !ids.contains(id))
            .expect("Bounded independent labels leave a free item ID");
        self.begin(LabelDraft::point(item_id, frame));
        Ok(())
    }

    pub(super) fn begin_edit(&mut self) -> Result<(), String> {
        self.require_no_draft()?;
        let label = self
            .document
            .labels
            .get(self.selected)
            .ok_or("No independent label selected")?;
        self.begin(LabelDraft::from_label(label));
        Ok(())
    }

    pub(super) fn remove_selected(&mut self) -> Result<(), String> {
        self.require_no_draft()?;
        if self.selected >= self.document.labels.len() {
            return Err("No independent label selected".into());
        }
        self.document.labels.remove(self.selected);
        self.select(self.selected);
        Ok(())
    }

    pub(super) fn select(&mut self, index: usize) {
        self.selected = index.min(self.document.labels.len().saturating_sub(1));
    }

    pub(super) fn browse(&mut self, step: i32) {
        self.select(self.selected.saturating_add_signed(step as isize));
    }

    pub(super) fn apply(&mut self) -> Result<usize, String> {
        let draft = self.draft.as_ref().ok_or("No independent label draft")?;
        if self.initial_draft.as_ref().map(|initial| initial.item_id) != Some(draft.item_id) {
            return Err("Independent label item_id cannot be edited".into());
        }
        let label = draft.label()?;
        let mut document = self.document.clone();
        let index = document
            .labels
            .iter()
            .position(|existing| existing.item_id == label.item_id)
            .unwrap_or(document.labels.len());
        if index == document.labels.len() {
            document.labels.push(label);
        } else {
            document.labels[index] = label;
        }
        document.validate(&self.initial.source)?;
        self.document = document;
        self.selected = index;
        self.cancel();
        Ok(index)
    }

    pub(super) fn cancel(&mut self) {
        self.draft = None;
        self.initial_draft = None;
    }

    fn begin(&mut self, draft: LabelDraft) {
        self.initial_draft = Some(draft.clone());
        self.draft = Some(draft);
    }

    fn require_no_draft(&self) -> Result<(), String> {
        if self.draft.is_some() {
            Err("Apply or cancel the current independent label draft first".into())
        } else {
            Ok(())
        }
    }
}

fn parse_frame(value: &str) -> Result<i64, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Label position must be an exact non-negative ASCII integer frame".into());
    }
    value
        .parse()
        .map_err(|_| "Label position exceeds the integer frame range".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(frames: u64) -> Source {
        Source {
            content_id: format!("package-blake3:{}", "1".repeat(64)),
            audio_blake3: "2".repeat(64),
            canonical_frames: frames,
            audio_basis: labels::AudioBasis::CanonicalDecoded,
        }
    }

    fn view() -> LabelView {
        let mut view = LabelView::new(source(100));
        view.document.reviewer = "reviewer".into();
        view
    }

    fn add(view: &mut LabelView, frame: i64, decision: AnchorDecision) {
        view.begin_add(frame).unwrap();
        let draft = view.draft.as_mut().unwrap();
        draft.reason = "manual reason".into();
        draft.decision = decision;
        view.apply().unwrap();
    }

    #[test]
    fn actual_unapplied_text_is_dirty_and_cancel_does_not_commit() {
        let mut view = LabelView::new(source(100));
        assert!(view.document.labels.is_empty());
        assert!(view.document.reviewer.is_empty());
        assert!(!view.dirty());
        view.begin_add(0).unwrap();
        assert_eq!(
            view.draft.as_ref().unwrap().decision,
            AnchorDecision::Uncertain
        );
        assert!(!view.dirty());
        view.draft.as_mut().unwrap().reason = "未应用の草稿 한글 Україна 🥁".into();
        assert!(view.dirty());
        assert!(view.begin_add(1).is_err());
        assert!(view.remove_selected().is_err());
        view.cancel();
        assert!(!view.dirty());
        assert!(view.document.labels.is_empty());
        view.document.reviewer = "新しい reviewer".into();
        assert!(view.dirty());
    }

    #[test]
    fn edit_targets_item_id_not_the_later_browsed_row_and_delete_preserves_other_ids() {
        let mut view = view();
        add(&mut view, 0, AnchorDecision::ShouldAnchor);
        add(&mut view, 99, AnchorDecision::ShouldNotAnchor);
        add(&mut view, 50, AnchorDecision::Uncertain);
        view.select(0);
        view.begin_edit().unwrap();
        assert!(!view.draft_dirty());
        view.draft.as_mut().unwrap().frame = "1".into();
        view.browse(1);
        assert_eq!(view.selected, 1);
        assert_eq!(view.apply().unwrap(), 0);
        assert_eq!(
            view.document.labels[0].location,
            Location::Point { frame: 1 }
        );
        assert_eq!(
            view.document.labels[1].location,
            Location::Point { frame: 99 }
        );
        view.select(1);
        view.remove_selected().unwrap();
        assert_eq!(
            view.document
                .labels
                .iter()
                .map(|label| label.item_id)
                .collect::<Vec<_>>(),
            [0, 2]
        );
        add(&mut view, 25, AnchorDecision::Uncertain);
        assert_eq!(
            view.document
                .labels
                .iter()
                .map(|label| label.item_id)
                .collect::<Vec<_>>(),
            [0, 2, 1]
        );
        view.browse(i32::MIN);
        assert_eq!(view.selected, 0);
        view.browse(i32::MAX);
        assert_eq!(view.selected, 2);
    }

    #[test]
    fn rejected_apply_preserves_exact_draft_and_existing_document() {
        let mut view = view();
        add(&mut view, 0, AnchorDecision::ShouldAnchor);
        view.begin_edit().unwrap();
        for frame in [
            "100",
            "-1",
            "1.0",
            " 1",
            "+1",
            "١",
            "9223372036854775808",
            "",
        ] {
            view.draft.as_mut().unwrap().frame = frame.into();
            let document = view.document.clone();
            let draft = view.draft.clone();
            assert!(view.apply().is_err(), "{frame:?}");
            assert_eq!(view.document, document);
            assert_eq!(view.draft, draft);
        }
        view.draft.as_mut().unwrap().frame = "99".into();
        view.draft.as_mut().unwrap().reason = " \n".into();
        assert!(view.apply().is_err());
        view.draft.as_mut().unwrap().reason = " restored ".into();
        view.apply().unwrap();
        assert_eq!(view.document.labels[0].reason, " restored ");
    }

    #[test]
    fn interval_right_endpoint_is_exclusive_and_may_equal_song_extent() {
        let mut view = view();
        view.begin_add(0).unwrap();
        let draft = view.draft.as_mut().unwrap();
        draft.kind = LocationKind::Interval;
        draft.start_frame = "0".into();
        draft.end_frame = "100".into();
        draft.reason = "entire song".into();
        view.apply().unwrap();
        assert_eq!(
            view.document.labels[0].location,
            Location::Interval {
                start_frame: 0,
                end_frame: 100
            }
        );
        view.begin_edit().unwrap();
        for (start, end) in [("0", "101"), ("100", "100"), ("2", "1")] {
            let draft = view.draft.as_mut().unwrap();
            draft.start_frame = start.into();
            draft.end_frame = end.into();
            assert!(view.apply().is_err());
        }
        view.cancel();
        assert_eq!(
            view.document.labels[0].location,
            Location::Interval {
                start_frame: 0,
                end_frame: 100
            }
        );
    }

    #[test]
    fn unicode_byte_limits_and_original_source_are_preserved() {
        let mut view = view();
        view.document.reviewer = "界".repeat(21) + "a";
        view.begin_add(0).unwrap();
        view.draft.as_mut().unwrap().reason = "界".repeat(682) + "ab";
        view.apply().unwrap();
        let saved = view.document.clone();
        assert_eq!(saved.source, source(100));
        assert_eq!(saved.reviewer.len(), 64);
        assert_eq!(saved.labels[0].reason.len(), 2048);
        assert_eq!(saved.labels[0].playback, Playback::Stereo);
        view.begin_edit().unwrap();
        view.draft.as_mut().unwrap().reason.push('界');
        assert!(view.apply().is_err());
        view.draft.as_mut().unwrap().reason = saved.labels[0].reason.clone();
        view.document.reviewer.push('界');
        assert!(view.apply().is_err());
        view.document.reviewer = saved.reviewer;
        view.document.source.canonical_frames += 1;
        assert_eq!(view.source(), &source(100));
        assert!(view.apply().is_err());
        view.document.source = source(100);
        view.draft.as_mut().unwrap().item_id = 9;
        assert!(view.apply().is_err());
        view.draft.as_mut().unwrap().item_id = 0;
        view.apply().unwrap();
        assert_eq!(view.document.source, source(100));
    }

    #[test]
    fn full_record_limit_rejects_new_record_and_keeps_the_manual_draft() {
        let mut view = view();
        view.document.labels = (0..1024)
            .map(|item_id| Label {
                item_id,
                location: Location::Point { frame: 0 },
                anchor_decision: AnchorDecision::Uncertain,
                reason: "manual".into(),
                playback: Playback::Stereo,
            })
            .collect();
        view.begin_add(0).unwrap();
        view.draft.as_mut().unwrap().reason = "record 1025".into();
        assert!(view.apply().is_err());
        assert_eq!(view.document.labels.len(), 1024);
        assert_eq!(view.draft.as_ref().unwrap().item_id, 1024);
        assert_eq!(view.draft.as_ref().unwrap().reason, "record 1025");
    }
}
