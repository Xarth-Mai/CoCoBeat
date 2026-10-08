//! Experimental Anchor suitability estimates from explicitly complete candidate judgments
//! Original package analysis and native evidence remain unchanged

use crate::{anchors, labels};
use cocobeat_media::{AnchorPolicy, AnchorProposal, ValidatedPackage};
use cocobeat_schema::MusicAnalysis;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const MAX_INPUT_BYTES: usize = 1_048_576;
const MAX_REPORT_BYTES: usize = 4 * MAX_INPUT_BYTES;
const MAX_DATASETS: usize = 8;
const MAX_CANDIDATES: usize = 1024;
const PROFILE: &str = "native-small0-hfc1024-interbeat-v1-candidate";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Scope {
    ExperimentalAnchorCalibration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Method {
    FixedBinBeta11,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScoreDefinition {
    OriginalHfcNormalizedStrengthF32Bits,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema_version: u32,
    scope: Scope,
    method: Method,
    score_definition: ScoreDefinition,
    profile: String,
    channel: usize,
    edge_bits: Vec<u32>,
    min_bin_support: usize,
    policies: Vec<Policy>,
    train: Vec<Dataset>,
    evaluation: Vec<Dataset>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    pub(crate) min_confidence: f32,
    pub(crate) min_gap_frames: i64,
    pub(crate) density_window_frames: i64,
    pub(crate) max_anchors_per_window: usize,
}

impl Policy {
    fn compile(self, analysis: &MusicAnalysis, n: u64) -> Result<AnchorProposal, String> {
        AnchorPolicy {
            min_confidence: self.min_confidence,
            min_gap_frames: self.min_gap_frames,
        }
        .compile_with_density(
            analysis,
            n,
            self.density_window_frames,
            self.max_anchors_per_window,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    source: labels::Source,
    analysis_blake3: String,
    evidence_summary_blake3: String,
    profile: String,
    channel: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Dataset {
    package: PathBuf,
    evidence: PathBuf,
    labels: PathBuf,
    binding: Binding,
    labels_blake3: String,
    origin_group_id: String,
    reviewed: Vec<Reviewed>,
    mappings: Vec<Mapping>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reviewed {
    start_frame: u64,
    end_frame: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    onset_index: usize,
    label_item_id: u64,
    frame: i64,
    original_score_bits: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    schema_version: u32,
    input_blake3: String,
    train_result_blake3: String,
    policy_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    mapping: Mapping,
    decision: labels::AnchorDecision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DatasetSummary {
    binding: Binding,
    labels_blake3: String,
    origin_group_id: String,
    reviewed: Vec<Reviewed>,
    candidates: Vec<Candidate>,
    unused_label_item_ids: Vec<u64>,
}

struct Case {
    package_path: PathBuf,
    evidence_path: PathBuf,
    labels_path: PathBuf,
    package: ValidatedPackage,
    summary: DatasetSummary,
}

struct Snapshot {
    input: Input,
    input_path: PathBuf,
    input_blake3: String,
    train: Vec<Case>,
    evaluation: Vec<Case>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bin {
    positive: usize,
    negative: usize,
    probability_bits: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Training {
    scope: Scope,
    method: Method,
    score_definition: ScoreDefinition,
    profile: String,
    channel: usize,
    edge_bits: Vec<u32>,
    min_bin_support: usize,
    datasets: Vec<DatasetSummary>,
    bins: Vec<Bin>,
    policies: Vec<Policy>,
    policy_counts: Vec<Counts>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Counts {
    accepted_positive: usize,
    accepted_negative: usize,
    rejected_positive: usize,
    rejected_negative: usize,
    uncertain: usize,
    unknown_estimate: usize,
    max_window_count: usize,
}

impl Counts {
    fn add(&mut self, other: Self) {
        self.accepted_positive += other.accepted_positive;
        self.accepted_negative += other.accepted_negative;
        self.rejected_positive += other.rejected_positive;
        self.rejected_negative += other.rejected_negative;
        self.uncertain += other.uncertain;
        self.unknown_estimate += other.unknown_estimate;
        self.max_window_count = self.max_window_count.max(other.max_window_count);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Applicable,
    NotApplicable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    input_blake3: String,
    train_result_blake3: String,
    choice_blake3: String,
    training: Training,
    policy_index: usize,
    evaluation: Vec<DatasetSummary>,
    // Per original score bin: explicit positive and negative judgments, without fitting on evaluation
    evaluation_bin_counts: Vec<[usize; 2]>,
    counts: Counts,
    known_positive: usize,
    known_negative: usize,
    brier: Option<f64>,
    status: Status,
    quality_status: String,
    production_admission: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Estimate {
    pub(crate) onset_index: usize,
    pub(crate) original_frame: i64,
    pub(crate) original_score_bits: u32,
    pub(crate) probability_bits: Option<u32>,
    pub(crate) bin_index: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    pub(crate) input_blake3: String,
    pub(crate) calibration_report_blake3: String,
    pub(crate) choice_blake3: String,
    pub(crate) train_result_blake3: String,
    pub(crate) policy_index: usize,
}

// This is transient compiler input, never a package mutation or native evidence replacement
pub(crate) struct Compilation {
    pub(crate) proposal: AnchorProposal,
    pub(crate) estimates: Vec<Estimate>,
    pub(crate) policy: Policy,
    pub(crate) context: Context,
    snapshot: Snapshot,
    report_path: PathBuf,
    choice_path: PathBuf,
}

impl Compilation {
    pub(crate) fn require_fresh_sources(&self) -> Result<(), String> {
        self.snapshot.require_fresh()?;
        let (_, report_hash): (Report, _) =
            anchors::read_document_identified(&self.report_path, MAX_REPORT_BYTES)?;
        let (_, choice_hash): (Choice, _) =
            anchors::read_document_identified(&self.choice_path, MAX_INPUT_BYTES)?;
        if report_hash.to_string() != self.context.calibration_report_blake3
            || choice_hash.to_string() != self.context.choice_blake3
        {
            return Err("Calibration report or choice raw bytes changed after load".into());
        }
        Ok(())
    }

    pub(crate) fn output_path(&self, source: &Path, destination: &Path) -> Result<PathBuf, String> {
        let destination = self.snapshot.output_path(destination)?;
        let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
        // The CLI may name another byte-identical copy of the evaluation package
        if destination
            .parent()
            .is_some_and(|parent| parent.starts_with(source))
        {
            return Err("Calibration output must be outside the supplied source package".into());
        }
        Ok(destination)
    }
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    Ok(blake3::hash(&bytes).to_string())
}

fn bin_index(bits: u32, edges: &[u32]) -> Option<usize> {
    let value = f32::from_bits(bits);
    let low = f32::from_bits(*edges.first()?);
    let high = f32::from_bits(*edges.last()?);
    if !value.is_finite() || value < low || value > high {
        return None;
    }
    if value == high {
        return Some(edges.len() - 2);
    }
    (0..edges.len() - 1)
        .find(|&i| value >= f32::from_bits(edges[i]) && value < f32::from_bits(edges[i + 1]))
}

fn check_input(input: &Input) -> Result<(), String> {
    if input.schema_version != 1
        || input.profile != PROFILE
        || input.channel > 1
        || !(2..=9).contains(&input.edge_bits.len())
        || input
            .edge_bits
            .iter()
            .any(|&v| !f32::from_bits(v).is_finite())
        || input
            .edge_bits
            .windows(2)
            .any(|v| f32::from_bits(v[0]) >= f32::from_bits(v[1]))
        || !(2..=MAX_DATASETS * MAX_CANDIDATES).contains(&input.min_bin_support)
        || !(1..=8).contains(&input.policies.len())
        || !(1..=MAX_DATASETS).contains(&input.train.len())
        || !(1..=MAX_DATASETS).contains(&input.evaluation.len())
    {
        return Err(
            "Invalid explicit calibration extent/profile/channel/bin/support/policy/split".into(),
        );
    }
    for policy in &input.policies {
        if !policy.min_confidence.is_finite()
            || !(0.0 < policy.min_confidence && policy.min_confidence <= 1.0)
            || !(1..=cocobeat_schema::MAX_CANONICAL_FRAMES as i64).contains(&policy.min_gap_frames)
            || !(1..=cocobeat_schema::MAX_CANONICAL_FRAMES as i64)
                .contains(&policy.density_window_frames)
            || !(1..=cocobeat_schema::MAX_CONTENT_ITEMS).contains(&policy.max_anchors_per_window)
        {
            return Err(
                "Calibration policies require explicit finite confidence, gap and density".into(),
            );
        }
    }
    Ok(())
}

fn binding(package: &ValidatedPackage, evidence: &Path) -> Result<Binding, String> {
    // The original None protocol is checked before any in-memory calibration
    let native = cocobeat_media::read_native_beat_evidence(package, evidence)?;
    Ok(Binding {
        source: labels::Source::from_package(package),
        analysis_blake3: blake3::Hash::from_bytes(package.manifest.analysis.blake3).to_string(),
        evidence_summary_blake3: native.metadata.summary_blake3,
        profile: native.metadata.profile,
        channel: native.metadata.channel,
    })
}

fn candidate_rows(
    package: &ValidatedPackage,
    labels: &labels::Document,
    mappings: &[Mapping],
) -> Result<(Vec<Candidate>, Vec<u64>), String> {
    if package.analysis.onsets.len() > MAX_CANDIDATES
        || mappings.len() != package.analysis.onsets.len()
    {
        return Err("Calibration requires a complete mapping of at most 1024 candidates".into());
    }
    let mut frames = BTreeSet::new();
    for label in &labels.labels {
        let labels::Location::Point { frame } = label.location else {
            return Err(
                "Calibration labels require exact points, not interval denominators".into(),
            );
        };
        if !frames.insert(frame) {
            return Err("Calibration label points duplicate or conflict".into());
        }
    }
    let available: std::collections::BTreeMap<_, _> =
        labels.labels.iter().map(|v| (v.item_id, v)).collect();
    let mut used = BTreeSet::new();
    let mut candidates = Vec::with_capacity(mappings.len());
    for (index, (onset, mapping)) in package.analysis.onsets.iter().zip(mappings).enumerate() {
        let label = available
            .get(&mapping.label_item_id)
            .ok_or("Calibration label item_id is missing")?;
        if mapping.onset_index != index
            || mapping.frame != onset.time.frames()
            || mapping.original_score_bits != onset.strength.to_bits()
            || onset.confidence.is_some()
            || !used.insert(mapping.label_item_id)
            || label.location
                != (labels::Location::Point {
                    frame: onset.time.frames(),
                })
        {
            return Err("Calibration mapping differs from original index/frame/score/None or exact independent point".into());
        }
        candidates.push(Candidate {
            mapping: *mapping,
            decision: label.anchor_decision,
        });
    }
    let unused = available
        .keys()
        .copied()
        .filter(|id| !used.contains(id))
        .collect();
    Ok((candidates, unused))
}

fn load_case(base: &Path, dataset: &Dataset, input: &Input) -> Result<Case, String> {
    let package_path = base.join(&dataset.package);
    let evidence_path = base.join(&dataset.evidence);
    let labels_path = base.join(&dataset.labels);
    let package = cocobeat_media::validate_package(&package_path)?;
    let actual = binding(&package, &evidence_path)?;
    let n = package.manifest.canonical_frames;
    if actual != dataset.binding
        || actual.profile != input.profile
        || actual.channel != input.channel
        || dataset.reviewed
            != [Reviewed {
                start_frame: 0,
                end_frame: n,
            }]
        || dataset.origin_group_id.trim() != dataset.origin_group_id
        || dataset.origin_group_id.is_empty()
        || dataset.origin_group_id.len() > 128
    {
        return Err("Calibration case identity, complete coverage or origin group differs".into());
    }
    let (document, hash) = labels::load_identified(&labels_path, &actual.source)?;
    if hash.to_string() != dataset.labels_blake3 {
        return Err("Calibration label raw bytes differ".into());
    }
    let (candidates, unused_label_item_ids) =
        candidate_rows(&package, &document, &dataset.mappings)?;
    for policy in &input.policies {
        if policy.density_window_frames > n as i64 {
            return Err("Calibration density window exceeds a case N".into());
        }
    }
    Ok(Case {
        package_path,
        evidence_path,
        labels_path,
        package,
        summary: DatasetSummary {
            binding: actual,
            labels_blake3: dataset.labels_blake3.clone(),
            origin_group_id: dataset.origin_group_id.clone(),
            reviewed: dataset.reviewed.clone(),
            candidates,
            unused_label_item_ids,
        },
    })
}

fn check_denominator(split: &[DatasetSummary]) -> Result<(), String> {
    let mut audio = BTreeSet::new();
    for case in split {
        if !audio.insert(&case.binding.source.audio_blake3) {
            return Err("Calibration split duplicates canonical audio".into());
        }
    }
    let positive = split
        .iter()
        .flat_map(|v| &v.candidates)
        .any(|v| v.decision == labels::AnchorDecision::ShouldAnchor);
    let negative = split
        .iter()
        .flat_map(|v| &v.candidates)
        .any(|v| v.decision == labels::AnchorDecision::ShouldNotAnchor);
    if !positive || !negative {
        return Err("Calibration split requires explicit positive and negative judgments".into());
    }
    Ok(())
}

fn check_splits(train: &[DatasetSummary], evaluation: &[DatasetSummary]) -> Result<(), String> {
    check_denominator(train)?;
    check_denominator(evaluation)?;
    let audio: BTreeSet<_> = train
        .iter()
        .map(|v| &v.binding.source.audio_blake3)
        .collect();
    let groups: BTreeSet<_> = train.iter().map(|v| &v.origin_group_id).collect();
    if evaluation.iter().any(|v| {
        audio.contains(&v.binding.source.audio_blake3) || groups.contains(&v.origin_group_id)
    }) {
        return Err("Calibration train/evaluation share canonical audio or an origin group".into());
    }
    Ok(())
}

impl Snapshot {
    fn training_snapshot(path: &Path) -> Result<Self, String> {
        let (input, hash): (Input, _) = anchors::read_document_identified(path, MAX_INPUT_BYTES)?;
        check_input(&input)?;
        let base = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let train = input
            .train
            .iter()
            .map(|v| load_case(base, v, &input))
            .collect::<Result<Vec<_>, _>>()?;
        check_denominator(&train.iter().map(|v| v.summary.clone()).collect::<Vec<_>>())?;
        Ok(Self {
            input,
            input_path: path.to_path_buf(),
            input_blake3: hash.to_string(),
            train,
            evaluation: Vec::new(),
        })
    }

    fn load(path: &Path, choice: &Choice) -> Result<Self, String> {
        let mut snapshot = Self::training_snapshot(path)?;
        let training = snapshot.training()?;
        check_choice(&snapshot.input_blake3, &training, choice)?;
        let base = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        // Heldout labels and records are read only after the frozen train choice matches
        snapshot.evaluation = snapshot
            .input
            .evaluation
            .iter()
            .map(|v| load_case(base, v, &snapshot.input))
            .collect::<Result<Vec<_>, _>>()?;
        check_splits(
            &snapshot
                .train
                .iter()
                .map(|v| v.summary.clone())
                .collect::<Vec<_>>(),
            &snapshot
                .evaluation
                .iter()
                .map(|v| v.summary.clone())
                .collect::<Vec<_>>(),
        )?;
        Ok(snapshot)
    }

    fn output_path(&self, destination: &Path) -> Result<PathBuf, String> {
        let name = destination
            .file_name()
            .ok_or("Calibration output must name a new file")?;
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = fs::canonicalize(parent).map_err(|e| e.to_string())?;
        let base = self
            .input_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        // Directory identity checks do not consume heldout labels or candidate records
        for dataset in self.input.train.iter().chain(&self.input.evaluation) {
            for protected in [&dataset.package, &dataset.evidence] {
                let source = fs::canonicalize(base.join(protected)).map_err(|e| e.to_string())?;
                if parent.starts_with(source) {
                    return Err(
                        "Calibration output must be outside all source packages and evidence"
                            .into(),
                    );
                }
            }
        }
        Ok(parent.join(name))
    }

    fn require_fresh(&self) -> Result<(), String> {
        let (_, hash): (Input, _) =
            anchors::read_document_identified(&self.input_path, MAX_INPUT_BYTES)?;
        if hash.to_string() != self.input_blake3 {
            return Err("Calibration input raw bytes changed after load".into());
        }
        for case in self.train.iter().chain(&self.evaluation) {
            let package = cocobeat_media::validate_package(&case.package_path)?;
            if binding(&package, &case.evidence_path)? != case.summary.binding {
                return Err("Calibration source or native evidence changed after load".into());
            }
            let (_, hash) =
                labels::load_identified(&case.labels_path, &case.summary.binding.source)?;
            if hash.to_string() != case.summary.labels_blake3 {
                return Err("Calibration labels changed after load".into());
            }
        }
        Ok(())
    }

    fn training(&self) -> Result<Training, String> {
        let datasets = self
            .train
            .iter()
            .map(|v| v.summary.clone())
            .collect::<Vec<_>>();
        let bins = fit_bins(&datasets, &self.input.edge_bits, self.input.min_bin_support);
        let policy_counts = self
            .input
            .policies
            .iter()
            .map(|&p| {
                let mut count = Counts::default();
                for case in &self.train {
                    count.add(evaluate_policy(case, &bins, &self.input.edge_bits, p)?.0);
                }
                Ok(count)
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Training {
            scope: self.input.scope,
            method: self.input.method,
            score_definition: self.input.score_definition,
            profile: self.input.profile.clone(),
            channel: self.input.channel,
            edge_bits: self.input.edge_bits.clone(),
            min_bin_support: self.input.min_bin_support,
            datasets,
            bins,
            policies: self.input.policies.clone(),
            policy_counts,
        })
    }

    fn report(&self, choice: &Choice, choice_hash: &str) -> Result<Report, String> {
        let training = self.training()?;
        let train_hash = digest(&training)?;
        check_choice(&self.input_blake3, &training, choice)?;
        let policy = training.policies[choice.policy_index];
        let mut counts = Counts::default();
        let (mut known_positive, mut known_negative, mut squared_error) = (0usize, 0usize, 0.0f64);
        for case in &self.evaluation {
            let (count, _, estimates) =
                evaluate_policy(case, &training.bins, &training.edge_bits, policy)?;
            counts.add(count);
            for (candidate, estimate) in case.summary.candidates.iter().zip(estimates) {
                let Some(bits) = estimate.probability_bits else {
                    continue;
                };
                let truth = match candidate.decision {
                    labels::AnchorDecision::ShouldAnchor => {
                        known_positive += 1;
                        1.0
                    }
                    labels::AnchorDecision::ShouldNotAnchor => {
                        known_negative += 1;
                        0.0
                    }
                    labels::AnchorDecision::Uncertain => continue,
                };
                let error = f64::from(f32::from_bits(bits)) - truth;
                squared_error += error * error;
            }
        }
        let known = known_positive + known_negative;
        let evaluation = self
            .evaluation
            .iter()
            .map(|v| v.summary.clone())
            .collect::<Vec<_>>();
        let evaluation_bin_counts = fit_bins(&evaluation, &training.edge_bits, usize::MAX)
            .into_iter()
            .map(|v| [v.positive, v.negative])
            .collect();
        Ok(Report {
            schema_version: 1,
            input_blake3: self.input_blake3.clone(),
            train_result_blake3: train_hash,
            choice_blake3: choice_hash.into(),
            training,
            policy_index: choice.policy_index,
            evaluation,
            evaluation_bin_counts,
            counts,
            known_positive,
            known_negative,
            brier: (known > 0).then(|| squared_error / known as f64),
            status: if known_positive > 0 && known_negative > 0 {
                Status::Applicable
            } else {
                Status::NotApplicable
            },
            quality_status: "UNASSESSED".into(),
            production_admission: false,
        })
    }
}

fn check_choice(input_hash: &str, training: &Training, choice: &Choice) -> Result<(), String> {
    if choice.schema_version != 1
        || choice.input_blake3 != input_hash
        || choice.train_result_blake3 != digest(training)?
        || choice.policy_index >= training.policies.len()
    {
        return Err("Calibration choice differs from frozen input/train result/policy".into());
    }
    Ok(())
}

#[derive(Serialize)]
struct TrainReceipt {
    schema_version: u32,
    input_blake3: String,
    train_result_blake3: String,
    training: Training,
    quality_status: &'static str,
    production_admission: bool,
}

pub(crate) fn train(input_path: &Path, destination: &Path) -> Result<(), String> {
    let snapshot = Snapshot::training_snapshot(input_path)?;
    let training = snapshot.training()?;
    let train_result_blake3 = digest(&training)?;
    let receipt = TrainReceipt {
        schema_version: 1,
        input_blake3: snapshot.input_blake3.clone(),
        train_result_blake3: train_result_blake3.clone(),
        training,
        quality_status: "UNASSESSED",
        production_admission: false,
    };
    let destination = snapshot.output_path(destination)?;
    snapshot.require_fresh()?;
    let saved = labels::write_new(&destination, &receipt, MAX_REPORT_BYTES)?;
    println!(
        "{}",
        serde_json::json!({
            "input_blake3": snapshot.input_blake3,
            "train_result_blake3": train_result_blake3,
            "train_digest_basis": "compact_typed_training_content",
            "saved_train_receipt_blake3": saved.to_string(),
            "saved_digest_basis": "exact_pretty_receipt_file_bytes_with_newline",
            "destination": destination,
            "scope": "experimental_anchor_calibration", "quality_status": "UNASSESSED", "production_admission": false,
        })
    );
    Ok(())
}

pub(crate) fn evaluate(
    input_path: &Path,
    choice_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    let (choice, hash): (Choice, _) =
        anchors::read_document_identified(choice_path, MAX_INPUT_BYTES)?;
    let snapshot = Snapshot::load(input_path, &choice)?;
    let report = snapshot.report(&choice, &hash.to_string())?;
    let destination = snapshot.output_path(destination)?;
    snapshot.require_fresh()?;
    let (_, fresh): (Choice, _) = anchors::read_document_identified(choice_path, MAX_INPUT_BYTES)?;
    if fresh != hash {
        return Err("Calibration choice raw bytes changed after load".into());
    }
    let saved = labels::write_new(&destination, &report, MAX_REPORT_BYTES)?;
    println!(
        "{}",
        serde_json::json!({
            "input_blake3": snapshot.input_blake3,
            "train_result_blake3": report.train_result_blake3,
            "choice_blake3": hash.to_string(), "policy_index": choice.policy_index,
            "saved_calibration_report_blake3": saved.to_string(), "destination": destination,
            "status": report.status, "scope": "experimental_anchor_calibration", "quality_status": "UNASSESSED", "production_admission": false,
        })
    );
    Ok(())
}

fn fit_bins(datasets: &[DatasetSummary], edges: &[u32], support: usize) -> Vec<Bin> {
    let mut bins = vec![
        Bin {
            positive: 0,
            negative: 0,
            probability_bits: None
        };
        edges.len() - 1
    ];
    for candidate in datasets.iter().flat_map(|v| &v.candidates) {
        let Some(index) = bin_index(candidate.mapping.original_score_bits, edges) else {
            continue;
        };
        match candidate.decision {
            labels::AnchorDecision::ShouldAnchor => bins[index].positive += 1,
            labels::AnchorDecision::ShouldNotAnchor => bins[index].negative += 1,
            labels::AnchorDecision::Uncertain => {}
        }
    }
    for bin in &mut bins {
        let count = bin.positive + bin.negative;
        if count >= support {
            bin.probability_bits =
                Some((((bin.positive + 1) as f64 / (count + 2) as f64) as f32).to_bits());
        }
    }
    bins
}

fn estimates(candidates: &[Candidate], bins: &[Bin], edges: &[u32]) -> Vec<Estimate> {
    candidates
        .iter()
        .map(|candidate| {
            let index = bin_index(candidate.mapping.original_score_bits, edges);
            Estimate {
                onset_index: candidate.mapping.onset_index,
                original_frame: candidate.mapping.frame,
                original_score_bits: candidate.mapping.original_score_bits,
                bin_index: index,
                probability_bits: if candidate.decision == labels::AnchorDecision::Uncertain {
                    None
                } else {
                    index.and_then(|i| bins[i].probability_bits)
                },
            }
        })
        .collect()
}

fn evaluate_policy(
    case: &Case,
    bins: &[Bin],
    edges: &[u32],
    policy: Policy,
) -> Result<(Counts, AnchorProposal, Vec<Estimate>), String> {
    let estimates = estimates(&case.summary.candidates, bins, edges);
    let mut temporary = case.package.analysis.clone();
    for (onset, estimate) in temporary.onsets.iter_mut().zip(&estimates) {
        onset.confidence = estimate.probability_bits.map(f32::from_bits);
    }
    let proposal = policy.compile(&temporary, case.package.manifest.canonical_frames)?;
    let mut count = Counts::default();
    let mut windows = std::collections::BTreeMap::<i64, usize>::new();
    for ((candidate, estimate), evidence) in case
        .summary
        .candidates
        .iter()
        .zip(&estimates)
        .zip(&proposal.evidence)
    {
        let selected = matches!(
            evidence.decision,
            cocobeat_media::AnchorDecision::Selected { .. }
        );
        if estimate.probability_bits.is_none() {
            count.unknown_estimate += 1;
        }
        match (candidate.decision, selected) {
            (labels::AnchorDecision::ShouldAnchor, true) => count.accepted_positive += 1,
            (labels::AnchorDecision::ShouldNotAnchor, true) => count.accepted_negative += 1,
            (labels::AnchorDecision::ShouldAnchor, false) => count.rejected_positive += 1,
            (labels::AnchorDecision::ShouldNotAnchor, false) => count.rejected_negative += 1,
            (labels::AnchorDecision::Uncertain, _) => count.uncertain += 1,
        }
        if selected {
            *windows
                .entry(candidate.mapping.frame / policy.density_window_frames)
                .or_default() += 1;
        }
    }
    count.max_window_count = windows.values().copied().max().unwrap_or(0);
    Ok((count, proposal, estimates))
}

// Recompute the frozen calibration context before generating or adopting a v2 proposal
pub(crate) fn recompile(
    package: &ValidatedPackage,
    input_path: &Path,
    report_path: &Path,
    choice_path: &Path,
) -> Result<Compilation, String> {
    let (choice, choice_hash): (Choice, _) =
        anchors::read_document_identified(choice_path, MAX_INPUT_BYTES)?;
    let snapshot = Snapshot::load(input_path, &choice)?;
    let (reported, report_hash): (Report, _) =
        anchors::read_document_identified(report_path, MAX_REPORT_BYTES)?;
    let expected = snapshot.report(&choice, &choice_hash.to_string())?;
    if reported != expected
        || serde_json::to_vec(&reported).map_err(|e| e.to_string())?
            != serde_json::to_vec(&expected).map_err(|e| e.to_string())?
    {
        return Err("Calibration report differs from full frozen-input recomputation".into());
    }
    if expected.status != Status::Applicable {
        return Err("Calibration lacks heldout known positive and negative support".into());
    }
    let source = labels::Source::from_package(package);
    let case = snapshot
        .evaluation
        .iter()
        .find(|v| v.summary.binding.source == source)
        .ok_or("Calibration only applies to fully labeled evaluation packages")?;
    if package != &case.package {
        return Err("Calibration package data differs from its validated source snapshot".into());
    }
    let policy = expected.training.policies[choice.policy_index];
    let (_, proposal, estimates) = evaluate_policy(
        case,
        &expected.training.bins,
        &expected.training.edge_bits,
        policy,
    )?;
    snapshot.require_fresh()?;
    Ok(Compilation {
        proposal,
        estimates,
        policy,
        context: Context {
            input_blake3: snapshot.input_blake3.clone(),
            calibration_report_blake3: report_hash.to_string(),
            choice_blake3: choice_hash.to_string(),
            train_result_blake3: expected.train_result_blake3,
            policy_index: choice.policy_index,
        },
        snapshot,
        report_path: report_path.to_path_buf(),
        choice_path: choice_path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use labels::AnchorDecision::{
        ShouldAnchor as Positive, ShouldNotAnchor as Negative, Uncertain,
    };

    // Explicit integer-class controls exercise mechanics, not human music labels or recommended thresholds
    fn summary(
        audio: &str,
        group: &str,
        classes: &[(f32, labels::AnchorDecision)],
    ) -> DatasetSummary {
        let hash = blake3::hash(audio.as_bytes()).to_string();
        DatasetSummary {
            binding: Binding {
                source: labels::Source {
                    content_id: format!("package-blake3:{hash}"),
                    audio_blake3: hash.clone(),
                    canonical_frames: 100,
                    audio_basis: labels::AudioBasis::CanonicalDecoded,
                },
                analysis_blake3: hash.clone(),
                evidence_summary_blake3: hash,
                profile: PROFILE.into(),
                channel: 0,
            },
            labels_blake3: blake3::hash(group.as_bytes()).to_string(),
            origin_group_id: group.into(),
            reviewed: vec![Reviewed {
                start_frame: 0,
                end_frame: 100,
            }],
            candidates: classes
                .iter()
                .enumerate()
                .map(|(index, &(score, decision))| Candidate {
                    mapping: Mapping {
                        onset_index: index,
                        label_item_id: index as u64 + 1,
                        frame: index as i64 * 10,
                        original_score_bits: score.to_bits(),
                    },
                    decision,
                })
                .collect(),
            unused_label_item_ids: Vec::new(),
        }
    }

    #[test]
    fn fixed_bins_use_train_integer_counts_and_keep_unknown_without_support_or_judgment() {
        let train = summary(
            "mechanical-train",
            "train-origin",
            &[
                (0.0, Positive),
                (0.25, Positive),
                (0.5, Negative),
                (1.0, Negative),
                (1.0, Uncertain),
            ],
        );
        let edges = [0.0f32.to_bits(), 0.5f32.to_bits(), 1.0f32.to_bits()];
        let bins = fit_bins(std::slice::from_ref(&train), &edges, 2);
        assert_eq!(
            bins,
            [
                Bin {
                    positive: 2,
                    negative: 0,
                    probability_bits: Some(0.75f32.to_bits())
                },
                Bin {
                    positive: 0,
                    negative: 2,
                    probability_bits: Some(0.25f32.to_bits())
                }
            ]
        );
        assert_eq!(bin_index(0.5f32.to_bits(), &edges), Some(1));
        assert_eq!(bin_index(1.0f32.to_bits(), &edges), Some(1));
        assert_eq!(bin_index((-1.0f32).to_bits(), &edges), None);
        assert_eq!(bin_index(f32::NAN.to_bits(), &edges), None);
        let evaluation = summary(
            "mechanical-evaluation",
            "evaluation-origin",
            &[(0.25, Negative), (0.75, Positive), (1.0, Uncertain)],
        );
        let actual = estimates(&evaluation.candidates, &bins, &edges);
        assert_eq!(
            actual
                .iter()
                .map(|v| v.probability_bits)
                .collect::<Vec<_>>(),
            [Some(0.75f32.to_bits()), Some(0.25f32.to_bits()), None]
        );
        assert!(
            fit_bins(std::slice::from_ref(&train), &edges, 3)
                .iter()
                .all(|v| v.probability_bits.is_none())
        );
        assert_eq!(bins, fit_bins(std::slice::from_ref(&train), &edges, 2));
        let training = Training {
            scope: Scope::ExperimentalAnchorCalibration,
            method: Method::FixedBinBeta11,
            score_definition: ScoreDefinition::OriginalHfcNormalizedStrengthF32Bits,
            profile: PROFILE.into(),
            channel: 0,
            edge_bits: edges.to_vec(),
            min_bin_support: 2,
            datasets: vec![train],
            bins,
            policies: vec![Policy {
                min_confidence: 0.5,
                min_gap_frames: 1,
                density_window_frames: 100,
                max_anchors_per_window: 1,
            }],
            policy_counts: vec![Counts::default()],
        };
        let content_digest = digest(&training).unwrap();
        let mut choice = Choice {
            schema_version: 1,
            input_blake3: "frozen-input-raw".into(),
            train_result_blake3: content_digest.clone(),
            policy_index: 0,
        };
        assert!(check_choice("frozen-input-raw", &training, &choice).is_ok());
        choice.policy_index = 1;
        assert!(check_choice("frozen-input-raw", &training, &choice).is_err());
        choice.policy_index = 0;
        choice.input_blake3.push('x');
        assert!(check_choice("frozen-input-raw", &training, &choice).is_err());
        let receipt = TrainReceipt {
            schema_version: 1,
            input_blake3: "frozen-input-raw".into(),
            train_result_blake3: content_digest.clone(),
            training,
            quality_status: "UNASSESSED",
            production_admission: false,
        };
        let mut raw_receipt = serde_json::to_vec_pretty(&receipt).unwrap();
        raw_receipt.push(b'\n');
        assert_ne!(blake3::hash(&raw_receipt).to_string(), content_digest);
        choice.input_blake3 = "frozen-input-raw".into();
        choice.train_result_blake3 = blake3::hash(&raw_receipt).to_string();
        assert!(check_choice("frozen-input-raw", &receipt.training, &choice).is_err());
    }

    #[test]
    fn complete_mapping_and_split_identity_fail_closed_and_compilation_preserves_original_none() {
        let train = summary(
            "mechanical-train",
            "train-origin",
            &[(0.25, Positive), (0.25, Negative)],
        );
        let evaluation = summary(
            "mechanical-evaluation",
            "evaluation-origin",
            &[(0.25, Positive), (0.25, Negative)],
        );
        assert!(
            check_splits(
                std::slice::from_ref(&train),
                std::slice::from_ref(&evaluation)
            )
            .is_ok()
        );
        let mut leaked = evaluation.clone();
        leaked.binding.source.audio_blake3 = train.binding.source.audio_blake3.clone();
        assert!(check_splits(std::slice::from_ref(&train), &[leaked]).is_err());
        let mut leaked = evaluation.clone();
        leaked.origin_group_id = train.origin_group_id.clone();
        assert!(check_splits(std::slice::from_ref(&train), &[leaked]).is_err());
        let no_negative = summary(
            "negative-missing",
            "negative-missing-origin",
            &[(0.25, Positive), (0.25, Uncertain)],
        );
        assert!(check_splits(&[train], &[no_negative]).is_err());
        let (root, mut package) = anchors::tests::fixture("calibration-mapping-mechanism");
        for onset in &mut package.analysis.onsets {
            onset.confidence = None;
        }
        let source = labels::Source::from_package(&package);
        let document = labels::Document {
            schema_version: 1,
            source: source.clone(),
            reviewer: "mechanical-control".into(),
            labels: package
                .analysis
                .onsets
                .iter()
                .enumerate()
                .map(|(index, onset)| labels::Label {
                    item_id: index as u64,
                    location: labels::Location::Point {
                        frame: onset.time.frames(),
                    },
                    anchor_decision: if index == 0 {
                        Uncertain
                    } else if index % 2 == 0 {
                        Positive
                    } else {
                        Negative
                    },
                    reason: "Constructed mechanical judgment; not a human music label".into(),
                    playback: labels::Playback::Stereo,
                })
                .collect(),
        };
        let mappings = package
            .analysis
            .onsets
            .iter()
            .enumerate()
            .map(|(index, onset)| Mapping {
                onset_index: index,
                label_item_id: index as u64,
                frame: onset.time.frames(),
                original_score_bits: onset.strength.to_bits(),
            })
            .collect::<Vec<_>>();
        let (candidates, unused_label_item_ids) =
            candidate_rows(&package, &document, &mappings).unwrap();
        assert!(candidate_rows(&package, &document, &mappings[1..]).is_err());
        let mut changed = mappings.clone();
        changed[0].original_score_bits ^= 1;
        assert!(candidate_rows(&package, &document, &changed).is_err());
        let mut changed = mappings.clone();
        changed[0].frame += 1;
        assert!(candidate_rows(&package, &document, &changed).is_err());
        let mut changed = document.clone();
        changed.labels[0].location = labels::Location::Interval {
            start_frame: 0,
            end_frame: 1,
        };
        assert!(candidate_rows(&package, &changed, &mappings).is_err());
        let original = package.analysis.clone();
        let upper = package
            .analysis
            .onsets
            .iter()
            .map(|v| v.strength)
            .fold(0.0f32, f32::max)
            + 1.0;
        let edges = [0.0f32.to_bits(), upper.to_bits()];
        let bins = [Bin {
            positive: 1,
            negative: 1,
            probability_bits: Some(0.5f32.to_bits()),
        }];
        let case = Case {
            package_path: root.join("source"),
            evidence_path: PathBuf::new(),
            labels_path: PathBuf::new(),
            package,
            summary: DatasetSummary {
                binding: Binding {
                    source,
                    analysis_blake3: String::new(),
                    evidence_summary_blake3: String::new(),
                    profile: PROFILE.into(),
                    channel: 0,
                },
                labels_blake3: String::new(),
                origin_group_id: "mechanical-only".into(),
                reviewed: vec![Reviewed {
                    start_frame: 0,
                    end_frame: 4800,
                }],
                candidates,
                unused_label_item_ids,
            },
        };
        let policy = Policy {
            min_confidence: 0.5,
            min_gap_frames: 1,
            density_window_frames: 4800,
            max_anchors_per_window: 100,
        };
        let (_, proposal, estimates) = evaluate_policy(&case, &bins, &edges, policy).unwrap();
        assert_eq!(case.package.analysis, original);
        assert!(
            case.package
                .analysis
                .onsets
                .iter()
                .all(|v| v.confidence.is_none())
        );
        assert_eq!(
            proposal.evidence[0].decision,
            cocobeat_media::AnchorDecision::UnknownConfidence
        );
        assert!(estimates[0].probability_bits.is_none());
        for (original, estimate) in case.package.analysis.onsets.iter().zip(&estimates) {
            assert_eq!(original.time.frames(), estimate.original_frame);
            assert_eq!(original.strength.to_bits(), estimate.original_score_bits);
        }
        assert_eq!(
            proposal,
            evaluate_policy(&case, &bins, &edges, policy).unwrap().1
        );
        let context = Context {
            input_blake3: "input-raw".into(),
            calibration_report_blake3: "report-raw".into(),
            choice_blake3: "choice-raw".into(),
            train_result_blake3: "typed-training-content".into(),
            policy_index: 0,
        };
        let report =
            anchors::make_calibrated_report(&case.package, &proposal, &estimates, policy, &context)
                .unwrap();
        let bytes = serde_json::to_vec(&report).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["report_version"], 2);
        assert_eq!(value["quality_status"], "UNASSESSED");
        assert_eq!(value["production_admission"], false);
        assert!(
            value["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v["confidence"].is_null())
        );
        assert!(value["evidence"][0]["calibrated_estimate"].is_null());
        assert_eq!(
            value["evidence"][1]["calibrated_estimate"]["probability_bits"],
            0.5f32.to_bits()
        );
        assert_eq!(
            value["evidence"][1]["calibrated_estimate"]["original_score_bits"],
            estimates[1].original_score_bits
        );
        // The existing workbench and no-context v1 loader reject v2 rather than treating it as legacy confidence
        assert!(serde_json::from_slice::<anchors::Report>(&bytes).is_err());
        let hash = blake3::hash(&bytes).to_string();
        let selected = proposal
            .evidence
            .iter()
            .position(|v| matches!(v.decision, cocobeat_media::AnchorDecision::Selected { .. }))
            .unwrap();
        let mut selection = anchors::CalibratedSelection {
            schema_version: 2,
            source_content_id: labels::Source::from_package(&case.package).content_id,
            proposal_blake3: hash.clone(),
            calibration: context.clone(),
            onset_indices: vec![selected],
        };
        assert_eq!(
            anchors::select_calibrated(&report, &hash, &selection)
                .unwrap()
                .len(),
            1
        );
        selection.onset_indices = vec![0];
        assert!(anchors::select_calibrated(&report, &hash, &selection).is_err());
        selection.onset_indices = vec![selected, selected];
        assert!(anchors::select_calibrated(&report, &hash, &selection).is_err());
        selection.onset_indices.clear();
        assert!(
            anchors::select_calibrated(&report, &hash, &selection)
                .unwrap()
                .is_empty()
        );
        selection.calibration.choice_blake3.push('x');
        assert!(anchors::select_calibrated(&report, &hash, &selection).is_err());
        selection.calibration = context;
        assert!(anchors::select_calibrated(&report, "different-proposal-raw", &selection).is_err());
        let mut wrong_estimates = estimates.clone();
        wrong_estimates[1].original_score_bits ^= 1;
        assert!(
            anchors::make_calibrated_report(
                &case.package,
                &proposal,
                &wrong_estimates,
                policy,
                &selection.calibration
            )
            .is_err()
        );
        let mut missing_context = value.clone();
        missing_context
            .as_object_mut()
            .unwrap()
            .remove("calibration");
        assert!(serde_json::from_value::<anchors::CalibratedReport>(missing_context).is_err());
        let mut changed_report = value;
        changed_report["calibration"]["choice_blake3"] = serde_json::json!("tampered-choice");
        let changed: anchors::CalibratedReport = serde_json::from_value(changed_report).unwrap();
        assert_ne!(changed, report);
        std::fs::remove_dir_all(root).unwrap();
    }
}
