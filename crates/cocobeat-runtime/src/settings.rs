use crate::i18n::Locale;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QualityPreset {
    Low,
    Medium,
    High,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AntiAliasing {
    Off,
    Msaa2,
    Msaa4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RainAmount {
    Off,
    Quarter,
    Half,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QualitySettings {
    pub preset: QualityPreset,
    pub antialiasing: AntiAliasing,
    pub rain: RainAmount,
    pub fog: bool,
    pub shadows: bool,
    pub bloom: bool,
}

impl Default for QualitySettings {
    fn default() -> Self {
        Self {
            preset: QualityPreset::Medium,
            antialiasing: AntiAliasing::Msaa2,
            rain: RainAmount::Half,
            fog: true,
            shadows: true,
            bloom: true,
        }
    }
}

impl QualitySettings {
    pub fn set_preset(&mut self, preset: QualityPreset) {
        let (antialiasing, rain, effects) = match preset {
            QualityPreset::Low => (AntiAliasing::Off, RainAmount::Quarter, false),
            QualityPreset::Medium => (AntiAliasing::Msaa2, RainAmount::Half, true),
            QualityPreset::High => (AntiAliasing::Msaa4, RainAmount::Full, true),
            QualityPreset::Custom => {
                self.preset = preset;
                return;
            }
        };
        *self = Self {
            preset,
            antialiasing,
            rain,
            fog: true,
            shadows: effects,
            bloom: effects,
        };
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FrameLimit {
    /// Resolved to the monitor limit after the first display observation
    #[default]
    Display,
    Limited(u32),
    Unlimited,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PacingSettings {
    pub frame_limit: FrameLimit,
    pub vsync: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    pub display: DisplaySettings,
    pub locale: Locale,
    pub quality: QualitySettings,
    pub pacing: PacingSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            display: DisplaySettings::default(),
            locale: Locale::system_default(),
            quality: QualitySettings::default(),
            pacing: PacingSettings::default(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    display: DisplaySettings,
    #[serde(default = "Locale::system_default")]
    locale: Locale,
    #[serde(default)]
    quality: QualitySettings,
    #[serde(default)]
    pacing: PacingSettings,
}

fn invalid(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn validate(display: DisplaySettings, pacing: PacingSettings) -> io::Result<()> {
    if display
        .window_size
        .into_iter()
        .chain(display.fullscreen_size)
        .any(|size| !(1..=16384).contains(&size))
    {
        return Err(invalid("Display dimensions must be between 1 and 16384"));
    }
    if matches!(pacing.frame_limit, FrameLimit::Limited(rate) if rate < 1000) {
        return Err(invalid(
            "Frame limit must be at least 1000 millihertz (1 FPS)",
        ));
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
    let mut document: Document = serde_json::from_slice(&bytes).map_err(invalid)?;
    if document.version != VERSION {
        return Err(invalid("Unsupported settings version"));
    }
    validate(document.display, document.pacing)?;
    if document.quality.preset != QualityPreset::Custom {
        let mut preset = document.quality;
        preset.set_preset(preset.preset);
        if document.quality != preset {
            document.quality.preset = QualityPreset::Custom;
        }
    }
    Ok(Settings {
        display: document.display,
        locale: document.locale,
        quality: document.quality,
        pacing: document.pacing,
    })
}

/// Syncs a same-directory temporary file before replacing the destination
/// The old file is never deleted first; directory-entry power-loss durability
/// remains subject to the filesystem, as with Replay saves
pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    validate(settings.display, settings.pacing)?;
    let bytes = serde_json::to_vec_pretty(&Document {
        version: VERSION,
        display: settings.display,
        locale: settings.locale,
        quality: settings.quality,
        pacing: settings.pacing,
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
        settings.locale = Locale::Ja;
        settings.quality.set_preset(QualityPreset::Low);
        settings.pacing = PacingSettings {
            frame_limit: FrameLimit::Limited(59940),
            vsync: true,
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(document["locale"], "ja");
        document["quality"]["antialiasing"] = serde_json::json!("msaa4");
        document["quality"]["shadows"] = serde_json::json!(true);
        document["quality"]["bloom"] = serde_json::json!(true);
        fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        settings.quality.antialiasing = AntiAliasing::Msaa4;
        settings.quality.shadows = true;
        settings.quality.bloom = true;
        settings.quality.preset = QualityPreset::Custom;
        assert_eq!(load(&path).unwrap(), settings);

        let previous = fs::read(&path).unwrap();
        settings.display.window_size = [0, 900];
        assert!(save(&path, &settings).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);

        settings.display.window_size = [1600, 900];
        settings.pacing.frame_limit = FrameLimit::Limited(999);
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
            "display": {"fullscreen": false, "window_size": [1280, 800], "fullscreen_size": [1280, 800]},
            "locale": "en-US",
            "quality": QualitySettings::default(),
            "pacing": PacingSettings { frame_limit: FrameLimit::Limited(60000), vsync: false }
        });
        let mut invalid_documents = vec![b"{".to_vec(), vec![b' '; MAX_FILE_BYTES as usize + 1]];
        for (pointer, value) in [
            ("/version", serde_json::json!(2)),
            ("/display/window_size", serde_json::json!([0, 800])),
            ("/display/fullscreen_size", serde_json::json!([1280, 16385])),
            ("/display/window_size", serde_json::json!([1280])),
            ("/display/fullscreen", serde_json::json!("false")),
            ("/locale", serde_json::json!("unsupported")),
            ("/locale", serde_json::json!(null)),
            ("/quality/preset", serde_json::json!("ultra")),
            ("/quality/antialiasing", serde_json::json!("msaa8")),
            ("/quality/rain", serde_json::json!("double")),
            ("/quality/fog", serde_json::json!("true")),
            ("/pacing/frame_limit", serde_json::json!({"limited": 0})),
            ("/pacing/frame_limit", serde_json::json!({"limited": 999})),
            ("/pacing/frame_limit", serde_json::json!("fast")),
            ("/pacing/vsync", serde_json::json!(null)),
        ] {
            let mut document = valid.clone();
            *document.pointer_mut(pointer).unwrap() = value;
            invalid_documents.push(serde_json::to_vec(&document).unwrap());
        }
        for pointer in ["", "/display", "/quality", "/pacing"] {
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

    #[test]
    fn old_v1_keeps_display_and_uses_system_language() {
        let path = env::temp_dir().join(format!(
            "cocobeat-settings-legacy-{}-{}.json",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(
            &path,
            br#"{"version":1,"display":{"fullscreen":true,"window_size":[1600,900],"fullscreen_size":[1920,1080]}}"#,
        )
        .unwrap();
        assert_eq!(
            load(&path).unwrap(),
            Settings {
                display: DisplaySettings {
                    fullscreen: true,
                    window_size: [1600, 900],
                    fullscreen_size: [1920, 1080],
                },
                locale: Locale::system_default(),
                quality: QualitySettings::default(),
                pacing: PacingSettings::default(),
            }
        );
        fs::remove_file(path).unwrap();
    }
}
