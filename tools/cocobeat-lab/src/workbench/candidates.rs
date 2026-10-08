use crate::anchors::{self, Decision, Evidence, Report};
use cocobeat_media::{
    NativeBeatEvidence, NativeBeatKind, REPETITION_CANDIDATE_PROFILE, StructureFeatureEvidence,
    ValidatedPackage, read_native_beat_evidence,
};
use cocobeat_runtime::{Locale, Message};
use cocobeat_schema::{AnalysisSource, MusicAnalysis, RepetitionFeature};
use serde_json::{Value, json};
use std::path::Path;

enum CandidateData {
    Onsets(Report),
    Calibrated(Vec<Value>),
    Native(NativeBeatEvidence),
    Structure(StructureFeatureEvidence),
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

    pub(super) fn load_calibrated(
        source: &Path,
        package: &ValidatedPackage,
        proposal: &Path,
        input: &Path,
        calibration_report: &Path,
        choice: &Path,
    ) -> Result<Self, String> {
        const LIMIT: usize = 32 * 1024 * 1024;
        let (report, proposal_hash): (anchors::CalibratedReport, _) =
            anchors::read_document_identified(proposal, LIMIT)?;
        let compilation =
            crate::anchor_calibration::recompile(package, input, calibration_report, choice)?;
        let expected = anchors::make_calibrated_report(
            package,
            &compilation.proposal,
            &compilation.estimates,
            compilation.policy,
            &compilation.context,
        )?;
        if report != expected
            || serde_json::to_vec(&report).map_err(|e| e.to_string())?
                != serde_json::to_vec(&expected).map_err(|e| e.to_string())?
        {
            return Err("Calibrated candidate proposal differs from full original-source/context recompilation".into());
        }
        compilation.require_fresh_sources()?;
        let (_, fresh_hash): (anchors::CalibratedReport, _) =
            anchors::read_document_identified(proposal, LIMIT)?;
        if fresh_hash != proposal_hash {
            return Err("Calibrated candidate proposal raw bytes changed after load".into());
        }
        if cocobeat_media::validate_package(source)? != *package {
            return Err("Calibrated candidate supplied source changed after load".into());
        }
        Self::from_calibrated(package, &report, &proposal_hash.to_string(), source)
    }

    fn from_calibrated(
        package: &ValidatedPackage,
        report: &anchors::CalibratedReport,
        proposal_hash: &str,
        source: &Path,
    ) -> Result<Self, String> {
        let mut header = serde_json::to_value(report).map_err(|e| e.to_string())?;
        let fields = header
            .as_object_mut()
            .ok_or("Calibrated proposal must be an object")?;
        let Some(Value::Array(mut evidence)) = fields.remove("evidence") else {
            return Err("Calibrated proposal lacks evidence rows".into());
        };
        let Some(Value::Array(anchors)) = fields.remove("anchors") else {
            return Err("Calibrated proposal lacks Anchor rows".into());
        };
        let proposed_anchor_count = anchors.len();
        if evidence.len() != package.analysis.onsets.len() {
            return Err("Calibrated display requires all original onset rows".into());
        }
        for (row, onset) in evidence.iter_mut().zip(&package.analysis.onsets) {
            // Original bits remain visible even when an estimate is unknown
            row["original_score_bits"] = json!(onset.strength.to_bits());
        }
        header["proposal_blake3"] = json!(proposal_hash);
        header["source_path"] = json!(std::fs::canonicalize(source).map_err(|e| e.to_string())?);
        header["candidate_count"] = json!(evidence.len());
        header["proposed_anchor_count"] = json!(proposed_anchor_count);
        header["source_chart_anchor_count"] = json!(package.chart.anchors.len());
        header["analysis_diagnostics"] = json!(package.analysis.diagnostics);
        header["analysis_capabilities"] =
            json!(package.analysis.capabilities.map(|c| format!("{c:?}")));
        Ok(Self {
            data: CandidateData::Calibrated(evidence),
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

    pub(super) fn from_structure(
        package: &ValidatedPackage,
        evidence: StructureFeatureEvidence,
    ) -> Self {
        let diagnostics =
            serde_json::from_str::<Value>(&package.analysis.diagnostics).unwrap_or(Value::Null);
        let repetition_capability = package.analysis.capabilities.map(|c| c.repetition);
        let known_origin = package.manifest.analysis_version == REPETITION_CANDIDATE_PROFILE
            && repetition_capability.is_some_and(|c| c.source == AnalysisSource::Algorithm)
            && diagnostics["profile"] == REPETITION_CANDIDATE_PROFILE
            && diagnostics["audio_blake3"]
                == blake3::Hash::from_bytes(package.manifest.audio.blake3)
                    .to_hex()
                    .as_str()
            && diagnostics["canonical_frames"] == package.manifest.canonical_frames;
        let header = json!({
            "source": crate::labels::Source::from_package(package),
            "profile": evidence.profile,
            "sample_rate": evidence.sample_rate,
            "channels": evidence.channels,
            "channel": evidence.channel,
            "inspection_channel": evidence.channel,
            "origin_channel": known_origin.then(|| diagnostics["channel"].as_u64().filter(|c| *c < 2)).flatten(),
            "origin_source_content_id": known_origin.then(|| diagnostics["source_content_id"].as_str()).flatten(),
            "analysis_profile": package.manifest.analysis_version,
            "analysis_diagnostics": package.analysis.diagnostics,
            "repetition_capability": repetition_capability.map(|c| json!({
                "state": format!("{:?}", c.state),
                "source": format!("{:?}", c.source),
                "confidence": c.confidence,
            })),
            "declared_repetition_count": package.analysis.repetitions.len(),
            "canonical_frames": evidence.canonical_frames,
            "fft_frames": evidence.fft_frames,
            "windows_per_bin": evidence.windows_per_bin,
            "bin_frames": evidence.bin_frames,
            "band_edges": evidence.band_edges,
            "window": evidence.window,
            "power_convention": evidence.power_convention,
            "padded_frames": evidence.padded_frames,
            "complete_windows": evidence.complete_windows,
            "partial_tail_frames": evidence.partial_tail_frames,
            "confidence": evidence.confidence,
            "beat_unit": evidence.beat_unit,
            "meter": evidence.meter,
            "quality_status": evidence.quality_status,
            "production_admission": evidence.production_admission,
            "source_chart_anchor_count": package.chart.anchors.len(),
            "declared_section_count": package.analysis.sections.len(),
            "analysis_capabilities": package.analysis.capabilities.map(|c| format!("{c:?}")),
        });
        Self {
            data: CandidateData::Structure(evidence),
            analysis: package.analysis.clone(),
            header,
            selected: 0,
        }
    }

    pub(super) fn is_structure(&self) -> bool {
        matches!(self.data, CandidateData::Structure(_))
    }

    pub(super) fn structure_evidence(&self) -> Option<&StructureFeatureEvidence> {
        match &self.data {
            CandidateData::Structure(evidence) => Some(evidence),
            _ => None,
        }
    }

    fn repetition_row(&self, index: usize) -> Option<(usize, bool, &RepetitionFeature)> {
        let offset = index.checked_sub(self.structure_evidence()?.bins.len())?;
        let relation_index = offset / 2;
        Some((
            relation_index,
            offset % 2 == 1,
            self.analysis.repetitions.get(relation_index)?,
        ))
    }

    pub(super) fn selected_repetition_spans(&self) -> Option<[(i64, i64); 2]> {
        let (_, _, relation) = self.repetition_row(self.selected)?;
        Some([
            (relation.source_start.frames(), relation.source_end.frames()),
            (relation.target_start.frames(), relation.target_end.frames()),
        ])
    }

    pub(super) fn is_native(&self) -> bool {
        matches!(self.data, CandidateData::Native(_))
    }

    pub(super) fn len(&self) -> usize {
        match &self.data {
            CandidateData::Onsets(report) => report.evidence.len(),
            CandidateData::Calibrated(evidence) => evidence.len(),
            CandidateData::Native(evidence) => evidence.records.len(),
            CandidateData::Structure(evidence) => {
                evidence.bins.len() + self.analysis.repetitions.len() * 2
            }
        }
    }

    fn frame(&self, index: usize) -> Option<i64> {
        match &self.data {
            CandidateData::Onsets(report) => report.evidence.get(index).map(|e| e.frame),
            CandidateData::Calibrated(_) => {
                self.analysis.onsets.get(index).map(|e| e.time.frames())
            }
            CandidateData::Native(evidence) => evidence.records.get(index).map(|e| e.frame),
            CandidateData::Structure(evidence) => evidence
                .bins
                .get(index)
                .map(|b| b.start_frame as i64)
                .or_else(|| {
                    self.repetition_row(index).map(|(_, target, relation)| {
                        if target {
                            relation.target_start.frames()
                        } else {
                            relation.source_start.frames()
                        }
                    })
                }),
        }
    }

    pub(super) fn select(&mut self, index: usize) -> Option<i64> {
        let frame = self.frame(index)?;
        self.selected = index;
        Some(frame)
    }

    pub(super) fn nearest(&self, frame: i64, tolerance: i64) -> Option<usize> {
        if let Some(evidence) = self.structure_evidence() {
            let frame = u64::try_from(frame).ok()?;
            let index = evidence.bins.partition_point(|bin| bin.end_frame <= frame);
            return evidence
                .bins
                .get(index)
                .filter(|bin| bin.start_frame <= frame)
                .map(|_| index);
        }
        let tolerance = u64::try_from(tolerance).ok()?;
        let after = match &self.data {
            CandidateData::Onsets(report) => report.evidence.partition_point(|e| e.frame < frame),
            CandidateData::Calibrated(_) => self
                .analysis
                .onsets
                .partition_point(|e| e.time.frames() < frame),
            CandidateData::Native(evidence) => {
                evidence.records.partition_point(|e| e.frame < frame)
            }
            CandidateData::Structure(_) => unreachable!("Structure uses interval containment"),
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
        (0..if self.is_structure() { 0 } else { self.len() }).map(|index| {
            let accepted = match &self.data {
                CandidateData::Onsets(report) => matches!(
                    report.evidence[index].decision,
                    Decision::SelectedByExperimentalPolicy { .. }
                ),
                CandidateData::Calibrated(evidence) => {
                    evidence[index]["decision"]["kind"] == "selected_by_experimental_policy"
                }
                CandidateData::Native(_) => false,
                CandidateData::Structure(_) => unreachable!("Structure has no point events"),
            };
            (
                self.frame(index)
                    .expect("Index is inside validated records"),
                accepted,
            )
        })
    }

    pub(super) fn selected_points(&self) -> Vec<(i64, bool)> {
        if self.is_structure() {
            return Vec::new();
        }
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
            CandidateData::Calibrated(evidence) => {
                if let Some(blocker) = evidence[self.selected]["decision"]["blocking_onset_index"]
                    .as_u64()
                    .and_then(|i| usize::try_from(i).ok())
                    .and_then(|i| self.frame(i))
                {
                    points.push((blocker, true));
                }
            }
            CandidateData::Native(evidence) => {
                if let Some(alignment) = &evidence.records[self.selected].alignment {
                    points.push((alignment.beat_frame, true));
                }
            }
            CandidateData::Structure(_) => unreachable!("Structure has no selected point event"),
        }
        points
    }

    pub(super) fn row(&self, index: usize, locale: Locale) -> String {
        if let Some(evidence) = self.structure_evidence() {
            let (label, start, end) = if let Some(bin) = evidence.bins.get(index) {
                (
                    bin.index.to_string(),
                    bin.start_frame as i64,
                    bin.end_frame as i64,
                )
            } else if let Some((relation_index, target, relation)) = self.repetition_row(index) {
                let side = if target { "target" } else { "source" };
                let (start, end) = if target {
                    (relation.target_start.frames(), relation.target_end.frames())
                } else {
                    (relation.source_start.frames(), relation.source_end.frames())
                };
                (format!("repetition[{relation_index}].{side}"), start, end)
            } else {
                return String::new();
            };
            return Message::with(
                "structure.row",
                [
                    ("index", label),
                    ("start", start.to_string()),
                    ("end", end.to_string()),
                ],
            )
            .render(locale);
        }
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
            CandidateData::Calibrated(evidence) => (
                index,
                evidence[index]["decision"]["kind"]
                    .as_str()
                    .expect("Typed calibrated decisions have a kind"),
            ),
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
            CandidateData::Structure(_) => unreachable!("Structure uses interval rows"),
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

    fn analysis_context(&self, frame: i64) -> Value {
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
        json!({ "beat_at_or_before": beat_after.checked_sub(1).and_then(beat),
            "beat_after": beat(beat_after), "section": section, "energy": energy })
    }

    fn record(&self, report: &Report, evidence: &Evidence) -> Value {
        let frame = evidence.frame;
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
            "analysis_context": self.analysis_context(frame),
        })
    }

    fn calibrated_record(&self, evidence: &[Value], index: usize) -> Value {
        let candidate = evidence.get(index);
        let frame = self.frame(index);
        let decision = candidate.map(|row| &row["decision"]);
        let proposed_anchor = decision
            .filter(|d| d["kind"] == "selected_by_experimental_policy")
            .map(|d| json!({ "id": d["anchor_id"], "frame": frame }));
        let blocker = decision
            .and_then(|d| d["blocking_onset_index"].as_u64())
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| evidence.get(i));
        json!({ "candidate": candidate, "proposed_anchor": proposed_anchor,
            "blocking_candidate": blocker, "analysis_context": frame.map(|f| self.analysis_context(f)), "header": self.header })
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
            CandidateData::Structure(evidence) => {
                if let Some((relation_index, target, relation)) = self.repetition_row(self.selected)
                {
                    return serde_json::to_string_pretty(&json!({
                        "compiled_repetition": {
                            "relation_index": relation_index,
                            "selected_side": if target { "target" } else { "source" },
                            "source_start": relation.source_start.frames(),
                            "source_end": relation.source_end.frames(),
                            "target_start": relation.target_start.frames(),
                            "target_end": relation.target_end.frames(),
                            "confidence": relation.confidence,
                        },
                        "header": self.header,
                    }))
                    .expect("Validated repetitions contain valid JSON");
                }
                let adjacent: Vec<_> = evidence
                    .adjacent
                    .iter()
                    .filter(|pair| {
                        pair.left_index == self.selected || pair.right_index == self.selected
                    })
                    .collect();
                return serde_json::to_string_pretty(&json!({
                    "bin": evidence.bins.get(self.selected),
                    "adjacent": adjacent,
                    "header": self.header,
                }))
                .expect("Measured structure descriptors contain valid JSON");
            }
            CandidateData::Onsets(report) => report,
            CandidateData::Calibrated(evidence) => {
                return serde_json::to_string_pretty(
                    &self.calibrated_record(evidence, self.selected),
                )
                .expect("Fully recompiled calibrated evidence contains valid JSON");
            }
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

// Constructed display-only calibration, not a positive strict SDK/input reader fixture
#[cfg(test)]
fn calibrated_components() -> (
    std::path::PathBuf,
    ValidatedPackage,
    anchors::CalibratedReport,
) {
    let (root, mut package) = anchors::tests::fixture("calibrated-candidate-view");
    for onset in &mut package.analysis.onsets {
        onset.confidence = None;
    }
    let mut transient = package.analysis.clone();
    let probabilities = [None, Some(0.2), Some(0.8), Some(0.9), Some(0.8), Some(1.0)];
    for (onset, probability) in transient.onsets.iter_mut().zip(probabilities) {
        onset.confidence = probability;
    }
    let policy = crate::anchor_calibration::Policy {
        min_confidence: 0.5,
        min_gap_frames: 500,
        density_window_frames: 4800,
        max_anchors_per_window: 2,
    };
    let proposal = cocobeat_media::AnchorPolicy {
        min_confidence: policy.min_confidence,
        min_gap_frames: policy.min_gap_frames,
    }
    .compile_with_density(
        &transient,
        package.manifest.canonical_frames,
        policy.density_window_frames,
        policy.max_anchors_per_window,
    )
    .unwrap();
    let estimates = package
        .analysis
        .onsets
        .iter()
        .zip(probabilities)
        .enumerate()
        .map(
            |(index, (onset, probability))| crate::anchor_calibration::Estimate {
                onset_index: index,
                original_frame: onset.time.frames(),
                original_score_bits: onset.strength.to_bits(),
                probability_bits: probability.map(f32::to_bits),
                bin_index: probability.map(|_| 0),
            },
        )
        .collect::<Vec<_>>();
    let context = crate::anchor_calibration::Context {
        input_blake3: "constructed-input-raw".into(),
        calibration_report_blake3: "constructed-report-raw".into(),
        choice_blake3: "constructed-choice-raw".into(),
        train_result_blake3: "constructed-training-content".into(),
        policy_index: 0,
    };
    let report =
        anchors::make_calibrated_report(&package, &proposal, &estimates, policy, &context).unwrap();
    (root, package, report)
}

#[cfg(test)]
pub(super) fn calibrated_fixture() -> CandidateView {
    let (root, package, report) = calibrated_components();
    let view = CandidateView::from_calibrated(
        &package,
        &report,
        "constructed-proposal-raw",
        &root.join("source"),
    )
    .unwrap();
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

// Constructed display-only intervals, not a validated long audio package or music truth
#[cfg(test)]
pub(super) fn structure_fixture() -> CandidateView {
    let (package, evidence) = structure_components();
    CandidateView::from_structure(&package, evidence)
}

#[cfg(test)]
fn structure_components() -> (ValidatedPackage, StructureFeatureEvidence) {
    use cocobeat_media::{StructureAdjacentChange, StructureFeatureBin, StructureNeighbor};
    let (root, mut package) = anchors::tests::fixture("structure-candidate-view");
    let (_, mut evidence) =
        cocobeat_media::inspect_structure_features_package(root.join("source"), 0).unwrap();
    let bin = |index, start_frame, end_frame, band: Option<usize>| {
        let log = band.map(|band| std::array::from_fn(|i| if i == band { 1.0 } else { 0.0 }));
        StructureFeatureBin {
            index,
            start_frame,
            end_frame,
            rms: if band.is_some() { 1.0 } else { 2.0 },
            peak: if band.is_some() { 1.0 } else { 2.0 },
            spectral_frames: if band.is_some() { 24_576 } else { 0 },
            partial_tail_frames: if band.is_some() { 0 } else { 1 },
            spectrum_status: if band.is_some() {
                "MEASURED"
            } else {
                "INSUFFICIENT_FULL_WINDOW"
            },
            mean_band_power: log.map(|v| v.map(f64::exp_m1)),
            log_band_power: log,
            neighbors: Vec::new(),
        }
    };
    let mut first = bin(0, 0, 24_576, Some(0));
    first.neighbors.push(StructureNeighbor {
        index: 1,
        start_frame: 24_576,
        end_frame: 49_152,
        raw_cosine: 0.0,
    });
    let mut second = bin(1, 24_576, 49_152, Some(1));
    second.neighbors.push(StructureNeighbor {
        index: 0,
        start_frame: 0,
        end_frame: 24_576,
        raw_cosine: 0.0,
    });
    evidence.profile = "constructed-view-only";
    evidence.canonical_frames = 49_153;
    evidence.complete_windows = 48;
    evidence.partial_tail_frames = 1;
    evidence.bins = vec![first, second, bin(2, 49_152, 49_153, None)];
    evidence.adjacent = vec![
        StructureAdjacentChange {
            left_index: 0,
            right_index: 1,
            raw_cosine: Some(0.0),
            descriptor_distance: Some(1.0),
        },
        StructureAdjacentChange {
            left_index: 1,
            right_index: 2,
            raw_cosine: None,
            descriptor_distance: None,
        },
    ];
    package.manifest.canonical_frames = 49_153;
    std::fs::remove_dir_all(root).unwrap();
    (package, evidence)
}

#[cfg(test)]
fn repetition_components() -> (ValidatedPackage, StructureFeatureEvidence) {
    use cocobeat_schema::{
        ANALYSIS_SCHEMA_VERSION, AnalysisCapabilities, AnalysisCapability, AnalysisState, SongTime,
    };
    let (mut package, mut evidence) = structure_components();
    package.analysis.repetitions = [
        (1, 16_385, 16_385, 32_769),
        (1, 16_385, 32_769, 49_153),
        (16_385, 32_769, 32_769, 49_153),
    ]
    .map(|(a, b, c, d)| RepetitionFeature {
        source_start: SongTime::from_frames(a),
        source_end: SongTime::from_frames(b),
        target_start: SongTime::from_frames(c),
        target_end: SongTime::from_frames(d),
        confidence: None,
    })
    .to_vec();
    let mut capabilities = AnalysisCapabilities::authored();
    capabilities.repetition = AnalysisCapability {
        state: AnalysisState::Candidate,
        source: AnalysisSource::Algorithm,
        confidence: None,
    };
    package.analysis.schema_version = ANALYSIS_SCHEMA_VERSION;
    package.analysis.capabilities = Some(capabilities);
    package.manifest.analysis_version = REPETITION_CANDIDATE_PROFILE.into();
    package.analysis.diagnostics = json!({
        "profile": REPETITION_CANDIDATE_PROFILE,
        "audio_blake3": blake3::Hash::from_bytes(package.manifest.audio.blake3).to_hex().as_str(),
        "canonical_frames": package.manifest.canonical_frames,
        "channel": 0,
        "source_content_id": "original-package-before-repetition",
    })
    .to_string();
    evidence.channel = 1;
    (package, evidence)
}

#[cfg(test)]
pub(super) fn repetition_fixture() -> CandidateView {
    let (package, evidence) = repetition_components();
    CandidateView::from_structure(&package, evidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibrated_rows_keep_original_none_bits_estimate_provenance_and_density_decisions() {
        let mut view = calibrated_fixture();
        let original = view.analysis.clone();
        assert_eq!(view.len(), 6);
        assert_eq!(
            view.points()
                .filter(|(_, selected)| *selected)
                .map(|(frame, _)| frame)
                .collect::<Vec<_>>(),
            [1200, 4799]
        );
        for index in 0..view.len() {
            view.select(index).unwrap();
            let detail: Value = serde_json::from_str(&view.details(Locale::EnUs)).unwrap();
            assert!(detail["candidate"]["confidence"].is_null());
            assert_eq!(
                detail["candidate"]["frame"],
                original.onsets[index].time.frames()
            );
            assert_eq!(
                detail["candidate"]["original_score_bits"],
                original.onsets[index].strength.to_bits()
            );
            assert_eq!(detail["header"]["quality_status"], "UNASSESSED");
            assert_eq!(detail["header"]["production_admission"], false);
            assert_eq!(
                detail["header"]["calibration"]["choice_blake3"],
                "constructed-choice-raw"
            );
            assert_eq!(detail["header"]["source_chart_anchor_count"], 1);
        }
        view.select(0).unwrap();
        let detail: Value = serde_json::from_str(&view.details(Locale::EnUs)).unwrap();
        assert_eq!(
            detail["candidate"]["original_score_bits"],
            (-0.0f32).to_bits()
        );
        assert!(detail["candidate"]["calibrated_estimate"].is_null());
        assert_eq!(
            detail["candidate"]["decision"]["kind"],
            "unknown_confidence"
        );
        view.select(2).unwrap();
        assert_eq!(view.selected_points(), [(1000, false), (1200, true)]);
        let detail: Value = serde_json::from_str(&view.details(Locale::EnUs)).unwrap();
        assert_eq!(
            detail["candidate"]["calibrated_estimate"]["probability_bits"],
            0.8f32.to_bits()
        );
        assert_eq!(
            detail["candidate"]["calibrated_estimate"]["method"],
            "fixed_bin_beta11"
        );
        assert_eq!(
            detail["candidate"]["calibrated_estimate"]["calibration_report_blake3"],
            "constructed-report-raw"
        );
        assert_eq!(detail["blocking_candidate"]["onset_index"], 3);
        view.select(4).unwrap();
        let detail: Value = serde_json::from_str(&view.details(Locale::EnUs)).unwrap();
        assert_eq!(detail["candidate"]["decision"]["kind"], "density_limited");
        assert!(detail["proposed_anchor"].is_null());
        assert!(view.row(4, Locale::EnUs).contains("density_limited"));
        assert_eq!(view.nearest(1100, 100), Some(2));
        assert_eq!(view.nearest(1100, 99), None);
        assert_eq!(view.select(6), None);
        assert_eq!(view.analysis, original);
    }

    #[test]
    fn calibrated_consumer_requires_explicit_context_and_legacy_loader_rejects_v2() {
        let (root, package, report) = calibrated_components();
        let path = root.join("proposal.json");
        std::fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
        assert!(CandidateView::load(&package, &path).is_err());
        assert!(
            CandidateView::load_calibrated(
                &root.join("source"),
                &package,
                &path,
                &root.join("missing-input"),
                &root.join("missing-report"),
                &root.join("missing-choice")
            )
            .is_err()
        );
        let mut missing = serde_json::to_value(&report).unwrap();
        missing.as_object_mut().unwrap().remove("calibration");
        std::fs::write(&path, serde_json::to_vec(&missing).unwrap()).unwrap();
        assert!(
            CandidateView::load_calibrated(
                &root.join("source"),
                &package,
                &path,
                &root.join("missing-input"),
                &root.join("missing-report"),
                &root.join("missing-choice")
            )
            .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repetition_rows_preserve_both_sides_and_all_original_endpoints() {
        let mut view = repetition_fixture();
        assert_eq!(view.len(), 9);
        for (row, relation_index, target, spans) in [
            (3, 0, false, [(1, 16_385), (16_385, 32_769)]),
            (4, 0, true, [(1, 16_385), (16_385, 32_769)]),
            (5, 1, false, [(1, 16_385), (32_769, 49_153)]),
            (6, 1, true, [(1, 16_385), (32_769, 49_153)]),
            (7, 2, false, [(16_385, 32_769), (32_769, 49_153)]),
            (8, 2, true, [(16_385, 32_769), (32_769, 49_153)]),
        ] {
            let side = if target { "target" } else { "source" };
            assert_eq!(view.select(row), Some(spans[usize::from(target)].0));
            assert_eq!(view.selected_repetition_spans(), Some(spans));
            assert!(
                view.row(row, Locale::EnUs)
                    .contains(&format!("repetition[{relation_index}].{side}"))
            );
            let detail: Value = serde_json::from_str(&view.details(Locale::EnUs)).unwrap();
            let record = &detail["compiled_repetition"];
            assert_eq!(record["relation_index"], relation_index);
            assert_eq!(record["selected_side"], side);
            assert_eq!(record["source_start"], spans[0].0);
            assert_eq!(record["source_end"], spans[0].1);
            assert_eq!(record["target_start"], spans[1].0);
            assert_eq!(record["target_end"], spans[1].1);
            assert!(record["confidence"].is_null());
            assert!(detail.get("bin").is_none());
            assert!(detail.get("proposed_anchor").is_none());
        }
        assert_eq!(view.select(9), None);
        assert!(view.row(9, Locale::EnUs).is_empty());
        assert_eq!(view.nearest(32_769, 0), Some(1));
        assert_eq!(view.nearest(49_153, 0), None);
        assert!(view.points().next().is_none());
        assert!(view.selected_points().is_empty());
        view.select(0).unwrap();
        assert!(view.selected_repetition_spans().is_none());
        let detail: Value = serde_json::from_str(&view.details(Locale::EnUs)).unwrap();
        assert_eq!(detail["bin"]["neighbors"][0]["raw_cosine"], 0.0);
    }

    #[test]
    fn repetition_origin_is_separate_from_inspection_and_unknown_without_matching_identity() {
        let (package, evidence) = repetition_components();
        let header = |package: &ValidatedPackage| {
            let (_, mut evidence) = structure_components();
            evidence.channel = 1;
            CandidateView::from_structure(package, evidence).header
        };
        let known = header(&package);
        assert_eq!(known["inspection_channel"], 1);
        assert_eq!(known["channel"], 1);
        assert_eq!(known["origin_channel"], 0);
        assert_eq!(
            known["origin_source_content_id"],
            "original-package-before-repetition"
        );
        assert_eq!(known["repetition_capability"]["state"], "Candidate");
        assert_eq!(known["repetition_capability"]["source"], "Algorithm");
        assert!(known["repetition_capability"]["confidence"].is_null());
        assert_eq!(known["analysis_diagnostics"], package.analysis.diagnostics);
        let mut unknown_profile = package.clone();
        unknown_profile.manifest.analysis_version = "unknown-profile".into();
        assert!(header(&unknown_profile)["origin_channel"].is_null());
        let mut malformed = package.clone();
        malformed.analysis.diagnostics = "not JSON".into();
        assert!(header(&malformed)["origin_channel"].is_null());
        for field in ["profile", "audio_blake3", "canonical_frames", "channel"] {
            let mut changed = package.clone();
            let mut diagnostics: Value =
                serde_json::from_str(&changed.analysis.diagnostics).unwrap();
            diagnostics[field] = Value::Null;
            changed.analysis.diagnostics = diagnostics.to_string();
            assert!(header(&changed)["origin_channel"].is_null(), "{field}");
        }
        let mut authored = package.clone();
        authored
            .analysis
            .capabilities
            .as_mut()
            .unwrap()
            .repetition
            .source = AnalysisSource::Authored;
        assert_eq!(
            header(&authored)["repetition_capability"]["source"],
            "Authored"
        );
        assert!(header(&authored)["origin_channel"].is_null());
        let mut legacy = package.clone();
        legacy.analysis.capabilities = None;
        legacy.analysis.schema_version = 1;
        legacy.manifest.analysis_version = "legacy-author-profile".into();
        assert!(header(&legacy)["repetition_capability"].is_null());
        assert!(header(&legacy)["origin_channel"].is_null());
        let view = CandidateView::from_structure(&legacy, evidence);
        assert_eq!(view.len(), 9);
        assert!(view.header["confidence"].is_null());
        let empty = structure_fixture();
        assert_eq!(empty.len(), 3);
        assert_eq!(empty.header["declared_repetition_count"], 0);
        assert!(empty.header["origin_channel"].is_null());
    }

    #[test]
    fn structure_intervals_keep_half_open_edges_and_do_not_become_point_events() {
        let mut view = structure_fixture();
        assert!(view.is_structure());
        assert!(!view.is_native());
        assert_eq!(view.len(), 3);
        for (frame, index) in [(0, 0), (24_575, 0), (24_576, 1), (49_151, 1), (49_152, 2)] {
            assert_eq!(view.nearest(frame, 0), Some(index));
        }
        for frame in [-1, 49_153, i64::MAX] {
            assert_eq!(view.nearest(frame, i64::MAX), None);
        }
        assert_eq!(view.select(2), Some(49_152));
        assert_eq!(view.select(3), None);
        assert_eq!(view.selected, 2);
        assert!(view.points().next().is_none());
        assert!(view.selected_points().is_empty());
        let tail: Value = serde_json::from_str(&view.details(Locale::EnUs)).unwrap();
        assert_eq!(tail["bin"]["start_frame"], 49_152);
        assert_eq!(tail["bin"]["end_frame"], 49_153);
        assert_eq!(tail["bin"]["spectral_frames"], 0);
        assert_eq!(tail["bin"]["partial_tail_frames"], 1);
        assert!(tail["bin"]["mean_band_power"].is_null());
        assert!(tail["bin"]["log_band_power"].is_null());
        assert_eq!(tail["bin"]["neighbors"], json!([]));
        assert_eq!(tail["bin"]["spectrum_status"], "INSUFFICIENT_FULL_WINDOW");
        assert_eq!(tail["adjacent"].as_array().unwrap().len(), 1);
        assert!(tail["adjacent"][0]["raw_cosine"].is_null());
        assert!(tail["header"]["confidence"].is_null());
        assert!(tail["header"]["meter"].is_null());
        assert!(tail["header"]["beat_unit"].is_null());
        assert!(tail["header"]["analysis_capabilities"].is_null());
        assert_eq!(tail["header"]["quality_status"], "UNASSESSED");
        assert_eq!(tail["header"]["production_admission"], false);
        view.select(0).unwrap();
        let measured: Value = serde_json::from_str(&view.details(Locale::EnUs)).unwrap();
        assert_eq!(measured["bin"]["neighbors"][0]["index"], 1);
        assert_eq!(measured["bin"]["neighbors"][0]["start_frame"], 24_576);
        assert_eq!(measured["bin"]["neighbors"][0]["end_frame"], 49_152);
        assert_eq!(measured["bin"]["neighbors"][0]["raw_cosine"], 0.0);
        assert_eq!(measured["adjacent"][0]["descriptor_distance"], 1.0);
        assert!(view.row(0, Locale::EnUs).contains("24576"));
        assert!(view.row(3, Locale::EnUs).is_empty());
    }

    #[test]
    fn nearby_candidates_preserve_indices_blockers_and_unknown_scores() {
        let mut view = fixture();
        assert_eq!(view.len(), 6);
        assert!(!view.is_structure());
        assert!(view.structure_evidence().is_none());
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
        assert!(!view.is_structure());
        assert!(view.structure_evidence().is_none());
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
