//! Prepares one real canonical audio object before analysis and package assembly

use crate::{
    decode::{MAX_SOURCE_BYTES, MAX_SOURCE_SECONDS},
    decode_canonical,
};
use cocobeat_schema::{AssetRef, CANONICAL_SAMPLE_RATE};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCanonicalAudio {
    pub asset: AssetRef,
    pub canonical_frames: u64,
}

/// Copies and verifies the exact final Ogg in a new staging directory
/// Success establishes audio object identity, not encoder quality or package Ready
pub fn prepare_canonical_audio(
    source: impl AsRef<Path>,
    expected_frames: u64,
    new_staging_dir: impl AsRef<Path>,
) -> Result<PreparedCanonicalAudio, String> {
    prepare_canonical_audio_checked(
        source.as_ref(),
        expected_frames,
        new_staging_dir.as_ref(),
        &|| Ok(()),
    )
}

pub(crate) fn prepare_canonical_audio_checked(
    source: &Path,
    expected_frames: u64,
    new_staging_dir: &Path,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<PreparedCanonicalAudio, String> {
    check()?;
    if !(1..=u64::from(CANONICAL_SAMPLE_RATE) * MAX_SOURCE_SECONDS).contains(&expected_frames) {
        return Err(
            "Expected canonical frames must cover more than zero and at most ten minutes".into(),
        );
    }
    if !source.is_file() {
        return Err("Canonical audio source must be a regular file".into());
    }
    let source = File::open(source)
        .map_err(|error| format!("Cannot open canonical audio source: {error}"))?;
    let metadata = source.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE_BYTES {
        return Err("Canonical audio source must be a regular file no larger than 512 MiB".into());
    }
    check()?;
    let directory = new_staging_dir;
    fs::create_dir(directory)
        .map_err(|error| format!("Cannot create new audio staging directory: {error}"))?;
    let file_name = "song.audio.ogg";
    let output = directory.join(file_name);
    let mut created = false;
    let result: Result<PreparedCanonicalAudio, String> = (|| {
        check()?;
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|error| format!("Cannot create staged canonical audio: {error}"))?;
        created = true;
        // The live byte limit also catches a source growing after its metadata check
        let mut source = source.take(MAX_SOURCE_BYTES + 1);
        let mut hasher = blake3::Hasher::new();
        let mut byte_len = 0;
        let mut buffer = [0; 32 * 1024];
        loop {
            check()?;
            let read = source
                .read(&mut buffer)
                .map_err(|error| format!("Cannot read canonical audio source: {error}"))?;
            if read == 0 {
                break;
            }
            byte_len += read as u64;
            if byte_len > MAX_SOURCE_BYTES {
                return Err(
                    "Canonical audio source exceeds the 512 MiB limit during copying".into(),
                );
            }
            writer
                .write_all(&buffer[..read])
                .map_err(|error| format!("Cannot write staged canonical audio: {error}"))?;
            hasher.update(&buffer[..read]);
        }
        check()?;
        writer
            .sync_all()
            .map_err(|error| format!("Cannot sync staged canonical audio: {error}"))?;
        drop(writer);
        check()?;
        let canonical_frames = decode_canonical(&output, expected_frames, |_| check())?;
        check()?;
        Ok(PreparedCanonicalAudio {
            asset: AssetRef {
                file_name: file_name.into(),
                byte_len,
                blake3: *hasher.finalize().as_bytes(),
            },
            canonical_frames,
        })
    })();
    result.map_err(|mut error| {
        if created && let Err(cleanup) = fs::remove_file(&output) {
            error.push_str(&format!(
                "; remove staged audio {}: {cleanup}",
                output.display()
            ));
        }
        // Only remove an empty directory; any other content belongs to its creator
        if let Err(cleanup) = fs::remove_dir(directory) {
            error.push_str(&format!(
                "; remove audio staging directory {}: {cleanup}",
                directory.display()
            ));
        }
        error
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
    const STEREO: &[u8] =
        include_bytes!("../../../testdata/synthetic/media-import/stereo-canonical.ogg");

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cocobeat-audio-asset-{}-{}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn prepares_exact_audio_identity_and_preserves_existing_staging_content() {
        let root = TestDirectory::new();
        let source = root.0.join("source.ogg");
        fs::write(&source, STEREO).unwrap();
        let stage = root.0.join("staging");
        let prepared = prepare_canonical_audio(&source, 4_800, &stage).unwrap();
        assert_eq!(prepared.canonical_frames, 4_800);
        assert_eq!(prepared.asset.file_name, "song.audio.ogg");
        assert_eq!(prepared.asset.byte_len, 5_245);
        assert_eq!(prepared.asset.blake3, *blake3::hash(STEREO).as_bytes());
        assert_eq!(
            fs::read(stage.join(&prepared.asset.file_name)).unwrap(),
            STEREO
        );
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 1);
        fs::write(stage.join("keep"), b"existing staging content").unwrap();
        assert!(prepare_canonical_audio(&source, 4_800, &stage).is_err());
        assert_eq!(
            fs::read(stage.join("keep")).unwrap(),
            b"existing staging content"
        );
        assert_eq!(fs::read(stage.join("song.audio.ogg")).unwrap(), STEREO);
        assert_eq!(fs::read(source).unwrap(), STEREO);
    }

    #[test]
    fn rejects_invalid_sources_and_lengths_before_creating_staging() {
        let root = TestDirectory::new();
        let source = root.0.join("source.ogg");
        fs::write(&source, STEREO).unwrap();
        let stage = root.0.join("staging");
        for frames in [
            0,
            u64::from(CANONICAL_SAMPLE_RATE) * MAX_SOURCE_SECONDS + 1,
            u64::MAX,
        ] {
            assert!(prepare_canonical_audio(&source, frames, &stage).is_err());
            assert!(!stage.exists());
        }
        for source in [root.0.clone(), root.0.join("missing.ogg")] {
            assert!(prepare_canonical_audio(source, 4_800, &stage).is_err());
            assert!(!stage.exists());
        }
        File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_len(MAX_SOURCE_BYTES + 1)
            .unwrap();
        assert!(prepare_canonical_audio(source, 4_800, &stage).is_err());
        assert!(!stage.exists());
    }

    #[test]
    fn failed_final_readback_removes_only_the_new_audio_and_directory() {
        let root = TestDirectory::new();
        let source = root.0.join("source.ogg");
        let stage = root.0.join("staging");
        for (bytes, frames) in [
            (b"not an Ogg".as_slice(), 4_800),
            (STEREO, 4_801),
            (
                include_bytes!("../../../testdata/synthetic/media-import/mono.ogg").as_slice(),
                100_800,
            ),
            (&STEREO[..STEREO.len() - 1], 4_800),
        ] {
            fs::write(&source, bytes).unwrap();
            assert!(prepare_canonical_audio(&source, frames, &stage).is_err());
            assert!(!stage.exists());
            assert_eq!(fs::read(&source).unwrap(), bytes);
            assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
        }
    }
    #[test]
    fn checked_audio_copy_cleans_owned_file_and_reports_foreign_content() {
        let root = TestDirectory::new();
        let source = root.0.join("source.ogg");
        let stage = root.0.join("checked-stage");
        fs::write(&source, STEREO).unwrap();
        let error = prepare_canonical_audio_checked(&source, 4_800, &stage, &|| {
            if fs::metadata(stage.join("song.audio.ogg")).is_ok_and(|v| v.len() > 0) {
                fs::write(stage.join("keep"), b"foreign staging content").unwrap();
                Err("cancel after audio copy".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.starts_with("cancel after audio copy"));
        assert!(error.contains("remove audio staging directory"));
        assert!(!stage.join("song.audio.ogg").exists());
        assert_eq!(
            fs::read(stage.join("keep")).unwrap(),
            b"foreign staging content"
        );
        assert_eq!(fs::read(&source).unwrap(), STEREO);
        fs::remove_file(stage.join("keep")).unwrap();
        fs::remove_dir(&stage).unwrap();
        let checked = prepare_canonical_audio_checked(&source, 4_800, &stage, &|| Ok(())).unwrap();
        let ordinary =
            prepare_canonical_audio(&source, 4_800, root.0.join("ordinary-stage")).unwrap();
        assert_eq!(checked, ordinary);
        assert_eq!(fs::read(stage.join("song.audio.ogg")).unwrap(), STEREO);
    }
}
