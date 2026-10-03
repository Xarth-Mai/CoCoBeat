use cocobeat_media::{PackageBuildInput, ValidatedPackage};
use cocobeat_schema::{
    Anchor, SongTime,
    content::{
        CONTENT_SCHEMA_VERSION, CompiledChart, EnergySample, MAX_CANONICAL_FRAMES, MusicAnalysis,
        SectionCue, SectionFeature,
    },
};
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path};

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

pub fn build(
    audio: &Path,
    frames: u64,
    authoring_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    if !(1..=MAX_CANONICAL_FRAMES).contains(&frames) {
        return Err(
            "Expected canonical frames must cover more than zero and at most ten minutes".into(),
        );
    }
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
    let validated = cocobeat_media::build_package(
        audio,
        frames,
        destination,
        |staged, prepared| {
            let audio_hash = prepared.asset.blake3;
            let energy = measure_energy(staged, prepared.canonical_frames)?;
            let analysis = MusicAnalysis {
                schema_version: CONTENT_SCHEMA_VERSION,
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
                    authoring.source_note
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
                importer_version: format!("cocobeat-lab/{}", env!("CARGO_PKG_VERSION")),
                analysis_version: "canonical-rms-1024-v1".into(),
                chart_version: "manual-anchors-v1".into(),
                analysis,
                chart,
            })
        },
    )?;
    summary(&validated);
    println!("Committed package: {}", destination.display());
    Ok(())
}

pub fn verify(path: &Path) -> Result<(), String> {
    let validated = cocobeat_media::validate_package(path)?;
    summary(&validated);
    Ok(())
}

fn summary(package: &ValidatedPackage) {
    let hash: String = package
        .manifest
        .package_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    println!(
        "Validated package: {} frames, {} energy blocks, {} anchors, {} section cues",
        package.manifest.canonical_frames,
        package.analysis.energy.len(),
        package.chart.anchors.len(),
        package.chart.sections.len()
    );
    println!("Package BLAKE3: {hash}");
    println!(
        "Object integrity and initial content schema verified; automatic MIR, stage compilation and encoder quality remain separate gates."
    );
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
    cocobeat_media::decode_canonical(path, expected, |frames| {
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

    #[test]
    fn energy_covers_final_pcm_in_fixed_windows_and_preserves_stereo() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/synthetic/media-import/stereo-canonical.ogg");
        let energy = measure_energy(&path, 4800).unwrap();
        assert_eq!(energy.len(), 5);
        assert_eq!(energy.last().unwrap().start.frames(), 4096);
        assert_eq!(energy.last().unwrap().frames, 704);
        let mut pcm = Vec::new();
        cocobeat_media::decode_canonical(&path, 4800, |block| {
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
}
