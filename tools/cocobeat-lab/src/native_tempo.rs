//! Publishes raw native tempo observations outside an unchanged canonical package

use crate::{labels, labels_cli};
use serde::Serialize;
use std::path::Path;

// At most 225000 observations over the existing ten-minute canonical input limit
const MAX_REPORT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Serialize)]
struct Report<'a> {
    schema_version: u32,
    source: &'a labels::Source,
    analysis: &'a cocobeat_media::NativeTempoEvidence,
}

pub(crate) fn inspect(package: &Path, channel: &str, destination: &Path) -> Result<(), String> {
    let channel = match channel {
        "left" => 0,
        "right" => 1,
        _ => return Err("Native tempo requires an explicit left or right channel".into()),
    };
    let destination = labels_cli::outside_package(package, destination)?;
    let (validated, evidence) = cocobeat_media::inspect_native_tempo_package(package, channel)?;
    let source = labels::Source::from_package(&validated);
    let report = Report {
        schema_version: 1,
        source: &source,
        analysis: &evidence,
    };
    labels_cli::require_fresh_source(package, &source)?;
    let saved = labels::write_new(&destination, &report, MAX_REPORT_BYTES)?;
    println!(
        "{}",
        serde_json::json!({
            "source": source,
            "saved_blake3": saved.to_hex().as_str(),
            "destination": destination,
            "record_count": evidence.records.len(),
            "quality_status": evidence.quality_status,
            "production_admission": evidence.production_admission,
            "scope": "raw_causal_tempo_observations_only"
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn report_has_real_extent_unknown_semantics_and_preserves_package_and_existing_output() {
        let (root, package) = crate::anchors::tests::fixture("native-tempo-report");
        let source = root.join("source");
        let before =
            cocobeat_media::PACKAGE_OBJECT_NAMES.map(|name| fs::read(source.join(name)).unwrap());
        let output = root.join("tempo.json");
        inspect(&source, "left", &output).unwrap();
        let bytes = fs::read(&output).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let analysis = &report["analysis"];
        assert_eq!(
            analysis["canonical_frames"],
            package.manifest.canonical_frames
        );
        assert_eq!(
            analysis["records"].as_array().unwrap().last().unwrap()["consumed_frames"],
            package.manifest.canonical_frames
        );
        assert_eq!(
            analysis["records"].as_array().unwrap().len() as u64,
            package.manifest.canonical_frames.div_ceil(128)
        );
        for field in ["beat_unit", "meter", "confidence"] {
            assert!(analysis[field].is_null());
        }
        assert_eq!(analysis["production_admission"], false);
        assert_eq!(
            analysis["quality_status"],
            "UNSCORED_NO_ADMISSION_THRESHOLD"
        );
        assert!(inspect(&source, "left", &output).is_err());
        assert_eq!(fs::read(&output).unwrap(), bytes);
        assert!(inspect(&source, "stereo", &root.join("stereo.json")).is_err());
        assert!(inspect(&source, "right", &source.join("inside.json")).is_err());
        assert_eq!(
            before,
            cocobeat_media::PACKAGE_OBJECT_NAMES.map(|name| fs::read(source.join(name)).unwrap())
        );
        fs::remove_dir_all(root).unwrap();
    }
}
