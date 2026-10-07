//! Fresh source checks and new-file CLI outputs for independent manual labels

use crate::labels::{self, Source};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn inspect_source(source: &Path) -> Result<(), String> {
    let source = Source::from_package(&cocobeat_media::validate_package(source)?);
    println!(
        "{}",
        serde_json::to_string_pretty(&source).map_err(|error| error.to_string())?
    );
    Ok(())
}

pub(crate) fn import(source: &Path, input: &Path, destination: &Path) -> Result<(), String> {
    let initial = Source::from_package(&cocobeat_media::validate_package(source)?);
    let document = labels::load(input, &initial)?;
    let destination = outside_package(source, destination)?;
    require_fresh_source(source, &initial)?;
    labels::save(&destination, &initial, &document)?;
    println!(
        "{}",
        serde_json::json!({
            "source": initial,
            "reviewer": document.reviewer,
            "label_count": document.labels.len(),
            "destination": destination,
            "scope": "manual_records_only",
        })
    );
    Ok(())
}

pub(crate) fn compare(
    source: &Path,
    left: &Path,
    right: &Path,
    destination: &Path,
) -> Result<(), String> {
    let initial = Source::from_package(&cocobeat_media::validate_package(source)?);
    let left = labels::load(left, &initial)?;
    let right = labels::load(right, &initial)?;
    let comparison = labels::compare(&initial, &left, &right)?;
    let destination = outside_package(source, destination)?;
    require_fresh_source(source, &initial)?;
    labels::save_comparison(&destination, &comparison)?;
    println!(
        "{}",
        serde_json::json!({
            "source": initial,
            "left_reviewer": comparison.left_reviewer,
            "right_reviewer": comparison.right_reviewer,
            "common_count": comparison.common.len(),
            "left_only_count": comparison.left_only.len(),
            "right_only_count": comparison.right_only.len(),
            "destination": destination,
            "scope": "explicit_item_id_comparison_only",
        })
    );
    Ok(())
}

// Reusable by a later GUI save; expected identity comes from the original load
pub(crate) fn require_fresh_source(source: &Path, expected: &Source) -> Result<(), String> {
    let fresh = Source::from_package(&cocobeat_media::validate_package(source)?);
    if fresh != *expected {
        return Err("Label source package changed after initial validation; keep the draft and reopen the intended package".into());
    }
    Ok(())
}

pub(crate) fn outside_package(source: &Path, destination: &Path) -> Result<PathBuf, String> {
    let name = destination
        .file_name()
        .ok_or("Label output must name a new file")?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|error| format!("Cannot resolve label output parent: {error}"))?;
    let package = fs::canonicalize(source)
        .map_err(|error| format!("Cannot resolve label source package: {error}"))?;
    if parent.starts_with(package) {
        return Err(
            "Independent labels and reports must be written outside the source package".into(),
        );
    }
    // The checked canonical parent is also used for the subsequent create_new
    Ok(parent.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::{AnchorDecision, Document, Label, Location, Playback};
    use cocobeat_schema::{Anchor, SongTime};
    fn document(source: Source, reviewer: &str) -> Document {
        Document {
            schema_version: 1,
            source,
            reviewer: reviewer.into(),
            labels: vec![Label {
                item_id: 7,
                location: Location::Point { frame: 100 },
                anchor_decision: AnchorDecision::ShouldNotAnchor,
                reason: "Constructed mechanism control, not a human label".into(),
                playback: Playback::Stereo,
            }],
        }
    }
    #[test]
    fn import_and_comparison_preserve_source_input_and_both_reviewers() {
        let (root, package) = crate::anchors::tests::fixture("independent-label-cli");
        let source = root.join("source");
        let identity = Source::from_package(&package);
        let names = cocobeat_media::PACKAGE_OBJECT_NAMES;
        let before = names.map(|name| fs::read(source.join(name)).unwrap());
        let left = document(identity.clone(), "reviewer-a");
        let mut right = document(identity.clone(), "reviewer-b");
        right.labels[0].location = Location::Interval {
            start_frame: 99,
            end_frame: 102,
        };
        right.labels[0].anchor_decision = AnchorDecision::Uncertain;
        let a = root.join("a.json");
        let b = root.join("b.json");
        labels::save(&a, &identity, &left).unwrap();
        labels::save(&b, &identity, &right).unwrap();
        let input_before = fs::read(&a).unwrap();
        let copied = root.join("a-copy.json");
        import(&source, &a, &copied).unwrap();
        assert_eq!(labels::load(&copied, &identity).unwrap(), left);
        assert!(import(&source, &a, &copied).is_err());
        assert_eq!(fs::read(&a).unwrap(), input_before);
        let report = root.join("comparison.json");
        compare(&source, &a, &b, &report).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        assert_eq!(value["common"][0]["left"]["location"]["frame"], 100);
        assert_eq!(value["common"][0]["right"]["location"]["end_frame"], 102);
        assert_eq!(value["common"][0]["same_location"], false);
        assert_eq!(value["common"][0]["same_anchor_decision"], false);
        assert_eq!(
            before,
            names.map(|name| fs::read(source.join(name)).unwrap())
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn package_inside_and_alias_outputs_are_rejected() {
        let (root, _) = crate::anchors::tests::fixture("independent-label-boundary");
        let source = root.join("source");
        assert!(outside_package(&source, &source.join("labels.json")).is_err());
        fs::create_dir(source.join("nested")).unwrap();
        assert!(outside_package(&source, &source.join("nested/labels.json")).is_err());
        assert!(outside_package(&source, &root.join("missing-parent/labels.json")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&source, root.join("alias")).unwrap();
            assert!(outside_package(&source, &root.join("alias/labels.json")).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_different_fresh_valid_package_is_rejected() {
        let (root, package) = crate::anchors::tests::fixture("independent-label-fresh");
        let source = root.join("source");
        let initial = Source::from_package(&package);
        let revised = root.join("revised");
        cocobeat_media::export_anchors(
            &source,
            package.manifest.package_hash,
            &[Anchor {
                id: 77,
                song_time: SongTime::from_frames(200),
            }],
            &revised,
        )
        .unwrap();
        for name in cocobeat_media::PACKAGE_OBJECT_NAMES {
            fs::copy(revised.join(name), source.join(name)).unwrap();
        }
        assert!(cocobeat_media::validate_package(&source).is_ok());
        assert!(require_fresh_source(&source, &initial).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
