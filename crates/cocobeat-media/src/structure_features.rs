//! Bounded canonical descriptors and explicitly compiled candidate sections

use crate::{ValidatedPackage, read_package};
use cocobeat_schema::{CANONICAL_SAMPLE_RATE, MAX_CANONICAL_FRAMES};
use oximedia_audio::spectrum::fft::{FftProcessor, WindowFunction};
use serde::Serialize;
use std::path::Path;

const FFT_FRAMES: usize = 1024;
const WINDOWS_PER_BIN: usize = 24;
const BIN_FRAMES: u64 = (FFT_FRAMES * WINDOWS_PER_BIN) as u64;
const MAX_BINS: usize = MAX_CANONICAL_FRAMES.div_ceil(BIN_FRAMES) as usize;
const BAND_EDGES: [usize; 9] = [0, 4, 8, 16, 32, 64, 128, 256, 513];
const NEIGHBORS: usize = 4;

#[derive(Debug, Serialize)]
pub struct StructureFeatureEvidence {
    pub profile: &'static str,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel: usize,
    pub canonical_frames: u64,
    pub fft_frames: usize,
    pub windows_per_bin: usize,
    pub bin_frames: u64,
    pub band_edges: [usize; 9],
    pub window: &'static str,
    pub coordinate: &'static str,
    pub power_convention: &'static str,
    pub padded_frames: u64,
    pub complete_windows: u64,
    pub partial_tail_frames: u64,
    pub beat_unit: Option<&'static str>,
    pub meter: Option<&'static str>,
    pub confidence: Option<f32>,
    pub quality_status: &'static str,
    pub production_admission: bool,
    pub bins: Vec<StructureFeatureBin>,
    pub adjacent: Vec<StructureAdjacentChange>,
}

#[derive(Debug, Serialize)]
pub struct StructureFeatureBin {
    pub index: usize,
    pub start_frame: u64,
    pub end_frame: u64,
    pub rms: f64,
    pub peak: f64,
    pub spectral_frames: u64,
    pub partial_tail_frames: u64,
    pub spectrum_status: &'static str,
    pub mean_band_power: Option<[f64; 8]>,
    pub log_band_power: Option<[f64; 8]>,
    pub neighbors: Vec<StructureNeighbor>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StructureNeighbor {
    pub index: usize,
    pub start_frame: u64,
    pub end_frame: u64,
    pub raw_cosine: f64,
}

#[derive(Debug, Serialize)]
pub struct StructureAdjacentChange {
    pub left_index: usize,
    pub right_index: usize,
    pub raw_cosine: Option<f64>,
    pub descriptor_distance: Option<f64>,
}

/// All features remain provisional until the same owned package snapshot passes validation
pub fn inspect_structure_features_package(
    root: impl AsRef<Path>,
    channel: usize,
) -> Result<(ValidatedPackage, StructureFeatureEvidence), String> {
    read_structure_features_package(root, channel, |_| Ok(()))
}

/// Shares the same provisional PCM with consumers such as waveform and stereo audition
/// Consumers must discard their accumulated state if this call returns an error
pub fn read_structure_features_package(
    root: impl AsRef<Path>,
    channel: usize,
    mut consume: impl FnMut(&[[f32; 2]]) -> Result<(), String>,
) -> Result<(ValidatedPackage, StructureFeatureEvidence), String> {
    let mut features = Features::new(channel)?;
    let package = read_package(root, |frames| {
        features.push(frames)?;
        consume(frames)
    })?;
    if features.consumed != package.manifest.canonical_frames {
        return Err("Structure features differ from the validated canonical extent".into());
    }
    Ok((package, features.finish()?))
}

pub const STRUCTURE_SEGMENTATION_PROFILE: &str = "canonical-logbands-4x4-novelty-v1-candidate";
pub const REPETITION_CANDIDATE_PROFILE: &str = "canonical-logbands-diagonal-8bin-v1-candidate";
const CONTEXT_BINS: usize = 4;
const MIN_SECTION_FRAMES: u64 = 8 * BIN_FRAMES;
const MAX_BOUNDARIES: usize = 64;
const MIN_REPETITION_BINS: usize = 8;
const MAX_REPETITION_LAGS: usize = 64;
const MAX_REPETITIONS: usize = 64;
const MIN_REPETITION_COSINE: f64 = 0.95;
const MIN_REPETITION_CHANGE: f64 = 0.05;

/// Publishes candidate sections and matching cues from the same staged canonical audio
/// No supported boundary is an error; authored sections are never a fallback
pub fn compile_structure_candidate_package(
    source: impl AsRef<Path>,
    channel: usize,
    destination: impl AsRef<Path>,
) -> Result<ValidatedPackage, String> {
    compile_structure_features_candidate_package(
        source.as_ref(),
        channel,
        destination.as_ref(),
        |original, evidence| {
            let boundaries = section_boundaries(&evidence.bins, evidence.canonical_frames)?;
            section_content(original, channel, &boundaries)
        },
    )
}

/// Publishes fixed-grid spectral repetition candidates without altering chart or sections
pub fn compile_repetition_candidate_package(
    source: impl AsRef<Path>,
    channel: usize,
    destination: impl AsRef<Path>,
) -> Result<ValidatedPackage, String> {
    compile_structure_features_candidate_package(
        source.as_ref(),
        channel,
        destination.as_ref(),
        |original, evidence| {
            let candidates = repetition_candidates(&evidence.bins, evidence.canonical_frames)?;
            repetition_content(original, channel, &candidates)
        },
    )
}

fn compile_structure_features_candidate_package(
    source: &Path,
    channel: usize,
    destination: &Path,
    build_content: impl FnOnce(
        &ValidatedPackage,
        &StructureFeatureEvidence,
    ) -> Result<crate::PackageBuildInput, String>,
) -> Result<ValidatedPackage, String> {
    if channel > 1 {
        return Err(
            "Structure segmentation requires explicit left (0) or right (1) channel".into(),
        );
    }
    let original = crate::validate_package(source)?;
    if original.analysis.schema_version != cocobeat_schema::ANALYSIS_SCHEMA_VERSION
        || original.analysis.capabilities.is_none()
    {
        return Err(
            "Structure segmentation requires analysis v2 with original capabilities".into(),
        );
    }
    let source_root = std::fs::canonicalize(source).map_err(|e| e.to_string())?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
    if parent.starts_with(&source_root) {
        return Err("Structure candidate output must be outside the source package".into());
    }
    let destination = parent.join(
        destination
            .file_name()
            .ok_or("Structure output must name a new directory")?,
    );
    crate::build_package(
        source.join(&original.manifest.audio.file_name),
        original.manifest.canonical_frames,
        destination,
        |staged, prepared| {
            if prepared.asset != original.manifest.audio {
                return Err(
                    "Staged structure audio differs from the validated source asset".into(),
                );
            }
            let mut features = Features::new(channel)?;
            crate::decode_canonical(staged, original.manifest.canonical_frames, |frames| {
                features.push(frames)
            })?;
            let evidence = features.finish()?;
            let content = build_content(&original, &evidence)?;
            if crate::validate_package(source)? != original {
                return Err("Structure source package changed after initial validation".into());
            }
            Ok(content)
        },
    )
}

#[derive(Clone, Debug, PartialEq)]
struct RepetitionMatch {
    source: usize,
    target: usize,
    length: usize,
    min_cosine: f64,
    max_rms_ratio: f64,
    changes: usize,
}

#[derive(Debug, PartialEq)]
struct RepetitionCandidates {
    seed_lag_count: usize,
    selected_lags: Vec<usize>,
    matches: Vec<RepetitionMatch>,
}

fn repetition_bin_supported(bin: &StructureFeatureBin) -> bool {
    bin.spectral_frames == BIN_FRAMES
        && bin.end_frame - bin.start_frame == BIN_FRAMES
        && bin.partial_tail_frames == 0
        && bin.rms > 0.0
        && cosine(bin.log_band_power, bin.log_band_power).is_some_and(f64::is_finite)
}

fn repetition_candidates(
    bins: &[StructureFeatureBin],
    frames: u64,
) -> Result<RepetitionCandidates, String> {
    if frames == 0 || frames > MAX_CANONICAL_FRAMES || bins.len() > MAX_BINS {
        return Err("Repetition input exceeds the canonical extent limit".into());
    }
    if bins.len() != frames.div_ceil(BIN_FRAMES) as usize {
        return Err("Repetition bins differ from the canonical extent".into());
    }
    for (index, bin) in bins.iter().enumerate() {
        if bin.index != index
            || bin.start_frame != index as u64 * BIN_FRAMES
            || bin.end_frame != ((index + 1) as u64 * BIN_FRAMES).min(frames)
            || !bin.rms.is_finite()
            || bin.rms < 0.0
            || bin
                .log_band_power
                .is_some_and(|v| v.iter().any(|x| !x.is_finite() || *x < 0.0))
        {
            return Err("Invalid repetition bin coordinate or descriptor".into());
        }
    }
    let full = (frames / BIN_FRAMES) as usize;
    if full < 2 * MIN_REPETITION_BINS {
        return Err("Repetition requires two complete eight-bin intervals".into());
    }
    let mut seeds = vec![(0usize, 0.0f64); full];
    for (i, bin) in bins[..full].iter().enumerate() {
        if !repetition_bin_supported(bin) {
            continue;
        }
        for neighbor in &bin.neighbors {
            let j = neighbor.index;
            if j >= bins.len() || !neighbor.raw_cosine.is_finite() {
                return Err("Invalid repetition neighbor seed".into());
            }
            if j < full
                && j >= i + MIN_REPETITION_BINS
                && neighbor.raw_cosine >= MIN_REPETITION_COSINE
                && repetition_bin_supported(&bins[j])
            {
                let seed = &mut seeds[j - i];
                seed.0 += 1;
                seed.1 = seed.1.max(neighbor.raw_cosine);
            }
        }
    }
    let mut lags: Vec<_> = seeds
        .iter()
        .enumerate()
        .filter(|(_, v)| v.0 > 0)
        .map(|(lag, _)| lag)
        .collect();
    let seed_lag_count = lags.len();
    lags.sort_unstable_by(|a, b| {
        seeds[*b]
            .0
            .cmp(&seeds[*a].0)
            .then(seeds[*b].1.total_cmp(&seeds[*a].1))
            .then(a.cmp(b))
    });
    lags.truncate(MAX_REPETITION_LAGS);
    let mut matches = Vec::new();
    // ponytail: at most64 lags *1172 bins; top4 seeds trade recall for bounded work
    for &lag in &lags {
        let mut run: Option<RepetitionMatch> = None;
        for i in 0..full - lag {
            let j = i + lag;
            let score = cosine(bins[i].log_band_power, bins[j].log_band_power);
            let ratio = (bins[i].rms / bins[j].rms).max(bins[j].rms / bins[i].rms);
            let supported = repetition_bin_supported(&bins[i])
                && repetition_bin_supported(&bins[j])
                && score.is_some_and(|v| v.is_finite() && v >= MIN_REPETITION_COSINE)
                && ratio <= 2.0;
            if !supported {
                if let Some(candidate) = run.take() {
                    accept_repetition(&mut matches, candidate)?;
                }
                continue;
            }
            let candidate = run.get_or_insert(RepetitionMatch {
                source: i,
                target: j,
                length: 0,
                min_cosine: score.unwrap(),
                max_rms_ratio: 1.0,
                changes: 0,
            });
            if candidate.length > 0 {
                let source_change =
                    1.0 - cosine(bins[i - 1].log_band_power, bins[i].log_band_power).unwrap();
                let target_change =
                    1.0 - cosine(bins[j - 1].log_band_power, bins[j].log_band_power).unwrap();
                if source_change >= MIN_REPETITION_CHANGE && target_change >= MIN_REPETITION_CHANGE
                {
                    candidate.changes += 1;
                }
            }
            candidate.length += 1;
            candidate.min_cosine = candidate.min_cosine.min(score.unwrap());
            candidate.max_rms_ratio = candidate.max_rms_ratio.max(ratio);
            if candidate.length == lag {
                accept_repetition(&mut matches, run.take().unwrap())?;
            }
        }
        if let Some(candidate) = run {
            accept_repetition(&mut matches, candidate)?;
        }
    }
    if matches.is_empty() {
        return Err("Repetition found no supported nonstatic interval pair".into());
    }
    matches.sort_unstable_by_key(|v| (v.source, v.target));
    matches.dedup_by_key(|v| (v.source, v.target));
    Ok(RepetitionCandidates {
        seed_lag_count,
        selected_lags: lags,
        matches,
    })
}

fn accept_repetition(
    matches: &mut Vec<RepetitionMatch>,
    candidate: RepetitionMatch,
) -> Result<(), String> {
    if candidate.length >= MIN_REPETITION_BINS && candidate.changes >= 2 {
        if candidate.source + candidate.length > candidate.target {
            return Err("Repetition candidate intervals overlap".into());
        }
        if matches.len() >= MAX_REPETITIONS {
            return Err("Repetition exceeds 64 supported relations".into());
        }
        matches.push(candidate);
    }
    Ok(())
}

fn repetition_content(
    original: &ValidatedPackage,
    channel: usize,
    candidates: &RepetitionCandidates,
) -> Result<crate::PackageBuildInput, String> {
    use cocobeat_schema::{
        AnalysisCapability, AnalysisSource, AnalysisState, RepetitionFeature, SongTime,
    };
    let mut analysis = original.analysis.clone();
    let capabilities = analysis
        .capabilities
        .as_mut()
        .ok_or("Repetition requires analysis v2 capabilities")?;
    let old = capabilities.repetition;
    capabilities.repetition = AnalysisCapability {
        state: AnalysisState::Candidate,
        source: AnalysisSource::Algorithm,
        confidence: None,
    };
    analysis.repetitions = candidates
        .matches
        .iter()
        .map(|v| RepetitionFeature {
            source_start: SongTime::from_frames((v.source as u64 * BIN_FRAMES) as i64),
            source_end: SongTime::from_frames(((v.source + v.length) as u64 * BIN_FRAMES) as i64),
            target_start: SongTime::from_frames((v.target as u64 * BIN_FRAMES) as i64),
            target_end: SongTime::from_frames(((v.target + v.length) as u64 * BIN_FRAMES) as i64),
            confidence: None,
        })
        .collect();
    analysis.diagnostics = serde_json::to_string(&serde_json::json!({
        "profile": REPETITION_CANDIDATE_PROFILE,
        "source_content_id": blake3::Hash::from_bytes(original.manifest.package_hash).to_hex().as_str(),
        "audio_blake3": blake3::Hash::from_bytes(original.manifest.audio.blake3).to_hex().as_str(),
        "canonical_frames": original.manifest.canonical_frames,
        "channel": channel,
        "bin_frames": BIN_FRAMES,
        "min_bins": MIN_REPETITION_BINS,
        "min_cosine": MIN_REPETITION_COSINE,
        "max_rms_ratio": 2,
        "min_change": MIN_REPETITION_CHANGE,
        "min_changes": 2,
        "max_lags": MAX_REPETITION_LAGS,
        "max_relations": MAX_REPETITIONS,
        "seed_lag_count": candidates.seed_lag_count,
        "selected_lags": candidates.selected_lags,
        "search_scope": "top4_seeded_fixed_grid_original_speed_pitch_not_exhaustive",
        "original_repetition_count": original.analysis.repetitions.len(),
        "original_repetition_capability": { "state": format!("{:?}", old.state), "source": format!("{:?}", old.source), "confidence": old.confidence },
        "original_diagnostics_blake3": blake3::hash(original.analysis.diagnostics.as_bytes()).to_hex().as_str(),
        "support_columns": ["relation_index", "min_cosine", "max_rms_ratio", "aligned_change_count"],
        "support": candidates.matches.iter().enumerate().map(|(i, v)| (i, v.min_cosine, v.max_rms_ratio, v.changes)).collect::<Vec<_>>(),
        "confidence": null,
        "production_admission": false,
    })).map_err(|e| e.to_string())?;
    analysis.validate(original.manifest.canonical_frames)?;
    Ok(crate::PackageBuildInput {
        song_id: original.manifest.song_id.clone(),
        importer_version: original.manifest.importer_version.clone(),
        analysis_version: REPETITION_CANDIDATE_PROFILE.into(),
        chart_version: original.manifest.chart_version.clone(),
        analysis,
        chart: original.chart.clone(),
    })
}

#[derive(Clone, Copy, Debug)]
struct Boundary {
    frame: u64,
    novelty: f64,
}

#[derive(Clone, Copy)]
struct Novelty {
    score: f64,
    sustained: bool,
}

struct Context {
    descriptor: [f64; 8],
    rms: f64,
    sustained: bool,
}

fn context(bins: &[StructureFeatureBin]) -> Result<Option<Context>, String> {
    let mut descriptor = [0.0; 8];
    let mut squares = 0.0;
    let mut min_rms = f64::INFINITY;
    let mut max_rms: f64 = 0.0;
    for bin in bins {
        let Some(log) = bin.log_band_power else {
            return Ok(None);
        };
        if bin.spectral_frames != BIN_FRAMES
            || bin.rms <= 0.0
            || cosine(Some(log), Some(log)).is_none()
        {
            return Ok(None);
        }
        if !bin.rms.is_finite() || log.iter().any(|v| !v.is_finite()) {
            return Err("Non-finite structure context".into());
        }
        for (sum, value) in descriptor.iter_mut().zip(log) {
            *sum += value;
        }
        squares += bin.rms * bin.rms;
        min_rms = min_rms.min(bin.rms);
        max_rms = max_rms.max(bin.rms);
    }
    for value in &mut descriptor {
        *value /= CONTEXT_BINS as f64;
    }
    let rms = (squares / CONTEXT_BINS as f64).sqrt();
    let mut sustained = max_rms / min_rms <= 2.0;
    for bin in bins {
        let Some(similarity) = cosine(bin.log_band_power, Some(descriptor)) else {
            return Ok(None);
        };
        sustained &= 1.0 - similarity <= 0.15;
    }
    if !rms.is_finite() || descriptor.iter().any(|v| !v.is_finite()) {
        return Err("Non-finite pooled structure context".into());
    }
    Ok(Some(Context {
        descriptor,
        rms,
        sustained,
    }))
}

fn section_boundaries(bins: &[StructureFeatureBin], frames: u64) -> Result<Vec<Boundary>, String> {
    let full = (frames / BIN_FRAMES) as usize;
    if full < 16 {
        return Err(
            "Structure segmentation has insufficient complete context and minimum duration".into(),
        );
    }
    let mut scores = vec![None; full + 1];
    for k in CONTEXT_BINS..=full - CONTEXT_BINS {
        let (Some(left), Some(right)) = (
            context(&bins[k - CONTEXT_BINS..k])?,
            context(&bins[k..k + CONTEXT_BINS])?,
        ) else {
            continue;
        };
        let Some(similarity) = cosine(Some(left.descriptor), Some(right.descriptor)) else {
            continue;
        };
        let energy_change = ((right.rms / left.rms).ln().abs() / 4.0_f64.ln()).min(1.0);
        let score = 0.5 * (1.0 - similarity) + 0.5 * energy_change;
        if !score.is_finite() {
            return Err("Non-finite structure novelty".into());
        }
        scores[k] = Some(Novelty {
            score,
            sustained: left.sustained && right.sustained,
        });
    }
    let mut candidates = Vec::new();
    for k in CONTEXT_BINS + 2..=full - CONTEXT_BINS - 2 {
        let Some(current) = scores[k] else { continue };
        let frame = k as u64 * BIN_FRAMES;
        if !current.sustained
            || current.score < 0.35
            || frame < MIN_SECTION_FRAMES
            || frames - frame < MIN_SECTION_FRAMES
        {
            continue;
        }
        if (1..=2).all(|offset| {
            scores[k - offset].is_some_and(|v| current.score > v.score)
                && scores[k + offset].is_some_and(|v| current.score >= v.score)
        }) {
            candidates.push(Boundary {
                frame,
                novelty: current.score,
            });
        }
    }
    candidates.sort_unstable_by(|a, b| b.novelty.total_cmp(&a.novelty).then(a.frame.cmp(&b.frame)));
    let mut accepted: Vec<Boundary> = Vec::new();
    for candidate in candidates {
        if accepted
            .iter()
            .all(|v| v.frame.abs_diff(candidate.frame) >= MIN_SECTION_FRAMES)
        {
            accepted.push(candidate);
            if accepted.len() > MAX_BOUNDARIES {
                return Err("Structure segmentation exceeds 64 supported boundaries".into());
            }
        }
    }
    if accepted.is_empty() {
        return Err("Structure segmentation found no supported persistent boundary".into());
    }
    accepted.sort_unstable_by_key(|v| v.frame);
    Ok(accepted)
}

fn section_content(
    original: &ValidatedPackage,
    channel: usize,
    boundaries: &[Boundary],
) -> Result<crate::PackageBuildInput, String> {
    use cocobeat_schema::{
        AnalysisCapability, AnalysisSource, AnalysisState, SectionCue, SectionFeature, SongTime,
    };
    let mut analysis = original.analysis.clone();
    let mut chart = original.chart.clone();
    let capabilities = analysis
        .capabilities
        .as_mut()
        .ok_or("Structure segmentation requires original capabilities")?;
    let old = capabilities.sections;
    capabilities.sections = AnalysisCapability {
        state: AnalysisState::Candidate,
        source: AnalysisSource::Algorithm,
        confidence: None,
    };
    analysis.sections.clear();
    chart.sections.clear();
    let mut start = 0;
    for (index, end) in boundaries
        .iter()
        .map(|v| v.frame)
        .chain(std::iter::once(original.manifest.canonical_frames))
        .enumerate()
    {
        let label = format!("structure-candidate-{index:04}");
        analysis.sections.push(SectionFeature {
            start: SongTime::from_frames(start as i64),
            end: SongTime::from_frames(end as i64),
            confidence: None,
            label: label.clone(),
        });
        chart.sections.push(SectionCue {
            id: index as u64,
            time: SongTime::from_frames(start as i64),
            label,
        });
        start = end;
    }
    let old_state = match old.state {
        AnalysisState::NotRun => "not_run",
        AnalysisState::Unsupported => "unsupported",
        AnalysisState::Candidate => "candidate",
        AnalysisState::Validated => "validated",
    };
    let old_source = match old.source {
        AnalysisSource::Algorithm => "algorithm",
        AnalysisSource::Authored => "authored",
        AnalysisSource::Measured => "measured",
    };
    analysis.diagnostics = serde_json::to_string(&serde_json::json!({
        "profile": STRUCTURE_SEGMENTATION_PROFILE,
        "channel": channel,
        "source_content_id": blake3::Hash::from_bytes(original.manifest.package_hash).to_hex().as_str(),
        "audio_blake3": blake3::Hash::from_bytes(original.manifest.audio.blake3).to_hex().as_str(),
        "canonical_frames": original.manifest.canonical_frames,
        "original_analysis_version": original.manifest.analysis_version,
        "original_sections": original.analysis.sections.len(),
        "original_cues": original.chart.sections.len(),
        "original_sections_capability": { "state": old_state, "source": old_source, "confidence": old.confidence },
        "original_diagnostics_blake3": blake3::hash(original.analysis.diagnostics.as_bytes()).to_hex().as_str(),
        "boundaries_frame_novelty": boundaries.iter().map(|v| (v.frame, v.novelty)).collect::<Vec<_>>(),
        "confidence": null,
        "production_admission": false,
    })).map_err(|e| e.to_string())?;
    if analysis.diagnostics.len() > cocobeat_schema::MAX_CONTENT_DIAGNOSTICS_BYTES {
        return Err("Structure diagnostics exceed the schema byte limit".into());
    }
    Ok(crate::PackageBuildInput {
        song_id: original.manifest.song_id.clone(),
        importer_version: original.manifest.importer_version.clone(),
        analysis_version: STRUCTURE_SEGMENTATION_PROFILE.into(),
        chart_version: "structure-candidate-cues-v1".into(),
        analysis,
        chart,
    })
}

struct Features {
    channel: usize,
    fft: FftProcessor,
    tail: [f64; FFT_FRAMES],
    filled: usize,
    consumed: u64,
    bin_samples: u64,
    square_sum: f64,
    peak: f64,
    band_sum: [f64; 8],
    windows: u64,
    bins: Vec<StructureFeatureBin>,
}

impl Features {
    fn new(channel: usize) -> Result<Self, String> {
        if channel > 1 {
            return Err("Structure features require explicit left (0) or right (1) channel".into());
        }
        let mut fft = FftProcessor::new(FFT_FRAMES, WindowFunction::Rectangle);
        let mut impulse = [0.0; FFT_FRAMES];
        impulse[0] = 1.0;
        let probe = fft.magnitude_spectrum(&impulse);
        if probe.len() != FFT_FRAMES
            || probe
                .iter()
                .any(|v| !v.is_finite() || (v - 1.0).abs() > 1e-9)
        {
            return Err("Structure FFT forward impulse / scale check failed".into());
        }
        Ok(Self {
            channel,
            fft,
            tail: [0.0; FFT_FRAMES],
            filled: 0,
            consumed: 0,
            bin_samples: 0,
            square_sum: 0.0,
            peak: 0.0,
            band_sum: [0.0; 8],
            windows: 0,
            bins: Vec::new(),
        })
    }

    fn push(&mut self, frames: &[[f32; 2]]) -> Result<(), String> {
        if frames.len() as u64 > MAX_CANONICAL_FRAMES - self.consumed {
            return Err("Structure PCM exceeds the canonical frame limit".into());
        }
        for frame in frames {
            let sample = f64::from(frame[self.channel]);
            if !sample.is_finite() {
                return Err("Structure PCM contains a non-finite selected sample".into());
            }
            self.tail[self.filled] = sample;
            self.filled += 1;
            self.consumed += 1;
            self.bin_samples += 1;
            self.square_sum += sample * sample;
            self.peak = self.peak.max(sample.abs());
            if self.filled == FFT_FRAMES {
                let power = band_power(&mut self.fft, &self.tail)?;
                for (sum, value) in self.band_sum.iter_mut().zip(power) {
                    *sum += value;
                }
                self.windows += 1;
                self.filled = 0;
            }
            if self.bin_samples == BIN_FRAMES {
                self.finish_bin()?;
            }
        }
        Ok(())
    }

    fn finish_bin(&mut self) -> Result<(), String> {
        if self.bins.len() >= MAX_BINS {
            return Err("Structure feature count exceeds the canonical limit".into());
        }
        let mean = (self.windows > 0).then(|| self.band_sum.map(|v| v / self.windows as f64));
        let log = mean.map(|v| v.map(f64::ln_1p));
        self.bins
            .try_reserve(1)
            .map_err(|_| "Cannot reserve bounded structure bins")?;
        self.bins.push(StructureFeatureBin {
            index: self.bins.len(),
            start_frame: self.consumed - self.bin_samples,
            end_frame: self.consumed,
            rms: (self.square_sum / self.bin_samples as f64).sqrt(),
            peak: self.peak,
            spectral_frames: self.windows * FFT_FRAMES as u64,
            partial_tail_frames: self.bin_samples - self.windows * FFT_FRAMES as u64,
            spectrum_status: if self.windows == 0 {
                "INSUFFICIENT_FULL_WINDOW"
            } else if self.band_sum.iter().all(|v| *v == 0.0) {
                "EXACT_ZERO_SPECTRUM"
            } else {
                "MEASURED"
            },
            mean_band_power: mean,
            log_band_power: log,
            neighbors: Vec::new(),
        });
        self.bin_samples = 0;
        self.square_sum = 0.0;
        self.peak = 0.0;
        self.band_sum = [0.0; 8];
        self.windows = 0;
        Ok(())
    }

    fn finish(mut self) -> Result<StructureFeatureEvidence, String> {
        if self.consumed == 0 {
            return Err("Structure features require nonempty canonical PCM".into());
        }
        if self.bin_samples > 0 {
            // The actual partial FFT tail contributes energy only, never a padded spectrum
            self.finish_bin()?;
        }
        let adjacent = relations(&mut self.bins)?;
        Ok(StructureFeatureEvidence {
            profile: "canonical-spectrum-1024x24-v1-candidate",
            sample_rate: CANONICAL_SAMPLE_RATE,
            channels: 2,
            channel: self.channel,
            canonical_frames: self.consumed,
            fft_frames: FFT_FRAMES,
            windows_per_bin: WINDOWS_PER_BIN,
            bin_frames: BIN_FRAMES,
            band_edges: BAND_EDGES,
            window: "Rectangle_nonoverlapping_actual_1024_frames",
            coordinate: "canonical_pcm_half_open_frames",
            power_convention: "mean_window_sum_unscaled_magnitude_squared_then_log1p_no_doubling",
            padded_frames: 0,
            complete_windows: self.consumed / FFT_FRAMES as u64,
            partial_tail_frames: self.consumed % FFT_FRAMES as u64,
            beat_unit: None,
            meter: None,
            confidence: None,
            quality_status: "UNASSESSED",
            production_admission: false,
            bins: self.bins,
            adjacent,
        })
    }
}

fn band_power(fft: &mut FftProcessor, window: &[f64; FFT_FRAMES]) -> Result<[f64; 8], String> {
    let spectrum = fft.magnitude_spectrum(window);
    if spectrum.len() != FFT_FRAMES || spectrum.iter().any(|v| !v.is_finite()) {
        return Err("Structure FFT produced invalid magnitudes".into());
    }
    if window.iter().any(|v| *v != 0.0) && spectrum.iter().all(|v| *v == 0.0) {
        return Err("Structure FFT produced all zeros for nonzero PCM".into());
    }
    Ok(std::array::from_fn(|band| {
        spectrum[BAND_EDGES[band]..BAND_EDGES[band + 1]]
            .iter()
            .map(|v| v * v)
            .sum()
    }))
}

fn cosine(left: Option<[f64; 8]>, right: Option<[f64; 8]>) -> Option<f64> {
    let (left, right) = (left?, right?);
    let dot: f64 = left.iter().zip(right).map(|(a, b)| a * b).sum();
    let left_norm: f64 = left.iter().map(|v| v * v).sum();
    let right_norm: f64 = right.iter().map(|v| v * v).sum();
    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }
    Some(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

fn relations(bins: &mut [StructureFeatureBin]) -> Result<Vec<StructureAdjacentChange>, String> {
    let mut adjacent = Vec::new();
    adjacent
        .try_reserve_exact(bins.len().saturating_sub(1))
        .map_err(|_| "Cannot reserve bounded structure changes")?;
    for pair in bins.windows(2) {
        let score = cosine(pair[0].log_band_power, pair[1].log_band_power);
        adjacent.push(StructureAdjacentChange {
            left_index: pair[0].index,
            right_index: pair[1].index,
            raw_cosine: score,
            descriptor_distance: score.map(|v| 1.0 - v),
        });
    }
    let mut row = Vec::new();
    row.try_reserve_exact(bins.len().saturating_sub(1))
        .map_err(|_| "Cannot reserve bounded structure neighbor row")?;
    // ponytail: at most1172 bins, one bounded row; add an index only if this ceiling changes
    for i in 0..bins.len() {
        row.clear();
        for (j, candidate) in bins.iter().enumerate() {
            if i != j
                && let Some(raw_cosine) = cosine(bins[i].log_band_power, candidate.log_band_power)
            {
                row.push(StructureNeighbor {
                    index: j,
                    start_frame: candidate.start_frame,
                    end_frame: candidate.end_frame,
                    raw_cosine,
                });
            }
        }
        row.sort_unstable_by(|a, b| {
            b.raw_cosine
                .total_cmp(&a.raw_cosine)
                .then(a.index.cmp(&b.index))
        });
        bins[i]
            .neighbors
            .try_reserve_exact(row.len().min(NEIGHBORS))
            .map_err(|_| "Cannot reserve bounded structure neighbors")?;
        bins[i]
            .neighbors
            .extend(row.iter().take(NEIGHBORS).cloned());
    }
    Ok(adjacent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn inspect(frames: &[[f32; 2]], channel: usize, chunk: usize) -> StructureFeatureEvidence {
        let mut features = Features::new(channel).unwrap();
        for block in frames.chunks(chunk) {
            features.push(block).unwrap();
        }
        features.finish().unwrap()
    }

    #[test]
    fn actual_tail_energy_and_spectral_support_do_not_pad_or_depend_on_chunks() {
        for n in [1, 1023, 1024, 1025, 24575, 24576, 24577] {
            let mut frames = vec![[0.0; 2]; n];
            frames[0][0] = 1.0;
            frames[n - 1][0] += 2.0;
            for chunk in [1, 137, 1024, 24577] {
                let evidence = inspect(&frames, 0, chunk);
                assert_eq!(evidence.canonical_frames, n as u64);
                assert_eq!(evidence.complete_windows, (n / FFT_FRAMES) as u64);
                assert_eq!(evidence.partial_tail_frames, (n % FFT_FRAMES) as u64);
                assert_eq!(evidence.bins.len(), n.div_ceil(BIN_FRAMES as usize));
                for bin in &evidence.bins {
                    let samples = &frames[bin.start_frame as usize..bin.end_frame as usize];
                    let squares: f64 = samples.iter().map(|v| f64::from(v[0]).powi(2)).sum();
                    let expected_peak = if n == 1 {
                        3.0
                    } else if bin.end_frame == n as u64 {
                        2.0
                    } else {
                        1.0
                    };
                    assert_eq!(bin.peak, expected_peak);
                    assert!((bin.rms - (squares / samples.len() as f64).sqrt()).abs() < 1e-12);
                    assert_eq!(
                        bin.spectral_frames,
                        (samples.len() / FFT_FRAMES * FFT_FRAMES) as u64
                    );
                    assert_eq!(bin.partial_tail_frames, (samples.len() % FFT_FRAMES) as u64);
                    assert_eq!(bin.mean_band_power.is_none(), samples.len() < FFT_FRAMES);
                }
            }
        }
    }

    #[test]
    fn rectangular_fft_band_formula_matches_independent_dft_and_nyquist() {
        let window = std::array::from_fn(|i| if i < 7 { (i as f64 - 3.0) / 8.0 } else { 0.0 });
        let mut fft = FftProcessor::new(FFT_FRAMES, WindowFunction::Rectangle);
        let actual = band_power(&mut fft, &window).unwrap();
        let mut expected = [0.0; 8];
        for k in 0..=FFT_FRAMES / 2 {
            let (mut real, mut imaginary) = (0.0, 0.0);
            for (i, sample) in window[..7].iter().enumerate() {
                let phase = TAU * (i * k) as f64 / FFT_FRAMES as f64;
                real += sample * phase.cos();
                imaginary -= sample * phase.sin();
            }
            let band = BAND_EDGES.partition_point(|v| *v <= k) - 1;
            expected[band] += real * real + imaginary * imaginary;
        }
        for (a, e) in actual.into_iter().zip(expected) {
            assert!((a - e).abs() <= 1e-10 * e.max(1.0));
        }
        let mut unequal_windows = vec![[1.0, 0.0]; FFT_FRAMES];
        unequal_windows.extend(vec![[2.0, 0.0]; FFT_FRAMES]);
        let evidence = inspect(&unequal_windows, 0, 127);
        let expected_mean = 2.5 * (FFT_FRAMES * FFT_FRAMES) as f64;
        assert_eq!(evidence.bins[0].mean_band_power.unwrap()[0], expected_mean);
        assert_eq!(
            evidence.bins[0].log_band_power.unwrap()[0],
            expected_mean.ln_1p()
        );
        let alternating = std::array::from_fn(|i| if i % 2 == 0 { 1.0 } else { -1.0 });
        let powers = band_power(&mut fft, &alternating).unwrap();
        assert_eq!(powers[7], (FFT_FRAMES * FFT_FRAMES) as f64);
        assert!(powers[..7].iter().all(|v| v.abs() < 1e-20));
    }

    #[test]
    fn frequency_not_rms_and_explicit_channels_determine_descriptors() {
        let dc = vec![[1.0, 0.0]; FFT_FRAMES];
        let nyquist: Vec<_> = (0..FFT_FRAMES)
            .map(|i| [if i % 2 == 0 { 1.0 } else { -1.0 }, 0.0])
            .collect();
        let a = inspect(&dc, 0, 127);
        let b = inspect(&nyquist, 0, 127);
        assert_eq!(a.bins[0].rms, 1.0);
        assert_eq!(b.bins[0].rms, 1.0);
        assert_eq!(
            cosine(a.bins[0].log_band_power, b.bins[0].log_band_power),
            Some(0.0)
        );
        let antiphase = vec![[1.0, -1.0]; FFT_FRAMES];
        let right = inspect(&antiphase, 1, 133);
        assert_eq!(right.bins[0].mean_band_power, a.bins[0].mean_band_power);
        let silent = inspect(&dc, 1, 133);
        assert_eq!(silent.bins[0].spectrum_status, "EXACT_ZERO_SPECTRUM");
        assert_eq!(
            cosine(silent.bins[0].log_band_power, silent.bins[0].log_band_power),
            None
        );
    }

    #[test]
    fn neighbors_exclude_self_tie_by_original_index_and_stay_bounded() {
        let evidence = inspect(&vec![[1.0, 0.0]; BIN_FRAMES as usize * 6], 0, 997);
        for bin in &evidence.bins {
            let expected: Vec<_> = (0..6).filter(|i| *i != bin.index).take(4).collect();
            assert_eq!(
                bin.neighbors.iter().map(|v| v.index).collect::<Vec<_>>(),
                expected
            );
            for neighbor in &bin.neighbors {
                assert!((neighbor.raw_cosine - 1.0).abs() < 1e-12);
                assert_eq!(neighbor.start_frame, neighbor.index as u64 * BIN_FRAMES);
                assert_eq!(neighbor.end_frame, (neighbor.index + 1) as u64 * BIN_FRAMES);
            }
        }
        assert_eq!(evidence.adjacent.len(), 5);
        assert!(
            evidence
                .adjacent
                .iter()
                .all(|v| (v.raw_cosine.unwrap() - 1.0).abs() < 1e-12
                    && v.descriptor_distance.unwrap().abs() < 1e-12)
        );
        let short = inspect(&[[1.0, 0.0]], 0, 1);
        assert_eq!(short.bins[0].spectrum_status, "INSUFFICIENT_FULL_WINDOW");
        assert!(short.bins[0].neighbors.is_empty());
    }

    #[test]
    fn shared_callback_compile_failure_and_constructed_section_content_preserve_source() {
        use std::fs;
        let root = std::env::temp_dir().join(format!(
            "cocobeat-structure-shared-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir(&root).unwrap();
        let audio = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/synthetic/media-import/stereo-canonical.ogg");
        let authoring = root.join("authoring.json");
        fs::write(
            &authoring,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "song_id": "shared-structure-callback",
                "ruleset_id": "duo-watermark-v1",
                "source_note": "Constructed software callback control",
                "anchors": [],
                "sections": [],
            }))
            .unwrap(),
        )
        .unwrap();
        let destination = root.join("package");
        let expected_package = crate::build_authored_package(
            &audio,
            4800,
            &authoring,
            &destination,
            "structure-callback-test/v1",
        )
        .unwrap();
        let protected =
            crate::PACKAGE_OBJECT_NAMES.map(|name| fs::read(destination.join(name)).unwrap());
        let mut expected_pcm = Vec::new();
        crate::decode_canonical(&audio, 4800, |frames| {
            expected_pcm.extend_from_slice(frames);
            Ok(())
        })
        .unwrap();
        let mut seen = Vec::new();
        let (package, evidence) = read_structure_features_package(&destination, 1, |frames| {
            seen.extend_from_slice(frames);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            package.manifest.package_hash,
            expected_package.manifest.package_hash
        );
        assert_eq!(seen, expected_pcm);
        assert_eq!(evidence.canonical_frames, 4800);
        assert_eq!(evidence.channel, 1);
        assert_eq!(evidence.complete_windows, 4);
        assert_eq!(evidence.partial_tail_frames, 704);
        let squares: f64 = expected_pcm.iter().map(|v| f64::from(v[1]).powi(2)).sum();
        assert_eq!(evidence.bins[0].rms, (squares / 4800.0).sqrt());
        assert!(evidence.bins[0].rms > 0.0);
        let mut calls = 0;
        let error = read_structure_features_package(&destination, 0, |_| {
            calls += 1;
            Err("consumer-first-block-failure".into())
        })
        .unwrap_err();
        assert_eq!(error, "consumer-first-block-failure");
        assert_eq!(calls, 1);
        assert_eq!(
            protected,
            crate::PACKAGE_OBJECT_NAMES.map(|name| fs::read(destination.join(name)).unwrap())
        );
        let failed = root.join("unsupported-segmentation");
        assert!(compile_structure_candidate_package(&destination, 0, &failed).is_err());
        assert!(!failed.exists());
        assert_eq!(
            protected,
            crate::PACKAGE_OBJECT_NAMES.map(|name| fs::read(destination.join(name)).unwrap())
        );
        assert!(
            compile_structure_candidate_package(&destination, 2, root.join("wrong-channel"))
                .is_err()
        );
        assert!(
            compile_structure_candidate_package(&destination, 0, destination.join("inside"))
                .is_err()
        );
        fs::write(&failed, b"keep existing output").unwrap();
        assert!(compile_structure_candidate_package(&destination, 0, &failed).is_err());
        assert_eq!(fs::read(&failed).unwrap(), b"keep existing output");
        let mut constructed = expected_package;
        constructed.manifest.canonical_frames = 32 * BIN_FRAMES + 1;
        constructed.analysis.energy = vec![cocobeat_schema::EnergySample {
            start: cocobeat_schema::SongTime::ZERO,
            frames: constructed.manifest.canonical_frames as u32,
            rms: [0.25; 2],
            peak: [0.5; 2],
        }];
        constructed.chart.anchors = vec![cocobeat_schema::Anchor {
            id: 77,
            song_time: cocobeat_schema::SongTime::from_frames(123),
        }];
        let content = section_content(
            &constructed,
            1,
            &[Boundary {
                frame: 16 * BIN_FRAMES,
                novelty: 0.5,
            }],
        )
        .unwrap();
        assert_eq!(content.chart.anchors, constructed.chart.anchors);
        assert_eq!(content.chart.ruleset_id, constructed.chart.ruleset_id);
        assert_eq!(content.analysis.energy, constructed.analysis.energy);
        assert_eq!(content.analysis.beats, constructed.analysis.beats);
        assert_eq!(content.analysis.onsets, constructed.analysis.onsets);
        assert_eq!(
            content.analysis.tempo_regions,
            constructed.analysis.tempo_regions
        );
        assert_eq!(
            content.analysis.repetitions,
            constructed.analysis.repetitions
        );
        let mut expected_caps = constructed.analysis.capabilities.unwrap();
        expected_caps.sections = cocobeat_schema::AnalysisCapability {
            state: cocobeat_schema::AnalysisState::Candidate,
            source: cocobeat_schema::AnalysisSource::Algorithm,
            confidence: None,
        };
        assert_eq!(content.analysis.capabilities, Some(expected_caps));
        let sections = &content.analysis.sections;
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].start.frames(), 0);
        assert_eq!(sections[0].end.frames(), (16 * BIN_FRAMES) as i64);
        assert_eq!(sections[1].start, sections[0].end);
        assert_eq!(sections[1].end.frames(), (32 * BIN_FRAMES + 1) as i64);
        for (index, (section, cue)) in sections.iter().zip(&content.chart.sections).enumerate() {
            assert_eq!(cue.id, index as u64);
            assert_eq!(cue.time, section.start);
            assert_eq!(cue.label, format!("structure-candidate-{index:04}"));
            assert_eq!(section.label, cue.label);
            assert_eq!(section.confidence, None);
        }
        content
            .analysis
            .validate(constructed.manifest.canonical_frames)
            .unwrap();
        content
            .chart
            .validate(constructed.manifest.canonical_frames)
            .unwrap();
        let candidates = RepetitionCandidates {
            seed_lag_count: 1,
            selected_lags: vec![8],
            matches: vec![RepetitionMatch {
                source: 0,
                target: 8,
                length: 8,
                min_cosine: 1.0,
                max_rms_ratio: 1.0,
                changes: 7,
            }],
        };
        let repetition = repetition_content(&constructed, 1, &candidates).unwrap();
        let mut expected_analysis = constructed.analysis.clone();
        expected_analysis.capabilities.as_mut().unwrap().repetition =
            cocobeat_schema::AnalysisCapability {
                state: cocobeat_schema::AnalysisState::Candidate,
                source: cocobeat_schema::AnalysisSource::Algorithm,
                confidence: None,
            };
        expected_analysis.repetitions = repetition.analysis.repetitions.clone();
        expected_analysis.diagnostics = repetition.analysis.diagnostics.clone();
        assert_eq!(repetition.analysis, expected_analysis);
        assert_eq!(repetition.chart, constructed.chart);
        assert_eq!(repetition.chart_version, constructed.manifest.chart_version);
        assert_eq!(repetition.analysis.repetitions[0].confidence, None);
        assert_eq!(
            repetition.analysis.repetitions[0].target_end.frames(),
            (16 * BIN_FRAMES) as i64
        );
        let diagnostic: serde_json::Value =
            serde_json::from_str(&repetition.analysis.diagnostics).unwrap();
        assert_eq!(diagnostic["seed_lag_count"], 1);
        assert_eq!(diagnostic["selected_lags"], serde_json::json!([8]));
        assert_eq!(diagnostic["confidence"], serde_json::Value::Null);
        assert_eq!(diagnostic["production_admission"], false);
        fs::remove_dir_all(root).unwrap();
    }

    // Hand-built descriptors are pure algorithm controls, not decoded musical truth
    fn segmentation_bins(kinds: &[(usize, f64)]) -> Vec<StructureFeatureBin> {
        kinds
            .iter()
            .enumerate()
            .map(|(index, &(band, rms))| {
                let mut log = [0.0; 8];
                log[band] = 1.0;
                StructureFeatureBin {
                    index,
                    start_frame: index as u64 * BIN_FRAMES,
                    end_frame: (index + 1) as u64 * BIN_FRAMES,
                    rms,
                    peak: rms,
                    spectral_frames: BIN_FRAMES,
                    partial_tail_frames: 0,
                    spectrum_status: "HAND_BUILT_ALGORITHM_CONTROL",
                    mean_band_power: None,
                    log_band_power: Some(log),
                    neighbors: Vec::new(),
                }
            })
            .collect()
    }

    #[test]
    fn repetition_requires_continuous_varied_spans_and_sorts_relations_by_original_frames() {
        let kinds: Vec<_> = (0..24).map(|i| (i % 8, 0.25)).collect();
        let mut bins = segmentation_bins(&kinds);
        relations(&mut bins).unwrap();
        let candidates = repetition_candidates(&bins, 24 * BIN_FRAMES).unwrap();
        assert_eq!(candidates.seed_lag_count, 2);
        assert_eq!(candidates.selected_lags, vec![8, 16]);
        assert_eq!(
            candidates
                .matches
                .iter()
                .map(|v| (v.source, v.target, v.length))
                .collect::<Vec<_>>(),
            vec![(0, 8, 8), (0, 16, 8), (8, 16, 8)]
        );
        assert_eq!(
            repetition_candidates(&bins, 24 * BIN_FRAMES).unwrap(),
            candidates
        );
        assert!(candidates.matches.iter().all(|v| v.min_cosine == 1.0
            && v.max_rms_ratio == 1.0
            && v.changes == 7
            && v.source + v.length <= v.target));
        let mut broken = segmentation_bins(&(0..16).map(|i| (i % 8, 0.25)).collect::<Vec<_>>());
        broken[12].log_band_power = Some([1.0; 8]);
        relations(&mut broken).unwrap();
        assert!(repetition_candidates(&broken, 16 * BIN_FRAMES).is_err());
        let short = segmentation_bins(&(0..14).map(|i| (i % 7, 0.25)).collect::<Vec<_>>());
        assert!(repetition_candidates(&short, 14 * BIN_FRAMES).is_err());
    }

    #[test]
    fn repetition_constant_silence_partial_tail_and_overlap_do_not_produce_relations() {
        for rms in [0.0, 0.25] {
            let mut bins = segmentation_bins(&vec![(0, rms); 16]);
            for (i, bin) in bins.iter_mut().take(8).enumerate() {
                bin.neighbors.push(StructureNeighbor {
                    index: i + 8,
                    start_frame: (i + 8) as u64 * BIN_FRAMES,
                    end_frame: (i + 9) as u64 * BIN_FRAMES,
                    raw_cosine: 1.0,
                });
            }
            assert!(repetition_candidates(&bins, 16 * BIN_FRAMES).is_err());
        }
        let mut bins = segmentation_bins(&(0..16).map(|i| (i % 8, 0.25)).collect::<Vec<_>>());
        bins[15].end_frame -= 1;
        bins[15].spectral_frames -= FFT_FRAMES as u64;
        bins[15].partial_tail_frames = FFT_FRAMES as u64 - 1;
        relations(&mut bins).unwrap();
        assert!(repetition_candidates(&bins, 16 * BIN_FRAMES - 1).is_err());
        let overlapping = RepetitionMatch {
            source: 0,
            target: 7,
            length: 8,
            min_cosine: 1.0,
            max_rms_ratio: 1.0,
            changes: 7,
        };
        let mut matches = Vec::new();
        assert!(
            accept_repetition(&mut matches, overlapping)
                .unwrap_err()
                .contains("overlap")
        );
        assert!(matches.is_empty());
        for source in 0..64 {
            accept_repetition(
                &mut matches,
                RepetitionMatch {
                    source,
                    target: source + 8,
                    length: 8,
                    min_cosine: 1.0,
                    max_rms_ratio: 1.0,
                    changes: 7,
                },
            )
            .unwrap();
        }
        assert!(
            accept_repetition(
                &mut matches,
                RepetitionMatch {
                    source: 64,
                    target: 72,
                    length: 8,
                    min_cosine: 1.0,
                    max_rms_ratio: 1.0,
                    changes: 7
                }
            )
            .unwrap_err()
            .contains("exceeds 64")
        );
        assert_eq!(matches.len(), 64);
    }

    #[test]
    fn persistent_colour_energy_changes_and_real_tail_follow_independent_oracles() {
        let mut kinds = vec![(0, 0.25); 16];
        kinds.extend(vec![(7, 0.25); 16]);
        let bins = segmentation_bins(&kinds);
        let boundaries = section_boundaries(&bins, 32 * BIN_FRAMES + 1).unwrap();
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].frame, 16 * BIN_FRAMES);
        assert_eq!(boundaries[0].novelty, 0.5);
        let mut kinds = vec![(0, 0.125); 16];
        kinds.extend(vec![(0, 0.5); 16]);
        let energy = section_boundaries(&segmentation_bins(&kinds), 32 * BIN_FRAMES).unwrap();
        assert_eq!(energy.len(), 1);
        assert_eq!(energy[0].frame, 16 * BIN_FRAMES);
        assert_eq!(energy[0].novelty, 0.5);
        let pool = context(&segmentation_bins(&[
            (0, 1.0),
            (0, 1.0),
            (0, 2.0),
            (0, 2.0),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(pool.rms, 2.5_f64.sqrt());
        assert!(pool.sustained);
    }

    #[test]
    fn silence_constant_short_and_unsustained_transients_never_fallback_to_one_section() {
        for kinds in [vec![(0, 0.25); 32], vec![(0, 0.0); 32], vec![(0, 0.25); 8]] {
            assert!(
                section_boundaries(&segmentation_bins(&kinds), kinds.len() as u64 * BIN_FRAMES)
                    .is_err()
            );
        }
        for transient in [(7, 1.0), (0, 2.0)] {
            let mut kinds = vec![(0, 0.25); 32];
            kinds[16] = transient;
            assert!(section_boundaries(&segmentation_bins(&kinds), 32 * BIN_FRAMES).is_err());
        }
    }

    #[test]
    fn equal_scored_close_boundaries_choose_earlier_and_65_boundaries_are_not_truncated() {
        let mut kinds = vec![(0, 0.25); 8];
        kinds.extend(vec![(7, 0.25); 4]);
        kinds.extend(vec![(0, 0.25); 8]);
        let boundaries = section_boundaries(&segmentation_bins(&kinds), 20 * BIN_FRAMES).unwrap();
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].frame, 8 * BIN_FRAMES);
        let kinds: Vec<_> = (0..66)
            .flat_map(|i| std::iter::repeat_n((if i % 2 == 0 { 0 } else { 7 }, 0.25), 8))
            .collect();
        let error = section_boundaries(&segmentation_bins(&kinds), 528 * BIN_FRAMES).unwrap_err();
        assert!(error.contains("exceeds 64"));
    }

    #[test]
    fn invalid_channel_nonfinite_extent_and_empty_pcm_are_rejected() {
        assert!(Features::new(2).is_err());
        assert!(Features::new(0).unwrap().finish().is_err());
        let mut features = Features::new(0).unwrap();
        assert!(features.push(&[[f32::NAN, 0.0]]).is_err());
        assert_eq!(features.consumed, 0);
        features.consumed = MAX_CANONICAL_FRAMES;
        assert!(features.push(&[[0.0; 2]]).is_err());
        assert_eq!(features.consumed, MAX_CANONICAL_FRAMES);
    }
}
