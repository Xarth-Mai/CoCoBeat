use crate::replay::{self, ReportIndex};
use cocobeat_core::DuoEngine;
use cocobeat_media::ValidatedPackage;
use cocobeat_replay::{Replay, timing::TimingSidecar};
use cocobeat_runtime::{Locale, Message};
use cocobeat_schema::{DuoInput, DuoRules, Hit, PlayerId};
use serde_json::Value;
use std::path::Path;

pub(super) struct ReplayView {
    replay: Replay,
    engine: DuoEngine,
    index: ReportIndex,
    header: Value,
    summary: Value,
    timing: Option<TimingSidecar>,
    pub(super) selected: usize,
}

impl ReplayView {
    pub(super) fn load(
        package: &ValidatedPackage,
        path: &Path,
        timing_path: Option<&Path>,
    ) -> Result<Self, String> {
        let (replay, engine, timing) = replay::load_with_timing(package, path, timing_path)?;
        let mut header = replay::header(package, &replay, &engine, DuoRules::default());
        if let Some(timing) = &timing {
            header["report_version"] = 3.into();
            header["local_timing"] = replay::timing_header(timing);
        }
        Ok(Self {
            index: ReportIndex::new(&package.chart.anchors, &replay),
            header,
            summary: replay::summary(package.chart.anchors.len(), &replay, &engine)?,
            replay,
            engine,
            timing,
            selected: 0,
        })
    }

    pub(super) fn len(&self) -> usize {
        self.replay.facts().len() + self.engine.events().len()
    }

    fn record(&self, index: usize) -> Option<Value> {
        let mut record = if let Some(fact) = self.replay.facts().get(index) {
            Some(replay::fact_row(index, *fact))
        } else {
            let index = index.checked_sub(self.replay.facts().len())?;
            self.engine.events().get(index).map(|event| {
                self.index.event_row(index, *event).expect(
                    "Replayed events refer to the validated source Anchors and recorded Hits",
                )
            })
        }?;
        if let Some(timing) = &self.timing {
            replay::annotate_timing(&mut record, timing);
        }
        Some(record)
    }

    pub(super) fn select(&mut self, index: usize) -> Option<i64> {
        let record = self.record(index)?;
        self.selected = index;
        frame(&record)
    }

    pub(super) fn row(&self, index: usize, locale: Locale) -> String {
        let Some(record) = self.record(index) else {
            return String::new();
        };
        let kind = record["type"]
            .as_str()
            .expect("Diagnostic records have a type");
        let kind = match kind {
            "hit" | "watermark" => format!("P{} {kind}", record["player"]),
            "anchor_judged" => format!("P{} {kind} #{}", record["player"], record["anchor_id"]),
            "anchor_sync" => format!("{kind} #{}", record["anchor_id"]),
            _ => kind.to_owned(),
        };
        let (key, position) = if index < self.replay.facts().len() {
            ("workbench.replay.fact", index + 1)
        } else {
            (
                "workbench.replay.event",
                index + 1 - self.replay.facts().len(),
            )
        };
        Message::with(
            key,
            [
                ("index", position.to_string()),
                ("kind", kind),
                (
                    "frame",
                    frame(&record)
                        .expect("Diagnostic records have an integer frame")
                        .to_string(),
                ),
            ],
        )
        .render(locale)
    }

    pub(super) fn details(&self, locale: Locale) -> String {
        let selected = self.record(self.selected).unwrap_or(Value::Null);
        let pretty = |value: &Value| {
            serde_json::to_string_pretty(value).expect("Diagnostic values contain valid JSON")
        };
        let mut details = format!(
            "{}\n\n{}\n\n{}\n\n{}",
            pretty(&selected),
            pretty(&self.header),
            pretty(&self.summary),
            locale.text("workbench.replay.fields"),
        );
        if self.timing.is_some() {
            details.push_str(&format!(
                "\n\n{}\n{}",
                locale.text("workbench.replay.timing"),
                locale.text("workbench.replay.timing_scope"),
            ));
            let captures = [
                &selected["local_timing"],
                &selected["p1"]["local_timing"],
                &selected["p2"]["local_timing"],
            ];
            if captures
                .iter()
                .any(|capture| capture["status"] == "not_recorded")
            {
                details.push('\n');
                details.push_str(locale.text("workbench.replay.timing_missing"));
            }
            if captures
                .iter()
                .any(|capture| capture["status"] == "recorded")
            {
                details.push('\n');
                details.push_str(locale.text("workbench.replay.timing_audio_history"));
            }
            if captures.iter().any(|capture| {
                capture["status"] == "recorded"
                    && capture["independent_audio_read_history"]["before_observation"].is_null()
                    && capture["independent_audio_read_history"]["at_or_after_observation"]
                        .is_null()
            }) {
                details.push('\n');
                details.push_str(locale.text("workbench.replay.timing_audio_missing"));
            }
        }
        details
    }

    pub(super) fn hits(&self) -> impl Iterator<Item = &Hit> {
        self.replay.facts().iter().filter_map(|fact| match fact {
            DuoInput::Hit(hit) => Some(hit),
            DuoInput::Watermark { .. } => None,
        })
    }

    pub(super) fn selected_hits(&self) -> Vec<(PlayerId, i64)> {
        let Some(record) = self.record(self.selected) else {
            return Vec::new();
        };
        if let Some(player) = record["player"].as_u64() {
            return record["song_time_frames"]
                .as_i64()
                .or_else(|| record["input_song_time_frames"].as_i64())
                .map(|frame| {
                    (
                        if player == 1 {
                            PlayerId::P1
                        } else {
                            PlayerId::P2
                        },
                        frame,
                    )
                })
                .into_iter()
                .collect();
        }
        [("p1", PlayerId::P1), ("p2", PlayerId::P2)]
            .into_iter()
            .filter_map(|(key, player)| {
                record[key]["input_song_time_frames"]
                    .as_i64()
                    .map(|frame| (player, frame))
            })
            .collect()
    }

    pub(super) fn selected_anchor(&self) -> Option<u64> {
        self.record(self.selected)?["anchor_id"].as_u64()
    }
}

fn frame(record: &Value) -> Option<i64> {
    [
        "song_time_frames",
        "through_frames",
        "anchor_frame",
        "midpoint_frames",
    ]
    .into_iter()
    .find_map(|key| record[key].as_i64())
}

#[cfg(test)]
pub(super) fn fixture() -> ReplayView {
    use cocobeat_replay::ReplayIdentity;
    use cocobeat_schema::{Anchor, SessionEpoch, SongTime};
    let epoch = SessionEpoch(17);
    let mut replay = Replay::new(
        ReplayIdentity {
            content_id: "constructed-workbench-test".into(),
            rules_id: "duo-watermark-v1".into(),
            build_id: "test".into(),
            stage_compiler_version: None,
        },
        epoch,
    )
    .unwrap();
    let hit = |player, seq, frame| {
        DuoInput::Hit(Hit {
            epoch,
            player,
            seq,
            song_time: SongTime::from_frames(frame),
        })
    };
    let watermark = |player, frame| DuoInput::Watermark {
        epoch,
        player,
        through: SongTime::from_frames(frame),
    };
    for fact in [
        hit(PlayerId::P1, u64::MAX, 1_200),
        hit(PlayerId::P2, 0, 1_200),
        watermark(PlayerId::P1, -1),
        hit(PlayerId::P1, 0, 3_000),
        hit(PlayerId::P2, u64::MAX, 3_000),
        watermark(PlayerId::P1, 25_681),
        watermark(PlayerId::P2, 25_681),
    ] {
        replay.record(fact).unwrap();
    }
    let anchors = vec![Anchor {
        id: 7,
        song_time: SongTime::from_frames(1_200),
    }];
    let engine = replay
        .replay(
            &replay.identity().content_id,
            &replay.identity().rules_id,
            anchors.clone(),
            DuoRules::default(),
        )
        .unwrap();
    ReplayView {
        index: ReportIndex::new(&anchors, &replay),
        header: serde_json::json!({"type": "header", "fact_count": replay.facts().len()}),
        summary: replay::summary(anchors.len(), &replay, &engine).unwrap(),
        replay,
        engine,
        timing: None,
        selected: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_details_preserve_selected_facts_pairs_and_cursor() {
        use cocobeat_replay::timing::{CaptureTiming, InputKind, MappingAnchor, TimingClock};

        let mut view = fixture();
        let default_details = view.details(Locale::EnUs);
        let default_records: Vec<_> = (0..view.len())
            .map(|index| view.record(index).unwrap())
            .collect();
        assert_eq!(view.details(Locale::EnUs), default_details);
        let raw = view.replay.encode().unwrap();
        let mut timing = TimingSidecar::new(
            &view.replay,
            &raw,
            48_000,
            vec![1],
            TimingClock {
                max_extrapolation_ns: 250_000_000,
                max_drift_ppm: 1_000,
                history_capacity: 256,
            },
        )
        .unwrap();
        for (index, fact) in view.replay.facts().iter().enumerate() {
            if let DuoInput::Hit(hit) = fact
                && hit.player == PlayerId::P1
            {
                let observed_ns = hit.song_time.frames() as u64 * 1_000_000_000 / 48_000;
                timing.captures.push(CaptureTiming {
                    fact_index: index as u64 + 1,
                    player: 1,
                    seq: hit.seq,
                    song_time_frames: hit.song_time.frames(),
                    observed_ns,
                    consumed_ns: observed_ns + 2_000_000,
                    uncertainty_frames: 2_402,
                    input_kind: InputKind::GamepadMessage,
                    mapping_anchor: MappingAnchor {
                        monotonic_ns: 0,
                        song_time_frames: 0,
                        uncertainty_frames: 2_400,
                    },
                });
            }
        }
        timing.validate(&view.replay, &raw, 48_000).unwrap();
        view.timing = Some(timing);
        assert_eq!(view.select(9), Some(1_200));
        assert_eq!(
            view.selected_hits(),
            vec![(PlayerId::P1, 1_200), (PlayerId::P2, 1_200)]
        );
        let pair = view.record(9).unwrap();
        assert_eq!(
            pair["p1"]["local_timing"]["capture"]["seq"],
            serde_json::json!(u64::MAX)
        );
        assert_eq!(pair["p2"]["local_timing"]["status"], "not_recorded");
        assert!(
            view.details(Locale::EnUs)
                .contains("software_message_consumption_wait_ns")
        );
        assert_eq!(view.select(2), Some(-1));
        assert_eq!(view.record(2).unwrap(), default_records[2]);
        view.timing = None;
        assert_eq!(
            (0..view.len())
                .map(|index| view.record(index).unwrap())
                .collect::<Vec<_>>(),
            default_records
        );
        view.select(0);
        assert_eq!(view.details(Locale::EnUs), default_details);
    }

    #[test]
    fn dense_facts_watermarks_and_paired_events_keep_exact_identity_and_frames() {
        let mut view = fixture();
        assert_eq!(view.len(), 11);
        assert_eq!(view.hits().count(), 4);
        assert_eq!(view.select(0), Some(1_200));
        assert_eq!(view.selected_hits(), vec![(PlayerId::P1, 1_200)]);
        assert_eq!(view.record(0).unwrap()["seq"], serde_json::json!(u64::MAX));
        assert_eq!(view.select(1), Some(1_200));
        assert_eq!(view.selected, 1);
        assert_eq!(view.selected_hits(), vec![(PlayerId::P2, 1_200)]);
        assert_ne!(view.row(0, Locale::EnUs), view.row(1, Locale::EnUs));
        assert_eq!(view.select(2), Some(-1));
        assert!(view.selected_hits().is_empty());
        assert_eq!(view.select(5), Some(25_681));
        assert_eq!(view.select(9), Some(1_200));
        assert_eq!(view.selected_anchor(), Some(7));
        assert_eq!(
            view.selected_hits(),
            vec![(PlayerId::P1, 1_200), (PlayerId::P2, 1_200)]
        );
        assert_eq!(view.record(9).unwrap()["p1"]["input_fact_index"], 1);
        assert_eq!(view.record(9).unwrap()["p2"]["input_fact_index"], 2);
        assert_eq!(view.select(10), Some(3_000));
        assert_eq!(view.selected_anchor(), None);
        assert_eq!(
            view.selected_hits(),
            vec![(PlayerId::P1, 3_000), (PlayerId::P2, 3_000)]
        );
        assert_eq!(
            view.record(10).unwrap()["p2"]["input_seq"],
            serde_json::json!(u64::MAX)
        );
        assert_eq!(view.select(view.len()), None);
        assert_eq!(view.selected, 10);
        assert!(
            view.details(Locale::EnUs)
                .contains("\"pending_anchor_count\": 0")
        );
    }
}
