use crate::{
    display::DisplayState,
    input::SettingsAction,
    settings::{self, DisplaySettings, Settings},
};
use bevy::prelude::Resource;
use std::{io, path::PathBuf};

const PREVIEW_SECONDS: f64 = 15.0;
const ROWS: usize = 5;

struct Preview {
    original: DisplaySettings,
    deadline: f64,
}

#[derive(Resource, Default)]
pub(crate) struct SettingsMenu {
    pub values: Settings,
    pub notice: String,
    path: Option<PathBuf>,
    draft: Option<DisplaySettings>,
    selection: usize,
    resolution_selection: Option<usize>,
    preview: Option<Preview>,
}

impl SettingsMenu {
    pub fn load() -> Self {
        match settings::default_path() {
            Ok(path) => Self::from_path(path),
            Err(error) => Self {
                notice: format!("Settings unavailable: {error}"),
                ..Self::default()
            },
        }
    }

    fn from_path(path: PathBuf) -> Self {
        let (values, notice) = match settings::load(&path) {
            Ok(values) => (values, String::new()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                (Settings::default(), String::new())
            }
            Err(error) => (
                Settings::default(),
                format!("Could not read settings; using defaults: {error}"),
            ),
        };
        Self {
            values,
            notice,
            path: Some(path),
            ..Self::default()
        }
    }

    pub fn begin(&mut self, display: &DisplayState) {
        self.draft = Some(display.actual());
        self.selection = 0;
        self.resolution_selection = None;
        self.preview = None;
    }

    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    fn rollback(&mut self, display: &mut DisplayState, notice: &str) {
        if let Some(preview) = self.preview.take() {
            display.request(preview.original);
            self.draft = Some(preview.original);
        }
        self.notice = notice.into();
    }

    pub fn tick(&mut self, now: f64, display: &mut DisplayState) {
        if self.preview.as_ref().is_some_and(|p| now >= p.deadline) {
            self.rollback(
                display,
                "Display preview expired; restoring previous settings",
            );
        }
    }

    fn save_actual(&mut self, display: &DisplayState) -> bool {
        let settings = Settings {
            display: display.actual(),
        };
        let saved = self
            .path
            .as_deref()
            .ok_or_else(|| io::Error::other("Configuration directory is unavailable"))
            .and_then(|path| settings::save(path, &settings));
        match saved {
            Ok(()) => {
                self.values = settings;
                self.preview = None;
                self.draft = None;
                self.notice = "Settings applied and saved".into();
                true
            }
            Err(error) => {
                self.notice = format!("Settings were not saved: {error}");
                false
            }
        }
    }

    /// Returns true only when the settings menu closes, never to resume a song
    pub fn handle(&mut self, action: SettingsAction, now: f64, display: &mut DisplayState) -> bool {
        self.tick(now, display);
        let Some(mut draft) = self.draft else {
            return false;
        };
        if let Some(selected) = self.resolution_selection {
            let options = display.resolution_options(draft.fullscreen);
            if options.is_empty() || (!draft.fullscreen && display.window_managed) {
                self.resolution_selection = None;
                return false;
            }
            let selected = selected.min(options.len() - 1);
            match action {
                SettingsAction::Up | SettingsAction::Previous => {
                    self.resolution_selection =
                        Some((selected + options.len() - 1) % options.len());
                }
                SettingsAction::Down | SettingsAction::Next => {
                    self.resolution_selection = Some((selected + 1) % options.len());
                }
                SettingsAction::Confirm => {
                    if draft.fullscreen {
                        draft.fullscreen_size = options[selected];
                    } else {
                        draft.window_size = options[selected];
                    }
                    self.draft = Some(draft);
                    self.resolution_selection = None;
                }
                SettingsAction::Back => self.resolution_selection = None,
                SettingsAction::Open => {}
            }
            return false;
        }
        if self.preview.is_some() {
            match action {
                SettingsAction::Confirm if !display.pending => return self.save_actual(display),
                SettingsAction::Back => self.rollback(display, "Display changes cancelled"),
                _ => {}
            }
            return false;
        }
        match action {
            SettingsAction::Back => {
                self.draft = None;
                self.notice = "Changes cancelled".into();
                return true;
            }
            SettingsAction::Up => self.selection = (self.selection + ROWS - 1) % ROWS,
            SettingsAction::Down => self.selection = (self.selection + 1) % ROWS,
            SettingsAction::Previous | SettingsAction::Next | SettingsAction::Confirm => {
                match self.selection {
                    0 if draft.fullscreen || !display.window_managed => {
                        let options = display.resolution_options(draft.fullscreen);
                        let current = if draft.fullscreen {
                            &mut draft.fullscreen_size
                        } else {
                            &mut draft.window_size
                        };
                        if !options.is_empty() {
                            let index = options.iter().position(|size| size == current);
                            if action == SettingsAction::Confirm {
                                self.resolution_selection = Some(index.unwrap_or(0));
                                return false;
                            }
                            let next = if action == SettingsAction::Previous {
                                index.map_or(options.len() - 1, |i| {
                                    (i + options.len() - 1) % options.len()
                                })
                            } else {
                                index.map_or(0, |i| (i + 1) % options.len())
                            };
                            *current = options[next];
                        }
                    }
                    1 => draft.fullscreen = !draft.fullscreen,
                    2 if action == SettingsAction::Confirm && !display.pending => {
                        if draft == display.actual() {
                            return self.save_actual(display);
                        }
                        self.preview = Some(Preview {
                            original: display.actual(),
                            deadline: now + PREVIEW_SECONDS,
                        });
                        display.request(draft);
                        self.notice = "Check the display, then confirm to save".into();
                    }
                    3 if action == SettingsAction::Confirm => {
                        self.draft = None;
                        self.notice = "Changes cancelled".into();
                        return true;
                    }
                    4 if action == SettingsAction::Confirm => {
                        draft = DisplaySettings::default();
                        self.notice = "Defaults are a draft; choose Apply to use them".into();
                    }
                    _ => {}
                }
            }
            SettingsAction::Open => {}
        }
        self.draft = Some(draft);
        false
    }

    pub fn text(&self, now: f64, display: &DisplayState) -> String {
        let Some(draft) = self.draft else {
            return self.notice.clone();
        };
        let actual = display.actual();
        if let Some(selected) = self.resolution_selection {
            let options = display.resolution_options(draft.fullscreen);
            let rows = options
                .iter()
                .enumerate()
                .map(|(i, size)| {
                    format!(
                        "{} {} x {}",
                        if i == selected { ">" } else { " " },
                        size[0],
                        size[1]
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return format!(
                "RESOLUTION\n{rows}\nUp/Down / D-pad: select    Enter / South: choose    Esc / East: back\nChoice remains a draft until Apply"
            );
        }
        let actual_size = if actual.fullscreen {
            actual.fullscreen_size
        } else {
            actual.window_size
        };
        let observed = format!(
            "Last confirmed: {} x {} {} | observed window {} x {}",
            actual_size[0],
            actual_size[1],
            if actual.fullscreen {
                "render / fullscreen"
            } else {
                "windowed"
            },
            display.physical_size[0],
            display.physical_size[1],
        );
        if let Some(preview) = &self.preview {
            return format!(
                "KEEP DISPLAY CHANGES?  {}s\n{observed}\n{}\nEnter / South: keep and save    Esc / East: restore\n{}\n{}",
                (preview.deadline - now).max(0.0).ceil() as u32,
                if display.pending {
                    "Waiting for window response; confirmation disabled"
                } else {
                    "Confirm the actual display shown above"
                },
                self.notice,
                display.notice,
            );
        }
        let size = if draft.fullscreen {
            draft.fullscreen_size
        } else {
            draft.window_size
        };
        let custom = !display.resolution_options(draft.fullscreen).contains(&size);
        let rows = [
            format!(
                "Resolution: {} x {}{}{}",
                size[0],
                size[1],
                if custom { " (custom)" } else { "" },
                if !draft.fullscreen && display.window_managed {
                    " (controlled by window manager)"
                } else {
                    ""
                }
            ),
            format!(
                "[{}] Borderless fullscreen",
                if draft.fullscreen { "x" } else { " " }
            ),
            "Apply".into(),
            "Cancel".into(),
            "Restore defaults".into(),
        ];
        let rows = rows
            .iter()
            .enumerate()
            .map(|(i, row)| format!("{} {row}", if i == self.selection { ">" } else { " " }))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "SETTINGS\n{rows}\nUp/Down: select   Left/Right / Enter / South: change   Esc / East: back\n{observed}\n{}\n{}",
            self.notice, display.notice
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_timeout_cancel_save_and_failure_preserve_settings() {
        let root =
            std::env::temp_dir().join(format!("cocobeat-settings-menu-{}", std::process::id()));
        let path = root.join("settings.json");
        let mut menu = SettingsMenu::from_path(path.clone());
        let mut display = DisplayState::new(DisplaySettings::default());
        display.set_headless_surface([1920, 1080]);
        let original = display.actual();
        menu.begin(&display);
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        assert!(menu.text(0.0, &display).starts_with("RESOLUTION\n"));
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.handle(SettingsAction::Back, 0.0, &mut display);
        assert_eq!(menu.draft, Some(original));
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        assert!(menu.resolution_selection.is_none());
        assert_eq!(display.actual(), original);
        menu.selection = 2;
        assert!(!menu.handle(SettingsAction::Confirm, 1.0, &mut display));
        assert_ne!(display.actual(), original);
        assert!(!path.exists());
        menu.tick(16.0, &mut display);
        assert_eq!(display.actual(), original);
        assert!(!path.exists());
        menu.selection = 1;
        menu.handle(SettingsAction::Confirm, 17.0, &mut display);
        menu.selection = 2;
        menu.handle(SettingsAction::Confirm, 18.0, &mut display);
        assert!(!menu.handle(SettingsAction::Back, 19.0, &mut display));
        assert_eq!(display.actual(), original);
        menu.selection = 1;
        menu.handle(SettingsAction::Confirm, 20.0, &mut display);
        menu.selection = 2;
        menu.handle(SettingsAction::Confirm, 21.0, &mut display);
        assert!(menu.handle(SettingsAction::Confirm, 22.0, &mut display));
        assert!(
            SettingsMenu::from_path(path.clone())
                .values
                .display
                .fullscreen
        );
        let persisted = std::fs::read(&path).unwrap();
        menu.begin(&display);
        menu.selection = 4;
        menu.handle(SettingsAction::Confirm, 23.0, &mut display);
        assert!(display.actual().fullscreen);
        assert_eq!(std::fs::read(&path).unwrap(), persisted);
        assert!(menu.handle(SettingsAction::Back, 24.0, &mut display));
        menu.begin(&display);
        menu.selection = 4;
        menu.handle(SettingsAction::Confirm, 25.0, &mut display);
        menu.selection = 2;
        menu.handle(SettingsAction::Confirm, 26.0, &mut display);
        menu.path = Some(root.clone());
        assert!(!menu.handle(SettingsAction::Confirm, 27.0, &mut display));
        assert!(menu.notice.contains("not saved"));
        assert_eq!(std::fs::read(&path).unwrap(), persisted);
        menu.tick(41.0, &mut display);
        assert!(display.actual().fullscreen);
        std::fs::write(&path, b"corrupt").unwrap();
        let recovered = SettingsMenu::from_path(path);
        assert_eq!(recovered.values, Settings::default());
        assert!(recovered.notice.contains("using defaults"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
