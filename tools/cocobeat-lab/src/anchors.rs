use cocobeat_media::{
    ANCHOR_COMPILER_VERSION, AnchorDecision, AnchorPolicy, ValidatedPackage,
    compile_anchor_proposal,
};
use cocobeat_schema::{Anchor, MAX_CONTENT_ITEMS, SongTime};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const REPORT_VERSION: u32 = 1;
const MAX_REPORT_BYTES: usize = 32 * 1024 * 1024;
const MAX_SELECTION_BYTES: usize = 1024 * 1024;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    pub(crate) report_version: u32,
    pub(crate) compiler_version: u32,
    pub(crate) source: Source,
    pub(crate) policy: Policy,
    pub(crate) production_admission: String,
    pub(crate) anchors: Vec<ProposedAnchor>,
    pub(crate) evidence: Vec<Evidence>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    content_id: String,
    analysis_blake3: String,
    audio_blake3: String,
    chart_blake3: String,
    canonical_frames: u64,
    analysis_version: String,
    ruleset_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    min_confidence: f32,
    min_gap_frames: i64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProposedAnchor {
    id: u64,
    frame: i64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Evidence {
    pub(crate) onset_index: usize,
    pub(crate) frame: i64,
    strength: f32,
    confidence: Option<f32>,
    pub(crate) decision: Decision,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Decision {
    SelectedByExperimentalPolicy {
        anchor_id: u64,
    },
    UnknownConfidence {},
    BelowConfidence {},
    TooClose {
        blocking_onset_index: usize,
        distance_frames: i64,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    schema_version: u32,
    source_content_id: String,
    onset_indices: Vec<usize>,
}

pub fn propose(
    source: &Path,
    confidence: &str,
    gap: &str,
    destination: &Path,
) -> Result<(), String> {
    let policy = Policy {
        min_confidence: confidence
            .parse()
            .map_err(|_| "Minimum confidence must be a finite f32 probability".to_string())?,
        min_gap_frames: gap
            .parse()
            .map_err(|_| "Minimum gap must be an integer number of frames".to_string())?,
    };
    let package = cocobeat_media::validate_package(source)?;
    let report = make_report(&package, policy)?;
    let bytes = serde_json::to_vec(&report).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_REPORT_BYTES {
        return Err("Anchor proposal exceeds 32 MiB".into());
    }
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if fs::canonicalize(parent)
        .map_err(|error| format!("Cannot resolve report destination parent: {error}"))?
        .starts_with(fs::canonicalize(source).map_err(|error| error.to_string())?)
    {
        return Err("Anchor proposal must be written outside the source package".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("Cannot create Anchor proposal: {error}"))?;
    let result = file.write_all(&bytes).and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = result {
        return Err(match fs::remove_file(destination) {
            Ok(()) => format!("Cannot write Anchor proposal: {error}"),
            Err(cleanup) => {
                format!("Cannot write Anchor proposal: {error}; cleanup failed: {cleanup}")
            }
        });
    }
    println!(
        "{}",
        serde_json::json!({
            "source_content_id": report.source.content_id,
            "compiler_version": report.compiler_version,
            "anchor_count": report.anchors.len(),
            "candidate_count": report.evidence.len(),
            "production_admission": report.production_admission,
        })
    );
    Ok(())
}

pub fn adopt(
    source: &Path,
    report_path: &Path,
    selection_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    let selection: Selection = read_document(selection_path, MAX_SELECTION_BYTES)?;
    if selection.schema_version != 1 {
        return Err("Unsupported Anchor selection schema".into());
    }
    if selection.onset_indices.len() > MAX_CONTENT_ITEMS {
        return Err("Anchor selection exceeds the content item limit".into());
    }
    let package = cocobeat_media::validate_package(source)?;
    let report = load_report(&package, report_path)?;
    if selection.source_content_id != report.source.content_id {
        return Err("Anchor selection source identity does not match the package".into());
    }
    let mut chosen_ids = BTreeSet::new();
    for index in selection.onset_indices {
        let evidence = report
            .evidence
            .get(index)
            .ok_or_else(|| format!("Selected onset index is out of range: {index}"))?;
        let Decision::SelectedByExperimentalPolicy { anchor_id } = evidence.decision else {
            return Err(format!(
                "Onset {index} was rejected by the experimental policy"
            ));
        };
        if !chosen_ids.insert(anchor_id) {
            return Err(format!("Duplicate selected onset index: {index}"));
        }
    }
    let anchors: Vec<_> = report
        .anchors
        .iter()
        .filter(|anchor| chosen_ids.contains(&anchor.id))
        .map(|anchor| Anchor {
            id: anchor.id,
            song_time: SongTime::from_frames(anchor.frame),
        })
        .collect();
    let exported = cocobeat_media::export_anchors(
        source,
        package.manifest.package_hash,
        &anchors,
        destination,
    )?;
    println!(
        "{}",
        serde_json::json!({
            "source_content_id": report.source.content_id,
            "content_id": format!("package-blake3:{}", hex(exported.manifest.package_hash)),
            "anchor_count": exported.chart.anchors.len(),
            "section_cue_count": exported.chart.sections.len(),
            "changed": exported.manifest.package_hash != package.manifest.package_hash,
            "production_admission": "not_assessed",
        })
    );
    Ok(())
}

// v2 is a separate DTO so legacy reports and their workbench reader remain unchanged
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CalibratedReport {
    report_version: u32,
    compiler_version: u32,
    source: Source,
    policy: crate::anchor_calibration::Policy,
    scope: String,
    quality_status: String,
    production_admission: bool,
    calibration: crate::anchor_calibration::Context,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inference_source: Option<crate::anchor_calibration::Binding>,
    anchors: Vec<ProposedAnchor>,
    evidence: Vec<CalibratedEvidence>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalibratedEvidence {
    onset_index: usize,
    frame: i64,
    strength: f32,
    confidence: Option<f32>,
    calibrated_estimate: Option<EstimatedEvidence>,
    decision: CalibratedDecision,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EstimatedEvidence {
    probability: f32,
    probability_bits: u32,
    bin_index: usize,
    original_score_bits: u32,
    method: String,
    calibration_report_blake3: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum CalibratedDecision {
    Legacy(Decision),
    Density(DensityDecision),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum DensityDecision {
    DensityLimited {
        window_start: i64,
        window_end: i64,
        max_anchors: usize,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CalibratedSelection {
    pub(crate) schema_version: u32,
    pub(crate) source_content_id: String,
    pub(crate) proposal_blake3: String,
    pub(crate) calibration: crate::anchor_calibration::Context,
    pub(crate) onset_indices: Vec<usize>,
}

pub(crate) fn make_calibrated_report(
    package: &ValidatedPackage,
    proposal: &cocobeat_media::AnchorProposal,
    estimates: &[crate::anchor_calibration::Estimate],
    policy: crate::anchor_calibration::Policy,
    calibration: &crate::anchor_calibration::Context,
) -> Result<CalibratedReport, String> {
    if package.analysis.onsets.len() != estimates.len()
        || estimates.len() != proposal.evidence.len()
    {
        return Err("Calibrated evidence must preserve all original onset rows".into());
    }
    let evidence = package
        .analysis
        .onsets
        .iter()
        .zip(estimates)
        .zip(&proposal.evidence)
        .enumerate()
        .map(|(index, ((original, estimate), compiled))| {
            if original.confidence.is_some()
                || estimate.onset_index != index
                || compiled.onset_index != index
                || estimate.original_frame != original.time.frames()
                || compiled.time != original.time
                || estimate.original_score_bits != original.strength.to_bits()
                || compiled.strength.to_bits() != original.strength.to_bits()
                || compiled.confidence.map(f32::to_bits) != estimate.probability_bits
            {
                return Err(
                    "Calibrated compilation differs from original None/frame/scorebits".to_string(),
                );
            }
            let calibrated_estimate = match estimate.probability_bits {
                None => None,
                Some(bits) => {
                    let probability = f32::from_bits(bits);
                    if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
                        return Err("Invalid calibrated probability bits".to_string());
                    }
                    Some(EstimatedEvidence {
                        probability,
                        probability_bits: bits,
                        bin_index: estimate
                            .bin_index
                            .ok_or("Known estimate requires its training bin")?,
                        original_score_bits: estimate.original_score_bits,
                        method: "fixed_bin_beta11".into(),
                        calibration_report_blake3: calibration.calibration_report_blake3.clone(),
                    })
                }
            };
            let decision = match compiled.decision {
                AnchorDecision::Selected { anchor_id } => {
                    CalibratedDecision::Legacy(Decision::SelectedByExperimentalPolicy { anchor_id })
                }
                AnchorDecision::UnknownConfidence => {
                    CalibratedDecision::Legacy(Decision::UnknownConfidence {})
                }
                AnchorDecision::BelowConfidence => {
                    CalibratedDecision::Legacy(Decision::BelowConfidence {})
                }
                AnchorDecision::TooClose {
                    blocking_onset_index,
                    distance_frames,
                } => CalibratedDecision::Legacy(Decision::TooClose {
                    blocking_onset_index,
                    distance_frames,
                }),
                AnchorDecision::DensityLimited {
                    window_start,
                    window_end,
                    max_anchors,
                } => CalibratedDecision::Density(DensityDecision::DensityLimited {
                    window_start,
                    window_end,
                    max_anchors,
                }),
            };
            Ok(CalibratedEvidence {
                onset_index: index,
                frame: original.time.frames(),
                strength: original.strength,
                confidence: original.confidence,
                calibrated_estimate,
                decision,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(CalibratedReport {
        report_version: 2,
        compiler_version: 2,
        source: source_identity(package),
        policy,
        scope: "experimental_anchor_calibration".into(),
        quality_status: "UNASSESSED".into(),
        production_admission: false,
        calibration: calibration.clone(),
        inference_source: None,
        anchors: proposal
            .anchors
            .iter()
            .map(|anchor| ProposedAnchor {
                id: anchor.id,
                frame: anchor.song_time.frames(),
            })
            .collect(),
        evidence,
    })
}

pub(crate) fn select_calibrated(
    report: &CalibratedReport,
    proposal_hash: &str,
    selection: &CalibratedSelection,
) -> Result<Vec<Anchor>, String> {
    if selection.schema_version != 2
        || selection.source_content_id != report.source.content_id
        || selection.proposal_blake3 != proposal_hash
        || selection.calibration != report.calibration
        || selection.onset_indices.len() > MAX_CONTENT_ITEMS
    {
        return Err(
            "Calibrated selection must bind the source, exact proposal bytes and complete context"
                .into(),
        );
    }
    let mut chosen = BTreeSet::new();
    for &index in &selection.onset_indices {
        let row = report
            .evidence
            .get(index)
            .ok_or_else(|| format!("Selected onset index is out of range: {index}"))?;
        let CalibratedDecision::Legacy(Decision::SelectedByExperimentalPolicy { anchor_id }) =
            &row.decision
        else {
            return Err(format!(
                "Onset {index} was rejected by the calibrated experimental policy"
            ));
        };
        if !chosen.insert(*anchor_id) {
            return Err(format!("Duplicate selected onset index: {index}"));
        }
    }
    Ok(report
        .anchors
        .iter()
        .filter(|v| chosen.contains(&v.id))
        .map(|v| Anchor {
            id: v.id,
            song_time: SongTime::from_frames(v.frame),
        })
        .collect())
}

pub(crate) fn propose_calibrated(
    source: &Path,
    input: &Path,
    calibration_report: &Path,
    choice: &Path,
    destination: &Path,
) -> Result<(), String> {
    let package = cocobeat_media::validate_package(source)?;
    let compilation =
        crate::anchor_calibration::recompile(&package, input, calibration_report, choice)?;
    propose_compiled(source, &package, &compilation, destination)
}

pub(crate) fn propose_inferred(
    source: &Path,
    evidence: &Path,
    input: &Path,
    calibration_report: &Path,
    choice: &Path,
    destination: &Path,
) -> Result<(), String> {
    let package = cocobeat_media::validate_package(source)?;
    let compilation = crate::anchor_calibration::recompile_inference(
        &package,
        evidence,
        input,
        calibration_report,
        choice,
    )?;
    propose_compiled(source, &package, &compilation, destination)
}

fn compiled_report(
    package: &ValidatedPackage,
    compilation: &crate::anchor_calibration::Compilation,
) -> Result<CalibratedReport, String> {
    let mut report = make_calibrated_report(
        package,
        &compilation.proposal,
        &compilation.estimates,
        compilation.policy,
        &compilation.context,
    )?;
    if let Some(binding) = &compilation.inference_source {
        report.scope = "experimental_anchor_calibration_inference".into();
        report.inference_source = Some(binding.clone());
    }
    Ok(report)
}

fn propose_compiled(
    source: &Path,
    package: &ValidatedPackage,
    compilation: &crate::anchor_calibration::Compilation,
    destination: &Path,
) -> Result<(), String> {
    let report = compiled_report(package, compilation)?;
    let destination = compilation.output_path(source, destination)?;
    compilation.require_fresh_sources()?;
    if cocobeat_media::validate_package(source)? != *package {
        return Err("Calibration supplied source changed after load".into());
    }
    let hash = crate::labels::write_new(&destination, &report, MAX_REPORT_BYTES)?;
    println!(
        "{}",
        serde_json::json!({
            "source_content_id": report.source.content_id, "compiler_version": report.compiler_version,
            "proposal_blake3": hash.to_string(), "calibration": report.calibration,
            "anchor_count": report.anchors.len(), "candidate_count": report.evidence.len(),
            "scope": report.scope, "quality_status": report.quality_status, "production_admission": false,
        })
    );
    Ok(())
}

pub(crate) fn adopt_calibrated(
    source: &Path,
    report_path: &Path,
    selection_path: &Path,
    destination: &Path,
    input: &Path,
    calibration_report: &Path,
    choice: &Path,
) -> Result<(), String> {
    adopt_with_context(
        source,
        report_path,
        selection_path,
        destination,
        [input, calibration_report, choice],
        None,
    )
}

pub(crate) fn adopt_inferred(
    source: &Path,
    evidence: &Path,
    report_path: &Path,
    selection_path: &Path,
    destination: &Path,
    context: [&Path; 3],
) -> Result<(), String> {
    adopt_with_context(
        source,
        report_path,
        selection_path,
        destination,
        context,
        Some(evidence),
    )
}

fn adopt_with_context(
    source: &Path,
    report_path: &Path,
    selection_path: &Path,
    destination: &Path,
    [input, calibration_report, choice]: [&Path; 3],
    inference_evidence: Option<&Path>,
) -> Result<(), String> {
    let (report, report_hash): (CalibratedReport, _) =
        read_document_identified(report_path, MAX_REPORT_BYTES)?;
    let (selection, selection_hash): (CalibratedSelection, _) =
        read_document_identified(selection_path, MAX_SELECTION_BYTES)?;
    let package = cocobeat_media::validate_package(source)?;
    let compilation = match inference_evidence {
        Some(evidence) => crate::anchor_calibration::recompile_inference(
            &package,
            evidence,
            input,
            calibration_report,
            choice,
        )?,
        None => crate::anchor_calibration::recompile(&package, input, calibration_report, choice)?,
    };
    let expected = compiled_report(&package, &compilation)?;
    if report != expected
        || serde_json::to_vec(&report).map_err(|e| e.to_string())?
            != serde_json::to_vec(&expected).map_err(|e| e.to_string())?
    {
        return Err(
            "Calibrated proposal differs from complete original-source/context recompilation"
                .into(),
        );
    }
    let anchors = select_calibrated(&expected, &report_hash.to_string(), &selection)?;
    let destination = compilation.output_path(source, destination)?;
    compilation.require_fresh_sources()?;
    let (_, fresh_report): (CalibratedReport, _) =
        read_document_identified(report_path, MAX_REPORT_BYTES)?;
    let (_, fresh_selection): (CalibratedSelection, _) =
        read_document_identified(selection_path, MAX_SELECTION_BYTES)?;
    if fresh_report != report_hash || fresh_selection != selection_hash {
        return Err("Calibrated proposal or selection raw bytes changed after load".into());
    }
    let exported = cocobeat_media::export_anchors(
        source,
        package.manifest.package_hash,
        &anchors,
        &destination,
    )?;
    println!(
        "{}",
        serde_json::json!({
            "source_content_id": report.source.content_id,
            "content_id": format!("package-blake3:{}", hex(exported.manifest.package_hash)),
            "anchor_count": exported.chart.anchors.len(), "section_cue_count": exported.chart.sections.len(),
            "changed": exported.manifest.package_hash != package.manifest.package_hash,
            "proposal_blake3": report_hash.to_string(), "calibration": report.calibration,
            "scope": report.scope, "quality_status": "UNASSESSED", "production_admission": false,
        })
    );
    Ok(())
}

// Strict inference consumer: caller supplies evidence explicitly; evaluation is never a fallback
pub(crate) fn load_inferred_report(
    source: &Path,
    package: &ValidatedPackage,
    proposal_path: &Path,
    evidence: &Path,
    input: &Path,
    calibration_report: &Path,
    choice: &Path,
) -> Result<(CalibratedReport, blake3::Hash), String> {
    let (report, hash): (CalibratedReport, _) =
        read_document_identified(proposal_path, MAX_REPORT_BYTES)?;
    let compilation = crate::anchor_calibration::recompile_inference(
        package,
        evidence,
        input,
        calibration_report,
        choice,
    )?;
    let expected = compiled_report(package, &compilation)?;
    if report != expected
        || serde_json::to_vec(&report).map_err(|e| e.to_string())?
            != serde_json::to_vec(&expected).map_err(|e| e.to_string())?
    {
        return Err(
            "Inference proposal differs from full original-source/context recompilation".into(),
        );
    }
    compilation.require_fresh_sources()?;
    let (_, fresh): (CalibratedReport, _) =
        read_document_identified(proposal_path, MAX_REPORT_BYTES)?;
    if fresh != hash || cocobeat_media::validate_package(source)? != *package {
        return Err("Inference proposal or supplied source changed after load".into());
    }
    Ok((report, hash))
}

pub(crate) fn load_report(package: &ValidatedPackage, path: &Path) -> Result<Report, String> {
    let report: Report = read_document(path, MAX_REPORT_BYTES)?;
    if report.report_version != REPORT_VERSION || report.compiler_version != ANCHOR_COMPILER_VERSION
    {
        return Err("Unsupported Anchor proposal report or compiler version".into());
    }
    if report.anchors.len() > MAX_CONTENT_ITEMS || report.evidence.len() > MAX_CONTENT_ITEMS {
        return Err("Anchor proposal exceeds the content item limit".into());
    }
    if report.source != source_identity(package) {
        return Err("Anchor proposal source identity does not match the package".into());
    }
    let expected = make_report(package, report.policy)?;
    // Numeric equality rejects non-finite report floats; canonical serialization also preserves -0
    if report != expected
        || serde_json::to_vec(&report).map_err(|error| error.to_string())?
            != serde_json::to_vec(&expected).map_err(|error| error.to_string())?
    {
        return Err("Anchor proposal differs from recompilation of its source and policy".into());
    }
    Ok(report)
}

fn make_report(package: &ValidatedPackage, policy: Policy) -> Result<Report, String> {
    let proposal = compile_anchor_proposal(
        &package.analysis,
        package.manifest.canonical_frames,
        AnchorPolicy {
            min_confidence: policy.min_confidence,
            min_gap_frames: policy.min_gap_frames,
        },
    )?;
    Ok(Report {
        report_version: REPORT_VERSION,
        compiler_version: ANCHOR_COMPILER_VERSION,
        source: source_identity(package),
        policy,
        production_admission: "not_assessed".into(),
        anchors: proposal
            .anchors
            .into_iter()
            .map(|anchor| ProposedAnchor {
                id: anchor.id,
                frame: anchor.song_time.frames(),
            })
            .collect(),
        evidence: proposal
            .evidence
            .into_iter()
            .map(|evidence| {
                let decision = match evidence.decision {
                    AnchorDecision::Selected { anchor_id } => {
                        Decision::SelectedByExperimentalPolicy { anchor_id }
                    }
                    AnchorDecision::UnknownConfidence => Decision::UnknownConfidence {},
                    AnchorDecision::BelowConfidence => Decision::BelowConfidence {},
                    AnchorDecision::TooClose {
                        blocking_onset_index,
                        distance_frames,
                    } => Decision::TooClose {
                        blocking_onset_index,
                        distance_frames,
                    },
                    AnchorDecision::DensityLimited { .. } => {
                        return Err("Legacy proposal cannot contain density decisions".to_string());
                    }
                };
                Ok(Evidence {
                    onset_index: evidence.onset_index,
                    frame: evidence.time.frames(),
                    strength: evidence.strength,
                    confidence: evidence.confidence,
                    decision,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
    })
}

fn source_identity(package: &ValidatedPackage) -> Source {
    Source {
        content_id: format!("package-blake3:{}", hex(package.manifest.package_hash)),
        analysis_blake3: hex(package.manifest.analysis.blake3),
        audio_blake3: hex(package.manifest.audio.blake3),
        chart_blake3: hex(package.manifest.chart.blake3),
        canonical_frames: package.manifest.canonical_frames,
        analysis_version: package.manifest.analysis_version.clone(),
        ruleset_id: package.chart.ruleset_id.clone(),
    }
}

fn hex(hash: [u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn read_document<T: DeserializeOwned>(path: &Path, limit: usize) -> Result<T, String> {
    read_document_identified(path, limit).map(|(document, _)| document)
}

pub(crate) fn read_document_identified<T: DeserializeOwned>(
    path: &Path,
    limit: usize,
) -> Result<(T, blake3::Hash), String> {
    if !fs::symlink_metadata(path)
        .map_err(|error| format!("Inspect JSON document: {error}"))?
        .is_file()
    {
        return Err("JSON document must be a regular file".into());
    }
    let file = File::open(path).map_err(|error| format!("Open JSON document: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Opened JSON document must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Read JSON document: {error}"))?;
    if bytes.len() > limit {
        return Err(format!("JSON document exceeds {limit} bytes"));
    }
    let document = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid JSON document: {error}"))?;
    Ok((document, blake3::hash(&bytes)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cocobeat_media::PackageBuildInput;
    use cocobeat_schema::{CompiledChart, EnergySample, MusicAnalysis, OnsetFeature, SectionCue};
    use std::{path::PathBuf, time::SystemTime};

    pub(crate) fn fixture(name: &str) -> (PathBuf, ValidatedPackage) {
        let suffix = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "cocobeat-anchor-proposal-{name}-{}-{suffix}",
            std::process::id(),
        ));
        fs::create_dir(&root).unwrap();
        let audio = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/synthetic/media-import/stereo-canonical.ogg");
        let package = cocobeat_media::build_package(&audio, 4800, root.join("source"), |_, audio| {
            Ok(PackageBuildInput {
                song_id: "proposal-mechanism-test".into(),
                importer_version: "test-v1".into(),
                analysis_version: "constructed-onsets-test-v1".into(),
                chart_version: "manual-test-v1".into(),
                analysis: MusicAnalysis {
                    capabilities: None,
            tempo_regions: Vec::new(),
            repetitions: Vec::new(),
            schema_version: 1,
                    audio_hash: audio.asset.blake3,
                    beats: vec![],
                    onsets: [
                        (0, -0.0, None),
                        (500, 1.0, Some(0.5)),
                        (1000, 1.0, Some(0.8)),
                        (1200, 1.0, Some(0.95)),
                        (3000, 1.0, Some(0.8)),
                        (4799, 1.0, Some(1.0)),
                    ]
                    .into_iter()
                    .map(|(frame, strength, confidence)| OnsetFeature {
                        time: SongTime::from_frames(frame),
                        strength,
                        confidence,
                    })
                    .collect(),
                    sections: vec![],
                    energy: vec![EnergySample {
                        start: SongTime::ZERO,
                        frames: 4800,
                        rms: [0.0; 2],
                        peak: [0.0; 2],
                    }],
                    diagnostics: "Constructed onset scores and placeholder energy for mechanism tests; not calibrated MIR".into(),
                },
                chart: CompiledChart {
                    schema_version: 1,
                    audio_hash: audio.asset.blake3,
                    ruleset_id: "duo-watermark-v1".into(),
                    anchors: vec![Anchor { id: 99, song_time: SongTime::from_frames(333) }],
                    sections: vec![SectionCue { id: 7, time: SongTime::from_frames(100), label: "keep original cue".into() }],
                },
            })
        }).unwrap();
        (root, package)
    }

    fn selection(path: &Path, source: &ValidatedPackage, indices: &[usize]) {
        fs::write(
            path,
            serde_json::json!({
                "schema_version": 1,
                "source_content_id": source_identity(source).content_id,
                "onset_indices": indices,
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn reports_round_trip_and_explicit_adoption_preserves_cues_and_source_objects() {
        let (root, package) = fixture("adopt");
        let source = root.join("source");
        let names = [
            "song.audio.ogg",
            "analysis.bin",
            "chart.bin",
            "song.package",
        ];
        let original = names.map(|name| fs::read(source.join(name)).unwrap());
        let report_path = root.join("proposal.json");
        propose(&source, "0.7", "1000", &report_path).unwrap();
        let report: Report = read_document(&report_path, MAX_REPORT_BYTES).unwrap();
        assert_eq!(
            report
                .anchors
                .iter()
                .map(|a| (a.id, a.frame))
                .collect::<Vec<_>>(),
            [(4, 1200), (5, 3000), (6, 4799)]
        );
        assert_eq!(report.evidence.len(), 6);
        assert!(matches!(
            report.evidence[0].decision,
            Decision::UnknownConfidence {}
        ));
        assert!(matches!(
            report.evidence[1].decision,
            Decision::BelowConfidence {}
        ));
        assert_eq!(
            report.evidence[2].decision,
            Decision::TooClose {
                blocking_onset_index: 3,
                distance_frames: 200
            }
        );
        assert_eq!(report.policy.min_confidence.to_bits(), 0.7_f32.to_bits());
        assert_eq!(report.evidence[0].strength.to_bits(), (-0.0_f32).to_bits());
        let repeated = root.join("repeated.json");
        propose(&source, "0.7", "1000", &repeated).unwrap();
        assert_eq!(
            fs::read(&report_path).unwrap(),
            fs::read(&repeated).unwrap()
        );

        let selection_path = root.join("selection.json");
        selection(&selection_path, &package, &[5, 3]);
        let destination = root.join("adopted");
        adopt(&source, &report_path, &selection_path, &destination).unwrap();
        let adopted = cocobeat_media::validate_package(&destination).unwrap();
        assert_eq!(
            adopted
                .chart
                .anchors
                .iter()
                .map(|a| (a.id, a.song_time.frames()))
                .collect::<Vec<_>>(),
            [(4, 1200), (6, 4799)]
        );
        assert_eq!(adopted.chart.sections, package.chart.sections);
        assert_eq!(adopted.chart.ruleset_id, package.chart.ruleset_id);
        assert_eq!(adopted.chart.audio_hash, package.chart.audio_hash);
        assert_eq!(adopted.analysis, package.analysis);
        assert_eq!(
            fs::read(destination.join("song.audio.ogg")).unwrap(),
            original[0]
        );
        assert_eq!(
            fs::read(destination.join("analysis.bin")).unwrap(),
            original[1]
        );
        assert_ne!(adopted.manifest.package_hash, package.manifest.package_hash);
        selection(&selection_path, &package, &[]);
        let empty = root.join("empty");
        adopt(&source, &report_path, &selection_path, &empty).unwrap();
        let emptied = cocobeat_media::validate_package(empty).unwrap();
        assert!(emptied.chart.anchors.is_empty());
        assert_eq!(emptied.chart.sections, package.chart.sections);
        assert_eq!(
            names.map(|name| fs::read(source.join(name)).unwrap()),
            original
        );

        let mut without_onsets = package;
        without_onsets.analysis.onsets.clear();
        let empty = make_report(&without_onsets, report.policy).unwrap();
        assert!(empty.anchors.is_empty() && empty.evidence.is_empty());
        assert_eq!(without_onsets.chart.anchors.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tampered_reports_and_invalid_selections_never_create_packages() {
        let (root, package) = fixture("reject");
        let source = root.join("source");
        let report_path = root.join("proposal.json");
        let selection_path = root.join("selection.json");
        let destination = root.join("must-not-exist");
        propose(&source, "0.7", "1000", &report_path).unwrap();
        selection(&selection_path, &package, &[3]);
        let valid = fs::read_to_string(&report_path).unwrap();
        for (pointer, replacement) in [
            ("/report_version", serde_json::json!(2)),
            ("/compiler_version", serde_json::json!(2)),
            (
                "/source/content_id",
                serde_json::json!("package-blake3:wrong"),
            ),
            ("/source/analysis_blake3", serde_json::json!("wrong")),
            ("/source/audio_blake3", serde_json::json!("wrong")),
            ("/source/chart_blake3", serde_json::json!("wrong")),
            ("/source/canonical_frames", serde_json::json!(4801)),
            ("/source/analysis_version", serde_json::json!("wrong")),
            ("/source/ruleset_id", serde_json::json!("wrong")),
            ("/policy/min_confidence", serde_json::json!(0.99)),
            ("/production_admission", serde_json::json!("passed")),
            ("/anchors/0/id", serde_json::json!(99)),
            ("/anchors/0/frame", serde_json::json!(1201)),
            ("/evidence/0/strength", serde_json::json!(0.0)),
            ("/evidence/0/confidence", serde_json::json!(0.9)),
            (
                "/evidence/2/decision/distance_frames",
                serde_json::json!(201),
            ),
            (
                "/evidence/2/decision/blocking_onset_index",
                serde_json::json!(4),
            ),
        ] {
            let mut forged: serde_json::Value = serde_json::from_str(&valid).unwrap();
            *forged.pointer_mut(pointer).unwrap() = replacement;
            fs::write(&report_path, forged.to_string()).unwrap();
            assert!(
                adopt(&source, &report_path, &selection_path, &destination).is_err(),
                "{pointer}"
            );
            assert!(load_report(&package, &report_path).is_err(), "{pointer}");
            assert!(!destination.exists());
        }
        for invalid in [
            valid.replacen("{", "{\"extra\":1,", 1),
            valid.replacen("{", "{\"report_version\":1,", 1),
            valid.replacen(
                "\"kind\":\"unknown_confidence\"",
                "\"kind\":\"unknown_confidence\",\"extra\":1",
                1,
            ),
            valid.replacen(
                "\"kind\":\"unknown_confidence\"",
                "\"kind\":\"unknown_confidence\",\"kind\":\"unknown_confidence\"",
                1,
            ),
            valid.replacen("\"confidence\":null", "\"confidence\":1e39", 1),
            valid.replacen("\"frame\":1200", "\"frame\":1200.0", 1),
        ] {
            assert_ne!(invalid, valid);
            fs::write(&report_path, invalid).unwrap();
            assert!(adopt(&source, &report_path, &selection_path, &destination).is_err());
            assert!(!destination.exists());
        }
        fs::write(&report_path, &valid).unwrap();
        for indices in [&[3, 3][..], &[0], &[1], &[2], &[6], &[usize::MAX]] {
            selection(&selection_path, &package, indices);
            assert!(adopt(&source, &report_path, &selection_path, &destination).is_err());
            assert!(!destination.exists());
        }
        selection(&selection_path, &package, &[3]);
        let valid_selection = fs::read_to_string(&selection_path).unwrap();
        for invalid in [
            valid_selection.replacen("\"schema_version\":1", "\"schema_version\":2", 1),
            valid_selection.replacen("package-blake3:", "wrong:", 1),
            valid_selection.replacen("{", "{\"schema_version\":1,", 1),
            valid_selection.replacen("{", "{\"extra\":1,", 1),
            valid_selection.replacen("[3]", "[-1]", 1),
            valid_selection.replacen("[3]", "[3.0]", 1),
        ] {
            assert_ne!(invalid, valid_selection);
            fs::write(&selection_path, invalid).unwrap();
            assert!(adopt(&source, &report_path, &selection_path, &destination).is_err());
            assert!(!destination.exists());
        }
        assert_eq!(cocobeat_media::validate_package(&source).unwrap(), package);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_and_selections_are_bounded_and_outputs_preserve_owned_paths() {
        let (root, package) = fixture("bounds");
        let source = root.join("source");
        let report_path = root.join("proposal.json");
        propose(&source, "0.7", "1000", &report_path).unwrap();
        let original_report = fs::read(&report_path).unwrap();
        assert!(propose(&source, "0.7", "1000", &report_path).is_err());
        assert_eq!(fs::read(&report_path).unwrap(), original_report);
        assert!(propose(&source, "0.7", "1000", &source.join("proposal.json")).is_err());
        assert!(propose(&source, "0.7", "1000", &root.join("missing/report.json")).is_err());
        #[cfg(unix)]
        {
            let alias = root.join("source-alias");
            std::os::unix::fs::symlink(&source, &alias).unwrap();
            assert!(propose(&source, "0.7", "1000", &alias.join("proposal.json")).is_err());
        }
        let invalid_output = root.join("invalid.json");
        for (confidence, gap) in [
            ("NaN", "1"),
            ("inf", "1"),
            ("0", "1"),
            ("1.1", "1"),
            ("0.5", "0"),
            ("0.5", "-1"),
            ("0.5", "1.5"),
        ] {
            assert!(propose(&source, confidence, gap, &invalid_output).is_err());
            assert!(!invalid_output.exists());
        }
        let selection_path = root.join("selection.json");
        selection(&selection_path, &package, &[]);
        let mut bytes = fs::read(&selection_path).unwrap();
        bytes.resize(MAX_SELECTION_BYTES, b' ');
        fs::write(&selection_path, &bytes).unwrap();
        assert!(read_document::<Selection>(&selection_path, MAX_SELECTION_BYTES).is_ok());
        bytes.push(b' ');
        fs::write(&selection_path, bytes).unwrap();
        assert!(read_document::<Selection>(&selection_path, MAX_SELECTION_BYTES).is_err());
        let mut bytes = original_report;
        bytes.resize(MAX_REPORT_BYTES, b' ');
        fs::write(&report_path, &bytes).unwrap();
        assert!(read_document::<Report>(&report_path, MAX_REPORT_BYTES).is_ok());
        bytes.push(b' ');
        fs::write(&report_path, bytes).unwrap();
        assert!(read_document::<Report>(&report_path, MAX_REPORT_BYTES).is_err());
        assert!(read_document::<Report>(&root, MAX_REPORT_BYTES).is_err());
        assert_eq!(fs::read_dir(&source).unwrap().count(), 4);
        assert_eq!(cocobeat_media::validate_package(source).unwrap(), package);
        fs::remove_dir_all(root).unwrap();
    }
}
