use cocobeat_editor::AnchorEditor;
use cocobeat_schema::SongTime;
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    schema_version: u32,
    source_content_id: String,
    operations: Vec<Operation>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Add { id: u64, frame: i64 },
    Remove { id: u64 },
    Move { id: u64, frame: i64 },
    Undo {},
    Redo {},
}

pub fn edit(source: &Path, patch: &Path, destination: &Path) -> Result<(), String> {
    let patch = read_patch(patch)?;
    let package = cocobeat_media::validate_package(source)?;
    if patch.source_content_id != content_id(package.manifest.package_hash) {
        return Err("Patch source_content_id does not match the validated source package".into());
    }
    let mut editor = AnchorEditor::new(
        SongTime::from_frames(package.manifest.canonical_frames as i64),
        package.chart.anchors.clone(),
    )?;
    for (index, operation) in patch.operations.iter().enumerate() {
        match *operation {
            Operation::Add { id, frame } => editor.add(id, SongTime::from_frames(frame)),
            Operation::Remove { id } => editor.remove(id),
            Operation::Move { id, frame } => editor.move_to(id, SongTime::from_frames(frame)),
            Operation::Undo {} => editor.undo(),
            Operation::Redo {} => editor.redo(),
        }
        .map_err(|error| format!("Patch operation {}: {error}", index + 1))?;
    }
    let exported = cocobeat_media::export_anchors(
        source,
        package.manifest.package_hash,
        editor.anchors(),
        destination,
    )?;
    println!(
        "{}",
        serde_json::json!({
            "source_content_id": patch.source_content_id,
            "content_id": content_id(exported.manifest.package_hash),
            "operations": patch.operations.len(),
            "anchor_count": exported.chart.anchors.len(),
            "end_frames": exported.manifest.canonical_frames,
            "changed": exported.manifest.package_hash != package.manifest.package_hash,
        })
    );
    Ok(())
}

fn content_id(hash: [u8; 32]) -> String {
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("package-blake3:{hex}")
}

fn read_patch(path: &Path) -> Result<Patch, String> {
    if !path.is_file() {
        return Err(format!("Expected a regular patch file: {}", path.display()));
    }
    let file = File::open(path).map_err(|error| format!("Open patch: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Opened patch must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Read patch: {error}"))?;
    if bytes.len() > 1_048_576 {
        return Err("Patch document exceeds 1 MiB".into());
    }
    let patch: Patch = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid patch document: {error}"))?;
    if patch.schema_version != 1 {
        return Err("Unsupported patch document schema".into());
    }
    if patch.operations.len() > 1024 {
        return Err("Patch document exceeds 1024 operations".into());
    }
    Ok(patch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocobeat_media::PackageBuildInput;
    use cocobeat_schema::{Anchor, CompiledChart, EnergySample, MusicAnalysis};
    use std::{fs, path::PathBuf, time::SystemTime};

    fn directory(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cocobeat-editor-cli-{name}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn patch_rejects_ambiguous_fields_numbers_and_excess_input() {
        let root = directory("patch");
        let path = root.join("patch.json");
        let document = |operations: &str| {
            format!(r#"{{"schema_version":1,"source_content_id":"x","operations":[{operations}]}}"#)
        };
        let valid = document(
            r#"{"op":"add","id":18446744073709551615,"frame":9223372036854775807},{"op":"remove","id":0},{"op":"move","id":1,"frame":-9223372036854775808},{"op":"undo"},{"op":"redo"}"#,
        );
        fs::write(&path, &valid).unwrap();
        assert_eq!(read_patch(&path).unwrap().operations.len(), 5);
        for operation in [
            r#"{"op":"add","id":1,"frame":2,"extra":0}"#,
            r#"{"op":"remove","id":1,"frame":2}"#,
            r#"{"op":"move","id":1,"frame":2,"extra":0}"#,
            r#"{"op":"undo","id":1}"#,
            r#"{"op":"redo","frame":1}"#,
            r#"{"op":"add","id":18446744073709551616,"frame":1}"#,
            r#"{"op":"add","id":-1,"frame":1}"#,
            r#"{"op":"add","id":1.0,"frame":1}"#,
            r#"{"op":"add","id":1,"frame":9223372036854775808}"#,
            r#"{"op":"add","id":1,"frame":-9223372036854775809}"#,
            r#"{"op":"add","id":1,"frame":1.0}"#,
            r#"{"op":"move","id":1}"#,
            r#"{"op":"clear"}"#,
        ] {
            fs::write(&path, document(operation)).unwrap();
            assert!(read_patch(&path).is_err(), "{operation}");
        }
        for invalid in [
            valid.replacen("\"schema_version\":1", "\"schema_version\":2", 1),
            valid.replacen("{", "{\"extra\":1,", 1),
            valid.replacen("{", "{\"schema_version\":1,", 1),
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(read_patch(&path).is_err());
        }
        let operations = vec![r#"{"op":"add","id":1,"frame":1}"#; 1024].join(",");
        fs::write(&path, document(&operations)).unwrap();
        assert_eq!(read_patch(&path).unwrap().operations.len(), 1024);
        fs::write(
            &path,
            document(&format!("{operations},{{\"op\":\"undo\"}}")),
        )
        .unwrap();
        assert!(read_patch(&path).err().unwrap().contains("1024"));
        let mut bytes = valid.into_bytes();
        bytes.resize(1_048_576, b' ');
        fs::write(&path, &bytes).unwrap();
        assert!(read_patch(&path).is_ok());
        bytes.push(b' ');
        fs::write(&path, bytes).unwrap();
        assert!(read_patch(&path).err().unwrap().contains("1 MiB"));
        assert!(read_patch(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn identity_and_later_operation_failures_leave_source_and_destination_intact() {
        let root = directory("transaction");
        let source = root.join("source");
        let audio = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/synthetic/media-import/stereo-canonical.ogg");
        let package = cocobeat_media::build_package(&audio, 4800, &source, |_, prepared| {
            Ok(PackageBuildInput {
                song_id: "editor-cli-test".into(),
                importer_version: "test-v1".into(),
                analysis_version: "test-v1".into(),
                chart_version: "test-v1".into(),
                analysis: MusicAnalysis {
                    capabilities: None,
                    tempo_regions: Vec::new(),
                    repetitions: Vec::new(),
                    schema_version: 1,
                    audio_hash: prepared.asset.blake3,
                    beats: Vec::new(),
                    onsets: Vec::new(),
                    sections: Vec::new(),
                    energy: vec![EnergySample {
                        start: SongTime::ZERO,
                        frames: 4800,
                        rms: [0.0; 2],
                        peak: [0.0; 2],
                    }],
                    diagnostics: "Manually authored test content with placeholder energy".into(),
                },
                chart: CompiledChart {
                    schema_version: 1,
                    audio_hash: prepared.asset.blake3,
                    ruleset_id: "duo-watermark-v1".into(),
                    anchors: vec![Anchor {
                        id: 1,
                        song_time: SongTime::from_frames(1200),
                    }],
                    sections: Vec::new(),
                },
            })
        })
        .unwrap();
        let names = [
            "song.audio.ogg",
            "analysis.bin",
            "chart.bin",
            "song.package",
        ];
        let original = names.map(|name| fs::read(source.join(name)).unwrap());
        let patch = root.join("patch.json");
        let destination = root.join("result");
        let mut document = serde_json::json!({
            "schema_version": 1,
            "source_content_id": content_id(package.manifest.package_hash),
            "operations": [
                {"op": "move", "id": 1, "frame": 1300},
                {"op": "remove", "id": 99},
            ],
        });
        fs::write(&patch, document.to_string()).unwrap();
        assert!(
            edit(&source, &patch, &destination)
                .unwrap_err()
                .contains("operation 2")
        );
        assert!(!destination.exists());
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("keep"), b"existing").unwrap();
        assert!(edit(&source, &patch, &destination).is_err());
        assert_eq!(fs::read(destination.join("keep")).unwrap(), b"existing");
        document["source_content_id"] = "package-blake3:wrong".into();
        fs::write(&patch, document.to_string()).unwrap();
        let mismatch = root.join("mismatch");
        assert!(
            edit(&source, &patch, &mismatch)
                .unwrap_err()
                .contains("source_content_id")
        );
        assert!(!mismatch.exists());
        assert_eq!(
            names.map(|name| fs::read(source.join(name)).unwrap()),
            original
        );
        assert_eq!(cocobeat_media::validate_package(&source).unwrap(), package);
        fs::remove_dir_all(root).unwrap();
    }
}
