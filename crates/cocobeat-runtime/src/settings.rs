use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 4096;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DisplaySettings {
    pub fullscreen: bool,
    pub window_size: [u32; 2],
    pub fullscreen_size: [u32; 2],
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            fullscreen: false,
            window_size: [1280, 800],
            fullscreen_size: [1280, 800],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Settings {
    pub display: DisplaySettings,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    display: DisplaySettings,
}

fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn validate(display: DisplaySettings) -> io::Result<()> {
    if display
        .window_size
        .into_iter()
        .chain(display.fullscreen_size)
        .any(|size| !(1..=16384).contains(&size))
    {
        return Err(invalid("Display dimensions must be between 1 and 16384"));
    }
    Ok(())
}

pub fn load(path: &Path) -> io::Result<Settings> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid("Settings file exceeds 4096 bytes"));
    }
    let document: Document = serde_json::from_slice(&bytes).map_err(invalid)?;
    if document.version != VERSION {
        return Err(invalid("Unsupported settings version"));
    }
    validate(document.display)?;
    Ok(Settings {
        display: document.display,
    })
}

/// Syncs a same-directory temporary file before replacing the destination
/// The old file is never deleted first; directory-entry power-loss durability
/// remains subject to the filesystem, as with Replay saves
pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    validate(settings.display)?;
    let bytes = serde_json::to_vec_pretty(&Document {
        version: VERSION,
        display: settings.display,
    })
    .map_err(invalid)?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid("Settings path requires a file name"))?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut temporary_name = name.to_os_string();
    temporary_name.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let temporary = path.with_file_name(temporary_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn absolute_env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub fn default_path() -> io::Result<PathBuf> {
    #[cfg(windows)]
    let directory = absolute_env_path("APPDATA")
        .ok_or_else(|| invalid("APPDATA must contain an absolute configuration directory"))?
        .join("CoCoBeat");
    #[cfg(not(windows))]
    let directory = absolute_env_path("XDG_CONFIG_HOME")
        .or_else(|| absolute_env_path("HOME").map(|home| home.join(".config")))
        .ok_or_else(|| invalid("XDG_CONFIG_HOME or HOME must contain an absolute directory"))?
        .join("cocobeat");
    Ok(directory.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_replaces_and_preserves_files_on_failure() {
        let root = env::temp_dir().join(format!(
            "cocobeat-settings-test-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let path = root.join("config/settings.json");
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::NotFound);
        let mut settings = Settings::default();
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        settings.display.fullscreen = true;
        settings.display.window_size = [1600, 900];
        settings.display.fullscreen_size = [1920, 1080];
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);

        let previous = fs::read(&path).unwrap();
        settings.display.window_size = [0, 900];
        assert!(save(&path, &settings).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);

        let occupied = root.join("occupied");
        fs::create_dir(&occupied).unwrap();
        fs::write(occupied.join("keep"), b"previous data").unwrap();
        assert!(save(&occupied, &Settings::default()).is_err());
        assert_eq!(fs::read(occupied.join("keep")).unwrap(), b"previous data");
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_corrupt_unknown_and_oversized_documents() {
        let path = env::temp_dir().join(format!(
            "cocobeat-settings-invalid-{}-{}.json",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let valid = serde_json::json!({
            "version": VERSION,
            "display": {"fullscreen": false, "window_size": [1280, 800], "fullscreen_size": [1280, 800]}
        });
        let mut invalid_documents = vec![b"{".to_vec(), vec![b' '; MAX_FILE_BYTES as usize + 1]];
        for (pointer, value) in [
            ("/version", serde_json::json!(2)),
            ("/display/window_size", serde_json::json!([0, 800])),
            ("/display/fullscreen_size", serde_json::json!([1280, 16385])),
            ("/display/window_size", serde_json::json!([1280])),
            ("/display/fullscreen", serde_json::json!("false")),
        ] {
            let mut document = valid.clone();
            *document.pointer_mut(pointer).unwrap() = value;
            invalid_documents.push(serde_json::to_vec(&document).unwrap());
        }
        for pointer in ["", "/display"] {
            let mut document = valid.clone();
            document.pointer_mut(pointer).unwrap()["unknown"] = serde_json::json!(true);
            invalid_documents.push(serde_json::to_vec(&document).unwrap());
        }
        for bytes in invalid_documents {
            fs::write(&path, bytes).unwrap();
            assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        }
        fs::remove_file(path).unwrap();
    }
}
