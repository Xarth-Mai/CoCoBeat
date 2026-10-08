use crate::anchors::{self, Decision, Evidence, Report};
use cocobeat_media::{
    NativeBeatEvidence, NativeBeatKind, ValidatedPackage, read_native_beat_evidence,
};
use cocobeat_runtime::{Locale, Message};
use cocobeat_schema::MusicAnalysis;
use serde_json::{Value, json};
use std::path::Path;

enum CandidateData {
    Onsets(Report),
    Native(NativeBeatEvidence),
}

pub(super) struct CandidateView {
    data: CandidateData,
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
            data: CandidateData::Onsets(report),
            analysis: package.analysis.clone(),
            header,
            selected: 0,
        })
    }

    pub(super) fn load_native(package: &ValidatedPackage, path: &Path) -> Result<Self, String> {
        Ok(Self::from_native(
            package,
            read_native_beat_evidence(package, path)?,
        ))
    }

    fn from_native(package: &ValidatedPackage, evidence: NativeBeatEvidence) -> Self {
        let hex = |hash: [u8; 32]| blake3::Hash::from(hash).to_hex().to_string();
        let header = json!({
            "source": {
                "content_id": format!("package-blake3:{}", hex(package.manifest.package_hash)),
                "chart_blake3": hex(package.manifest.chart.blake3),
                "analysis_blake3": hex(package.manifest.analysis.blake3),
            },
            "native_evidence": evidence.metadata,
            "candidate_count": evidence.records.len(),
            "confidence": null,
            "production_admission": false,
            "source_chart_anchor_count": package.chart.anchors.len(),
            "analysis_diagnostics": package.analysis.diagnostics,
        });
        Self {
            data: CandidateData::Native(evidence),
            analysis: package.analysis.clone(),
            header,
            selected: 0,
        }
    }

    pub(super) fn is_native(&self) -> bool {
        matches!(self.data, CandidateData::Native(_))
    }

    pub(super) fn len(&self) -> usize {
        match &self.data {
            CandidateData::Onsets(report) => report.evidence.len(),
            CandidateData::Native(evidence) => evidence.records.len(),
        }
    }

    fn frame(&self, index: usize) -> Option<i64> {
        match &self.data {
            CandidateData::Onsets(report) => report.evidence.get(index).map(|e| e.frame),
            CandidateData::Native(evidence) => evidence.records.get(index).map(|e| e.frame),
        }
    }

    pub(super) fn select(&mut self, index: usize) -> Option<i64> {
        let frame = self.frame(index)?;
        self.selected = index;
        Some(frame)
    }

    pub(super) fn nearest(&self, frame: i64, tolerance: i64) -> Option<usize> {
        let tolerance = u64::try_from(tolerance).ok()?;
        let after = match &self.data {
            CandidateData::Onsets(report) => report.evidence.partition_point(|e| e.frame < frame),
            CandidateData::Native(evidence) => {
                evidence.records.partition_point(|e| e.frame < frame)
            }
        };
        [after.checked_sub(1), Some(after)]
            .into_iter()
            .flatten()
            .filter_map(|index| Some((self.frame(index)?.abs_diff(frame), index)))
            .filter(|(distance, _)| *distance <= tolerance)
            .min()
            .map(|(_, index)| index)
    }

    pub(super) fn points(&self) -> impl Iterator<Item = (i64, bool)> + '_ {
        (0..self.len()).map(|index| {
            let accepted = match &self.data {
                CandidateData::Onsets(report) => matches!(
                    report.evidence[index].decision,
                    Decision::SelectedByExperimentalPolicy { .. }
                ),
                CandidateData::Native(_) => false,
            };
            (
                self.frame(index)
                    .expect("Index is inside validated records"),
                accepted,
            )
        })
    }

    pub(super) fn selected_points(&self) -> Vec<(i64, bool)> {
        let Some(frame) = self.frame(self.selected) else {
            return Vec::new();
        };
        let mut points = vec![(frame, false)];
        match &self.data {
            CandidateData::Onsets(report) => {
                if let Decision::TooClose {
                    blocking_onset_index,
                    ..
                } = report.evidence[self.selected].decision
                {
                    points.push((report.evidence[blocking_onset_index].frame, true));
                }
            }
            CandidateData::Native(evidence) => {
                if let Some(alignment) = &evidence.records[self.selected].alignment {
                    points.push((alignment.beat_frame, true));
                }
            }
        }
        points
    }

    pub(super) fn row(&self, index: usize, locale: Locale) -> String {
        let Some(frame) = self.frame(index) else {
            return String::new();
        };
        let (original_index, kind) = match &self.data {
            CandidateData::Onsets(report) => {
                let evidence = &report.evidence[index];
                (
                    evidence.onset_index,
                    match evidence.decision {
                        Decision::SelectedByExperimentalPolicy { .. } => {
                            "selected_by_experimental_policy"
                        }
                        Decision::UnknownConfidence {} => "unknown_confidence",
                        Decision::BelowConfidence {} => "below_confidence",
                        Decision::TooClose { .. } => "too_close",
                    },
                )
            }
            CandidateData::Native(evidence) => {
                let record = &evidence.records[index];
                (
                    record.group_index,
                    match record.kind {
                        NativeBeatKind::Beat => "beat",
                        NativeBeatKind::RawDownbeat => "raw_downbeat",
                    },
                )
            }
        };
        Message::with(
            "candidates.row",
            [
                ("index", original_index.to_string()),
                ("kind", kind.into()),
                ("frame", frame.to_string()),
            ],
        )
        .render(locale)
    }

    fn record(&self, report: &Report, evidence: &Evidence) -> Value {
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
            } => Some(&report.evidence[blocking_onset_index]),
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

    #[cfg(test)]
    fn test_record(&self, index: usize) -> Value {
        let CandidateData::Onsets(report) = &self.data else {
            unreachable!()
        };
        self.record(report, &report.evidence[index])
    }

    pub(super) fn details(&self, locale: Locale) -> String {
        let report = match &self.data {
            CandidateData::Onsets(report) => report,
            CandidateData::Native(evidence) => {
                return serde_json::to_string_pretty(&json!({
                    "candidate": evidence.records.get(self.selected),
                    "confidence": null,
                    "header": self.header,
                }))
                .expect("Validated native evidence contains valid JSON");
            }
        };
        let selected = report.evidence.get(self.selected);
        let record = selected.map_or(Value::Null, |evidence| self.record(report, evidence));
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

// Constructed display-only DTO; source/resource validation is covered by the media reader tests.
#[cfg(test)]
pub(super) fn native_fixture() -> CandidateView {
    use cocobeat_media::{
        NativeBeatMetadata, NativeBeatRecord, NativeDownbeatAlignment, NativePeakMember,
    };
    let (root, package) = anchors::tests::fixture("native-candidate-view");
    let record = |kind, group_index, original_q, frame, alignment| NativeBeatRecord {
        kind,
        group_index,
        original_q,
        frame,
        uncalibrated_score: 0.8,
        members: vec![NativePeakMember {
            q: 0,
            raw_logit: 1.0,
        }],
        alignment,
        package_downbeat_score: Some(0.8),
    };
    let view = CandidateView::from_native(
        &package,
        NativeBeatEvidence {
            metadata: NativeBeatMetadata {
                profile: "constructed-view-only".into(),
                channel: 0,
                canonical_frames: 4800,
                resampled_frames: 2205,
                spectrogram_frames: 6,
                audio_blake3: "constructed-view-only".into(),
                model_blake3: "constructed-view-only".into(),
                summary_blake3: "constructed-view-only".into(),
            },
            records: vec![
                record(NativeBeatKind::Beat, 0, 0.0, 0, None),
                record(
                    NativeBeatKind::RawDownbeat,
                    0,
                    0.5,
                    480,
                    Some(NativeDownbeatAlignment {
                        beat_group_index: 0,
                        beat_q: 0.0,
                        beat_frame: 0,
                    }),
                ),
                record(NativeBeatKind::Beat, 1, 1.0, 960, None),
                record(
                    NativeBeatKind::RawDownbeat,
                    1,
                    1.0,
                    960,
                    Some(NativeDownbeatAlignment {
                        beat_group_index: 1,
                        beat_q: 1.0,
                        beat_frame: 960,
                    }),
                ),
            ],
        },
    );
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
        let record = view.test_record(0);
        assert_eq!(record["candidate"]["confidence"], Value::Null);
        assert_eq!(
            record["candidate"]["strength"].as_f64().unwrap().to_bits(),
            (-0.0_f64).to_bits()
        );
        assert_eq!(record["proposed_anchor"], Value::Null);
        assert_eq!(view.select(2), Some(1000));
        assert_eq!(view.selected_points(), [(1000, false), (1200, true)]);
        let record = view.test_record(2);
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
        let context = view.test_record(2);
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
        assert_eq!(view.test_record(3)["proposed_anchor"]["id"], 4);
        assert_eq!(view.select(view.len()), None);
        assert_eq!(view.selected, 3);
        assert_eq!(view.header["production_admission"], "not_assessed");
        assert_eq!(view.header["source_chart_anchor_count"], 1);
        assert_eq!(view.header["proposed_anchor_count"], 3);
        assert!(view.details(Locale::EnUs).contains("not calibrated MIR"));
        let CandidateData::Onsets(report) = &mut view.data else {
            unreachable!()
        };
        report.evidence.clear();
        assert!(view.selected_points().is_empty());
        assert_eq!(view.nearest(0, 0), None);
        assert_eq!(view.select(0), None);
    }
    #[test]
    fn native_navigation_keeps_raw_downbeat_position_and_unknown_confidence() {
        let mut view = native_fixture();
        assert!(view.is_native());
        assert_eq!(view.len(), 4);
        assert_eq!(
            view.points().collect::<Vec<_>>(),
            [(0, false), (480, false), (960, false), (960, false)]
        );
        assert_eq!(view.nearest(960, 0), Some(2));
        assert_eq!(view.select(1), Some(480));
        assert_eq!(view.selected_points(), [(480, false), (0, true)]);
        let detail: Value = serde_json::from_str(&view.details(Locale::EnUs)).unwrap();
        assert_eq!(detail["candidate"]["original_q"], 0.5);
        assert_eq!(detail["candidate"]["frame"], 480);
        assert_eq!(detail["candidate"]["alignment"]["beat_frame"], 0);
        assert_eq!(detail["confidence"], Value::Null);
        assert_eq!(detail["header"]["production_admission"], false);
        assert!(detail["candidate"].get("proposed_anchor").is_none());
        assert!(view.row(1, Locale::EnUs).contains("raw_downbeat"));
        assert_eq!(view.select(3), Some(960));
        assert_eq!(view.selected, 3);
        assert_eq!(view.select(4), None);
        assert_eq!(view.selected, 3);
        assert_eq!(view.nearest(i64::MIN, i64::MAX), None);
        assert_eq!(view.nearest(i64::MAX, 0), None);
        assert_eq!(view.nearest(0, -1), None);
        let CandidateData::Native(evidence) = &mut view.data else {
            unreachable!()
        };
        evidence.records.clear();
        assert!(view.points().next().is_none());
        assert!(view.selected_points().is_empty());
        assert_eq!(view.select(0), None);
        assert_eq!(view.nearest(0, 0), None);
    }
}
