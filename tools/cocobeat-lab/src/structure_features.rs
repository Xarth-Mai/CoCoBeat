//! Saves raw canonical spectral descriptors outside the unchanged source package

use crate::{labels, labels_cli};
use cocobeat_schema::{AnalysisSource, AnalysisState};
use serde::Serialize;
use std::path::Path;

const MAX_REPORT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Serialize)]
struct Report<'a> {
    schema_version: u32,
    source: &'a labels::Source,
    analysis: &'a cocobeat_media::StructureFeatureEvidence,
    sections_context: SectionsContext,
}

#[derive(Serialize)]
struct SectionsContext {
    declared_feature_count: usize,
    capability: Option<SectionCapability>,
}

#[derive(Serialize)]
struct SectionCapability {
    state: &'static str,
    source: &'static str,
    confidence: Option<f32>,
}

pub(crate) fn inspect(package: &Path, channel: &str, destination: &Path) -> Result<(), String> {
    let channel = match channel {
        "left" => 0,
        "right" => 1,
        _ => return Err("Structure features require an explicit left or right channel".into()),
    };
    let destination = labels_cli::outside_package(package, destination)?;
    let (validated, evidence) =
        cocobeat_media::inspect_structure_features_package(package, channel)?;
    let source = labels::Source::from_package(&validated);
    let sections_context = SectionsContext {
        declared_feature_count: validated.analysis.sections.len(),
        capability: validated
            .analysis
            .capabilities
            .map(|capabilities| capabilities.sections)
            .map(|capability| SectionCapability {
                state: match capability.state {
                    AnalysisState::NotRun => "not_run",
                    AnalysisState::Unsupported => "unsupported",
                    AnalysisState::Candidate => "candidate",
                    AnalysisState::Validated => "validated",
                },
                source: match capability.source {
                    AnalysisSource::Algorithm => "algorithm",
                    AnalysisSource::Authored => "authored",
                    AnalysisSource::Measured => "measured",
                },
                confidence: capability.confidence,
            }),
    };
    let report = Report {
        schema_version: 1,
        source: &source,
        analysis: &evidence,
        sections_context,
    };
    labels_cli::require_fresh_source(package, &source)?;
    let saved = labels::write_new(&destination, &report, MAX_REPORT_BYTES)?;
    println!(
        "{}",
        serde_json::json!({
            "source": source,
            "saved_blake3": saved.to_hex().as_str(),
            "destination": destination,
            "profile": evidence.profile,
            "bin_count": evidence.bins.len(),
            "quality_status": evidence.quality_status,
            "production_admission": evidence.production_admission,
            "scope": "raw_descriptor_change_and_similarity_only"
        })
    );
    Ok(())
}

/// Builds a new four-object package; the embedded diagnostics retain the captured source CID
pub(crate) fn compile(package: &Path, channel: &str, destination: &Path) -> Result<(), String> {
    let channel = match channel {
        "left" => 0,
        "right" => 1,
        _ => {
            return Err("Structure segmentation requires an explicit left or right channel".into());
        }
    };
    let destination = labels_cli::outside_package(package, destination)?;
    let built =
        cocobeat_media::compile_structure_candidate_package(package, channel, &destination)?;
    println!(
        "{}",
        serde_json::json!({
            "content_id": blake3::Hash::from_bytes(built.manifest.package_hash).to_hex().as_str(),
            "destination": destination,
            "audio_blake3": blake3::Hash::from_bytes(built.manifest.audio.blake3).to_hex().as_str(),
            "canonical_frames": built.manifest.canonical_frames,
            "channel": channel,
            "section_count": built.analysis.sections.len(),
            "cue_count": built.chart.sections.len(),
            "profile": built.manifest.analysis_version,
            "confidence": null,
            "production_admission": false,
            "diagnostics": built.analysis.diagnostics,
            "scope": "candidate_sections_and_cues_not_musical_quality_admission",
        })
    );
    Ok(())
}

/// Publishes a complete analysis attempt, including unavailable segmentation
pub(crate) fn compile_analysis(
    package: &Path,
    channel: &str,
    destination: &Path,
) -> Result<(), String> {
    let channel = match channel {
        "left" => 0,
        "right" => 1,
        _ => {
            return Err("Structure segmentation requires an explicit left or right channel".into());
        }
    };
    let destination = labels_cli::outside_package(package, destination)?;
    let built = cocobeat_media::compile_structure_analysis_package(package, channel, &destination)?;
    println!(
        "{}",
        serde_json::json!({
            "content_id": blake3::Hash::from_bytes(built.manifest.package_hash).to_hex().as_str(),
            "destination": destination,
            "audio_blake3": blake3::Hash::from_bytes(built.manifest.audio.blake3).to_hex().as_str(),
            "canonical_frames": built.manifest.canonical_frames,
            "channel": channel,
            "section_count": built.analysis.sections.len(),
            "cue_count": built.chart.sections.len(),
            "profile": built.manifest.analysis_version,
            "confidence": null,
            "production_admission": false,
            "diagnostics": built.analysis.diagnostics,
            "scope": "structure_analysis_attempt_not_musical_quality_admission",
        })
    );
    Ok(())
}

/// Explicitly publishes interval candidates in a new package, preserving the existing chart
pub(crate) fn compile_repetition(
    package: &Path,
    channel: &str,
    destination: &Path,
) -> Result<(), String> {
    let channel = match channel {
        "left" => 0,
        "right" => 1,
        _ => return Err("Repetition candidates require an explicit left or right channel".into()),
    };
    let destination = labels_cli::outside_package(package, destination)?;
    let built =
        cocobeat_media::compile_repetition_candidate_package(package, channel, &destination)?;
    println!(
        "{}",
        serde_json::json!({
            "content_id": blake3::Hash::from_bytes(built.manifest.package_hash).to_hex().as_str(),
            "destination": destination,
            "audio_blake3": blake3::Hash::from_bytes(built.manifest.audio.blake3).to_hex().as_str(),
            "canonical_frames": built.manifest.canonical_frames,
            "channel": channel,
            "repetition_count": built.analysis.repetitions.len(),
            "profile": built.manifest.analysis_version,
            "confidence": null,
            "production_admission": false,
            "diagnostics": built.analysis.diagnostics,
            "scope": "candidate_spectral_interval_relations_not_musical_quality_admission",
        })
    );
    Ok(())
}

/// Publishes a complete repetition attempt without substituting authored relations
pub(crate) fn compile_repetition_analysis(
    package: &Path,
    channel: &str,
    destination: &Path,
) -> Result<(), String> {
    let channel = match channel {
        "left" => 0,
        "right" => 1,
        _ => return Err("Repetition candidates require an explicit left or right channel".into()),
    };
    let destination = labels_cli::outside_package(package, destination)?;
    let built =
        cocobeat_media::compile_repetition_analysis_package(package, channel, &destination)?;
    println!(
        "{}",
        serde_json::json!({
            "content_id": blake3::Hash::from_bytes(built.manifest.package_hash).to_hex().as_str(),
            "destination": destination,
            "audio_blake3": blake3::Hash::from_bytes(built.manifest.audio.blake3).to_hex().as_str(),
            "canonical_frames": built.manifest.canonical_frames,
            "channel": channel,
            "repetition_count": built.analysis.repetitions.len(),
            "profile": built.manifest.analysis_version,
            "confidence": null,
            "production_admission": false,
            "diagnostics": built.analysis.diagnostics,
            "scope": "repetition_analysis_attempt_not_musical_quality_admission",
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn report_binds_source_unknown_semantics_and_preserves_package_and_outputs() {
        let (root, package) = crate::anchors::tests::fixture("structure-feature-report");
        let source = root.join("source");
        let before =
            cocobeat_media::PACKAGE_OBJECT_NAMES.map(|name| fs::read(source.join(name)).unwrap());
        let destination = root.join("structure.json");
        inspect(&source, "right", &destination).unwrap();
        let bytes = fs::read(&destination).unwrap();
        assert!(bytes.len() <= MAX_REPORT_BYTES);
        let report: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            report["source"],
            serde_json::to_value(labels::Source::from_package(&package)).unwrap()
        );
        let analysis = &report["analysis"];
        assert_eq!(
            analysis["canonical_frames"],
            package.manifest.canonical_frames
        );
        assert_eq!(analysis["channel"], 1);
        assert_eq!(analysis["quality_status"], "UNASSESSED");
        assert_eq!(analysis["production_admission"], false);
        for field in ["confidence", "beat_unit", "meter"] {
            assert!(analysis[field].is_null());
        }
        assert_eq!(report["sections_context"]["declared_feature_count"], 0);
        assert!(report["sections_context"]["capability"].is_null());
        assert!(inspect(&source, "left", &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), bytes);
        assert!(inspect(&source, "left", &source.join("structure.json")).is_err());
        let wrong_channel = root.join("wrong-channel.json");
        assert!(inspect(&source, "stereo", &wrong_channel).is_err());
        assert!(!wrong_channel.exists());
        assert_eq!(
            before,
            cocobeat_media::PACKAGE_OBJECT_NAMES.map(|name| fs::read(source.join(name)).unwrap())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn report_byte_limit_rejects_before_creating_or_replacing_output() {
        let (root, _) = crate::anchors::tests::fixture("structure-feature-size-limit");
        let output = root.join("too-large.json");
        // Quotes and the final newline make this serialized JSON exceed the fixed limit
        let oversized = "x".repeat(MAX_REPORT_BYTES);
        assert!(labels::write_new(&output, &oversized, MAX_REPORT_BYTES).is_err());
        assert!(!output.exists());
        fs::write(&output, b"keep original report\n").unwrap();
        assert!(labels::write_new(&output, &oversized, MAX_REPORT_BYTES).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"keep original report\n");
        fs::remove_dir_all(root).unwrap();
    }
}
