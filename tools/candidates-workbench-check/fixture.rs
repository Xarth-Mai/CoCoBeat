use cocobeat_media::PackageBuildInput;
use cocobeat_schema::{
    Anchor, BeatFeature, CompiledChart, EnergySample, MusicAnalysis, OnsetFeature, SectionCue,
    SectionFeature, SongTime,
};
use std::{fs, path::Path};

pub fn build(destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|error| error.to_string())?;
    let audio = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/synthetic/media-import/stereo-canonical.ogg");
    for rich in [true, false] {
        let root = destination.join(if rich { "rich" } else { "empty" });
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let package = root.join("package");
        cocobeat_media::build_package(&audio, 4800, &package, |staged, prepared| {
            let mut sum = [0.0_f64; 2];
            let mut peak = [0.0_f32; 2];
            cocobeat_media::decode_canonical(staged, 4800, |frames| {
                for frame in frames {
                    for channel in 0..2 {
                        sum[channel] += f64::from(frame[channel]).powi(2);
                        peak[channel] = peak[channel].max(frame[channel].abs());
                    }
                }
                Ok(())
            })?;
            let mut points = if rich {
                vec![
                    (0, None),
                    (500, Some(0.5)),
                    (1000, Some(0.8)),
                    (1200, Some(0.95)),
                    (1201, Some(0.7)),
                ]
            } else {
                Vec::new()
            };
            if rich {
                points.extend((5..40).map(|index| {
                    (
                        1300 + (index - 5) * 90,
                        match index % 5 {
                            0 => None,
                            1 => Some(0.4),
                            _ => Some(0.8),
                        },
                    )
                }));
            }
            let diagnostics = "QA constructed uncalibrated onset/beat/section mechanism controls, not MIR output; RMS/peak measured from final canonical PCM";
            Ok(PackageBuildInput {
                song_id: format!("qa-candidate-{}", if rich { "rich" } else { "empty" }),
                importer_version: "qa-constructed-candidates-v1".into(),
                analysis_version: "qa-uncalibrated-evidence-v1".into(),
                chart_version: "qa-source-chart-v1".into(),
                analysis: MusicAnalysis {
                    capabilities: None,
                    tempo_regions: Vec::new(),
                    repetitions: Vec::new(),
                    schema_version: 1,
                    audio_hash: prepared.asset.blake3,
                    onsets: points
                        .into_iter()
                        .enumerate()
                        .map(|(index, (frame, confidence))| OnsetFeature {
                            time: SongTime::from_frames(frame),
                            strength: if index == 0 { -0.0 } else { 1.0 },
                            confidence,
                        })
                        .collect(),
                    beats: [(300, None), (1400, Some(0.8)), (3400, None)]
                        .into_iter()
                        .map(|(frame, confidence)| BeatFeature {
                            time: SongTime::from_frames(frame),
                            strength: 0.5,
                            confidence,
                            downbeat_probability: None,
                        })
                        .collect(),
                    sections: vec![SectionFeature {
                        start: SongTime::from_frames(900),
                        end: SongTime::from_frames(2500),
                        confidence: None,
                        label: "Constructed section context, not inferred".into(),
                    }],
                    energy: vec![EnergySample {
                        start: SongTime::ZERO,
                        frames: 4800,
                        rms: sum.map(|sum| (sum / 4800.0).sqrt() as f32),
                        peak,
                    }],
                    diagnostics: diagnostics.into(),
                },
                chart: CompiledChart {
                    schema_version: 1,
                    audio_hash: prepared.asset.blake3,
                    ruleset_id: "duo-watermark-v1".into(),
                    anchors: vec![Anchor {
                        id: 99,
                        song_time: SongTime::from_frames(333),
                    }],
                    sections: vec![SectionCue {
                        id: 7,
                        time: SongTime::from_frames(100),
                        label: "Original source chart cue".into(),
                    }],
                },
            })
        })?;
        crate::anchors::propose(&package, "0.7", "1000", &root.join("report.json"))?;
    }
    Ok(())
}
