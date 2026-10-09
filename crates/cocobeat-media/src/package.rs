//! Publishes and verifies a complete four-object package on one filesystem

use crate::{
    PreparedCanonicalAudio,
    audio_asset::prepare_canonical_audio_checked,
    content_codec::{
        MAX_ANALYSIS_BYTES, MAX_CHART_BYTES, MAX_PACKAGE_BYTES, decode_analysis, decode_chart,
        decode_package, encode_analysis, encode_chart, encode_package, package_hash,
    },
    decode::{MAX_SOURCE_BYTES, decode_canonical_bytes},
};
use cocobeat_schema::{
    Anchor, AssetRef, CANONICAL_SAMPLE_RATE, CONTENT_SCHEMA_VERSION, CompiledChart,
    MAX_CONTENT_ITEMS, MusicAnalysis, SongPackage,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const PACKAGE_OBJECT_NAMES: [&str; 4] = [
    "song.audio.ogg",
    "analysis.bin",
    "chart.bin",
    "song.package",
];
pub const PACKAGE_OBJECT_LIMITS: [u64; 4] = [
    MAX_SOURCE_BYTES,
    MAX_ANALYSIS_BYTES as u64,
    MAX_CHART_BYTES as u64,
    MAX_PACKAGE_BYTES as u64,
];
pub const MAX_RECEIVED_PACKAGE_BYTES: u64 = MAX_SOURCE_BYTES
    + MAX_ANALYSIS_BYTES as u64
    + MAX_CHART_BYTES as u64
    + MAX_PACKAGE_BYTES as u64;
static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct PackageBuildInput {
    pub song_id: String,
    pub importer_version: String,
    pub analysis_version: String,
    pub chart_version: String,
    pub analysis: MusicAnalysis,
    pub chart: CompiledChart,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedPackage {
    pub manifest: SongPackage,
    pub analysis: MusicAnalysis,
    pub chart: CompiledChart,
}

/// Receives four bounded objects in their fixed order without rebuilding their bytes
/// Dropping an unfinished receiver removes only its own objects and staging directory
pub struct ReceivedPackage {
    staging: PathBuf,
    destination: PathBuf,
    expected_package_hash: [u8; 32],
    remaining: [u64; 4],
    next_object: usize,
    files: Vec<File>,
    created: Vec<PathBuf>,
    failed: bool,
    active: bool,
}

impl ReceivedPackage {
    pub fn new(
        destination: impl AsRef<Path>,
        expected_package_hash: [u8; 32],
        lengths: [u64; 4],
    ) -> Result<Self, String> {
        if lengths
            .iter()
            .zip(PACKAGE_OBJECT_LIMITS)
            .any(|(&length, limit)| !(1..=limit).contains(&length))
            || lengths
                .iter()
                .try_fold(0_u64, |sum, length| sum.checked_add(*length))
                .is_none_or(|sum| sum > MAX_RECEIVED_PACKAGE_BYTES)
        {
            return Err("Received package object or total byte limit exceeded".into());
        }
        let destination = destination.as_ref();
        let staging = staging_path(destination)?;
        fs::create_dir(&staging)
            .map_err(|error| format!("Cannot create package staging directory: {error}"))?;
        let destination = staging
            .parent()
            .unwrap()
            .join(destination.file_name().unwrap());
        let mut received = Self {
            staging,
            destination,
            expected_package_hash,
            remaining: lengths,
            next_object: 0,
            files: Vec::with_capacity(4),
            created: Vec::with_capacity(4),
            failed: false,
            active: true,
        };
        let result = (|| {
            for name in PACKAGE_OBJECT_NAMES {
                let path = received.staging.join(name);
                let file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|error| format!("Cannot create package object {name}: {error}"))?;
                received.created.push(path);
                received.files.push(file);
            }
            Ok(())
        })();
        if let Err(error) = result {
            received.files.clear();
            received.active = false;
            return Err(cleanup_staging(&received.staging, &received.created, error));
        }
        Ok(received)
    }

    /// A rejected chunk invalidates the transaction, even if a caller ignores its error
    pub fn write(&mut self, object_index: usize, bytes: &[u8]) -> Result<(), String> {
        let result = (|| {
            if self.failed
                || object_index != self.next_object
                || object_index >= 4
                || bytes.is_empty()
                || bytes.len() as u64 > self.remaining[object_index]
            {
                return Err(
                    "Received package chunk is empty, out of order or exceeds its declared length"
                        .into(),
                );
            }
            self.files[object_index]
                .write_all(bytes)
                .map_err(|error| format!("Cannot write received package object: {error}"))?;
            self.remaining[object_index] -= bytes.len() as u64;
            if self.remaining[object_index] == 0 {
                self.next_object += 1;
            }
            Ok(())
        })();
        self.failed |= result.is_err();
        result
    }

    /// Call only after the transport has checked stream termination and rejected trailing bytes
    pub fn finish(mut self) -> Result<ValidatedPackage, String> {
        let result = (|| {
            if self.failed || self.next_object != 4 {
                return Err("Received package is incomplete or a chunk failed".into());
            }
            for file in &self.files {
                file.sync_all().map_err(|error| error.to_string())?;
            }
            self.files.clear();
            let validated = validate_package(&self.staging)?;
            if validated.manifest.package_hash != self.expected_package_hash {
                return Err(
                    "Received package identity does not match the authenticated peer".into(),
                );
            }
            publish_validated_package(&self.staging, &self.destination, validated)
        })();
        self.files.clear();
        self.active = false;
        result.map_err(|error| cleanup_staging(&self.staging, &self.created, error))
    }
}

impl Drop for ReceivedPackage {
    fn drop(&mut self) {
        if self.active {
            self.files.clear();
            let _ = cleanup_staging(&self.staging, &self.created, String::new());
        }
    }
}

/// Validates the staged final bytes before atomically publishing the whole directory
/// Content is built from the strictly verified staged audio, independent of later source changes
/// Existing destinations are preserved; failed staging removes only this call's own files
pub fn build_package(
    source_audio: impl AsRef<Path>,
    expected_frames: u64,
    destination: impl AsRef<Path>,
    build_content: impl FnOnce(&Path, &PreparedCanonicalAudio) -> Result<PackageBuildInput, String>,
) -> Result<ValidatedPackage, String> {
    build_package_checked(
        source_audio,
        expected_frames,
        destination,
        build_content,
        &|| Ok(()),
    )
}

pub(crate) fn build_package_checked(
    source_audio: impl AsRef<Path>,
    expected_frames: u64,
    destination: impl AsRef<Path>,
    build_content: impl FnOnce(&Path, &PreparedCanonicalAudio) -> Result<PackageBuildInput, String>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<ValidatedPackage, String> {
    let source_audio = source_audio.as_ref();
    let destination = destination.as_ref();
    let (staging, audio) = stage_audio_checked(source_audio, expected_frames, destination, check)?;
    let mut created = vec![staging.join(PACKAGE_OBJECT_NAMES[0])];
    let result = (|| {
        check()?;
        let input = build_content(&staging.join(&audio.asset.file_name), &audio)?;
        check()?;
        if input.analysis.audio_hash != audio.asset.blake3
            || input.chart.audio_hash != audio.asset.blake3
        {
            return Err("Analysis and chart must reference the exact canonical audio hash".into());
        }
        let analysis = write_object_checked(
            &staging,
            PACKAGE_OBJECT_NAMES[1],
            &encode_analysis(&input.analysis, audio.canonical_frames)?,
            &mut created,
            check,
        )?;
        let chart = write_object_checked(
            &staging,
            PACKAGE_OBJECT_NAMES[2],
            &encode_chart(&input.chart, audio.canonical_frames)?,
            &mut created,
            check,
        )?;
        let mut manifest = SongPackage {
            schema_version: CONTENT_SCHEMA_VERSION,
            song_id: input.song_id,
            audio: audio.asset,
            analysis,
            chart,
            canonical_sample_rate: CANONICAL_SAMPLE_RATE,
            channels: 2,
            canonical_frames: audio.canonical_frames,
            importer_version: input.importer_version,
            analysis_version: input.analysis_version,
            chart_version: input.chart_version,
            package_hash: [0; 32],
        };
        check()?;
        manifest.package_hash = package_hash(&manifest)?;
        write_object_checked(
            &staging,
            PACKAGE_OBJECT_NAMES[3],
            &encode_package(&manifest)?,
            &mut created,
            check,
        )?;
        publish_package_checked(&staging, destination, check)
    })();
    result.map_err(|error| cleanup_staging(&staging, &created, error))
}

/// Replaces only Anchors, preserving the exact audio and analysis objects
/// No-op edits preserve all four source objects, including noncanonical metadata bytes
pub fn export_anchors(
    source: impl AsRef<Path>,
    expected_package_hash: [u8; 32],
    anchors: &[Anchor],
    destination: impl AsRef<Path>,
) -> Result<ValidatedPackage, String> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    let (package, manifest_bytes) = read_package_snapshot(source, |_| Ok(()))?;
    if package.manifest.package_hash != expected_package_hash {
        return Err("Anchor edit source package identity does not match".into());
    }
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if fs::canonicalize(parent)
        .map_err(|error| format!("Cannot resolve package destination parent: {error}"))?
        .starts_with(fs::canonicalize(source).map_err(|error| error.to_string())?)
    {
        return Err("Anchor export destination must be outside the source package".into());
    }
    publish_anchors(source, package, manifest_bytes, anchors, destination)
}

fn publish_anchors(
    source: &Path,
    mut package: ValidatedPackage,
    mut manifest_bytes: Vec<u8>,
    anchors: &[Anchor],
    destination: &Path,
) -> Result<ValidatedPackage, String> {
    if anchors.len() > MAX_CONTENT_ITEMS {
        return Err("Content item limit exceeded".into());
    }
    let changed = anchors != package.chart.anchors;
    let chart_bytes = if changed {
        package.chart.anchors = anchors.to_vec();
        encode_chart(&package.chart, package.manifest.canonical_frames)?
    } else {
        read_referenced(source, &package.manifest.chart, MAX_CHART_BYTES)?
    };
    let analysis_bytes = read_referenced(source, &package.manifest.analysis, MAX_ANALYSIS_BYTES)?;
    let (staging, audio) = stage_audio(
        &source.join(&package.manifest.audio.file_name),
        package.manifest.canonical_frames,
        destination,
    )?;
    let mut created = vec![staging.join(PACKAGE_OBJECT_NAMES[0])];
    let result = (|| {
        if audio.asset != package.manifest.audio {
            return Err("Copied audio identity differs from the validated source package".into());
        }
        write_object(
            &staging,
            PACKAGE_OBJECT_NAMES[1],
            &analysis_bytes,
            &mut created,
        )?;
        let chart = write_object(
            &staging,
            PACKAGE_OBJECT_NAMES[2],
            &chart_bytes,
            &mut created,
        )?;
        if changed {
            package.manifest.chart = chart;
            package.manifest.chart_version = "manual-editor-v1".into();
            package.manifest.package_hash = package_hash(&package.manifest)?;
            manifest_bytes = encode_package(&package.manifest)?;
        }
        write_object(
            &staging,
            PACKAGE_OBJECT_NAMES[3],
            &manifest_bytes,
            &mut created,
        )?;
        publish_package(&staging, destination)
    })();
    result.map_err(|error| cleanup_staging(&staging, &created, error))
}

/// Returns content only after bounded object, envelope, semantic and canonical readback checks
pub fn validate_package(root: impl AsRef<Path>) -> Result<ValidatedPackage, String> {
    read_package(root, |_| Ok(()))
}

/// Includes the raw manifest identity from the same fully validated four-object snapshot
pub fn validate_package_objects(
    root: impl AsRef<Path>,
) -> Result<(ValidatedPackage, [AssetRef; 4]), String> {
    let (package, manifest_bytes) = read_package_snapshot(root.as_ref(), |_| Ok(()))?;
    let objects = [
        package.manifest.audio.clone(),
        package.manifest.analysis.clone(),
        package.manifest.chart.clone(),
        AssetRef {
            file_name: PACKAGE_OBJECT_NAMES[3].into(),
            byte_len: manifest_bytes.len() as u64,
            blake3: *blake3::hash(&manifest_bytes).as_bytes(),
        },
    ];
    Ok((package, objects))
}

/// Delivers PCM from the same owned audio snapshot used for hash and strict decoding checks
/// All blocks remain provisional until success; consumers must discard them on any error
pub fn read_package(
    root: impl AsRef<Path>,
    consume: impl FnMut(&[[f32; 2]]) -> Result<(), String>,
) -> Result<ValidatedPackage, String> {
    read_package_snapshot(root.as_ref(), consume).map(|(package, _)| package)
}

fn read_package_snapshot(
    root: &Path,
    consume: impl FnMut(&[[f32; 2]]) -> Result<(), String>,
) -> Result<(ValidatedPackage, Vec<u8>), String> {
    read_package_snapshot_checked(root, consume, &|| Ok(()))
}

fn read_package_snapshot_checked(
    root: &Path,
    mut consume: impl FnMut(&[[f32; 2]]) -> Result<(), String>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(ValidatedPackage, Vec<u8>), String> {
    check()?;
    if !fs::symlink_metadata(root)
        .map_err(|error| format!("Cannot inspect package directory: {error}"))?
        .is_dir()
    {
        return Err("Package root must be a directory, not a symlink".into());
    }
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        check()?;
        let entry = entry.map_err(|error| error.to_string())?;
        if !PACKAGE_OBJECT_NAMES
            .iter()
            .any(|name| entry.file_name() == *name)
        {
            return Err("Package directory must contain only its four declared objects".into());
        }
    }
    let manifest_bytes = read_object_checked(
        &root.join(PACKAGE_OBJECT_NAMES[3]),
        MAX_PACKAGE_BYTES,
        check,
    )?;
    check()?;
    let manifest = decode_package(&manifest_bytes)?;
    if package_hash(&manifest)? != manifest.package_hash {
        return Err("Package manifest hash does not match its canonical bytes".into());
    }
    let analysis_bytes =
        read_referenced_checked(root, &manifest.analysis, MAX_ANALYSIS_BYTES, check)?;
    let chart_bytes = read_referenced_checked(root, &manifest.chart, MAX_CHART_BYTES, check)?;
    check()?;
    let analysis = decode_analysis(&analysis_bytes, manifest.canonical_frames)?;
    check()?;
    let chart = decode_chart(&chart_bytes, manifest.canonical_frames)?;
    if analysis.audio_hash != manifest.audio.blake3 || chart.audio_hash != manifest.audio.blake3 {
        return Err("Analysis or chart references a different canonical audio hash".into());
    }
    let audio = read_referenced_checked(root, &manifest.audio, MAX_SOURCE_BYTES as usize, check)?;
    check()?;
    decode_canonical_bytes(audio, manifest.canonical_frames, |frames| {
        check()?;
        consume(frames)?;
        check()
    })?;
    check()?;
    Ok((
        ValidatedPackage {
            manifest,
            analysis,
            chart,
        },
        manifest_bytes,
    ))
}

fn stage_audio(
    source_audio: &Path,
    expected_frames: u64,
    destination: &Path,
) -> Result<(PathBuf, PreparedCanonicalAudio), String> {
    stage_audio_checked(source_audio, expected_frames, destination, &|| Ok(()))
}

pub(crate) fn stage_audio_checked(
    source_audio: &Path,
    expected_frames: u64,
    destination: &Path,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(PathBuf, PreparedCanonicalAudio), String> {
    check()?;
    drop(open_object(source_audio, MAX_SOURCE_BYTES)?);
    let path = staging_path(destination)?;
    check()?;
    prepare_canonical_audio_checked(source_audio, expected_frames, &path, check)
        .map(|audio| (path, audio))
}

fn staging_path(destination: &Path) -> Result<PathBuf, String> {
    require_absent(destination)?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if destination.file_name().is_none() {
        return Err("Package destination must name a new directory".into());
    }
    let parent = fs::canonicalize(parent)
        .map_err(|error| format!("Cannot resolve package destination parent: {error}"))?;
    (0..32)
        .find_map(|_| {
            let path = parent.join(format!(
                ".cocobeat-package-{}-{}",
                std::process::id(),
                NEXT_STAGING.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::symlink_metadata(&path) {
                Ok(_) => None,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Ok(path)),
                Err(error) => Some(Err(format!("Cannot inspect staging path: {error}"))),
            }
        })
        .ok_or("Cannot allocate a unique package staging directory after 32 attempts")?
}

fn publish_package(staging: &Path, destination: &Path) -> Result<ValidatedPackage, String> {
    publish_package_checked(staging, destination, &|| Ok(()))
}

fn publish_package_checked(
    staging: &Path,
    destination: &Path,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<ValidatedPackage, String> {
    check()?;
    let (validated, _) = read_package_snapshot_checked(staging, |_| Ok(()), check)?;
    publish_validated_package_checked(staging, destination, validated, check)
}

fn publish_validated_package(
    staging: &Path,
    destination: &Path,
    validated: ValidatedPackage,
) -> Result<ValidatedPackage, String> {
    publish_validated_package_checked(staging, destination, validated, &|| Ok(()))
}

fn publish_validated_package_checked(
    staging: &Path,
    destination: &Path,
    validated: ValidatedPackage,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<ValidatedPackage, String> {
    require_absent(destination)?;
    check()?;
    // Concurrent builders publish nonempty directories, which rename cannot replace
    fs::rename(staging, destination)
        .map_err(|error| format!("Cannot publish package directory: {error}"))?;
    Ok(validated)
}

fn require_absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(format!(
            "Package destination already exists: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Cannot inspect package destination: {error}")),
    }
}

fn open_object(path: &Path, max_bytes: u64) -> Result<(File, u64), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(format!(
            "Object must be a bounded regular file: {}",
            path.display()
        ));
    }
    let file = File::open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err("Opened object is not a bounded regular file".into());
    }
    Ok((file, metadata.len()))
}

fn read_object_checked(
    path: &Path,
    max_bytes: usize,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    check()?;
    let (mut file, length) = open_object(path, max_bytes as u64)?;
    let mut bytes = vec![0; length as usize];
    for chunk in bytes.chunks_mut(32 * 1024) {
        check()?;
        file.read_exact(chunk).map_err(|error| error.to_string())?;
    }
    check()?;
    if file.read(&mut [0]).map_err(|error| error.to_string())? != 0 {
        return Err("Object changed length while being read".into());
    }
    Ok(bytes)
}

fn read_referenced(root: &Path, reference: &AssetRef, max_bytes: usize) -> Result<Vec<u8>, String> {
    read_referenced_checked(root, reference, max_bytes, &|| Ok(()))
}

fn read_referenced_checked(
    root: &Path,
    reference: &AssetRef,
    max_bytes: usize,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    let bytes = read_object_checked(&root.join(&reference.file_name), max_bytes, check)?;
    let mut hasher = blake3::Hasher::new();
    for chunk in bytes.chunks(32 * 1024) {
        check()?;
        hasher.update(chunk);
    }
    if bytes.len() as u64 != reference.byte_len || *hasher.finalize().as_bytes() != reference.blake3
    {
        return Err(format!("Object identity mismatch: {}", reference.file_name));
    }
    Ok(bytes)
}

fn write_object(
    root: &Path,
    name: &str,
    bytes: &[u8],
    created: &mut Vec<PathBuf>,
) -> Result<AssetRef, String> {
    write_object_checked(root, name, bytes, created, &|| Ok(()))
}

fn write_object_checked(
    root: &Path,
    name: &str,
    bytes: &[u8],
    created: &mut Vec<PathBuf>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<AssetRef, String> {
    check()?;
    let path = root.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("Cannot create package object {name}: {error}"))?;
    created.push(path);
    let mut hasher = blake3::Hasher::new();
    for chunk in bytes.chunks(32 * 1024) {
        check()?;
        file.write_all(chunk).map_err(|error| error.to_string())?;
        hasher.update(chunk);
    }
    check()?;
    file.sync_all().map_err(|error| error.to_string())?;
    check()?;
    Ok(AssetRef {
        file_name: name.into(),
        byte_len: bytes.len() as u64,
        blake3: *hasher.finalize().as_bytes(),
    })
}

fn cleanup_staging(staging: &Path, created: &[PathBuf], mut error: String) -> String {
    for path in created.iter().rev() {
        if let Err(cleanup) = fs::remove_file(path)
            && cleanup.kind() != std::io::ErrorKind::NotFound
        {
            error.push_str(&format!("; cannot remove {}: {cleanup}", path.display()));
        }
    }
    if let Err(cleanup) = fs::remove_dir(staging) {
        error.push_str(&format!(
            "; cannot remove empty staging directory: {cleanup}"
        ));
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_canonical;
    use cocobeat_schema::{EnergySample, SectionCue, SongTime};
    use std::sync::{Arc, Barrier};

    const STEREO: &[u8] =
        include_bytes!("../../../testdata/synthetic/media-import/stereo-canonical.ogg");
    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "cocobeat-package-test-{}-{}",
                std::process::id(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn source_and_input(&self) -> (PathBuf, PackageBuildInput) {
            let source = self.0.join("source.ogg");
            fs::write(&source, STEREO).unwrap();
            let energy = measured_energy(&source, 4_800).unwrap();
            let hash = *blake3::hash(STEREO).as_bytes();
            (
                source,
                PackageBuildInput {
                    song_id: "measured-stereo-fixture".into(),
                    importer_version: "fixture-v1".into(),
                    analysis_version: "measured-energy-v1".into(),
                    chart_version: "hand-authored-v1".into(),
                    analysis: MusicAnalysis {
                        presentation: None,
                        capabilities: None,
                        tempo_regions: Vec::new(),
                        repetitions: Vec::new(),
                        schema_version: CONTENT_SCHEMA_VERSION,
                        audio_hash: hash,
                        beats: vec![],
                        onsets: vec![],
                        sections: vec![],
                        energy: vec![energy],
                        diagnostics: "Measured whole-file energy; no beat or onset inference"
                            .into(),
                    },
                    chart: CompiledChart {
                        schema_version: CONTENT_SCHEMA_VERSION,
                        audio_hash: hash,
                        ruleset_id: "manual-fixture-v1".into(),
                        anchors: [1_200, 3_600]
                            .into_iter()
                            .enumerate()
                            .map(|(id, frame)| Anchor {
                                id: id as u64,
                                song_time: SongTime::from_frames(frame),
                            })
                            .collect(),
                        sections: vec![],
                    },
                },
            )
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn measured_energy(path: &Path, frames: u64) -> Result<EnergySample, String> {
        let mut energy = [0.0_f64; 2];
        let mut peak = [0.0_f32; 2];
        decode_canonical(path, frames, |block| {
            for sample in block {
                for channel in 0..2 {
                    energy[channel] += f64::from(sample[channel]).powi(2);
                    peak[channel] = peak[channel].max(sample[channel].abs());
                }
            }
            Ok(())
        })?;
        Ok(EnergySample {
            start: SongTime::ZERO,
            frames: frames as u32,
            rms: energy.map(|sum| (sum / frames as f64).sqrt() as f32),
            peak,
        })
    }

    fn refresh_reference(root: &Path, reference: &mut AssetRef) {
        let bytes = fs::read(root.join(&reference.file_name)).unwrap();
        reference.byte_len = bytes.len() as u64;
        reference.blake3 = *blake3::hash(&bytes).as_bytes();
    }

    fn save_manifest(root: &Path, manifest: &mut SongPackage) {
        manifest.package_hash = package_hash(manifest).unwrap();
        fs::write(
            root.join(PACKAGE_OBJECT_NAMES[3]),
            encode_package(manifest).unwrap(),
        )
        .unwrap();
    }

    fn rewrite_package(root: &Path, package: &mut ValidatedPackage) {
        fs::write(
            root.join(PACKAGE_OBJECT_NAMES[1]),
            encode_analysis(&package.analysis, package.manifest.canonical_frames).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join(PACKAGE_OBJECT_NAMES[2]),
            encode_chart(&package.chart, package.manifest.canonical_frames).unwrap(),
        )
        .unwrap();
        for reference in [
            &mut package.manifest.audio,
            &mut package.manifest.analysis,
            &mut package.manifest.chart,
        ] {
            refresh_reference(root, reference);
        }
        save_manifest(root, &mut package.manifest);
    }

    fn object_bytes(root: &Path) -> [Vec<u8>; 4] {
        PACKAGE_OBJECT_NAMES.map(|name| fs::read(root.join(name)).unwrap())
    }

    fn noncanonical_version(bytes: &mut Vec<u8>) {
        assert_eq!(bytes[20], 1);
        bytes.splice(20..21, [0x81, 0]);
        let payload_len = (bytes.len() - 20) as u64;
        bytes[12..20].copy_from_slice(&payload_len.to_le_bytes());
    }

    #[test]
    fn received_packages_preserve_raw_objects_and_validate_before_publication() {
        let root = TestDirectory::new();
        let (audio, input) = root.source_and_input();
        let source = root.0.join("source-package");
        let mut package = build_package(audio, 4_800, &source, |_, _| Ok(input)).unwrap();
        for name in &PACKAGE_OBJECT_NAMES[1..3] {
            let path = source.join(name);
            let mut bytes = fs::read(&path).unwrap();
            noncanonical_version(&mut bytes);
            fs::write(path, bytes).unwrap();
        }
        refresh_reference(&source, &mut package.manifest.analysis);
        refresh_reference(&source, &mut package.manifest.chart);
        save_manifest(&source, &mut package.manifest);
        let manifest_path = source.join(PACKAGE_OBJECT_NAMES[3]);
        let mut manifest_bytes = fs::read(&manifest_path).unwrap();
        noncanonical_version(&mut manifest_bytes);
        fs::write(manifest_path, manifest_bytes).unwrap();
        assert_eq!(validate_package(&source).unwrap(), package);
        let objects = object_bytes(&source);
        let (validated, references) = validate_package_objects(&source).unwrap();
        assert_eq!(validated, package);
        for (index, reference) in references.iter().enumerate() {
            assert_eq!(reference.file_name, PACKAGE_OBJECT_NAMES[index]);
            assert_eq!(reference.byte_len, objects[index].len() as u64);
            assert_eq!(reference.blake3, *blake3::hash(&objects[index]).as_bytes());
        }
        assert_ne!(
            references[3].blake3,
            *blake3::hash(&encode_package(&package.manifest).unwrap()).as_bytes()
        );
        let destination = root.0.join("received");
        let mut received = ReceivedPackage::new(
            &destination,
            package.manifest.package_hash,
            objects.each_ref().map(|bytes| bytes.len() as u64),
        )
        .unwrap();
        for (index, bytes) in objects.iter().enumerate() {
            assert!(!destination.exists());
            for chunk in bytes.chunks(17) {
                received.write(index, chunk).unwrap();
            }
        }
        assert!(!destination.exists());
        assert_eq!(received.finish().unwrap(), package);
        assert_eq!(object_bytes(&destination), objects);
        assert_eq!(validate_package(destination).unwrap(), package);
    }

    #[test]
    fn received_packages_reject_lengths_bad_chunks_and_cancel_only_owned_files() {
        let root = TestDirectory::new();
        let destination = root.0.join("received");
        for index in 0..4 {
            for length in [0, PACKAGE_OBJECT_LIMITS[index] + 1, u64::MAX] {
                let mut lengths = [1; 4];
                lengths[index] = length;
                assert!(ReceivedPackage::new(&destination, [0; 32], lengths).is_err());
                assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
            }
        }
        for (index, bytes) in [(1, &[1][..]), (4, &[1][..]), (0, &[1, 2][..]), (0, &[][..])] {
            let mut received = ReceivedPackage::new(&destination, [0; 32], [1; 4]).unwrap();
            assert!(received.write(index, bytes).is_err());
            assert_eq!(
                fs::metadata(received.staging.join(PACKAGE_OBJECT_NAMES[0]))
                    .unwrap()
                    .len(),
                0
            );
            assert!(received.write(0, &[1]).is_err());
            assert!(received.finish().is_err());
            assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
        }
        let mut duplicate = ReceivedPackage::new(&destination, [0; 32], [1; 4]).unwrap();
        duplicate.write(0, &[1]).unwrap();
        assert!(duplicate.write(0, &[1]).is_err());
        drop(duplicate);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
        let mut cancelled = ReceivedPackage::new(&destination, [0; 32], [2; 4]).unwrap();
        cancelled.write(0, &[1]).unwrap();
        let staging = cancelled.staging.clone();
        let foreign = staging.join("foreign");
        fs::write(&foreign, b"preserve").unwrap();
        drop(cancelled);
        assert!(!destination.exists());
        assert_eq!(fs::read(&foreign).unwrap(), b"preserve");
        assert_eq!(fs::read_dir(staging).unwrap().count(), 1);
    }

    #[test]
    fn received_packages_reject_corruption_wrong_identity_and_existing_destinations() {
        let root = TestDirectory::new();
        let (audio, input) = root.source_and_input();
        let source = root.0.join("source-package");
        let package = build_package(audio, 4_800, &source, |_, _| Ok(input)).unwrap();
        let objects = object_bytes(&source);
        let lengths = objects.each_ref().map(|bytes| bytes.len() as u64);
        let destination = root.0.join("received");
        for bad_index in 0..6 {
            let mut expected = package.manifest.package_hash;
            let mut bad_objects = objects.clone();
            if bad_index < 4 {
                bad_objects[bad_index][0] ^= 1;
            } else if bad_index == 4 {
                expected[0] ^= 1;
            }
            let mut received = ReceivedPackage::new(&destination, expected, lengths).unwrap();
            for (index, bytes) in bad_objects.iter().enumerate() {
                if bad_index == 5 && index == 3 {
                    break;
                }
                received.write(index, bytes).unwrap();
            }
            assert!(received.finish().is_err());
            assert!(!destination.exists());
            assert_eq!(fs::read_dir(&root.0).unwrap().count(), 2);
        }
        let mut received =
            ReceivedPackage::new(&destination, package.manifest.package_hash, lengths).unwrap();
        for (index, bytes) in objects.iter().enumerate() {
            received.write(index, bytes).unwrap();
        }
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("sentinel"), b"keep").unwrap();
        assert!(received.finish().is_err());
        assert_eq!(fs::read(destination.join("sentinel")).unwrap(), b"keep");
        assert!(
            ReceivedPackage::new(&destination, package.manifest.package_hash, lengths).is_err()
        );
        assert_eq!(object_bytes(&source), objects);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn received_packages_preserve_destination_aliases_and_freeze_parent_resolution() {
        use std::os::unix::fs::symlink;
        let root = TestDirectory::new();
        let (audio, input) = root.source_and_input();
        let source = root.0.join("source-package");
        let package = build_package(audio, 4_800, &source, |_, _| Ok(input)).unwrap();
        let objects = object_bytes(&source);
        let lengths = objects.each_ref().map(|bytes| bytes.len() as u64);
        let alias = root.0.join("alias");
        for target in [&source, &root.0.join("missing")] {
            symlink(target, &alias).unwrap();
            assert!(ReceivedPackage::new(&alias, package.manifest.package_hash, lengths).is_err());
            assert!(fs::symlink_metadata(&alias).unwrap().is_symlink());
            fs::remove_file(&alias).unwrap();
        }
        let first = root.0.join("first");
        let second = root.0.join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        symlink(&first, &alias).unwrap();
        let mut received = ReceivedPackage::new(
            alias.join("received"),
            package.manifest.package_hash,
            lengths,
        )
        .unwrap();
        fs::remove_file(&alias).unwrap();
        symlink(&second, &alias).unwrap();
        for (index, bytes) in objects.iter().enumerate() {
            received.write(index, bytes).unwrap();
        }
        assert_eq!(received.finish().unwrap(), package);
        assert_eq!(object_bytes(&first.join("received")), objects);
        assert!(!second.join("received").exists());
        assert_eq!(object_bytes(&source), objects);
    }

    #[test]
    fn anchor_exports_preserve_raw_objects_and_change_only_the_authored_chart() {
        let root = TestDirectory::new();
        let (audio, mut input) = root.source_and_input();
        input.chart.sections.push(SectionCue {
            id: 17,
            time: SongTime::from_frames(2_300),
            label: "Authored cue independent of Anchors".into(),
        });
        let source = root.0.join("source-package");
        let mut package = build_package(audio, 4_800, &source, |_, _| Ok(input)).unwrap();
        for name in &PACKAGE_OBJECT_NAMES[1..3] {
            let path = source.join(name);
            let mut bytes = fs::read(&path).unwrap();
            noncanonical_version(&mut bytes);
            fs::write(path, bytes).unwrap();
        }
        refresh_reference(&source, &mut package.manifest.analysis);
        refresh_reference(&source, &mut package.manifest.chart);
        save_manifest(&source, &mut package.manifest);
        let mut manifest_bytes = fs::read(source.join(PACKAGE_OBJECT_NAMES[3])).unwrap();
        noncanonical_version(&mut manifest_bytes);
        fs::write(source.join(PACKAGE_OBJECT_NAMES[3]), &manifest_bytes).unwrap();
        assert_eq!(validate_package(&source).unwrap(), package);
        let original = object_bytes(&source);
        assert_ne!(encode_package(&package.manifest).unwrap(), original[3]);
        assert_ne!(encode_chart(&package.chart, 4_800).unwrap(), original[2]);
        assert_ne!(
            encode_analysis(&package.analysis, 4_800).unwrap(),
            original[1]
        );

        let no_op = root.0.join("no-op");
        assert_eq!(
            export_anchors(
                &source,
                package.manifest.package_hash,
                &package.chart.anchors,
                &no_op
            )
            .unwrap(),
            package
        );
        assert_eq!(object_bytes(&no_op), original);

        let anchors = [
            Anchor {
                id: 9,
                song_time: SongTime::ZERO,
            },
            Anchor {
                id: 0,
                song_time: SongTime::from_frames(4_799),
            },
            Anchor {
                id: 1,
                song_time: SongTime::from_frames(4_799),
            },
        ];
        let mut edited_bytes = None;
        for name in ["edited", "repeated"] {
            let destination = root.0.join(name);
            let edited = export_anchors(
                &source,
                package.manifest.package_hash,
                &anchors,
                &destination,
            )
            .unwrap();
            assert_eq!(validate_package(&destination).unwrap(), edited);
            let actual = object_bytes(&destination);
            assert_eq!(actual[..2], original[..2]);
            assert_ne!(actual[2], original[2]);
            assert_ne!(actual[3], original[3]);
            assert_ne!(edited.manifest.package_hash, package.manifest.package_hash);
            assert_eq!(edited.manifest.chart_version, "manual-editor-v1");
            assert_eq!(edited.chart.anchors, anchors);
            let mut unchanged = edited;
            unchanged.chart.anchors = package.chart.anchors.clone();
            unchanged.manifest.chart = package.manifest.chart.clone();
            unchanged.manifest.chart_version = package.manifest.chart_version.clone();
            unchanged.manifest.package_hash = package.manifest.package_hash;
            assert_eq!(unchanged, package);
            if let Some(previous) = &edited_bytes {
                assert_eq!(&actual, previous);
            }
            edited_bytes = Some(actual);
        }
        assert_eq!(object_bytes(&source), original);
    }

    #[test]
    fn export_copies_are_bound_to_the_original_validated_snapshot() {
        let root = TestDirectory::new();
        let (audio, input) = root.source_and_input();
        let source = root.0.join("source-package");
        let original = build_package(audio, 4_800, &source, |_, _| Ok(input)).unwrap();
        let original_bytes = object_bytes(&source);
        let destination = root.0.join("copy");

        // The canonical manifest identity does not identify its original wire encoding
        let (snapshot, manifest_bytes) = read_package_snapshot(&source, |_| {
            let mut bytes = original_bytes[3].clone();
            noncanonical_version(&mut bytes);
            fs::write(source.join(PACKAGE_OBJECT_NAMES[3]), bytes).unwrap();
            Ok(())
        })
        .unwrap();
        assert_eq!(validate_package(&source).unwrap(), original);
        assert_eq!(manifest_bytes, original_bytes[3]);
        publish_anchors(
            &source,
            snapshot,
            manifest_bytes,
            &original.chart.anchors,
            &destination,
        )
        .unwrap();
        assert_eq!(object_bytes(&destination), original_bytes);
        fs::remove_dir_all(&destination).unwrap();
        fs::write(source.join(PACKAGE_OBJECT_NAMES[3]), &original_bytes[3]).unwrap();

        for index in [1, 2] {
            let (snapshot, manifest_bytes) = read_package_snapshot(&source, |_| Ok(())).unwrap();
            let mut changed = original_bytes[index].clone();
            noncanonical_version(&mut changed);
            fs::write(source.join(PACKAGE_OBJECT_NAMES[index]), changed).unwrap();
            let error = publish_anchors(
                &source,
                snapshot,
                manifest_bytes,
                &original.chart.anchors,
                &destination,
            )
            .unwrap_err();
            assert!(error.contains("Object identity mismatch"));
            assert!(!destination.exists());
            assert_eq!(fs::read_dir(&root.0).unwrap().count(), 2);
            fs::write(
                source.join(PACKAGE_OBJECT_NAMES[index]),
                &original_bytes[index],
            )
            .unwrap();
        }

        // Retag each Ogg page and recompute CRC: valid PCM, different audio object identity
        let mut changed_audio = original_bytes[0].clone();
        let mut offset = 0;
        while offset < changed_audio.len() {
            let segments = changed_audio[offset + 26] as usize;
            let payload: usize = changed_audio[offset + 27..offset + 27 + segments]
                .iter()
                .map(|size| usize::from(*size))
                .sum();
            let end = offset + 27 + segments + payload;
            let page = &mut changed_audio[offset..end];
            page[14] ^= 1;
            page[22..26].fill(0);
            let mut crc = 0_u32;
            for byte in page.iter() {
                crc ^= u32::from(*byte) << 24;
                for _ in 0..8 {
                    crc = (crc << 1)
                        ^ if crc & 0x8000_0000 != 0 {
                            0x04c1_1db7
                        } else {
                            0
                        };
                }
            }
            page[22..26].copy_from_slice(&crc.to_le_bytes());
            offset = end;
        }
        let (snapshot, manifest_bytes) = read_package_snapshot(&source, |_| Ok(())).unwrap();
        fs::write(source.join(PACKAGE_OBJECT_NAMES[0]), &changed_audio).unwrap();
        decode_canonical(source.join(PACKAGE_OBJECT_NAMES[0]), 4_800, |_| Ok(())).unwrap();
        let error = publish_anchors(
            &source,
            snapshot,
            manifest_bytes,
            &original.chart.anchors,
            &destination,
        )
        .unwrap_err();
        assert!(error.contains("Copied audio identity differs"));
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 2);
        assert_eq!(
            fs::read(source.join(PACKAGE_OBJECT_NAMES[0])).unwrap(),
            changed_audio
        );
    }

    #[test]
    fn export_rejects_stale_identity_invalid_anchors_and_existing_destinations() {
        let root = TestDirectory::new();
        let (audio, input) = root.source_and_input();
        let source = root.0.join("source-package");
        let package = build_package(audio, 4_800, &source, |_, _| Ok(input)).unwrap();
        let original = object_bytes(&source);
        let destination = root.0.join("copy");
        assert!(
            export_anchors(&source, [0; 32], &[], &destination)
                .unwrap_err()
                .contains("identity")
        );
        for anchors in [
            vec![Anchor {
                id: 3,
                song_time: SongTime::from_frames(-1),
            }],
            vec![Anchor {
                id: 3,
                song_time: SongTime::from_frames(4_800),
            }],
            vec![Anchor {
                id: 3,
                song_time: SongTime::from_frames(i64::MAX),
            }],
            vec![package.chart.anchors[1], package.chart.anchors[0]],
            vec![package.chart.anchors[0]; 2],
            vec![package.chart.anchors[0]; MAX_CONTENT_ITEMS + 1],
        ] {
            assert!(
                export_anchors(
                    &source,
                    package.manifest.package_hash,
                    &anchors,
                    &destination
                )
                .is_err()
            );
            assert!(!destination.exists());
            assert_eq!(fs::read_dir(&root.0).unwrap().count(), 2);
        }
        for destination in [&source, &root.0.join("source.ogg")] {
            assert!(
                export_anchors(&source, package.manifest.package_hash, &[], destination).is_err()
            );
        }
        for destination in [source.join("copy"), source.join("./copy")] {
            assert!(
                export_anchors(&source, package.manifest.package_hash, &[], destination)
                    .unwrap_err()
                    .contains("outside the source package")
            );
        }
        for parent in [root.0.join("missing"), root.0.join("source.ogg")] {
            assert!(
                export_anchors(
                    &source,
                    package.manifest.package_hash,
                    &[],
                    parent.join("copy")
                )
                .is_err()
            );
        }
        fs::create_dir(&destination).unwrap();
        assert!(export_anchors(&source, package.manifest.package_hash, &[], &destination).is_err());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        fs::write(destination.join("keep"), b"existing contents").unwrap();
        assert!(export_anchors(&source, package.manifest.package_hash, &[], &destination).is_err());
        assert_eq!(
            fs::read(destination.join("keep")).unwrap(),
            b"existing contents"
        );
        assert_eq!(object_bytes(&source), original);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 3);
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let alias = root.0.join("alias");
            symlink(&source, &alias).unwrap();
            assert!(
                export_anchors(
                    &source,
                    package.manifest.package_hash,
                    &[],
                    alias.join("copy")
                )
                .unwrap_err()
                .contains("outside the source package")
            );
            assert!(export_anchors(&source, package.manifest.package_hash, &[], &alias).is_err());
            assert!(
                export_anchors(
                    &alias,
                    package.manifest.package_hash,
                    &[],
                    root.0.join("other")
                )
                .is_err()
            );
            fs::remove_file(&alias).unwrap();
            symlink(root.0.join("missing"), &alias).unwrap();
            assert!(export_anchors(&source, package.manifest.package_hash, &[], &alias).is_err());
            assert!(fs::symlink_metadata(&alias).unwrap().is_symlink());
            assert_eq!(object_bytes(&source), original);
            assert_eq!(validate_package(&source).unwrap(), package);
        }
    }

    #[test]
    fn real_stereo_energy_and_manual_chart_roundtrip_without_replacing_destinations() {
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("ready");
        let package =
            build_package(&source, 4_800, &destination, |_, _| Ok(input.clone())).unwrap();
        assert_eq!(validate_package(&destination).unwrap(), package);
        assert_eq!(package.analysis, input.analysis);
        assert_eq!(package.chart, input.chart);
        assert!(
            package.analysis.energy[0]
                .rms
                .into_iter()
                .all(|rms| rms > 0.01)
        );
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 4);
        assert_eq!(fs::read(&source).unwrap(), STEREO);
        for reference in [
            &package.manifest.audio,
            &package.manifest.analysis,
            &package.manifest.chart,
        ] {
            let bytes = fs::read(destination.join(&reference.file_name)).unwrap();
            assert_eq!(reference.byte_len, bytes.len() as u64);
            assert_eq!(reference.blake3, *blake3::hash(&bytes).as_bytes());
        }
        assert!(build_package(&source, 4_800, &destination, |_, _| Ok(input.clone())).is_err());
        assert_eq!(validate_package(&destination).unwrap(), package);
        let empty = root.0.join("existing-empty");
        fs::create_dir(&empty).unwrap();
        assert!(build_package(&source, 4_800, &empty, |_, _| Ok(input)).is_err());
        assert!(empty.is_dir());
        assert_eq!(fs::read_dir(empty).unwrap().count(), 0);
    }

    #[test]
    fn content_uses_only_prepared_audio_and_callback_errors_clean_staging() {
        let root = TestDirectory::new();
        for remove_source in [false, true] {
            let (source, mut input) = root.source_and_input();
            let expected = input.analysis.clone();
            let destination = root.0.join(format!("ready-{remove_source}"));
            let package = build_package(&source, 4_800, &destination, |staged, prepared| {
                assert_ne!(staged, source);
                assert_eq!(prepared.canonical_frames, 4_800);
                assert_eq!(prepared.asset.blake3, *blake3::hash(STEREO).as_bytes());
                if remove_source {
                    fs::remove_file(&source).unwrap();
                } else {
                    fs::write(&source, b"replacement is not an Ogg").unwrap();
                }
                input.analysis.energy = vec![measured_energy(staged, prepared.canonical_frames)?];
                input.analysis.audio_hash = prepared.asset.blake3;
                input.chart.audio_hash = prepared.asset.blake3;
                Ok(input)
            })
            .unwrap();
            assert_eq!(package.analysis, expected);
            assert_eq!(
                fs::read(destination.join(PACKAGE_OBJECT_NAMES[0])).unwrap(),
                STEREO
            );
            assert_eq!(validate_package(destination).unwrap(), package);
        }

        let (source, _) = root.source_and_input();
        let destination = root.0.join("failed");
        let mut staged_path = None;
        let error = build_package(&source, 4_800, &destination, |staged, _| {
            staged_path = Some(staged.to_owned());
            Err("analysis callback failed".into())
        })
        .unwrap_err();
        assert_eq!(error, "analysis callback failed");
        assert!(!staged_path.unwrap().parent().unwrap().exists());
        assert!(!destination.exists());
        assert_eq!(fs::read(source).unwrap(), STEREO);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 3);
    }

    #[test]
    fn package_pcm_matches_strict_readback_when_the_audio_path_changes_during_consumption() {
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("ready");
        let package = build_package(&source, 4_800, &destination, |_, _| Ok(input)).unwrap();
        let audio_path = destination.join(PACKAGE_OBJECT_NAMES[0]);
        let mut expected = Vec::new();
        decode_canonical(&audio_path, 4_800, |block| {
            expected.extend_from_slice(block);
            Ok(())
        })
        .unwrap();

        for remove_audio in [false, true] {
            fs::write(&audio_path, STEREO).unwrap();
            let mut actual = Vec::new();
            let mut callbacks = 0;
            let validated = read_package(&destination, |block| {
                if callbacks == 0 {
                    if remove_audio {
                        fs::remove_file(&audio_path).unwrap();
                    } else {
                        fs::write(&audio_path, b"different bytes after snapshot").unwrap();
                    }
                }
                callbacks += 1;
                actual.extend_from_slice(block);
                Ok(())
            })
            .unwrap();
            assert!(callbacks > 1);
            assert_eq!(actual, expected);
            assert_eq!(validated, package);
            assert!(validate_package(&destination).is_err());
        }
    }

    #[test]
    fn package_readback_propagates_cancellation_and_rejects_objects_before_pcm() {
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("ready");
        let mut package = build_package(&source, 4_800, &destination, |_, _| Ok(input)).unwrap();
        let mut callbacks = 0;
        let result = read_package(&destination, |_| {
            callbacks += 1;
            Err("consumer cancelled".into())
        });
        assert_eq!(result.unwrap_err(), "consumer cancelled");
        assert_eq!(callbacks, 1);

        for name in PACKAGE_OBJECT_NAMES {
            let path = destination.join(name);
            let bytes = fs::read(&path).unwrap();
            let mut changed = bytes.clone();
            changed[bytes.len() / 2] ^= 1;
            fs::write(&path, changed).unwrap();
            assert!(
                read_package(&destination, |_| panic!("invalid object delivered PCM")).is_err()
            );
            fs::write(path, bytes).unwrap();
        }

        // Even self-consistent object hashes cannot make a broken Ogg page valid
        let mut corrupt_audio = STEREO.to_vec();
        *corrupt_audio.last_mut().unwrap() ^= 1;
        let hash = *blake3::hash(&corrupt_audio).as_bytes();
        fs::write(destination.join(PACKAGE_OBJECT_NAMES[0]), corrupt_audio).unwrap();
        package.analysis.audio_hash = hash;
        package.chart.audio_hash = hash;
        rewrite_package(&destination, &mut package);
        assert!(read_package(&destination, |_| panic!("invalid CRC delivered PCM")).is_err());
    }

    #[test]
    fn rejects_changed_object_bytes_lengths_bounds_and_manifest_identity() {
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("ready");
        let package = build_package(source, 4_800, &destination, |_, _| Ok(input)).unwrap();
        for (name, limit) in PACKAGE_OBJECT_NAMES.into_iter().zip([
            MAX_SOURCE_BYTES,
            MAX_ANALYSIS_BYTES as u64,
            MAX_CHART_BYTES as u64,
            MAX_PACKAGE_BYTES as u64,
        ]) {
            let path = destination.join(name);
            let original = fs::read(&path).unwrap();
            let mut changed = original.clone();
            changed[original.len() / 2] ^= 0x80;
            fs::write(&path, changed).unwrap();
            assert!(validate_package(&destination).is_err(), "changed {name}");
            fs::write(&path, &original[..original.len() - 1]).unwrap();
            assert!(validate_package(&destination).is_err(), "truncated {name}");
            OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(limit + 1)
                .unwrap();
            assert!(validate_package(&destination).is_err(), "oversized {name}");
            fs::write(path, original).unwrap();
        }
        let mut manifest = package.manifest;
        manifest.package_hash[0] ^= 1;
        fs::write(
            destination.join(PACKAGE_OBJECT_NAMES[3]),
            encode_package(&manifest).unwrap(),
        )
        .unwrap();
        assert!(
            validate_package(&destination)
                .unwrap_err()
                .contains("manifest hash")
        );
    }

    #[test]
    fn rejects_cross_audio_references_bad_canonical_bytes_and_wrong_decoded_frames() {
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("ready");
        let original = build_package(&source, 4_800, &destination, |_, _| Ok(input)).unwrap();
        for analysis_mismatch in [true, false] {
            let mut package = original.clone();
            if analysis_mismatch {
                package.analysis.audio_hash[0] ^= 1;
            } else {
                package.chart.audio_hash[0] ^= 1;
            }
            rewrite_package(&destination, &mut package);
            assert!(
                validate_package(&destination)
                    .unwrap_err()
                    .contains("different canonical audio")
            );
        }
        let mut package = original.clone();
        package.manifest.canonical_frames += 1;
        package.analysis.energy[0].frames += 1;
        rewrite_package(&destination, &mut package);
        assert!(validate_package(&destination).is_err());

        let mut package = original;
        fs::write(
            destination.join(PACKAGE_OBJECT_NAMES[0]),
            b"not canonical audio",
        )
        .unwrap();
        let hash = *blake3::hash(b"not canonical audio").as_bytes();
        package.analysis.audio_hash = hash;
        package.chart.audio_hash = hash;
        rewrite_package(&destination, &mut package);
        assert!(validate_package(&destination).is_err());
    }

    #[test]
    fn rejects_unsupported_envelopes_and_escaping_references() {
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("ready");
        let package = build_package(source, 4_800, &destination, |_, _| Ok(input)).unwrap();
        for name in &PACKAGE_OBJECT_NAMES[1..] {
            let path = destination.join(name);
            let original = fs::read(&path).unwrap();
            let mut unsupported = original.clone();
            unsupported[8..12].copy_from_slice(&2_u32.to_le_bytes());
            fs::write(&path, unsupported).unwrap();
            let mut manifest = package.manifest.clone();
            if *name != PACKAGE_OBJECT_NAMES[3] {
                refresh_reference(&destination, &mut manifest.analysis);
                refresh_reference(&destination, &mut manifest.chart);
                save_manifest(&destination, &mut manifest);
            }
            assert!(
                validate_package(&destination).is_err(),
                "unsupported {name}"
            );
            fs::write(path, original).unwrap();
            save_manifest(&destination, &mut package.manifest.clone());
        }
        let manifest_path = destination.join(PACKAGE_OBJECT_NAMES[3]);
        let mut bytes = fs::read(&manifest_path).unwrap();
        let name = b"song.audio.ogg";
        let offset = bytes
            .windows(name.len())
            .position(|part| part == name)
            .unwrap();
        bytes[offset..offset + name.len()].copy_from_slice(b"../outside.ogg");
        fs::write(manifest_path, bytes).unwrap();
        assert!(validate_package(destination).is_err());
    }

    #[test]
    fn failures_clean_only_created_files_and_incomplete_staging_is_not_valid() {
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("ready");
        let mut wrong_reference = input.clone();
        wrong_reference.analysis.audio_hash[0] ^= 1;
        assert!(build_package(&source, 4_800, &destination, |_, _| Ok(wrong_reference)).is_err());
        let mut wrong_version = input.clone();
        wrong_version.chart.schema_version += 1;
        assert!(build_package(&source, 4_800, &destination, |_, _| Ok(wrong_version)).is_err());
        let mut late_error = input.clone();
        late_error.importer_version.clear();
        assert!(build_package(&source, 4_800, &destination, |_, _| Ok(late_error)).is_err());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
        assert!(!destination.exists());
        assert_eq!(fs::read(&source).unwrap(), STEREO);

        let interrupted = root.0.join("interrupted");
        crate::prepare_canonical_audio(&source, 4_800, &interrupted).unwrap();
        assert!(validate_package(&interrupted).is_err());
        let foreign = interrupted.join("keep");
        fs::write(&foreign, b"not created by the package transaction").unwrap();
        cleanup_staging(
            &interrupted,
            &[interrupted.join(PACKAGE_OBJECT_NAMES[0])],
            "failed".into(),
        );
        assert!(foreign.is_file());
        assert_eq!(fs::read_dir(&interrupted).unwrap().count(), 1);
        fs::write(&source, b"bad Ogg").unwrap();
        assert!(build_package(&source, 4_800, &destination, |_, _| Ok(input)).is_err());
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 2);
    }

    #[test]
    fn concurrent_builders_publish_one_complete_winner() {
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("ready");
        let barrier = Arc::new(Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|id| {
                let source = source.clone();
                let destination = destination.clone();
                let mut input = input.clone();
                input.song_id = format!("candidate-{id}");
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    build_package(source, 4_800, destination, |_, _| Ok(input))
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let winner = results.into_iter().find_map(Result::ok).unwrap();
        assert_eq!(validate_package(destination).unwrap(), winner);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_objects_roots_and_destinations() {
        use std::os::unix::fs::symlink;
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("ready");
        build_package(&source, 4_800, &destination, |_, _| Ok(input.clone())).unwrap();
        for name in PACKAGE_OBJECT_NAMES {
            let object = destination.join(name);
            let outside = root.0.join(name);
            fs::rename(&object, &outside).unwrap();
            symlink(&outside, &object).unwrap();
            assert!(validate_package(&destination).is_err());
            fs::remove_file(&object).unwrap();
            fs::create_dir(&object).unwrap();
            assert!(validate_package(&destination).is_err());
            fs::remove_dir(&object).unwrap();
            fs::rename(outside, object).unwrap();
        }
        let alias = root.0.join("alias");
        symlink(&destination, &alias).unwrap();
        assert!(validate_package(&alias).is_err());
        assert!(build_package(&source, 4_800, &alias, |_, _| Ok(input.clone())).is_err());
        fs::remove_file(&alias).unwrap();
        symlink(root.0.join("missing"), &alias).unwrap();
        assert!(build_package(source, 4_800, &alias, |_, _| Ok(input)).is_err());
        assert!(fs::symlink_metadata(alias).unwrap().is_symlink());
    }
    #[test]
    fn checked_builder_cancels_before_publication_and_keeps_content_error() {
        use std::cell::Cell;
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let destination = root.0.join("cancelled-package");
        let requested = Cell::new(false);
        let cancelled = build_package_checked(
            &source,
            4_800,
            &destination,
            |_, _| {
                requested.set(true);
                Ok(input.clone())
            },
            &|| {
                if requested.get() {
                    Err("cancel after content".into())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(cancelled.unwrap_err(), "cancel after content");
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
        requested.set(false);
        let failure = build_package_checked(
            &source,
            4_800,
            &destination,
            |_, _| {
                requested.set(true);
                Err("actual content failure".into())
            },
            &|| {
                if requested.get() {
                    Err("cancel requested".into())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(failure.unwrap_err(), "actual content failure");
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
        // All four staged objects have been written, but the cancellation checkpoint still prevents rename
        let failure = build_package_checked(
            &source,
            4_800,
            &destination,
            |_, _| Ok(input.clone()),
            &|| {
                let complete = fs::read_dir(&root.0)
                    .unwrap()
                    .filter_map(Result::ok)
                    .any(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".cocobeat-package-")
                            && fs::metadata(entry.path().join("song.package"))
                                .is_ok_and(|v| v.len() > 0)
                    });
                if complete {
                    Err("cancel complete staged package".into())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(failure.unwrap_err(), "cancel complete staged package");
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
        assert_eq!(fs::read(&source).unwrap(), STEREO);
    }

    #[test]
    fn checked_package_readback_and_final_gate_do_not_publish_after_cancel() {
        use std::cell::Cell;
        let root = TestDirectory::new();
        let (source, input) = root.source_and_input();
        let ordinary = root.0.join("ordinary");
        let checked = root.0.join("checked");
        let public = build_package(&source, 4_800, &ordinary, |_, _| Ok(input.clone())).unwrap();
        let result =
            build_package_checked(&source, 4_800, &checked, |_, _| Ok(input.clone()), &|| {
                Ok(())
            })
            .unwrap();
        assert_eq!(result, public);
        for name in PACKAGE_OBJECT_NAMES {
            assert_eq!(
                fs::read(ordinary.join(name)).unwrap(),
                fs::read(checked.join(name)).unwrap()
            );
        }
        let consumed = Cell::new(false);
        let error = read_package_snapshot_checked(
            &checked,
            |_| {
                consumed.set(true);
                Ok(())
            },
            &|| {
                if consumed.get() {
                    Err("cancel actual package PCM readback".into())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(error, "cancel actual package PCM readback");
        assert!(consumed.get());
        let unpublished = root.0.join("unpublished");
        let error = publish_validated_package_checked(&checked, &unpublished, result, &|| {
            Err("cancel before package rename".into())
        })
        .unwrap_err();
        assert_eq!(error, "cancel before package rename");
        assert!(!unpublished.exists());
        for name in PACKAGE_OBJECT_NAMES {
            assert_eq!(
                fs::read(ordinary.join(name)).unwrap(),
                fs::read(checked.join(name)).unwrap()
            );
        }
    }
}
