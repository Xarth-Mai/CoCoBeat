use crate::{PackageBuildInput, ValidatedPackage, decode::MAX_SOURCE_BYTES};
use cocobeat_schema::{
    Anchor, SongTime,
    content::{
        ANALYSIS_SCHEMA_VERSION, AnalysisCapabilities, CONTENT_SCHEMA_VERSION, CompiledChart,
        EnergySample, MAX_CANONICAL_FRAMES, MusicAnalysis, SectionCue, SectionFeature,
    },
};
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_IMPORT: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Authoring {
    schema_version: u32,
    song_id: String,
    ruleset_id: String,
    source_note: String,
    anchors: Vec<AuthoredAnchor>,
    sections: Vec<AuthoredSection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredAnchor {
    id: u64,
    frame: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredSection {
    id: u64,
    start_frame: i64,
    end_frame: i64,
    label: String,
}

/// Builds a new package from final canonical audio and explicit authored content
/// The caller supplies its actual importer identity; no automatic MIR is inferred
pub fn build_authored_package(
    audio: &Path,
    frames: u64,
    authoring_path: &Path,
    destination: &Path,
    importer_version: &str,
) -> Result<ValidatedPackage, String> {
    if !(1..=MAX_CANONICAL_FRAMES).contains(&frames) {
        return Err(
            "Expected canonical frames must cover more than zero and at most ten minutes".into(),
        );
    }
    build_authored(
        audio,
        frames,
        load_authoring(authoring_path)?,
        destination,
        importer_version,
        None,
    )
}

fn load_authoring(authoring_path: &Path) -> Result<Authoring, String> {
    let mut bytes = Vec::new();
    regular_file(authoring_path)?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Read authoring document: {error}"))?;
    if bytes.len() > 1_048_576 {
        return Err("Authoring document exceeds 1 MiB".into());
    }
    let authoring: Authoring = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid authoring document: {error}"))?;
    if authoring.schema_version != CONTENT_SCHEMA_VERSION {
        return Err("Unsupported authoring document schema".into());
    }
    if authoring.source_note.trim().is_empty() || authoring.source_note.len() > 2048 {
        return Err("Authoring source_note must contain 1..2048 UTF-8 bytes".into());
    }
    Ok(authoring)
}

fn build_authored(
    audio: &Path,
    frames: u64,
    authoring: Authoring,
    destination: &Path,
    importer_version: &str,
    provenance: Option<String>,
) -> Result<ValidatedPackage, String> {
    crate::build_package(audio, frames, destination, |staged, prepared| {
        let audio_hash = prepared.asset.blake3;
        let energy = measure_energy(staged, prepared.canonical_frames)?;
        let analysis = MusicAnalysis {
            capabilities: Some(AnalysisCapabilities::authored()),
            tempo_regions: Vec::new(),
            repetitions: Vec::new(),
            schema_version: ANALYSIS_SCHEMA_VERSION,
            audio_hash,
            beats: Vec::new(),
            onsets: Vec::new(),
            sections: authoring
                .sections
                .iter()
                .map(|section| SectionFeature {
                    start: SongTime::from_frames(section.start_frame),
                    end: SongTime::from_frames(section.end_frame),
                    confidence: None,
                    label: section.label.clone(),
                })
                .collect(),
            energy,
            diagnostics: format!(
                "Energy measured from final canonical PCM in 1024-frame blocks; beat/onset analysis not run; sections and anchors manually authored: {}",
                match &provenance {
                    Some(provenance) =>
                        format!("{}; source import: {provenance}", authoring.source_note),
                    None => authoring.source_note.clone(),
                }
            ),
        };
        let chart = CompiledChart {
            schema_version: CONTENT_SCHEMA_VERSION,
            audio_hash,
            ruleset_id: authoring.ruleset_id,
            anchors: authoring
                .anchors
                .into_iter()
                .map(|anchor| Anchor {
                    id: anchor.id,
                    song_time: SongTime::from_frames(anchor.frame),
                })
                .collect(),
            sections: authoring
                .sections
                .into_iter()
                .map(|section| SectionCue {
                    id: section.id,
                    time: SongTime::from_frames(section.start_frame),
                    label: section.label,
                })
                .collect(),
        };
        Ok(PackageBuildInput {
            song_id: authoring.song_id,
            importer_version: importer_version.into(),
            analysis_version: "canonical-rms-1024-v2".into(),
            chart_version: "manual-anchors-v1".into(),
            analysis,
            chart,
        })
    })
}

/// Imports the exact owned source snapshot through the sole production encoder
/// The caller's importer identity is suffixed with the actual encoder profile
/// Hand-authored content uses final 48 kHz coordinates; no automatic MIR is inferred
pub fn import_authored_package(
    source: &Path,
    authoring_path: &Path,
    destination: &Path,
    importer_version: &str,
) -> Result<ValidatedPackage, String> {
    if importer_version.is_empty() {
        return Err("Importer version must identify the calling tool".into());
    }
    let authoring = load_authoring(authoring_path)?;
    match fs::symlink_metadata(destination) {
        Ok(_) => return Err("Package destination already exists".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot inspect package destination: {error}")),
    }
    if destination.file_name().is_none() {
        return Err("Package destination must name a new directory".into());
    }
    let parent = fs::canonicalize(
        destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )
    .map_err(|error| format!("Cannot resolve package destination parent: {error}"))?;
    let staging = (0..32)
        .find_map(|_| {
            let path = parent.join(format!(
                ".cocobeat-import-{}-{}",
                std::process::id(),
                NEXT_IMPORT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => Some(Ok(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(format!("Cannot create import staging: {error}"))),
            }
        })
        .ok_or("Cannot allocate import staging after 32 attempts")??;
    let mut created = Vec::new();
    let result = (|| {
        let snapshot = staging.join("source.audio");
        let (length, hash) = snapshot_source(source, &snapshot, &mut created)?;
        let audio = staging.join("canonical.ogg");
        let metadata = crate::encode_canonical_audio(&snapshot, &audio)?;
        created.push(audio.clone());
        let provenance = serde_json::json!({
            "source_blake3": hash,
            "source_bytes": length,
            "source_sample_rate": metadata.source_sample_rate,
            "source_frames": metadata.source_frames,
            "canonical_frames": metadata.output_frames,
            "encoder_profile": crate::CANONICAL_ENCODER_PROFILE,
        })
        .to_string();
        build_authored(
            &audio,
            metadata.output_frames,
            authoring,
            destination,
            &format!("{importer_version}/{}", crate::CANONICAL_ENCODER_PROFILE),
            Some(provenance),
        )
    })();
    let mut cleanup = Vec::new();
    for path in created {
        if let Err(error) = fs::remove_file(&path) {
            cleanup.push(format!("remove {}: {error}", path.display()));
        }
    }
    if let Err(error) = fs::remove_dir(&staging) {
        cleanup.push(format!("remove {}: {error}", staging.display()));
    }
    match (result, cleanup.is_empty()) {
        (Ok(package), true) => Ok(package),
        (Ok(_), false) => Err(format!(
            "Package was committed to {}; import staging cleanup failed: {}",
            destination.display(),
            cleanup.join("; ")
        )),
        (Err(error), true) => Err(error),
        (Err(error), false) => Err(format!(
            "{error}; import staging cleanup failed: {}",
            cleanup.join("; ")
        )),
    }
}

fn snapshot_source(
    source: &Path,
    snapshot: &Path,
    created: &mut Vec<PathBuf>,
) -> Result<(u64, String), String> {
    if !fs::symlink_metadata(source)
        .map_err(|error| format!("Cannot inspect source: {error}"))?
        .is_file()
    {
        return Err("Import source must be a regular file, not a symlink".into());
    }
    let file = regular_file(source)?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    if length == 0 || length > MAX_SOURCE_BYTES {
        return Err("Import source must contain 1 byte..512 MiB".into());
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(snapshot)
        .map_err(|error| format!("Cannot create source snapshot: {error}"))?;
    created.push(snapshot.to_owned());
    let mut input = file.take(MAX_SOURCE_BYTES + 1);
    let mut hash = blake3::Hasher::new();
    let mut count = 0_u64;
    let mut block = [0; 8192];
    loop {
        let bytes = input
            .read(&mut block)
            .map_err(|error| format!("Cannot read source: {error}"))?;
        if bytes == 0 {
            break;
        }
        count += bytes as u64;
        if count > MAX_SOURCE_BYTES {
            return Err("Import source exceeds 512 MiB during copy".into());
        }
        output
            .write_all(&block[..bytes])
            .map_err(|error| format!("Cannot copy source snapshot: {error}"))?;
        hash.update(&block[..bytes]);
    }
    if count != length {
        return Err("Import source changed length during copy".into());
    }
    output
        .sync_all()
        .map_err(|error| format!("Cannot sync source snapshot: {error}"))?;
    Ok((count, hash.finalize().to_hex().to_string()))
}

fn regular_file(path: &Path) -> Result<File, String> {
    if !path.is_file() {
        return Err(format!("Expected a regular file: {}", path.display()));
    }
    let file = File::open(path).map_err(|error| format!("Open {}: {error}", path.display()))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err(format!("Expected a regular file: {}", path.display()));
    }
    Ok(file)
}

fn measure_energy(path: &Path, expected: u64) -> Result<Vec<EnergySample>, String> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut count = 0;
    let mut sum = [0.0; 2];
    let mut peak: [f32; 2] = [0.0; 2];
    let finish = |start, count, sum: [f64; 2], peak| EnergySample {
        start: SongTime::from_frames(start),
        frames: count,
        rms: sum.map(|value| (value / f64::from(count)).sqrt() as f32),
        peak,
    };
    crate::decode_canonical(path, expected, |frames| {
        for frame in frames {
            for channel in 0..2 {
                sum[channel] += f64::from(frame[channel]).powi(2);
                peak[channel] = peak[channel].max(frame[channel].abs());
            }
            count += 1;
            if count == 1024 {
                result.push(finish(start, count, sum, peak));
                start += i64::from(count);
                count = 0;
                sum = [0.0; 2];
                peak = [0.0; 2];
            }
        }
        Ok(())
    })?;
    if count != 0 {
        result.push(finish(start, count, sum, peak));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMPORTER: &str = "cocobeat-media-test/v1";

    #[test]
    fn energy_covers_final_pcm_in_fixed_windows_and_preserves_stereo() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/synthetic/media-import/stereo-canonical.ogg");
        let energy = measure_energy(&path, 4800).unwrap();
        assert_eq!(energy.len(), 5);
        assert_eq!(energy.last().unwrap().start.frames(), 4096);
        assert_eq!(energy.last().unwrap().frames, 704);
        let mut pcm = Vec::new();
        crate::decode_canonical(&path, 4800, |block| {
            pcm.extend_from_slice(block);
            Ok(())
        })
        .unwrap();
        for (index, block) in pcm.chunks(1024).enumerate() {
            let sample = &energy[index];
            assert_eq!(sample.start.frames(), (index * 1024) as i64);
            assert_eq!(sample.frames as usize, block.len());
            for channel in 0..2 {
                let squares: f64 = block
                    .iter()
                    .map(|frame| f64::from(frame[channel]).powi(2))
                    .sum();
                let rms = (squares / block.len() as f64).sqrt() as f32;
                assert_eq!(sample.rms[channel], rms);
                assert!(sample.peak[channel] >= rms && rms > 0.0);
            }
            assert!(sample.rms[1] > sample.rms[0] * 1.8);
        }
        assert!(measure_energy(&path, 4799).is_err());
    }
    #[test]
    fn source_import_preserves_provenance_final_pcm_and_transaction_boundaries() {
        let root = std::env::temp_dir().join(format!(
            "cocobeat-source-import-{}-{}",
            std::process::id(),
            NEXT_IMPORT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("source.wav");
        let authoring = root.join("authoring.json");
        let destination = root.join("package");
        let mut wav = Vec::new();
        let bytes = 4410_u32 * 8;
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&3_u16.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&44_100_u32.to_le_bytes());
        wav.extend_from_slice(&(44_100_u32 * 8).to_le_bytes());
        wav.extend_from_slice(&8_u16.to_le_bytes());
        wav.extend_from_slice(&32_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&bytes.to_le_bytes());
        for frame in 0..4410 {
            let sample = (std::f64::consts::TAU * 440.0 * f64::from(frame) / 44_100.0).sin() as f32;
            wav.extend_from_slice(&(sample * 0.25).to_le_bytes());
            wav.extend_from_slice(&(sample * -0.5).to_le_bytes());
        }
        fs::write(&source, &wav).unwrap();
        let document = serde_json::json!({
            "schema_version": CONTENT_SCHEMA_VERSION,
            "song_id": "authored-import-test",
            "ruleset_id": "duo-watermark-v1",
            "source_note": "Original synthetic stereo; hand-authored final 48 kHz coordinates",
            "anchors": [{"id": 1, "frame": 2400}],
            "sections": [{"id": 1, "start_frame": 0, "end_frame": 4800, "label": "test"}],
        });
        fs::write(&authoring, serde_json::to_vec(&document).unwrap()).unwrap();
        let package = import_authored_package(&source, &authoring, &destination, IMPORTER).unwrap();
        assert_eq!(package, crate::validate_package(&destination).unwrap());
        assert_eq!(
            package.manifest.importer_version,
            format!("{IMPORTER}/{}", crate::CANONICAL_ENCODER_PROFILE)
        );
        assert_eq!(package.manifest.canonical_frames, 4800);
        assert!(
            package
                .manifest
                .importer_version
                .ends_with(crate::CANONICAL_ENCODER_PROFILE)
        );
        assert!(
            package
                .analysis
                .diagnostics
                .contains(&blake3::hash(&wav).to_hex().to_string())
        );
        assert!(
            package
                .analysis
                .diagnostics
                .contains("\"source_sample_rate\":44100")
        );
        assert!(
            package
                .analysis
                .diagnostics
                .contains("\"source_frames\":4410")
        );
        assert!(package.analysis.beats.is_empty() && package.analysis.onsets.is_empty());
        assert_eq!(package.analysis.sections[0].confidence, None);
        assert_eq!(package.chart.anchors[0].song_time.frames(), 2400);
        let actual_energy = measure_energy(&destination.join("song.audio.ogg"), 4800).unwrap();
        assert_eq!(package.analysis.energy, actual_energy);
        assert_eq!(package.analysis.schema_version, ANALYSIS_SCHEMA_VERSION);
        assert_eq!(
            package.analysis.capabilities,
            Some(AnalysisCapabilities::authored())
        );
        assert_eq!(package.manifest.analysis_version, "canonical-rms-1024-v2");
        assert!(package.analysis.tempo_regions.is_empty());
        assert!(package.analysis.repetitions.is_empty());

        let original_audio = fs::read(destination.join("song.audio.ogg")).unwrap();
        assert!(import_authored_package(&source, &authoring, &destination, IMPORTER).is_err());
        assert_eq!(
            fs::read(destination.join("song.audio.ogg")).unwrap(),
            original_audio
        );
        assert_eq!(fs::read(&source).unwrap(), wav);

        let failed = root.join("failed");
        assert!(import_authored_package(&source, &authoring, &failed, "").is_err());
        assert!(!failed.exists());
        let mut invalid = document.clone();
        invalid["anchors"][0]["frame"] = serde_json::json!(4801);
        fs::write(&authoring, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(import_authored_package(&source, &authoring, &failed, IMPORTER).is_err());
        assert!(!failed.exists());
        fs::write(&authoring, serde_json::to_vec(&document).unwrap()).unwrap();
        fs::write(&source, b"invalid media").unwrap();
        assert!(import_authored_package(&source, &authoring, &failed, IMPORTER).is_err());
        assert!(!failed.exists());
        assert_eq!(fs::read(&source).unwrap(), b"invalid media");
        let remaining: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(remaining.len(), 3);
        assert!(
            remaining
                .iter()
                .all(|name| !name.to_string_lossy().starts_with(".cocobeat-"))
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn authored_build_records_the_actual_caller_and_rejects_invalid_input() {
        let root = std::env::temp_dir().join(format!(
            "cocobeat-authored-build-{}-{}",
            std::process::id(),
            NEXT_IMPORT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/synthetic/media-import/stereo-canonical.ogg");
        let authoring = root.join("authoring.json");
        let document = serde_json::json!({
            "schema_version": CONTENT_SCHEMA_VERSION,
            "song_id": "shared-authored-build-test",
            "ruleset_id": "duo-watermark-v1",
            "source_note": "Original canonical stereo with explicit hand-authored coordinates",
            "anchors": [{"id": 7, "frame": 2400}],
            "sections": [{"id": 3, "start_frame": 0, "end_frame": 4800, "label": "test"}],
        });
        fs::write(&authoring, serde_json::to_vec(&document).unwrap()).unwrap();
        let first = build_authored_package(
            &source,
            4800,
            &authoring,
            &root.join("first"),
            "cocobeat-lab/test",
        )
        .unwrap();
        let second = build_authored_package(
            &source,
            4800,
            &authoring,
            &root.join("second"),
            "cocobeat-game/test",
        )
        .unwrap();
        assert_eq!(first.manifest.importer_version, "cocobeat-lab/test");
        assert_eq!(second.manifest.importer_version, "cocobeat-game/test");
        assert_ne!(first.manifest.package_hash, second.manifest.package_hash);
        assert_eq!(first.manifest.audio, second.manifest.audio);
        assert_eq!(first.manifest.analysis, second.manifest.analysis);
        assert_eq!(first.manifest.chart, second.manifest.chart);
        assert_eq!(first.analysis, second.analysis);
        assert_eq!(first.chart, second.chart);
        assert_eq!(
            first.chart.anchors[0],
            Anchor {
                id: 7,
                song_time: SongTime::from_frames(2400)
            }
        );
        assert_eq!(first.chart.sections[0].time, SongTime::ZERO);
        assert!(first.analysis.beats.is_empty() && first.analysis.onsets.is_empty());
        assert!(
            first
                .analysis
                .diagnostics
                .contains("beat/onset analysis not run")
        );
        let failed = root.join("failed");
        for frames in [0, 4799, MAX_CANONICAL_FRAMES + 1] {
            assert!(
                build_authored_package(&source, frames, &authoring, &failed, IMPORTER).is_err()
            );
            assert!(!failed.exists());
        }
        assert!(build_authored_package(&source, 4800, &authoring, &failed, "").is_err());
        assert!(!failed.exists());
        let mut invalid = document;
        invalid["unexpected"] = serde_json::json!(true);
        fs::write(&authoring, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(build_authored_package(&source, 4800, &authoring, &failed, IMPORTER).is_err());
        assert!(!failed.exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        fs::remove_dir_all(root).unwrap();
    }
}
