use crate::anchors::{self, Decision, Evidence, Report};
use cocobeat_media::ValidatedPackage;
use cocobeat_runtime::{Locale, Message};
use cocobeat_schema::MusicAnalysis;
use serde_json::{Value, json};
use std::path::Path;

pub(super) struct CandidateView {
    report: Report,
    analysis: MusicAnalysis,
    header: Value,
    pub(super) selected: usize,
}

impl CandidateView {
    pub(super) fn load(package: &ValidatedPackage, path: &Path) -> Result<Self, String> {
        let report = anchors::load_report(package, path)?;
        let header = json!({
            "report_version": report.report_version,
            "compiler_version": report.compiler_version,
            "source": report.source,
            "policy": report.policy,
            "production_admission": report.production_admission,
            "candidate_count": report.evidence.len(),
            "proposed_anchor_count": report.anchors.len(),
            "source_chart_anchor_count": package.chart.anchors.len(),
            "analysis_diagnostics": package.analysis.diagnostics,
            "analysis_capabilities": package.analysis.capabilities.map(|capabilities| format!("{capabilities:?}")),
            "tempo_region_count": package.analysis.tempo_regions.len(),
            "repetition_count": package.analysis.repetitions.len(),
        });
        Ok(Self {
            report,
            analysis: package.analysis.clone(),
            header,
            selected: 0,
        })
    }

    pub(super) fn len(&self) -> usize {
        self.report.evidence.len()
    }

    pub(super) fn select(&mut self, index: usize) -> Option<i64> {
        let frame = self.report.evidence.get(index)?.frame;
        self.selected = index;
        Some(frame)
    }

    pub(super) fn nearest(&self, frame: i64, tolerance: i64) -> Option<usize> {
        let tolerance = u64::try_from(tolerance).ok()?;
        let after = self.report.evidence.partition_point(|e| e.frame < frame);
        [after.checked_sub(1), Some(after)]
            .into_iter()
            .flatten()
            .filter_map(|index| {
                let evidence = self.report.evidence.get(index)?;
                Some((evidence.frame.abs_diff(frame), index))
            })
            .filter(|(distance, _)| *distance <= tolerance)
            .min()
            .map(|(_, index)| index)
    }

    pub(super) fn points(&self) -> impl Iterator<Item = (i64, bool)> + '_ {
        self.report.evidence.iter().map(|evidence| {
            (
                evidence.frame,
                matches!(
                    evidence.decision,
                    Decision::SelectedByExperimentalPolicy { .. }
                ),
            )
        })
    }

    pub(super) fn selected_points(&self) -> Vec<(i64, bool)> {
        let Some(evidence) = self.report.evidence.get(self.selected) else {
            return Vec::new();
        };
        let mut points = vec![(evidence.frame, false)];
        if let Decision::TooClose {
            blocking_onset_index,
            ..
        } = evidence.decision
        {
            points.push((self.report.evidence[blocking_onset_index].frame, true));
        }
        points
    }

    pub(super) fn row(&self, index: usize, locale: Locale) -> String {
        let Some(evidence) = self.report.evidence.get(index) else {
            return String::new();
        };
        let kind = match evidence.decision {
            Decision::SelectedByExperimentalPolicy { .. } => "selected_by_experimental_policy",
            Decision::UnknownConfidence {} => "unknown_confidence",
            Decision::BelowConfidence {} => "below_confidence",
            Decision::TooClose { .. } => "too_close",
        };
        Message::with(
            "candidates.row",
            [
                ("index", evidence.onset_index.to_string()),
                ("kind", kind.into()),
                ("frame", evidence.frame.to_string()),
            ],
        )
        .render(locale)
    }

    fn record(&self, evidence: &Evidence) -> Value {
        let frame = evidence.frame;
        let beat_after = self
            .analysis
            .beats
            .partition_point(|beat| beat.time.frames() <= frame);
        let beat = |index: usize| {
            self.analysis.beats.get(index).map(|beat| {
                json!({
                    "beat_index": index,
                    "frame": beat.time.frames(),
                    "strength": beat.strength,
                    "confidence": beat.confidence,
                    "downbeat_probability": beat.downbeat_probability,
                })
            })
        };
        let section = self
            .analysis
            .sections
            .partition_point(|section| section.end.frames() <= frame);
        let section = self
            .analysis
            .sections
            .get(section)
            .filter(|section| section.start.frames() <= frame)
            .map(|section| {
                json!({
                    "start_frame": section.start.frames(),
                    "end_frame": section.end.frames(),
                    "label": section.label,
                    "confidence": section.confidence,
                })
            });
        let energy = self
            .analysis
            .energy
            .partition_point(|energy| energy.start.frames() + i64::from(energy.frames) <= frame);
        let energy = self.analysis.energy.get(energy).map(|energy| {
            json!({
                "start_frame": energy.start.frames(),
                "frames": energy.frames,
                "rms": energy.rms,
                "peak": energy.peak,
            })
        });
        let selected_anchor = match evidence.decision {
            Decision::SelectedByExperimentalPolicy { anchor_id } => {
                Some(json!({"id": anchor_id, "frame": frame}))
            }
            _ => None,
        };
        let blocker = match evidence.decision {
            Decision::TooClose {
                blocking_onset_index,
                ..
            } => Some(&self.report.evidence[blocking_onset_index]),
            _ => None,
        };
        json!({
            "candidate": evidence,
            "proposed_anchor": selected_anchor,
            "blocking_candidate": blocker,
            "analysis_context": {
                "beat_at_or_before": beat_after.checked_sub(1).and_then(beat),
                "beat_after": beat(beat_after),
                "section": section,
                "energy": energy,
            },
        })
    }

    pub(super) fn details(&self, locale: Locale) -> String {
        let selected = self.report.evidence.get(self.selected);
        let record = selected.map_or(Value::Null, |evidence| self.record(evidence));
        let relations = json!({
            "proposed_anchor": record["proposed_anchor"],
            "blocking_candidate": record["blocking_candidate"],
        });
        let pretty = |value: &Value| {
            serde_json::to_string_pretty(value).expect("Validated evidence contains valid JSON")
        };
        format!(
            "{}\n\n{}\n\n{}\n\n{}\n\n{}",
            serde_json::to_string_pretty(&selected).expect("Selected evidence contains valid JSON"),
            pretty(&relations),
            pretty(&record["analysis_context"]),
            pretty(&self.header),
            locale.text("candidates.fields"),
        )
    }
}

#[cfg(test)]
pub(super) fn fixture() -> CandidateView {
    let (root, package) = anchors::tests::fixture("candidate-view");
    let path = root.join("proposal.json");
    anchors::propose(&root.join("source"), "0.7", "1000", &path).unwrap();
    let view = CandidateView::load(&package, &path).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearby_candidates_preserve_indices_blockers_and_unknown_scores() {
        let mut view = fixture();
        assert_eq!(view.len(), 6);
        assert_eq!(view.select(0), Some(0));
        let record = view.record(&view.report.evidence[0]);
        assert_eq!(record["candidate"]["confidence"], Value::Null);
        assert_eq!(
            record["candidate"]["strength"].as_f64().unwrap().to_bits(),
            (-0.0_f64).to_bits()
        );
        assert_eq!(record["proposed_anchor"], Value::Null);
        assert_eq!(view.select(2), Some(1000));
        assert_eq!(view.selected_points(), [(1000, false), (1200, true)]);
        let record = view.record(&view.report.evidence[2]);
        assert_eq!(record["candidate"]["onset_index"], 2);
        assert_eq!(record["blocking_candidate"]["onset_index"], 3);
        assert_eq!(record["blocking_candidate"]["decision"]["anchor_id"], 4);
        assert_eq!(record["analysis_context"]["beat_after"], Value::Null);
        assert_eq!(record["analysis_context"]["section"], Value::Null);
        assert_eq!(record["analysis_context"]["energy"]["frames"], 4800);
        assert_eq!(view.nearest(1100, 100), Some(2));
        assert_eq!(view.nearest(1100, 99), None);
        assert_eq!(view.nearest(i64::MIN, i64::MAX), None);
        assert_eq!(view.nearest(i64::MAX, 0), None);
        assert_eq!(view.nearest(1000, -1), None);
        view.analysis.beats = vec![
            cocobeat_schema::BeatFeature {
                time: cocobeat_schema::SongTime::from_frames(300),
                strength: 0.5,
                confidence: None,
                downbeat_probability: None,
            },
            cocobeat_schema::BeatFeature {
                time: cocobeat_schema::SongTime::from_frames(1400),
                strength: 0.9,
                confidence: Some(0.8),
                downbeat_probability: Some(0.7),
            },
        ];
        view.analysis.sections = vec![cocobeat_schema::SectionFeature {
            start: cocobeat_schema::SongTime::from_frames(900),
            end: cocobeat_schema::SongTime::from_frames(1300),
            label: "constructed section evidence".into(),
            confidence: None,
        }];
        let context = view.record(&view.report.evidence[2]);
        assert_eq!(
            context["analysis_context"]["beat_at_or_before"]["frame"],
            300
        );
        assert_eq!(context["analysis_context"]["beat_after"]["frame"], 1400);
        assert_eq!(context["analysis_context"]["section"]["start_frame"], 900);
        assert_eq!(
            context["analysis_context"]["section"]["confidence"],
            Value::Null
        );
        assert_eq!(view.select(3), Some(1200));
        assert_eq!(
            view.record(&view.report.evidence[3])["proposed_anchor"]["id"],
            4
        );
        assert_eq!(view.select(view.len()), None);
        assert_eq!(view.selected, 3);
        assert_eq!(view.header["production_admission"], "not_assessed");
        assert_eq!(view.header["source_chart_anchor_count"], 1);
        assert_eq!(view.header["proposed_anchor_count"], 3);
        assert!(view.details(Locale::EnUs).contains("not calibrated MIR"));
        view.report.evidence.clear();
        assert!(view.selected_points().is_empty());
        assert_eq!(view.nearest(0, 0), None);
        assert_eq!(view.select(0), None);
    }
}
