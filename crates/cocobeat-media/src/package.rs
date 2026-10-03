//! Publishes and verifies a complete four-object package on one filesystem

use crate::{
    PreparedCanonicalAudio,
    content_codec::{
        MAX_ANALYSIS_BYTES, MAX_CHART_BYTES, MAX_PACKAGE_BYTES, decode_analysis, decode_chart,
        decode_package, encode_analysis, encode_chart, encode_package, package_hash,
    },
    decode::{MAX_SOURCE_BYTES, decode_canonical_bytes},
    prepare_canonical_audio,
};
use cocobeat_schema::{
    AssetRef, CANONICAL_SAMPLE_RATE, CONTENT_SCHEMA_VERSION, CompiledChart, MusicAnalysis,
    SongPackage,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const FILE_NAMES: [&str; 4] = [
    "song.audio.ogg",
    "analysis.bin",
    "chart.bin",
    "song.package",
];
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

/// Validates the staged final bytes before atomically publishing the whole directory
/// Content is built from the strictly verified staged audio, independent of later source changes
/// Existing destinations are preserved; failed staging removes only this call's own files
pub fn build_package(
    source_audio: impl AsRef<Path>,
    expected_frames: u64,
    destination: impl AsRef<Path>,
    build_content: impl FnOnce(&Path, &PreparedCanonicalAudio) -> Result<PackageBuildInput, String>,
) -> Result<ValidatedPackage, String> {
    let source_audio = source_audio.as_ref();
    let destination = destination.as_ref();
    require_absent(destination)?;
    drop(open_object(source_audio, MAX_SOURCE_BYTES)?);
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if destination.file_name().is_none() {
        return Err("Package destination must name a new directory".into());
    }

    let (staging, audio) = (0..32)
        .find_map(|_| {
            let path = parent.join(format!(
                ".cocobeat-package-{}-{}",
                std::process::id(),
                NEXT_STAGING.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::symlink_metadata(&path) {
                Ok(_) => None,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(
                    prepare_canonical_audio(source_audio, expected_frames, &path)
                        .map(|audio| (path, audio)),
                ),
                Err(error) => Some(Err(format!("Cannot inspect staging path: {error}"))),
            }
        })
        .ok_or("Cannot allocate a unique package staging directory after 32 attempts")??;
    let mut created = vec![staging.join(FILE_NAMES[0])];
    let result = (|| {
        let input = build_content(&staging.join(&audio.asset.file_name), &audio)?;
        if input.analysis.audio_hash != audio.asset.blake3
            || input.chart.audio_hash != audio.asset.blake3
        {
            return Err("Analysis and chart must reference the exact canonical audio hash".into());
        }
        let analysis = write_object(
            &staging,
            FILE_NAMES[1],
            &encode_analysis(&input.analysis, audio.canonical_frames)?,
            &mut created,
        )?;
        let chart = write_object(
            &staging,
            FILE_NAMES[2],
            &encode_chart(&input.chart, audio.canonical_frames)?,
            &mut created,
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
        manifest.package_hash = package_hash(&manifest)?;
        write_object(
            &staging,
            FILE_NAMES[3],
            &encode_package(&manifest)?,
            &mut created,
        )?;
        let validated = validate_package(&staging)?;
        require_absent(destination)?;
        // Concurrent builders publish nonempty directories, which rename cannot replace
        fs::rename(&staging, destination)
            .map_err(|error| format!("Cannot publish package directory: {error}"))?;
        Ok(validated)
    })();
    result.map_err(|error| cleanup_staging(&staging, &created, error))
}

/// Returns content only after bounded object, envelope, semantic and canonical readback checks
pub fn validate_package(root: impl AsRef<Path>) -> Result<ValidatedPackage, String> {
    read_package(root, |_| Ok(()))
}

/// Delivers PCM from the same owned audio snapshot used for hash and strict decoding checks
/// All blocks remain provisional until success; consumers must discard them on any error
pub fn read_package(
    root: impl AsRef<Path>,
    consume: impl FnMut(&[[f32; 2]]) -> Result<(), String>,
) -> Result<ValidatedPackage, String> {
    let root = root.as_ref();
    if !fs::symlink_metadata(root)
        .map_err(|error| format!("Cannot inspect package directory: {error}"))?
        .is_dir()
    {
        return Err("Package root must be a directory, not a symlink".into());
    }
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !FILE_NAMES.iter().any(|name| entry.file_name() == *name) {
            return Err("Package directory must contain only its four declared objects".into());
        }
    }
    let manifest = decode_package(&read_object(&root.join(FILE_NAMES[3]), MAX_PACKAGE_BYTES)?)?;
    if package_hash(&manifest)? != manifest.package_hash {
        return Err("Package manifest hash does not match its canonical bytes".into());
    }
    let analysis_bytes = read_referenced(root, &manifest.analysis, MAX_ANALYSIS_BYTES)?;
    let chart_bytes = read_referenced(root, &manifest.chart, MAX_CHART_BYTES)?;
    let analysis = decode_analysis(&analysis_bytes, manifest.canonical_frames)?;
    let chart = decode_chart(&chart_bytes, manifest.canonical_frames)?;
    if analysis.audio_hash != manifest.audio.blake3 || chart.audio_hash != manifest.audio.blake3 {
        return Err("Analysis or chart references a different canonical audio hash".into());
    }
    let audio = read_referenced(root, &manifest.audio, MAX_SOURCE_BYTES as usize)?;
    decode_canonical_bytes(audio, manifest.canonical_frames, consume)?;
    Ok(ValidatedPackage {
        manifest,
        analysis,
        chart,
    })
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

fn read_object(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let (mut file, length) = open_object(path, max_bytes as u64)?;
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    if file.read(&mut [0]).map_err(|error| error.to_string())? != 0 {
        return Err("Object changed length while being read".into());
    }
    Ok(bytes)
}

fn read_referenced(root: &Path, reference: &AssetRef, max_bytes: usize) -> Result<Vec<u8>, String> {
    let bytes = read_object(&root.join(&reference.file_name), max_bytes)?;
    if bytes.len() as u64 != reference.byte_len
        || *blake3::hash(&bytes).as_bytes() != reference.blake3
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
    let path = root.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("Cannot create package object {name}: {error}"))?;
    created.push(path);
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    Ok(AssetRef {
        file_name: name.into(),
        byte_len: bytes.len() as u64,
        blake3: *blake3::hash(bytes).as_bytes(),
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
    use cocobeat_schema::{Anchor, EnergySample, SongTime};
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
        fs::write(root.join(FILE_NAMES[3]), encode_package(manifest).unwrap()).unwrap();
    }

    fn rewrite_package(root: &Path, package: &mut ValidatedPackage) {
        fs::write(
            root.join(FILE_NAMES[1]),
            encode_analysis(&package.analysis, package.manifest.canonical_frames).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join(FILE_NAMES[2]),
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
            assert_eq!(fs::read(destination.join(FILE_NAMES[0])).unwrap(), STEREO);
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
        let audio_path = destination.join(FILE_NAMES[0]);
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

        for name in FILE_NAMES {
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
        fs::write(destination.join(FILE_NAMES[0]), corrupt_audio).unwrap();
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
        for (name, limit) in FILE_NAMES.into_iter().zip([
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
            destination.join(FILE_NAMES[3]),
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
        fs::write(destination.join(FILE_NAMES[0]), b"not canonical audio").unwrap();
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
        for name in &FILE_NAMES[1..] {
            let path = destination.join(name);
            let original = fs::read(&path).unwrap();
            let mut unsupported = original.clone();
            unsupported[8..12].copy_from_slice(&2_u32.to_le_bytes());
            fs::write(&path, unsupported).unwrap();
            let mut manifest = package.manifest.clone();
            if *name != FILE_NAMES[3] {
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
        let manifest_path = destination.join(FILE_NAMES[3]);
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
        prepare_canonical_audio(&source, 4_800, &interrupted).unwrap();
        assert!(validate_package(&interrupted).is_err());
        let foreign = interrupted.join("keep");
        fs::write(&foreign, b"not created by the package transaction").unwrap();
        cleanup_staging(
            &interrupted,
            &[interrupted.join(FILE_NAMES[0])],
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
        for name in FILE_NAMES {
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
}
