//! Explicit experimental beat/downbeat candidates; unknown confidence and old quality FAIL remain
//! ponytail: synchronous Lab-only CPU work; add measured resource/cancellation gates before game UI use

// Minimal postprocessing adapted from Beat This! (MIT)
// Copyright (c) 2024 Institute of Computational Perception, JKU Linz, Austria
// Original permission and notice: licenses/beat-this/LICENSE

use crate::PreparedCanonicalAudio;
use cocobeat_schema::{
    AnalysisCapability, AnalysisSource, AnalysisState, BeatFeature, MusicAnalysis, SongTime,
};
use ort::{
    ep::CPU,
    session::{Session, builder::GraphOptimizationLevel},
    value::{DynValue, Tensor, TensorElementType, ValueType},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[path = "native_beat_assets.rs"]
mod assets;

pub(crate) const ANALYSIS_VERSION: &str = "native-small0-high22050-f64fma-minimal-v1-candidate";
pub(crate) const MODEL_BLAKE3: [u8; 32] = assets::MODEL_BLAKE3;
// Lab is the sole ORT consumer; rc.13 cannot detect another caller loading a library without committing an environment
static OWN_ORT_ENVIRONMENT: OnceLock<()> = OnceLock::new();
const BORDER: usize = 6;
const CHUNK: usize = 1500;
const STEP: usize = CHUNK - BORDER * 2;

fn error(context: &str, failure: impl std::fmt::Display) -> String {
    format!("{context}: {failure}")
}

fn new_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| error(&format!("Create {}", path.display()), e))
}

pub(super) fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut file = new_file(path)?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|e| error("Write evidence JSON", e))?;
    file.sync_all().map_err(|e| error("Sync evidence JSON", e))
}

fn write_f32(path: &Path, values: impl IntoIterator<Item = f32>) -> Result<(), String> {
    let mut file = BufWriter::new(new_file(path)?);
    for value in values {
        file.write_all(&value.to_le_bytes())
            .map_err(|e| error("Write raw Float32", e))?;
    }
    file.flush().map_err(|e| error("Flush raw Float32", e))?;
    file.get_ref()
        .sync_all()
        .map_err(|e| error("Sync raw Float32", e))
}

fn write_f64(path: &Path, values: &[f64]) -> Result<(), String> {
    let mut file = BufWriter::new(new_file(path)?);
    for value in values {
        file.write_all(&value.to_le_bytes())
            .map_err(|e| error("Write raw Float64", e))?;
    }
    file.flush().map_err(|e| error("Flush raw Float64", e))?;
    file.get_ref()
        .sync_all()
        .map_err(|e| error("Sync raw Float64", e))
}

fn verified_asset(
    path: &Path,
    bytes: u64,
    hash: &[u8; 32],
    evidence: &Path,
    name: &str,
) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| error(&format!("Unsupported artifact {}", path.display()), e))?;
    if !metadata.is_file()
        || metadata.len() != bytes
        || fs::canonicalize(path).map_err(|e| error("Canonical artifact path", e))? != path
    {
        return Err(format!(
            "Unsupported artifact type/size/path: {}",
            path.display()
        ));
    }
    let mut data = Vec::new();
    data.try_reserve_exact(usize::try_from(bytes).map_err(|e| error("Artifact size", e))?)
        .map_err(|e| error("Allocate artifact bytes", e))?;
    File::open(path)
        .map_err(|e| error("Open artifact", e))?
        .take(bytes + 1)
        .read_to_end(&mut data)
        .map_err(|e| error("Read artifact", e))?;
    let actual = blake3::hash(&data);
    write_json(
        &evidence.join(format!("artifact-{name}.json")),
        &serde_json::json!({"path":path,"actual_bytes":data.len(),"expected_bytes":bytes,"actual_blake3":actual.to_hex().to_string(),"expected_blake3":blake3::Hash::from(*hash).to_hex().to_string()}),
    )?;
    if data.len() as u64 != bytes || actual.as_bytes() != hash {
        return Err(format!("Unsupported artifact identity: {}", path.display()));
    }
    Ok(data)
}

fn validate_library_header(data: &[u8]) -> Result<(), String> {
    let machine = if cfg!(target_arch = "x86_64") {
        (62u16, 0x8664u16)
    } else {
        (183u16, 0xaa64u16)
    };
    if cfg!(target_os = "linux") {
        if data.get(..6) != Some(&b"\x7fELF\x02\x01"[..])
            || data.get(18..20).map(|x| u16::from_le_bytes([x[0], x[1]])) != Some(machine.0)
        {
            return Err("Unsupported SDK ELF architecture".into());
        }
    } else {
        let offset = data
            .get(0x3c..0x40)
            .map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize)
            .ok_or("Unsupported SDK PE header")?;
        let end = offset.checked_add(6).ok_or("SDK PE offset overflow")?;
        let pe = data.get(offset..end).ok_or("Unsupported SDK PE offset")?;
        if data.get(..2) != Some(&b"MZ"[..])
            || pe[..4] != *b"PE\0\0"
            || u16::from_le_bytes([pe[4], pe[5]]) != machine.1
        {
            return Err("Unsupported SDK PE architecture".into());
        }
    }
    Ok(())
}

/// One Session per explicit import; paths and expected hashes come from the shipping binary
pub(crate) fn load_session(evidence: &Path) -> Result<Session, String> {
    let executable = std::env::current_exe()
        .map_err(|e| error("Locate executable", e))?
        .canonicalize()
        .map_err(|e| error("Canonical executable", e))?;
    let directory = executable.parent().ok_or("Executable has no parent")?;
    let root = if cfg!(target_os = "linux") {
        directory
            .parent()
            .ok_or("Linux executable must be in the shipping bin directory")?
    } else {
        directory
    };
    let sdk = root.join("lib/onnxruntime");
    let core = sdk.join(if cfg!(target_os = "linux") {
        "libonnxruntime.so.1.30.0"
    } else {
        "onnxruntime.dll"
    });
    let provider = sdk.join(if cfg!(target_os = "linux") {
        "libonnxruntime_providers_shared.so"
    } else {
        "onnxruntime_providers_shared.dll"
    });
    let core_bytes = verified_asset(
        &core,
        assets::SDK_BYTES,
        &assets::SDK_BLAKE3,
        evidence,
        "core",
    )?;
    validate_library_header(&core_bytes)?;
    drop(core_bytes);
    let provider_bytes = verified_asset(
        &provider,
        assets::PROVIDER_BYTES,
        &assets::PROVIDER_BLAKE3,
        evidence,
        "provider",
    )?;
    validate_library_header(&provider_bytes)?;
    drop(provider_bytes);
    let model = verified_asset(
        &root.join("assets/models/beat-this/small0.onnx"),
        assets::MODEL_BYTES,
        &assets::MODEL_BLAKE3,
        evidence,
        "model",
    )?;
    if OWN_ORT_ENVIRONMENT.get().is_none() {
        if !ort::init_from(&core)
            .map_err(|e| error("Unsupported ORT loader", e))?
            .with_telemetry(false)
            .with_execution_providers([CPU::default().with_arena_allocator(true).build()])
            .commit()
        {
            return Err("Unsupported prior ORT environment initialization in this process".into());
        }
        OWN_ORT_ENVIRONMENT.set(()).map_err(
            |_| "Concurrent ORT initialization is unsupported by this synchronous Lab adapter",
        )?;
    }
    let session = Session::builder()
        .map_err(|e| error("ORT Session builder", e))?
        .with_intra_threads(2)
        .map_err(|e| error("ORT intra threads", e))?
        .with_inter_threads(1)
        .map_err(|e| error("ORT inter threads", e))?
        .with_parallel_execution(false)
        .map_err(|e| error("ORT sequential mode", e))?
        .with_inter_op_spinning(false)
        .map_err(|e| error("ORT inter spinning", e))?
        .with_intra_op_spinning(false)
        .map_err(|e| error("ORT intra spinning", e))?
        .with_optimization_level(GraphOptimizationLevel::All)
        .map_err(|e| error("ORT optimization", e))?
        .commit_from_memory(&model)
        .map_err(|e| error("ORT fixed model", e))?;
    write_json(
        &evidence.join("session-metadata.json"),
        &serde_json::json!({"build_info":ort::info(),"inputs":format!("{:?}",session.inputs()),"outputs":format!("{:?}",session.outputs()),"cpu_intra_requested":2,"cpu_inter_requested":1,"telemetry_events":false,"production_admission":false,"ort_consumer_contract":"Lab adapter only; safe rc.13 API cannot detect foreign load-only initialization"}),
    )?;
    let inputs = session.inputs();
    if inputs.len() != 1
        || inputs[0].name() != "spect"
        || inputs[0].dtype().tensor_type() != Some(TensorElementType::Float32)
        || inputs[0].dtype().tensor_shape().map(|s| &s[..]) != Some(&[1, -1, 128][..])
        || session.outputs().len() != 2
    {
        return Err("Unexpected fixed small0 input/output signature".into());
    }
    for name in ["beat", "downbeat"] {
        let output = session
            .outputs()
            .iter()
            .find(|v| v.name() == name)
            .ok_or("Missing fixed small0 named output")?;
        if !matches!(output.dtype(), ValueType::Tensor { ty: TensorElementType::Float32, shape, dimension_symbols } if shape[..] == [-1,-1] && dimension_symbols.iter().map(String::as_str).eq(["Addbeat_dim_0","frames"]))
        {
            return Err(format!("Unexpected fixed small0 output signature: {name}"));
        }
    }
    Ok(session)
}

fn capture_output(value: &DynValue, path: &Path, frames: usize) -> Result<Vec<f32>, String> {
    write_json(
        &path.with_extension("metadata.json"),
        &serde_json::json!({"dtype":format!("{:?}",value.dtype()),"expected_frames":frames}),
    )?;
    if value.dtype().tensor_type() == Some(TensorElementType::Float64) {
        let (_, data) = value
            .try_extract_tensor::<f64>()
            .map_err(|e| error("Extract actual Float64", e))?;
        write_f64(&path.with_extension("f64"), data)?;
        return Err("Actual Float64 model output rejected; raw bytes retained".into());
    }
    let (shape, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|e| error("Extract actual Float32", e))?;
    write_f32(path, data.iter().copied())?;
    if shape[..] != [1, frames as i64]
        || data.len() != frames
        || data.iter().any(|v| !v.is_finite())
    {
        return Err("Actual Float32 output shape/count/finite failed; raw bytes retained".into());
    }
    Ok(data.to_vec())
}

fn chunk_starts(frames: usize) -> Result<Vec<i64>, String> {
    if frames == 0 || frames > 30_001 {
        return Err("Invalid native spect frame count".into());
    }
    let mut starts: Vec<i64> = (-(BORDER as i64)..frames as i64 - BORDER as i64)
        .step_by(STEP)
        .collect();
    if frames > STEP {
        *starts.last_mut().ok_or("Missing chunk starts")? = frames as i64 - (CHUNK - BORDER) as i64;
    }
    Ok(starts)
}

fn chunk_input(spect: &frontend::Spectrogram, start: i64) -> Result<Vec<f32>, String> {
    let left = usize::try_from(start.max(0)).map_err(|e| error("Chunk left", e))?;
    let right = usize::try_from((start + CHUNK as i64).min(spect.frames as i64))
        .map_err(|e| error("Chunk right", e))?;
    let padding_left =
        usize::try_from((-start).max(0)).map_err(|e| error("Chunk left padding", e))?;
    let padding_right =
        usize::try_from((start + CHUNK as i64 - spect.frames as i64).clamp(0, BORDER as i64))
            .map_err(|e| error("Chunk right padding", e))?;
    if right < left || right > spect.frames {
        return Err("Invalid chunk source range".into());
    }
    let frames = padding_left + right - left + padding_right;
    if !(BORDER * 2 + 1..=CHUNK).contains(&frames) {
        return Err("Invalid actual chunk frame count".into());
    }
    let mut values = vec![0.0; frames * 128];
    values[padding_left * 128..(padding_left + right - left) * 128]
        .copy_from_slice(&spect.values[left * 128..right * 128]);
    Ok(values)
}

fn infer(
    session: &mut Session,
    spect: &frontend::Spectrogram,
    evidence: &Path,
) -> Result<[Vec<f32>; 2], String> {
    if spect.values.len()
        != spect
            .frames
            .checked_mul(128)
            .ok_or("Spect length overflow")?
        || spect.values.iter().any(|v| !v.is_finite())
    {
        return Err("Invalid actual native spect shape/finite; raw native-spect retained".into());
    }
    let starts = chunk_starts(spect.frames)?;
    write_json(&evidence.join("chunk-starts.json"), &starts)?;
    let mut chunks = Vec::new();
    for (index, start) in starts.iter().copied().enumerate() {
        let values = chunk_input(spect, start)?;
        let frames = values.len() / 128;
        write_f32(
            &evidence.join(format!("chunk{index:03}-spect.f32")),
            values.iter().copied(),
        )?;
        let tensor = Tensor::<f32>::from_array(([1usize, frames, 128], values.into_boxed_slice()))
            .map_err(|e| error("Create actual spect tensor", e))?;
        let outputs = session
            .run(ort::inputs!["spect" => tensor])
            .map_err(|e| error("ORT actual Run", e))?;
        let mut failures = Vec::new();
        if outputs.len() != 2 {
            failures.push("Actual output count is not two".to_owned());
        }
        let mut pair = [Vec::new(), Vec::new()];
        // Capture both real outputs before deciding whether either one failed
        for (slot, name) in ["beat", "downbeat"].iter().enumerate() {
            let captured = outputs
                .get(*name)
                .ok_or_else(|| format!("Missing actual output {name}"))
                .and_then(|value| {
                    capture_output(
                        value,
                        &evidence.join(format!("chunk{index:03}-{name}.f32")),
                        frames,
                    )
                });
            match captured {
                Ok(data) => pair[slot] = data,
                Err(failure) => failures.push(failure),
            }
        }
        if !failures.is_empty() {
            return Err(failures.join("; "));
        }
        chunks.push((start, pair));
    }
    let mut result = [vec![0.0; spect.frames], vec![0.0; spect.frames]];
    let mut coverage = vec![false; spect.frames];
    for (start, pair) in chunks.iter().rev() {
        let left =
            usize::try_from(start + BORDER as i64).map_err(|e| error("Aggregate start", e))?;
        let count = pair[0]
            .len()
            .checked_sub(BORDER * 2)
            .ok_or("Aggregate border exceeds output")?;
        let end = left.checked_add(count).ok_or("Aggregate range overflow")?;
        if end > spect.frames || pair[1].len() != pair[0].len() {
            return Err("Invalid aggregate output range".into());
        }
        for output in 0..2 {
            result[output][left..end].copy_from_slice(&pair[output][BORDER..BORDER + count]);
        }
        coverage[left..end].fill(true);
    }
    for (index, name) in ["beat", "downbeat"].iter().enumerate() {
        write_f32(
            &evidence.join(format!("aggregate-{name}.f32")),
            result[index].iter().copied(),
        )?;
    }
    let mut mask = new_file(&evidence.join("aggregate-coverage.bin"))?;
    mask.write_all(&coverage.iter().map(|v| u8::from(*v)).collect::<Vec<_>>())
        .map_err(|e| error("Write coverage", e))?;
    mask.sync_all().map_err(|e| error("Sync coverage", e))?;
    if coverage.iter().any(|v| !v) {
        return Err("Aggregate missing frame coverage; actual raw retained".into());
    }
    Ok(result)
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct PeakGroup {
    pub(crate) q: f64,
    pub(crate) score: f32,
    pub(crate) members: Vec<usize>,
}

// Coordinate semantics ported from MIT Beat This minimal/deduplicate_peaks
// Score is a project adapter policy, not an official confidence output
pub(crate) fn peaks(logits: &[f32]) -> Result<Vec<PeakGroup>, String> {
    if logits.iter().any(|v| !v.is_finite()) {
        return Err("Nonfinite aggregate logits".into());
    }
    let mut groups: Vec<PeakGroup> = Vec::new();
    for (q, value) in logits.iter().copied().enumerate() {
        if value <= 0.0
            || logits[q.saturating_sub(3)..(q + 4).min(logits.len())]
                .iter()
                .any(|neighbor| *neighbor > value)
        {
            continue;
        }
        let exp = (-value).exp();
        let score = 1.0 / (1.0 + exp);
        if let Some(group) = groups.last_mut()
            && q as f64 - group.q <= 1.0
        {
            group.members.push(q);
            group.q += (q as f64 - group.q) / group.members.len() as f64;
            group.score = group.score.max(score);
        } else {
            groups.push(PeakGroup {
                q: q as f64,
                score,
                members: vec![q],
            });
        }
    }
    Ok(groups)
}

pub(crate) fn canonical_frame(q: f64, n: u64) -> Result<i64, String> {
    let center = q * 960.0;
    let frame = center.round();
    if !q.is_finite()
        || q < 0.0
        || !center.is_finite()
        || center >= n as f64
        || frame >= n as f64
        || frame > i64::MAX as f64
    {
        return Err(format!(
            "Original q={q}, canonical center={center} lies outside [0,{n})"
        ));
    }
    Ok(frame as i64)
}

// Caller has verified a nonempty, ascending beat list; equal distance chooses the left beat
pub(crate) fn nearest_beat_index(q: f64, beat: &[PeakGroup]) -> usize {
    let right = beat.partition_point(|v| v.q < q);
    if right == 0 {
        0
    } else if right == beat.len() || q - beat[right - 1].q <= beat[right].q - q {
        right - 1
    } else {
        right
    }
}

/// Preserve original coordinates before any bounded schema mapping or nearest alignment
pub(crate) fn minimal(
    logits: &[Vec<f32>; 2],
    n: u64,
    evidence: &Path,
) -> Result<Vec<BeatFeature>, String> {
    if logits[0].len() != logits[1].len() {
        return Err("Aggregate beat/downbeat length mismatch".into());
    }
    let beat = peaks(&logits[0])?;
    let downbeat = peaks(&logits[1])?;
    write_json(
        &evidence.join("minimal-raw-groups.json"),
        &serde_json::json!({"beat":beat,"downbeat":downbeat,"score_policy":"max raw sigmoid(member logit), uncalibrated","confidence":null}),
    )?;
    if !(1..=cocobeat_schema::MAX_CANONICAL_FRAMES).contains(&n) || logits[0].is_empty() {
        return Err("Invalid original N or empty aggregate".into());
    }
    let frames: Vec<i64> = beat
        .iter()
        .map(|v| canonical_frame(v.q, n))
        .collect::<Result<_, _>>()?;
    for group in &downbeat {
        canonical_frame(group.q, n)?;
    }
    if frames.windows(2).any(|v| v[0] >= v[1]) {
        return Err("Rounded beat coordinates duplicate or descend".into());
    }
    if beat.is_empty() && !downbeat.is_empty() {
        return Err("Raw downbeat has no beat for v2 mapping".into());
    }
    let mut scores = vec![None::<f32>; beat.len()];
    let mut alignment = Vec::new();
    for group in &downbeat {
        let index = nearest_beat_index(group.q, &beat);
        scores[index] = Some(scores[index].map_or(group.score, |old| old.max(group.score)));
        alignment.push(serde_json::json!({"original_downbeat_q":group.q,"nearest_beat_index":index,"aligned_beat_q":beat[index].q,"canonical_frame":frames[index]}));
    }
    write_json(&evidence.join("minimal-alignment.json"), &alignment)?;
    Ok(beat
        .iter()
        .enumerate()
        .map(|(index, group)| BeatFeature {
            time: SongTime::from_frames(frames[index]),
            strength: group.score,
            downbeat_probability: scores[index],
            confidence: None,
        })
        .collect())
}

fn copy_final_evidence(
    staged: &Path,
    prepared: &PreparedCanonicalAudio,
    evidence: &Path,
) -> Result<PathBuf, String> {
    let path = evidence.join("final-canonical.ogg");
    let mut input = File::open(staged)
        .map_err(|e| error("Open staged final Ogg", e))?
        .take(prepared.asset.byte_len + 1);
    let mut output = new_file(&path)?;
    let mut hash = blake3::Hasher::new();
    let mut count = 0u64;
    let mut buffer = [0; 32768];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|e| error("Read final Ogg evidence", e))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|e| error("Write final Ogg evidence", e))?;
        count += read as u64;
        hash.update(&buffer[..read]);
    }
    output
        .sync_all()
        .map_err(|e| error("Sync final Ogg evidence", e))?;
    write_json(
        &evidence.join("final-canonical-identity.json"),
        &serde_json::json!({"staged":staged,"N":prepared.canonical_frames,"bytes":count,"blake3":hash.finalize().to_hex().to_string()}),
    )?;
    if count != prepared.asset.byte_len || hash.finalize().as_bytes() != &prepared.asset.blake3 {
        return Err("Staged final Ogg identity changed; original evidence retained".into());
    }
    Ok(path)
}

pub(crate) fn analyze_staged(
    session: &mut Session,
    staged: &Path,
    prepared: &PreparedCanonicalAudio,
    channel: usize,
    evidence: &Path,
    analysis: &mut MusicAnalysis,
) -> Result<(), String> {
    if channel > 1 || analysis.audio_hash != prepared.asset.blake3 {
        return Err("Invalid channel or analysis audio identity".into());
    }
    let final_copy = copy_final_evidence(staged, prepared, evidence)?;
    let pcm = frontend::resample_canonical_22050(&final_copy, prepared.canonical_frames, evidence)?;
    write_f32(
        &evidence.join("resampled-stereo.f32"),
        pcm.iter().flatten().copied(),
    )?;
    let spect = frontend::log_mel_spectrogram(&pcm, channel, evidence)?;
    write_f32(
        &evidence.join("native-spect.f32"),
        spect.values.iter().copied(),
    )?;
    write_json(
        &evidence.join("native-shape.json"),
        &serde_json::json!({"N":prepared.canonical_frames,"M":pcm.len(),"F":spect.frames,"shape":[1,spect.frames,128],"channel":channel,"profile":ANALYSIS_VERSION,"old_frontend_numeric":"FAIL_PRESERVED_19_OF_28","old_music_quality":"FAIL_PRESERVED","confidence":null,"production_admission":false}),
    )?;
    drop(pcm);
    let logits = infer(session, &spect, evidence)?;
    let candidates = minimal(&logits, prepared.canonical_frames, evidence)?;
    let capabilities = analysis
        .capabilities
        .as_mut()
        .ok_or("Experimental adapter requires MusicAnalysis v2 capabilities")?;
    let candidate = AnalysisCapability {
        state: AnalysisState::Candidate,
        source: AnalysisSource::Algorithm,
        confidence: None,
    };
    let unsupported = AnalysisCapability {
        state: AnalysisState::Unsupported,
        ..candidate
    };
    capabilities.beat = candidate;
    capabilities.downbeat = candidate;
    capabilities.tempo = unsupported;
    capabilities.onset = unsupported;
    capabilities.repetition = unsupported;
    analysis.beats = candidates;
    let mut evidence_files = Vec::new();
    for name in [
        "native-shape.json",
        "native-spect.f32",
        "aggregate-beat.f32",
        "aggregate-downbeat.f32",
        "minimal-raw-groups.json",
        "minimal-alignment.json",
    ] {
        let bytes = fs::read(evidence.join(name))
            .map_err(|e| error("Read completed evidence for summary", e))?;
        evidence_files.push(serde_json::json!({"name":name,"bytes":bytes.len(),"blake3":blake3::hash(&bytes).to_hex().as_str()}));
    }
    let summary = serde_json::to_vec_pretty(&serde_json::json!({"profile":ANALYSIS_VERSION,"model_blake3":blake3::Hash::from_bytes(assets::MODEL_BLAKE3).to_hex().as_str(),"audio_blake3":blake3::Hash::from_bytes(prepared.asset.blake3).to_hex().as_str(),"files":evidence_files,"confidence":null,"production_admission":false})).map_err(|e| error("Serialize evidence summary", e))?;
    let summary_hash = blake3::hash(&summary);
    let mut summary_file = new_file(&evidence.join("evidence-summary.json"))?;
    summary_file
        .write_all(&summary)
        .map_err(|e| error("Write evidence summary", e))?;
    summary_file
        .sync_all()
        .map_err(|e| error("Sync evidence summary", e))?;
    analysis.diagnostics = analysis.diagnostics.replacen(
        "; beat/onset analysis not run;",
        "; experimental beat/downbeat candidates; onset unsupported;",
        1,
    );
    analysis.diagnostics.push_str(&format!("; experimental={ANALYSIS_VERSION}; model_blake3={}; evidence_summary_blake3={summary_hash}; channel={channel}; N={}; M={}; F={}; confidence=None; frontend/quality FAIL preserved; raw evidence accompanies package",blake3::Hash::from_bytes(assets::MODEL_BLAKE3),prepared.canonical_frames,(prepared.canonical_frames*22050).div_ceil(48000),spect.frames));
    analysis.validate(prepared.canonical_frames)?;
    write_json(
        &evidence.join("analysis-complete.json"),
        &serde_json::json!({"status":"CANDIDATE_ONLY","beats":analysis.beats.len(),"downbeats":analysis.beats.iter().filter(|v|v.downbeat_probability.is_some()).count(),"confidence":null,"production_admission":false}),
    )
}

fn record_audio_frame(
    output: &oximedia_audio::AudioFrame,
    raw: &mut File,
    metadata: &mut File,
) -> Result<(), String> {
    let offset = raw
        .stream_position()
        .map_err(|e| error("Actual resampler offset", e))?;
    let planes = match &output.samples {
        oximedia_audio::AudioBuffer::Interleaved(bytes) => {
            raw.write_all(bytes)
                .map_err(|e| error("Write actual interleaved resampler bytes", e))?;
            vec![bytes.len()]
        }
        oximedia_audio::AudioBuffer::Planar(planes) => {
            for bytes in planes {
                raw.write_all(bytes)
                    .map_err(|e| error("Write actual planar resampler bytes", e))?;
            }
            planes.iter().map(|bytes| bytes.len()).collect()
        }
    };
    serde_json::to_writer(&mut *metadata, &serde_json::json!({"offset":offset,"bytes":planes,"format":format!("{:?}",output.format),"rate":output.sample_rate,"channels":format!("{:?}",output.channels),"interleaved":matches!(&output.samples,oximedia_audio::AudioBuffer::Interleaved(_))})).map_err(|e|error("Record actual resampler metadata",e))?;
    metadata
        .write_all(b"\n")
        .map_err(|e| error("Write actual resampler metadata newline", e))
}

mod frontend {
    //! Research draft only: final canonical PCM -> native 22.05 kHz -> per-channel spect
    //! ponytail: synchronous research functions; add existing import-worker cancellation before production use

    use super::{new_file, record_audio_frame, write_f32, write_f64};
    use crate::decode_canonical;
    use oximedia_audio::{
        AudioBuffer, AudioFrame, ChannelLayout, Resampler, ResamplerQuality,
        spectrum::fft::{FftProcessor, WindowFunction},
    };
    use oximedia_core::SampleFormat;
    use std::{fs::File, path::Path};

    const CANONICAL_RATE: u64 = 48_000;
    const RATE: u64 = 22_050;
    // Same ten-minute bound as the existing canonical decoder; draft external API
    const MAX_SOURCE_SECONDS: u64 = 600;
    const FFT: usize = 1024;
    const HOP: usize = 441;
    const MELS: usize = 128;

    /// Contiguous float32 [1, frames, 128], with center q at canonical frame q * 960
    pub struct Spectrogram {
        pub frames: usize,
        pub values: Vec<f32>,
    }

    /// Consumes the staged final canonical Ogg and verifies its exact original N
    /// The caller owns the already hash-bound staged file; no source WAV substitution
    pub fn resample_canonical_22050(
        path: &Path,
        expected_frames: u64,
        evidence: &Path,
    ) -> Result<Vec<[f32; 2]>, String> {
        let mut raw = new_file(&evidence.join("resampler-output.bin"))?;
        let mut metadata = new_file(&evidence.join("resampler-output.jsonl"))?;
        if !(1..=CANONICAL_RATE * MAX_SOURCE_SECONDS).contains(&expected_frames) {
            return Err("Canonical input must cover more than zero and at most ten minutes".into());
        }
        let expected = (expected_frames * RATE).div_ceil(CANONICAL_RATE) as usize;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(expected)
            .map_err(|error| format!("Cannot allocate bounded resampled PCM: {error}"))?;
        let mut converter = Resampler::new(
            CANONICAL_RATE as u32,
            RATE as u32,
            2,
            ResamplerQuality::High,
        )
        .map_err(|error| format!("Cannot create native resampler: {error}"))?;
        let mut pushed = 0u64;
        decode_canonical(path, expected_frames, |block| {
            for chunk in block.chunks(1024) {
                let bytes: Vec<_> = chunk
                    .iter()
                    .flatten()
                    .flat_map(|sample| sample.to_le_bytes())
                    .collect();
                let mut input = AudioFrame::new(
                    SampleFormat::F32,
                    CANONICAL_RATE as u32,
                    ChannelLayout::Stereo,
                );
                input.samples = AudioBuffer::Interleaved(bytes.into());
                pushed = pushed
                    .checked_add(chunk.len() as u64)
                    .ok_or("Canonical input count overflow")?;
                let output = converter
                    .resample(&input)
                    .map_err(|error| format!("Cannot resample canonical block: {error}"))?;
                append_pcm(
                    output,
                    (chunk.len() as u64 * RATE).div_ceil(CANONICAL_RATE) as usize,
                    (pushed * RATE).div_ceil(CANONICAL_RATE) as usize,
                    &mut samples,
                    &mut raw,
                    &mut metadata,
                )?;
            }
            Ok(())
        })?;
        if pushed != expected_frames {
            return Err("Canonical decoder did not deliver the original complete N".into());
        }
        let tail = converter
            .flush()
            .map_err(|error| format!("Cannot flush native resampler: {error}"))?;
        // Installed OxiMedia High retains at most half its 192-tap kernel
        append_pcm(
            tail,
            (96 * RATE).div_ceil(CANONICAL_RATE) as usize,
            expected,
            &mut samples,
            &mut raw,
            &mut metadata,
        )?;
        if samples.len() != expected {
            return Err(format!(
                "Native resampler returned M={}, expected ceil(N*22050/48000)={expected}",
                samples.len()
            ));
        }
        raw.sync_all()
            .map_err(|e| format!("Sync actual resampler raw: {e}"))?;
        metadata
            .sync_all()
            .map_err(|e| format!("Sync actual resampler metadata: {e}"))?;
        Ok(samples)
    }

    fn append_pcm(
        output: AudioFrame,
        max_block: usize,
        max_total: usize,
        samples: &mut Vec<[f32; 2]>,
        raw: &mut File,
        metadata: &mut File,
    ) -> Result<(), String> {
        record_audio_frame(&output, raw, metadata)?;
        if output.format != SampleFormat::F32
            || output.sample_rate != RATE as u32
            || output.channels != ChannelLayout::Stereo
        {
            return Err("Native resampler returned an unexpected audio format".into());
        }
        let AudioBuffer::Interleaved(bytes) = output.samples else {
            return Err("Native resampler returned non-interleaved PCM".into());
        };
        let (frames, remainder) = bytes.as_chunks::<8>();
        if !remainder.is_empty()
            || frames.len() > max_block
            || samples
                .len()
                .checked_add(frames.len())
                .is_none_or(|n| n > max_total)
        {
            return Err("Native resampler returned an invalid frame count".into());
        }
        if bytes
            .as_chunks::<4>()
            .0
            .iter()
            .any(|sample| !f32::from_le_bytes(*sample).is_finite())
        {
            return Err("Native resampler returned non-finite PCM".into());
        }
        samples.extend(frames.iter().map(|frame| {
            [
                f32::from_le_bytes(frame[..4].try_into().unwrap()),
                f32::from_le_bytes(frame[4..].try_into().unwrap()),
            ]
        }));
        Ok(())
    }

    /// Runs each actual channel separately; mono averaging would erase antiphase audio
    pub fn log_mel_spectrogram(
        pcm: &[[f32; 2]],
        channel: usize,
        evidence: &Path,
    ) -> Result<Spectrogram, String> {
        if channel > 1
            || pcm.len() <= FFT / 2
            || pcm.len() > RATE as usize * MAX_SOURCE_SECONDS as usize
        {
            return Err(
                "Native spect requires channel 0/1 and 513..=13230000 actual PCM frames".into(),
            );
        }
        if pcm.iter().flatten().any(|sample| !sample.is_finite()) {
            return Err("Native spect input contains non-finite PCM".into());
        }
        let frames = 1 + pcm.len() / HOP;
        let mut values = Vec::new();
        values
            .try_reserve_exact(frames * MELS)
            .map_err(|error| format!("Cannot allocate bounded native spect: {error}"))?;
        let window: Vec<f32> = (0..FFT)
            .map(|j| (j as f32 * (std::f32::consts::TAU / FFT as f32)).cos() * -0.5 + 0.5)
            .collect();
        let filterbank = slaney_filterbank();
        let mut fft = FftProcessor::new(FFT, WindowFunction::Rectangle);
        // FftProcessor silently returns zeros when its internal FFT plan is unavailable
        let mut impulse = [0.0f64; FFT];
        impulse[0] = 1.0;
        let probe = fft.magnitude_spectrum(&impulse);
        if probe.len() != FFT
            || probe
                .iter()
                .any(|value| !value.is_finite() || (value - 1.0).abs() > 1e-9)
        {
            return Err("Native 1024 FFT impulse check failed".into());
        }
        let mut windowed = [0.0f64; FFT];
        for q in 0..frames {
            for (j, slot) in windowed.iter_mut().enumerate() {
                let index = q as isize * HOP as isize + j as isize - (FFT / 2) as isize;
                *slot = f64::from(pcm[reflect(index, pcm.len())][channel] * window[j]);
            }
            let magnitude = fft.magnitude_spectrum(&windowed);
            if magnitude.len() != FFT || magnitude[..=FFT / 2].iter().any(|v| !v.is_finite()) {
                write_f64(&evidence.join(format!("failed-window-q{q}.f64")), &windowed)?;
                write_f64(
                    &evidence.join(format!("failed-magnitude-q{q}.f64")),
                    &magnitude,
                )?;
                return Err(format!(
                    "Native FFT returned invalid values at original q={q}"
                ));
            }
            if windowed.iter().any(|value| *value != 0.0)
                && magnitude[..=FFT / 2].iter().all(|value| *value == 0.0)
            {
                write_f64(&evidence.join(format!("failed-window-q{q}.f64")), &windowed)?;
                write_f64(
                    &evidence.join(format!("failed-magnitude-q{q}.f64")),
                    &magnitude,
                )?;
                return Err(format!(
                    "Native FFT returned silence for nonzero input at q={q}"
                ));
            }
            for weights in &filterbank {
                let mel: f32 = magnitude
                    .iter()
                    .zip(weights)
                    .map(|(magnitude, weight)| (*magnitude / 32.0) as f32 * weight)
                    .sum();
                let value = (1000.0 * mel).ln_1p();
                if !value.is_finite() {
                    write_f32(
                        &evidence.join("failed-partial-spect.f32"),
                        values.iter().copied().chain(std::iter::once(value)),
                    )?;
                    write_f64(&evidence.join(format!("failed-window-q{q}.f64")), &windowed)?;
                    write_f64(
                        &evidence.join(format!("failed-magnitude-q{q}.f64")),
                        &magnitude,
                    )?;
                    return Err(format!(
                        "Native log-mel returned non-finite values at original q={q}"
                    ));
                }
                values.push(value);
            }
        }
        Ok(Spectrogram { frames, values })
    }

    fn reflect(index: isize, length: usize) -> usize {
        if index < 0 {
            (-index) as usize
        } else if index >= length as isize {
            (2 * length as isize - 2 - index) as usize
        } else {
            index as usize
        }
    }

    fn slaney_filterbank() -> Vec<Vec<f32>> {
        let log_step = 6.4f64.ln() / 27.0;
        let min_mel = (30.0 / (200.0 / 3.0)) as f32;
        let max_mel = (15.0 + (11_000.0f64 / 1000.0).ln() / log_step) as f32;
        let step = (max_mel - min_mel) / (MELS + 1) as f32;
        let points: Vec<f32> = (0..MELS + 2)
            .map(|j| {
                // Match the official float32 linspace's symmetric endpoint arithmetic
                let mel = if j < (MELS + 2) / 2 {
                    step.mul_add(j as f32, min_mel)
                } else {
                    (-step).mul_add((MELS + 1 - j) as f32, max_mel)
                };
                if mel < 15.0 {
                    mel * (200.0 / 3.0)
                } else {
                    1000.0 * ((log_step as f32) * (mel - 15.0)).exp()
                }
            })
            .collect();
        (0..MELS)
            .map(|j| {
                (0..=FFT / 2)
                    .map(|k| {
                        let frequency = k as f32 * RATE as f32 / FFT as f32;
                        let rising = (frequency - points[j]) / (points[j + 1] - points[j]);
                        let falling = (points[j + 2] - frequency) / (points[j + 2] - points[j + 1]);
                        rising.min(falling).max(0.0)
                    })
                    .collect()
            })
            .collect()
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            path::PathBuf,
            sync::atomic::{AtomicU64, Ordering},
        };
        static NEXT: AtomicU64 = AtomicU64::new(0);
        fn evidence_dir() -> PathBuf {
            let directory = std::env::temp_dir().join(format!(
                "cocobeat-native-candidate-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            directory
        }

        #[test]
        fn native_shape_reflection_channel_and_invalid_input_contract() {
            let evidence = evidence_dir();
            assert_eq!((reflect(-512, 513), reflect(1023, 513)), (512, 1));
            assert_eq!((reflect(-1, 513), reflect(513, 513)), (1, 511));
            assert!(log_mel_spectrogram(&vec![[0.0; 2]; 512], 0, &evidence).is_err());
            assert!(log_mel_spectrogram(&vec![[0.0; 2]; 513], 2, &evidence).is_err());
            assert!(log_mel_spectrogram(&vec![[f32::NAN, 0.0]; 513], 1, &evidence).is_err());
            let silent = log_mel_spectrogram(&vec![[0.0; 2]; 882], 0, &evidence).unwrap();
            assert_eq!((silent.frames, silent.values.len()), (3, 3 * MELS));
            assert!(silent.values.iter().all(|value| *value == 0.0));
            let antiphase: Vec<_> = (0..2205)
                .map(|j| {
                    let sample = (std::f32::consts::TAU * 440.0 * j as f32 / RATE as f32).sin();
                    [sample, -sample]
                })
                .collect();
            let left = log_mel_spectrogram(&antiphase, 0, &evidence).unwrap();
            let right = log_mel_spectrogram(&antiphase, 1, &evidence).unwrap();
            assert_eq!(left.frames, 6);
            assert!(left.values.iter().any(|value| *value > 0.0));
            assert_eq!(left.values, right.values);
            let independent: Vec<_> = antiphase.iter().map(|frame| [frame[0], 0.0]).collect();
            assert_eq!(
                log_mel_spectrogram(&independent, 0, &evidence)
                    .unwrap()
                    .values,
                left.values
            );
            assert!(
                log_mel_spectrogram(&independent, 1, &evidence)
                    .unwrap()
                    .values
                    .iter()
                    .all(|v| *v == 0.0)
            );
            std::fs::remove_dir(evidence).unwrap();
        }

        #[test]
        fn resampler_reads_the_final_ogg_and_preserves_its_exact_length() {
            let evidence = evidence_dir();
            let fixture = evidence.join("input.ogg");
            std::fs::write(
                &fixture,
                include_bytes!("../../../testdata/synthetic/media-import/stereo-canonical.ogg"),
            )
            .unwrap();
            let pcm = resample_canonical_22050(&fixture, 4800, &evidence).unwrap();
            assert_eq!(pcm.len(), 2205);
            assert!(pcm.iter().flatten().all(|sample| sample.is_finite()));
            assert!(pcm.iter().any(|frame| frame[0] != frame[1]));
            for n in [4801, 0] {
                let rejected = evidence_dir();
                assert!(resample_canonical_22050(&fixture, n, &rejected).is_err());
                std::fs::remove_dir_all(rejected).unwrap();
            }
            std::fs::remove_dir_all(evidence).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimal_ties_running_mean_and_original_coordinate_guards() {
        let groups = peaks(&[1.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
        assert_eq!(
            groups.iter().map(|v| v.q).collect::<Vec<_>>(),
            vec![0.5, 2.5, 4.0]
        );
        assert!(peaks(&[f32::NAN]).is_err());
        assert_eq!(canonical_frame(1.5, 2000).unwrap(), 1440);
        assert!(canonical_frame(2.0, 1920).is_err());
        assert!(canonical_frame(-1.0, 1920).is_err());
        assert!(canonical_frame(f64::INFINITY, 1920).is_err());
        assert_eq!(chunk_starts(1601).unwrap(), vec![-6, 107]);
        assert_eq!(chunk_starts(1488).unwrap(), vec![-6]);
        assert_eq!(chunk_starts(1489).unwrap(), vec![-6, -5]);
        assert!(chunk_starts(0).is_err());
        let evidence = std::env::temp_dir().join(format!(
            "cocobeat-native-minimal-test-{}",
            std::process::id()
        ));
        std::fs::create_dir(&evidence).unwrap();
        let valid = evidence.join("valid");
        let rejected = evidence.join("rejected");
        std::fs::create_dir(&valid).unwrap();
        std::fs::create_dir(&rejected).unwrap();
        let logits = [vec![1.0; 5], vec![-1.0, 1.0, 1.0, -1.0, -1.0]];
        let beats = minimal(&logits, 5000, &valid).unwrap();
        assert_eq!(
            beats.iter().map(|b| b.time.frames()).collect::<Vec<_>>(),
            vec![480, 2400, 3840]
        );
        assert!(beats.iter().all(|b| b.confidence.is_none()));
        assert!(beats[0].downbeat_probability.is_some());
        assert!(beats[1..].iter().all(|b| b.downbeat_probability.is_none()));
        assert!(minimal(&logits, 1920, &rejected).is_err());
        assert!(rejected.join("minimal-raw-groups.json").is_file());
        assert!(!rejected.join("minimal-alignment.json").exists());
        std::fs::remove_dir_all(evidence).unwrap();
    }
}
