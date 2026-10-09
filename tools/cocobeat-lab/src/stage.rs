use cocobeat_schema::{AnalysisSource, AnalysisState, MusicAnalysis, SongTime};
use cocobeat_stage::SegmentKind;
use std::path::Path;

pub fn inspect(path: &Path, frame: &str) -> Result<(), String> {
    let frame = frame
        .parse::<i64>()
        .map_err(|_| "Stage frame must be a signed integer".to_string())?;
    let package = cocobeat_media::validate_package(path)?;
    let plan = package_plan(&package)?;
    print_sample(&plan, frame)
}

/// Inspect the complete current plan and retain authored or unknown section provenance
pub fn inspect_plan(path: &Path) -> Result<(), String> {
    let package = cocobeat_media::validate_package(path)?;
    let plan = package_plan(&package)?;
    println!(
        "{}",
        plan_report(&plan, &package.analysis, &package.manifest.analysis_version)
    );
    Ok(())
}

fn package_plan(
    package: &cocobeat_media::ValidatedPackage,
) -> Result<cocobeat_stage::StagePlan, String> {
    cocobeat_stage::compile_analysis(
        &format!(
            "package-blake3:{}",
            blake3::Hash::from(package.manifest.package_hash)
        ),
        SongTime::from_frames(package.manifest.canonical_frames as i64),
        &package.analysis,
    )
}

fn capability_report(capability: cocobeat_schema::AnalysisCapability) -> serde_json::Value {
    serde_json::json!({
        "state": match capability.state {
            AnalysisState::NotRun => "not_run",
            AnalysisState::Unsupported => "unsupported",
            AnalysisState::Candidate => "candidate",
            AnalysisState::Validated => "validated",
        },
        "source": match capability.source {
            AnalysisSource::Algorithm => "algorithm",
            AnalysisSource::Authored => "authored",
            AnalysisSource::Measured => "measured",
        },
        "confidence": capability.confidence,
    })
}

fn plan_report(
    plan: &cocobeat_stage::StagePlan,
    analysis: &MusicAnalysis,
    analysis_version: &str,
) -> serde_json::Value {
    let capability = analysis.capabilities.map(|c| capability_report(c.sections));
    let sections: Vec<_> = analysis
        .sections
        .iter()
        .enumerate()
        .map(|(index, section)| {
            serde_json::json!({
                "section_index": index,
                "start_frames": section.start.frames(),
                "end_frames": section.end.frames(),
                "label": section.label,
                "confidence": section.confidence,
            })
        })
        .collect();
    let segments: Vec<_> = plan
        .segments()
        .iter()
        .map(|segment| {
            let index = analysis
                .sections
                .partition_point(|section| section.end <= segment.start);
            let source = analysis
                .sections
                .get(index)
                .filter(|section| section.start <= segment.start && segment.end <= section.end)
                .map(|_| index);
            serde_json::json!({
                "start_frames": segment.start.frames(),
                "end_frames": segment.end.frames(),
                "kind": kind_name(segment.kind),
                "source_section_index": source,
            })
        })
        .collect();
    serde_json::json!({
        "content_id": plan.content_id(),
        "compiler_version": plan.compiler_version(),
        "end_frames": plan.end().frames(),
        "analysis_schema_version": analysis.schema_version,
        "analysis_version": analysis_version,
        "sections_capability": capability,
        "sections": sections,
        "segments": segments,
        "repetitions": analysis.repetitions.iter().enumerate().map(|(index, r)| serde_json::json!({"relation_index": index, "source_start_frames": r.source_start.frames(), "source_end_frames": r.source_end.frames(), "target_start_frames": r.target_start.frames(), "target_end_frames": r.target_end.frames(), "confidence": r.confidence, "confidence_bits": r.confidence.map(f32::to_bits)})).collect::<Vec<_>>(),
        "repetition_capability": analysis.capabilities.map(|c| capability_report(c.repetition)),
        "energy_capability": analysis.capabilities.map(|c| capability_report(c.energy)),
        "presentation": analysis.presentation.as_ref().map(|presentation| serde_json::json!({
            "capability": capability_report(presentation.capability),
            "windows": presentation.windows.iter().map(|window| serde_json::json!({
                "start_frames": window.start.frames(),
                "end_frames": window.end.frames(),
                "chroma": window.chroma,
                "chord": window.chord.map(|chord| serde_json::json!({"root": chord.root, "minor": chord.minor, "agreement": chord.confidence})),
                "key": window.key.map(|key| serde_json::json!({"root": key.root, "minor": key.minor, "agreement": key.confidence})),
                "tonal_confidence": window.tonal_confidence,
                "onset_density": window.onset_density,
                "brightness": window.brightness,
                "energy": window.energy,
            })).collect::<Vec<_>>(),
        })),
        "motifs": plan.motifs().iter().map(|span| serde_json::json!({"start_frames": span.start.frames(), "end_frames": span.end.frames(), "motif": span.motif})).collect::<Vec<_>>(),
        "decor_energy": plan.decor_energy().iter().map(|span| serde_json::json!({"start_frames": span.start.frames(), "end_frames": span.end.frames(), "rms_bits": span.rms_bits, "peak_bits": span.peak_bits, "band": span.band})).collect::<Vec<_>>(),
    })
}

fn kind_name(kind: SegmentKind) -> &'static str {
    match kind {
        SegmentKind::Straight => "straight",
        SegmentKind::Plaza => "plaza",
        SegmentKind::Curve => "curve",
        SegmentKind::Bridge => "bridge",
    }
}

/// Reconstruct only recorded geometry; core validation is shared with diagnostics
pub fn inspect_replay(path: &Path, replay_path: &Path, frame: &str) -> Result<(), String> {
    let frame = frame
        .parse::<i64>()
        .map_err(|_| "Stage frame must be a signed integer".to_string())?;
    let package = cocobeat_media::validate_package(path)?;
    let (replay, _) = crate::replay::load(&package, replay_path)?;
    let version = replay
        .identity()
        .stage_compiler_version
        .ok_or("Replay has no Stage compiler identity; historical geometry is unsupported")?;
    let plan = cocobeat_stage::compile_analysis_version(
        &replay.identity().content_id,
        SongTime::from_frames(package.manifest.canonical_frames as i64),
        &package.analysis,
        version,
    )?;
    print_sample(&plan, frame)
}

fn print_sample(plan: &cocobeat_stage::StagePlan, frame: i64) -> Result<(), String> {
    let sample = plan
        .sample(SongTime::from_frames(frame))
        .ok_or_else(|| format!("Stage frame must be in 0..={}", plan.end().frames()))?;
    let decor = plan.decoration(SongTime::from_frames(frame)).unwrap();
    println!(
        "{}",
        serde_json::json!({
            "content_id": plan.content_id(),
            "compiler_version": plan.compiler_version(),
            "segment_count": plan.segments().len(),
            "end_frames": plan.end().frames(),
            "frame": frame,
            "kind": kind_name(sample.kind),
            "motif": decor.motif,
            "energy_band": decor.energy_band,
            "distance_mm": sample.distance_mm,
            "half_width_mm": sample.half_width_mm,
            "lateral_mm": sample.lateral_mm,
            "elevation_mm": sample.elevation_mm,
            "slope_x_ppm": sample.slope_x_ppm,
            "slope_y_ppm": sample.slope_y_ppm,
            "at_end": frame == plan.end().frames(),
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_schema::{AnalysisCapabilities, EnergySample, SectionFeature};

    #[test]
    fn complete_plan_keeps_section_provenance_and_unknown() {
        let end = SongTime::from_frames(960_000);
        let mut analysis = MusicAnalysis {
            presentation: None,
            schema_version: 2,
            audio_hash: [1; 32],
            capabilities: Some(AnalysisCapabilities::authored()),
            tempo_regions: vec![],
            repetitions: vec![],
            beats: vec![],
            onsets: vec![],
            sections: vec![
                SectionFeature {
                    start: SongTime::from_frames(10),
                    end: SongTime::from_frames(110),
                    confidence: None,
                    label: "人工短段".into(),
                },
                SectionFeature {
                    start: SongTime::from_frames(120),
                    end: SongTime::from_frames(768_121),
                    confidence: Some(0.5),
                    label: "manual long".into(),
                },
            ],
            energy: vec![EnergySample {
                start: SongTime::ZERO,
                frames: 960_000,
                rms: [0.0; 2],
                peak: [0.0; 2],
            }],
            diagnostics: "Synthetic provenance control, not music quality evidence".into(),
        };
        analysis.validate(end.frames() as u64).unwrap();
        let plan = cocobeat_stage::compile_analysis("mechanism-control", end, &analysis).unwrap();
        let report = plan_report(&plan, &analysis, "mechanism-control-v1");
        assert_eq!(report["compiler_version"], cocobeat_stage::COMPILER_VERSION);
        assert_eq!(
            report["segments"],
            serde_json::json!([
                {"start_frames": 0, "end_frames": 10, "kind": "straight", "source_section_index": null},
                {"start_frames": 10, "end_frames": 110, "kind": "plaza", "source_section_index": 0},
                {"start_frames": 110, "end_frames": 120, "kind": "straight", "source_section_index": null},
                {"start_frames": 120, "end_frames": 384_120, "kind": "curve", "source_section_index": 1},
                {"start_frames": 384_120, "end_frames": 768_121, "kind": "bridge", "source_section_index": 1},
                {"start_frames": 768_121, "end_frames": 960_000, "kind": "straight", "source_section_index": null},
            ])
        );
        assert_eq!(
            report["sections_capability"],
            serde_json::json!({"state": "not_run", "source": "authored", "confidence": null})
        );
        assert_eq!(report["sections"][0]["label"], "人工短段");
        assert!(report["sections"][0]["confidence"].is_null());
        assert_eq!(report["sections"][1]["confidence"], 0.5);
        assert!(report["presentation"].is_null());

        analysis.schema_version = cocobeat_schema::ANALYSIS_SCHEMA_VERSION;
        analysis.presentation = Some(cocobeat_schema::MusicPresentation {
            capability: cocobeat_schema::AnalysisCapability {
                state: AnalysisState::Candidate,
                source: AnalysisSource::Algorithm,
                confidence: None,
            },
            windows: vec![cocobeat_schema::PresentationWindow {
                start: SongTime::ZERO,
                end,
                chroma: [0.0; 12],
                chord: None,
                key: None,
                tonal_confidence: 0.0,
                onset_density: 0.0,
                brightness: 0.0,
                energy: 0.0,
            }],
        });
        analysis.validate(end.frames() as u64).unwrap();
        let presentation = plan_report(&plan, &analysis, "presentation-v3");
        assert_eq!(
            presentation["presentation"]["capability"]["state"],
            "candidate"
        );
        assert!(presentation["presentation"]["capability"]["confidence"].is_null());
        assert!(presentation["presentation"]["windows"][0]["chord"].is_null());
        assert_eq!(
            presentation["presentation"]["windows"][0]["end_frames"],
            end.frames()
        );
        assert_eq!(
            cocobeat_stage::compile_analysis("mechanism-control", end, &analysis).unwrap(),
            plan
        );

        analysis.capabilities.as_mut().unwrap().sections.state = AnalysisState::Candidate;
        analysis.capabilities.as_mut().unwrap().sections.source = AnalysisSource::Algorithm;
        analysis.validate(end.frames() as u64).unwrap();
        let candidate = plan_report(&plan, &analysis, "mechanism-control-v1");
        assert_eq!(candidate["sections_capability"]["state"], "candidate");
        assert!(candidate["sections_capability"]["confidence"].is_null());
        assert_eq!(candidate["segments"], report["segments"]);

        analysis.schema_version = 1;
        analysis.presentation = None;
        analysis.capabilities = None;
        analysis.validate(end.frames() as u64).unwrap();
        let unknown = plan_report(&plan, &analysis, "mechanism-control-v1");
        assert!(unknown["sections_capability"].is_null());
        assert_eq!(unknown["segments"], report["segments"]);
    }
}
