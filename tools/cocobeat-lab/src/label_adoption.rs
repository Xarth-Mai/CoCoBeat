//! Explicit manual point choices, independent of experimental MIR confidence

use crate::labels::{self, AnchorDecision, Document, Location, Source};
use cocobeat_schema::{Anchor, SongTime};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

const MAX_SELECTION_BYTES: usize = 1_048_576;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    schema_version: u32,
    source_content_id: String,
    labels_blake3: String,
    item_ids: Vec<u64>,
}

pub(crate) fn adopt(
    source: &Path,
    labels_path: &Path,
    selection_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    let package = cocobeat_media::validate_package(source)?;
    let initial = Source::from_package(&package);
    let (document, labels_hash) = labels::load_identified(labels_path, &initial)?;
    let selection = crate::anchors::read_document(selection_path, MAX_SELECTION_BYTES)?;
    let anchors = select(&document, labels_hash, &selection)?;
    let destination = crate::labels_cli::outside_package(source, destination)?;
    crate::labels_cli::require_fresh_source(source, &initial)?;
    let exported = cocobeat_media::export_anchors(
        source,
        package.manifest.package_hash,
        &anchors,
        &destination,
    )?;
    println!(
        "{}",
        serde_json::json!({
            "source_content_id": initial.content_id,
            "content_id": Source::from_package(&exported).content_id,
            "labels_blake3": labels_hash.to_hex().as_str(),
            "reviewer": document.reviewer,
            "item_ids": anchors.iter().map(|anchor| anchor.id).collect::<Vec<_>>(),
            "anchor_count": anchors.len(),
            "section_cue_count": exported.chart.sections.len(),
            "changed": exported.manifest.package_hash != package.manifest.package_hash,
            "scope": "explicit_manual_point_adoption",
            "production_admission": "not_assessed",
        })
    );
    Ok(())
}

fn select(
    document: &Document,
    labels_hash: blake3::Hash,
    selection: &Selection,
) -> Result<Vec<Anchor>, String> {
    if selection.schema_version != 1 {
        return Err("Unsupported labeled Anchor selection schema".into());
    }
    if selection.source_content_id != document.source.content_id {
        return Err("Labeled Anchor selection source identity does not match".into());
    }
    if selection.labels_blake3 != labels_hash.to_hex().as_str() {
        return Err("Labeled Anchor selection differs from the exact label file bytes".into());
    }
    if selection.item_ids.len() > 1024 {
        return Err("Labeled Anchor selection exceeds 1024 items".into());
    }
    let mut available: BTreeMap<_, _> = document
        .labels
        .iter()
        .map(|label| (label.item_id, label))
        .collect();
    let mut selected = BTreeMap::new();
    for &id in &selection.item_ids {
        let label = available
            .remove(&id)
            .ok_or_else(|| format!("Selected label item_id is missing or duplicated: {id}"))?;
        if label.anchor_decision != AnchorDecision::ShouldAnchor {
            return Err(format!(
                "Selected label {id} must explicitly say should_anchor"
            ));
        }
        let Location::Point { frame } = label.location else {
            return Err(format!(
                "Selected label {id} must have an exact point location"
            ));
        };
        if selected
            .insert(
                frame,
                Anchor {
                    id,
                    song_time: SongTime::from_frames(frame),
                },
            )
            .is_some()
        {
            return Err(format!(
                "Selected labels share the same Anchor frame: {frame}"
            ));
        }
    }
    Ok(selected.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::{Label, Playback};
    use cocobeat_media::{PACKAGE_OBJECT_NAMES, ValidatedPackage};
    use std::{fs, path::PathBuf};

    fn fixture(name: &str) -> (PathBuf, ValidatedPackage, Document) {
        let (root, package) = crate::anchors::tests::fixture(name);
        let document = Document {
            schema_version: 1,
            source: Source::from_package(&package),
            reviewer: "mechanism-control".into(),
            labels: [(7, 0), (99, 333), (u64::MAX, 4799)]
                .into_iter()
                .map(|(item_id, frame)| Label {
                    item_id,
                    location: Location::Point { frame },
                    anchor_decision: AnchorDecision::ShouldAnchor,
                    reason: "Constructed choice, not a human musical label".into(),
                    playback: Playback::Stereo,
                })
                .collect(),
        };
        (root, package, document)
    }

    fn inputs(root: &Path, document: &Document, ids: &[u64]) -> (PathBuf, PathBuf) {
        let labels = root.join("labels.json");
        let selection = root.join("selection.json");
        let bytes = serde_json::to_vec_pretty(document).unwrap();
        fs::write(&labels, &bytes).unwrap();
        fs::write(
            &selection,
            serde_json::json!({
                "schema_version": 1,
                "source_content_id": document.source.content_id,
                "labels_blake3": blake3::hash(&bytes).to_hex().as_str(),
                "item_ids": ids,
            })
            .to_string(),
        )
        .unwrap();
        (labels, selection)
    }

    fn objects(path: &Path) -> [Vec<u8>; 4] {
        PACKAGE_OBJECT_NAMES.map(|name| fs::read(path.join(name)).unwrap())
    }

    #[test]
    fn explicit_points_keep_ids_sort_by_frame_and_preserve_source_bytes() {
        let (root, package, document) = fixture("label-adoption-points");
        let source = root.join("source");
        let before = objects(&source);
        let (labels, selection) = inputs(&root, &document, &[u64::MAX, 7]);
        let input_before = [fs::read(&labels).unwrap(), fs::read(&selection).unwrap()];
        let destination = root.join("chosen");
        adopt(&source, &labels, &selection, &destination).unwrap();
        let exported = cocobeat_media::validate_package(&destination).unwrap();
        assert_eq!(
            exported.chart.anchors,
            vec![
                Anchor {
                    id: 7,
                    song_time: SongTime::ZERO
                },
                Anchor {
                    id: u64::MAX,
                    song_time: SongTime::from_frames(4799)
                },
            ]
        );
        assert_eq!(exported.chart.sections, package.chart.sections);
        assert_eq!(exported.chart.ruleset_id, package.chart.ruleset_id);
        assert_eq!(exported.analysis, package.analysis);
        let output = objects(&destination);
        assert_eq!(output[0], before[0]);
        assert_eq!(output[1], before[1]);
        assert_eq!(objects(&source), before);
        assert_eq!(
            [fs::read(&labels).unwrap(), fs::read(&selection).unwrap()],
            input_before
        );
        let (labels, selection) = inputs(&root, &document, &[7, u64::MAX]);
        adopt(&source, &labels, &selection, &root.join("reordered")).unwrap();
        assert_eq!(objects(&root.join("reordered")), output);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ambiguous_negative_missing_duplicate_and_same_frame_choices_are_rejected() {
        let (root, _, document) = fixture("label-adoption-choices");
        let source = root.join("source");
        let before = objects(&source);
        let destination = root.join("rejected");
        for ids in [&[7, 7][..], &[123], &[7, 123]] {
            let (labels, selection) = inputs(&root, &document, ids);
            let error = adopt(&source, &labels, &selection, &destination).unwrap_err();
            assert!(error.contains("missing or duplicated"), "{error}");
            assert!(!destination.exists());
        }
        for decision in [AnchorDecision::ShouldNotAnchor, AnchorDecision::Uncertain] {
            let mut changed = document.clone();
            changed.labels[0].anchor_decision = decision;
            let (labels, selection) = inputs(&root, &changed, &[7]);
            let error = adopt(&source, &labels, &selection, &destination).unwrap_err();
            assert!(error.contains("should_anchor"), "{error}");
            assert!(!destination.exists());
        }
        let mut changed = document.clone();
        changed.labels[0].location = Location::Interval {
            start_frame: 0,
            end_frame: 1,
        };
        let (labels, selection) = inputs(&root, &changed, &[7]);
        assert!(
            adopt(&source, &labels, &selection, &destination)
                .unwrap_err()
                .contains("exact point")
        );
        changed.labels[0].location = Location::Point { frame: 333 };
        let (labels, selection) = inputs(&root, &changed, &[7, 99]);
        assert!(
            adopt(&source, &labels, &selection, &destination)
                .unwrap_err()
                .contains("same Anchor frame")
        );
        assert!(!destination.exists());
        assert_eq!(objects(&source), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selection_identity_schema_numbers_fields_and_count_are_strict() {
        let (root, _, document) = fixture("label-adoption-selection");
        let source = root.join("source");
        let (labels, selection) = inputs(&root, &document, &[7]);
        let original = fs::read_to_string(&selection).unwrap();
        let destination = root.join("rejected");
        for (field, replacement, expected) in [
            ("schema_version", serde_json::json!(2), "Unsupported"),
            (
                "source_content_id",
                serde_json::json!("wrong"),
                "source identity",
            ),
            (
                "labels_blake3",
                serde_json::json!("wrong"),
                "exact label file bytes",
            ),
            ("item_ids", serde_json::json!(vec![7; 1025]), "1024"),
            ("item_ids", serde_json::json!([-1]), "Invalid JSON"),
            ("item_ids", serde_json::json!([7.0]), "Invalid JSON"),
        ] {
            let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
            value[field] = replacement;
            fs::write(&selection, value.to_string()).unwrap();
            let error = adopt(&source, &labels, &selection, &destination).unwrap_err();
            assert!(error.contains(expected), "{field}: {error}");
            assert!(!destination.exists());
        }
        for changed in [
            original.replacen('{', "{\"unknown\":1,", 1),
            original.replacen('{', "{\"schema_version\":1,", 1),
            original.replace("[7]", "[18446744073709551616]"),
        ] {
            assert_ne!(changed, original);
            fs::write(&selection, changed).unwrap();
            assert!(
                adopt(&source, &labels, &selection, &destination)
                    .unwrap_err()
                    .contains("Invalid JSON")
            );
            assert!(!destination.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_label_bytes_and_saved_hash_are_bound_without_reserialization() {
        let (root, _, document) = fixture("label-adoption-hash");
        let source = root.join("source");
        let (labels, selection) = inputs(&root, &document, &[7]);
        let raw = fs::read(&labels).unwrap();
        let (loaded, hash) = labels::load_identified(&labels, &document.source).unwrap();
        assert_eq!(loaded, document);
        assert_eq!(hash, blake3::hash(&raw));
        let saved = root.join("saved-labels.json");
        let saved_hash = labels::save(&saved, &document.source, &loaded).unwrap();
        assert_eq!(saved_hash, blake3::hash(&fs::read(&saved).unwrap()));
        let destination = root.join("rejected");
        let mut reformatted = raw;
        reformatted.push(b'\n');
        fs::write(&labels, reformatted).unwrap();
        assert_eq!(labels::load(&labels, &document.source).unwrap(), document);
        assert!(
            adopt(&source, &labels, &selection, &destination)
                .unwrap_err()
                .contains("exact label file bytes")
        );
        assert!(!destination.exists());
        let mut wrong_source = document;
        wrong_source.source.canonical_frames += 1;
        let (labels, selection) = inputs(&root, &wrong_source, &[7]);
        assert!(
            adopt(&source, &labels, &selection, &destination)
                .unwrap_err()
                .contains("source identity")
        );
        assert!(!destination.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bounded_regular_inputs_and_existing_or_source_outputs_are_protected() {
        let (root, _, document) = fixture("label-adoption-paths");
        let source = root.join("source");
        let before = objects(&source);
        let (labels, selection) = inputs(&root, &document, &[7]);
        let original = fs::read(&selection).unwrap();
        let mut padded = original.clone();
        padded.resize(MAX_SELECTION_BYTES, b' ');
        fs::write(&selection, &padded).unwrap();
        adopt(&source, &labels, &selection, &root.join("bounded")).unwrap();
        padded.push(b' ');
        fs::write(&selection, padded).unwrap();
        assert!(
            adopt(&source, &labels, &selection, &root.join("oversized"))
                .unwrap_err()
                .contains("exceeds")
        );
        assert!(!root.join("oversized").exists());
        fs::write(&selection, original).unwrap();
        for destination in [
            &labels,
            &selection,
            &source,
            &source.join("new-package"),
            &root.join("missing/new-package"),
        ] {
            assert!(adopt(&source, &labels, &selection, destination).is_err());
        }
        assert_eq!(objects(&source), before);
        assert!(!source.join("new-package").exists());
        assert!(
            adopt(&source, &root, &selection, &root.join("directory-label"))
                .unwrap_err()
                .contains("regular file")
        );
        assert!(
            adopt(&source, &labels, &root, &root.join("directory-selection"))
                .unwrap_err()
                .contains("regular file")
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&labels, root.join("label-alias")).unwrap();
            std::os::unix::fs::symlink(&selection, root.join("selection-alias")).unwrap();
            std::os::unix::fs::symlink(&source, root.join("source-alias")).unwrap();
            assert!(
                adopt(
                    &source,
                    &root.join("label-alias"),
                    &selection,
                    &root.join("alias-label")
                )
                .unwrap_err()
                .contains("regular file")
            );
            assert!(
                adopt(
                    &source,
                    &labels,
                    &root.join("selection-alias"),
                    &root.join("alias-selection")
                )
                .unwrap_err()
                .contains("regular file")
            );
            assert!(
                adopt(
                    &source,
                    &labels,
                    &selection,
                    &root.join("source-alias/new-package")
                )
                .is_err()
            );
        }
        let (labels, selection) = inputs(&root, &document, &[7]);
        let mut label_bytes = fs::read(&labels).unwrap();
        label_bytes.resize(MAX_SELECTION_BYTES + 1, b' ');
        fs::write(&labels, label_bytes).unwrap();
        assert!(
            adopt(&source, &labels, &selection, &root.join("oversized-label"))
                .unwrap_err()
                .contains("exceeds 1 MiB")
        );
        assert!(!root.join("oversized-label").exists());
        assert_eq!(objects(&source), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_selection_clears_anchors_and_noop_preserves_all_four_objects() {
        let (root, package, document) = fixture("label-adoption-empty-noop");
        let source = root.join("source");
        let before = objects(&source);
        let (labels, selection) = inputs(&root, &document, &[99]);
        adopt(&source, &labels, &selection, &root.join("noop")).unwrap();
        assert_eq!(objects(&root.join("noop")), before);
        let (labels, selection) = inputs(&root, &document, &[]);
        adopt(&source, &labels, &selection, &root.join("empty")).unwrap();
        let empty = cocobeat_media::validate_package(root.join("empty")).unwrap();
        assert!(empty.chart.anchors.is_empty());
        assert_eq!(empty.chart.sections, package.chart.sections);
        assert_ne!(empty.manifest.package_hash, package.manifest.package_hash);
        assert_eq!(empty.analysis, package.analysis);
        assert_eq!(objects(&source), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_content_rejects_old_replay_and_new_facts_match_the_same_core() {
        use cocobeat_core::DuoEngine;
        use cocobeat_replay::{Replay, ReplayIdentity};
        use cocobeat_schema::{DuoInput, DuoRules, Hit, PlayerId, SessionEpoch};
        let (root, package, document) = fixture("label-adoption-replay");
        let source = root.join("source");
        let (labels, selection) = inputs(&root, &document, &[7]);
        let destination = root.join("chosen");
        adopt(&source, &labels, &selection, &destination).unwrap();
        let exported = cocobeat_media::validate_package(&destination).unwrap();
        let identity = |content_id| ReplayIdentity {
            content_id,
            rules_id: package.chart.ruleset_id.clone(),
            build_id: "constructed-label-adoption-test".into(),
            stage_compiler_version: Some(2),
        };
        let epoch = SessionEpoch(7);
        let old = Replay::new(identity(document.source.content_id), epoch).unwrap();
        let old_path = root.join("old.replay");
        old.save(&old_path).unwrap();
        assert!(crate::replay::load(&package, &old_path).is_ok());
        assert!(
            crate::replay::load(&exported, &old_path)
                .unwrap_err()
                .contains("identity mismatch")
        );
        let mut replay =
            Replay::new(identity(Source::from_package(&exported).content_id), epoch).unwrap();
        let mut live =
            DuoEngine::new(epoch, exported.chart.anchors.clone(), DuoRules::default()).unwrap();
        let hits = [PlayerId::P1, PlayerId::P2].map(|player| {
            DuoInput::Hit(Hit {
                epoch,
                player,
                seq: 1,
                song_time: SongTime::ZERO,
            })
        });
        let watermarks = [PlayerId::P1, PlayerId::P2].map(|player| DuoInput::Watermark {
            epoch,
            player,
            through: SongTime::from_frames(48_000),
        });
        for fact in hits.into_iter().chain(watermarks) {
            live.ingest(fact).unwrap();
            replay.record(fact).unwrap();
        }
        let replay_path = root.join("new.replay");
        replay.save(&replay_path).unwrap();
        let (read, replayed) = crate::replay::load(&exported, &replay_path).unwrap();
        assert_eq!(read.facts(), replay.facts());
        assert!(!live.events().is_empty());
        assert_eq!(replayed.events(), live.events());
        assert_eq!(replayed.resonance(), live.resonance());
        assert_eq!(read.identity().stage_compiler_version, Some(2));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn candidate_unknowns_and_manual_sections_preserve_stage_geometry() {
        use cocobeat_media::PackageBuildInput;
        use cocobeat_schema::{AnalysisCapabilities, AnalysisState, BeatFeature, SectionFeature};
        let (root, package, mut document) = fixture("label-adoption-candidate");
        let original = root.join("source");
        let source = root.join("constructed-candidate");
        let mut analysis = package.analysis.clone();
        analysis.schema_version = 2;
        let mut capabilities = AnalysisCapabilities::authored();
        capabilities.beat.state = AnalysisState::Candidate;
        capabilities.downbeat.state = AnalysisState::Candidate;
        capabilities.onset.state = AnalysisState::Unsupported;
        analysis.capabilities = Some(capabilities);
        analysis.onsets.clear();
        analysis.beats = vec![BeatFeature {
            time: SongTime::from_frames(100),
            strength: 0.75,
            downbeat_probability: Some(0.4),
            confidence: None,
        }];
        analysis.sections = vec![SectionFeature {
            start: SongTime::from_frames(100),
            end: SongTime::from_frames(1000),
            confidence: None,
            label: "Authored interval, no automatic structure claim".into(),
        }];
        analysis.diagnostics =
            "Constructed Candidate-state preservation control, not a model run".into();
        let candidate = cocobeat_media::build_package(
            original.join(PACKAGE_OBJECT_NAMES[0]),
            package.manifest.canonical_frames,
            &source,
            |_, _| {
                Ok(PackageBuildInput {
                    song_id: package.manifest.song_id.clone(),
                    importer_version: "test-v1".into(),
                    analysis_version: "constructed-candidate-v2".into(),
                    chart_version: "manual-test-v1".into(),
                    analysis,
                    chart: package.chart.clone(),
                })
            },
        )
        .unwrap();
        document.source = Source::from_package(&candidate);
        let before = objects(&source);
        let (labels, selection) = inputs(&root, &document, &[7]);
        let destination = root.join("chosen");
        adopt(&source, &labels, &selection, &destination).unwrap();
        let exported = cocobeat_media::validate_package(&destination).unwrap();
        assert_eq!(exported.analysis, candidate.analysis);
        assert_eq!(objects(&destination)[1], before[1]);
        assert_eq!(objects(&source), before);
        assert!(exported.analysis.onsets.is_empty());
        assert_eq!(exported.analysis.beats[0].confidence, None);
        let end = SongTime::from_frames(candidate.manifest.canonical_frames as i64);
        let old_stage = cocobeat_stage::compile_version(
            &document.source.content_id,
            end,
            &candidate.analysis.sections,
            2,
        )
        .unwrap();
        let new_stage = cocobeat_stage::compile_version(
            &Source::from_package(&exported).content_id,
            end,
            &exported.analysis.sections,
            2,
        )
        .unwrap();
        assert_ne!(old_stage.content_id(), new_stage.content_id());
        assert_eq!(old_stage.segments(), new_stage.segments());
        for frame in [0, 99, 100, 550, 999, 1000, 4799, 4800] {
            assert_eq!(
                old_stage.sample(SongTime::from_frames(frame)),
                new_stage.sample(SongTime::from_frames(frame))
            );
        }
        assert_eq!(new_stage.compiler_version(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
