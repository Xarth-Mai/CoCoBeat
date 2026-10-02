use crate::{
    display::DisplayState,
    i18n::{Locale, Message},
    input::SettingsAction,
    settings::{self, AntiAliasing, FrameLimit, QualityPreset, RainAmount, Settings},
};
use bevy::prelude::Resource;
use std::{io, path::PathBuf};

const PREVIEW_SECONDS: f64 = 15.0;
const ROWS: usize = 8;

#[derive(Clone, Copy)]
enum Page {
    Quality,
    Pacing,
}

struct Preview {
    original: Settings,
    deadline: f64,
}

#[derive(Resource, Default)]
pub(crate) struct SettingsMenu {
    pub values: Settings,
    pub notice: Message,
    path: Option<PathBuf>,
    draft: Option<Settings>,
    selection: usize,
    resolution_selection: Option<usize>,
    language_selection: Option<usize>,
    page: Option<Page>,
    preview: Option<Preview>,
}

impl SettingsMenu {
    pub fn load() -> Self {
        match settings::default_path() {
            Ok(path) => Self::from_path(path),
            Err(error) => {
                eprintln!("Settings unavailable: {error}");
                Self {
                    notice: Message::new("settings_notice.unavailable"),
                    ..Self::default()
                }
            }
        }
    }

    fn from_path(path: PathBuf) -> Self {
        let (values, notice) = match settings::load(&path) {
            Ok(values) => (values, Message::default()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                (Settings::default(), Message::default())
            }
            Err(error) => {
                eprintln!("Could not read settings at {}: {error}", path.display());
                (
                    Settings::default(),
                    Message::new("settings_notice.read_failed"),
                )
            }
        };
        Self {
            values,
            notice,
            path: Some(path),
            ..Self::default()
        }
    }

    pub fn begin(&mut self, display: &DisplayState) {
        let mut draft = self.values.clone();
        draft.display = display.actual();
        self.draft = Some(draft);
        self.selection = 0;
        self.resolution_selection = None;
        self.language_selection = None;
        self.preview = None;
        self.page = None;
    }

    pub fn sync_pacing(&mut self, display: &DisplayState) {
        display.normalize_pacing(&mut self.values.pacing);
        if let Some(draft) = &mut self.draft {
            display.normalize_pacing(&mut draft.pacing);
        }
        if let Some(preview) = &mut self.preview {
            display.normalize_pacing(&mut preview.original.pacing);
        }
    }

    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    pub fn language_choices(&self) -> Option<usize> {
        self.language_selection
    }

    pub fn language_row(&self) -> Option<(Locale, bool)> {
        if self.resolution_selection.is_some()
            || self.language_selection.is_some()
            || self.preview.is_some()
            || self.page.is_some()
        {
            return None;
        }
        self.draft
            .as_ref()
            .map(|draft| (draft.locale, self.selection == 7))
    }

    fn rollback(&mut self, display: &mut DisplayState, notice: &'static str) {
        if let Some(preview) = self.preview.take() {
            display.request(preview.original.display);
            self.draft = Some(preview.original);
        }
        self.notice = Message::new(notice);
    }

    pub fn tick(&mut self, now: f64, display: &mut DisplayState) {
        if self.preview.as_ref().is_some_and(|p| now >= p.deadline) {
            self.rollback(display, "settings_notice.preview_expired");
        }
    }

    fn save_actual(&mut self, mut settings: Settings, display: &DisplayState) -> bool {
        settings.display = display.actual();
        display.normalize_pacing(&mut settings.pacing);
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
                self.notice = Message::new("settings_notice.saved");
                true
            }
            Err(error) => {
                eprintln!("Settings were not saved: {error}");
                self.notice = Message::new("settings_notice.save_failed");
                false
            }
        }
    }

    /// Returns true only when the settings menu closes, never to resume a song
    pub fn handle(&mut self, action: SettingsAction, now: f64, display: &mut DisplayState) -> bool {
        self.tick(now, display);
        let Some(mut draft) = self.draft.clone() else {
            return false;
        };
        if let Some(selected) = self.language_selection {
            match action {
                SettingsAction::Up | SettingsAction::Previous => {
                    self.language_selection =
                        Some((selected + Locale::ALL.len() - 1) % Locale::ALL.len());
                }
                SettingsAction::Down | SettingsAction::Next => {
                    self.language_selection = Some((selected + 1) % Locale::ALL.len());
                }
                SettingsAction::Confirm => {
                    draft.locale = Locale::ALL[selected];
                    self.draft = Some(draft);
                    self.language_selection = None;
                }
                SettingsAction::Back => self.language_selection = None,
                SettingsAction::Open => {}
            }
            return false;
        }
        if let Some(selected) = self.resolution_selection {
            let options = display.resolution_options(draft.display.fullscreen);
            if options.is_empty() || (!draft.display.fullscreen && display.window_managed) {
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
                    if draft.display.fullscreen {
                        draft.display.fullscreen_size = options[selected];
                    } else {
                        draft.display.window_size = options[selected];
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
                SettingsAction::Confirm if !display.pending => {
                    return self.save_actual(draft, display);
                }
                SettingsAction::Back => self.rollback(display, "settings_notice.display_cancelled"),
                _ => {}
            }
            return false;
        }
        if let Some(page) = self.page {
            let rows = match page {
                Page::Quality => 7,
                Page::Pacing => 3,
            };
            match action {
                SettingsAction::Back | SettingsAction::Confirm
                    if action == SettingsAction::Back || self.selection == rows - 1 =>
                {
                    self.page = None;
                    self.selection = match page {
                        Page::Quality => 5,
                        Page::Pacing => 6,
                    };
                }
                SettingsAction::Up => self.selection = (self.selection + rows - 1) % rows,
                SettingsAction::Down => self.selection = (self.selection + 1) % rows,
                SettingsAction::Previous | SettingsAction::Next | SettingsAction::Confirm => {
                    let previous = action == SettingsAction::Previous;
                    match page {
                        Page::Quality => {
                            match self.selection {
                                0 => {
                                    let preset = cycle(
                                        draft.quality.preset,
                                        &[
                                            QualityPreset::Low,
                                            QualityPreset::Medium,
                                            QualityPreset::High,
                                        ],
                                        previous,
                                    );
                                    draft.quality.set_preset(preset);
                                }
                                1 => {
                                    draft.quality.antialiasing = cycle(
                                        draft.quality.antialiasing,
                                        &[
                                            AntiAliasing::Off,
                                            AntiAliasing::Msaa2,
                                            AntiAliasing::Msaa4,
                                        ],
                                        previous,
                                    )
                                }
                                2 => {
                                    draft.quality.rain = cycle(
                                        draft.quality.rain,
                                        &[
                                            RainAmount::Off,
                                            RainAmount::Quarter,
                                            RainAmount::Half,
                                            RainAmount::Full,
                                        ],
                                        previous,
                                    )
                                }
                                3 => draft.quality.fog = !draft.quality.fog,
                                4 => draft.quality.shadows = !draft.quality.shadows,
                                5 => draft.quality.bloom = !draft.quality.bloom,
                                _ => {}
                            }
                            if (1..=5).contains(&self.selection) {
                                draft.quality.preset = QualityPreset::Custom;
                            }
                        }
                        Page::Pacing => match self.selection {
                            0 => {
                                draft.pacing.frame_limit = cycle(
                                    draft.pacing.frame_limit,
                                    &display.frame_rates(),
                                    previous,
                                )
                            }
                            1 => draft.pacing.vsync = !draft.pacing.vsync,
                            _ => {}
                        },
                    }
                }
                SettingsAction::Open | SettingsAction::Back => {}
            }
            self.draft = Some(draft);
            return false;
        }
        match action {
            SettingsAction::Back => {
                self.draft = None;
                self.notice = Message::new("settings_notice.cancelled");
                return true;
            }
            SettingsAction::Up => self.selection = (self.selection + ROWS - 1) % ROWS,
            SettingsAction::Down => self.selection = (self.selection + 1) % ROWS,
            SettingsAction::Previous | SettingsAction::Next | SettingsAction::Confirm => match self
                .selection
            {
                0 if draft.display.fullscreen || !display.window_managed => {
                    let options = display.resolution_options(draft.display.fullscreen);
                    let current = if draft.display.fullscreen {
                        &mut draft.display.fullscreen_size
                    } else {
                        &mut draft.display.window_size
                    };
                    if !options.is_empty() {
                        let index = options.iter().position(|size| size == current);
                        if action == SettingsAction::Confirm {
                            self.resolution_selection = Some(index.unwrap_or(0));
                            return false;
                        }
                        *current = cycle(*current, &options, action == SettingsAction::Previous);
                    }
                }
                1 => draft.display.fullscreen = !draft.display.fullscreen,
                2 if action == SettingsAction::Confirm && !display.pending => {
                    if draft.display == display.actual() {
                        return self.save_actual(draft, display);
                    }
                    self.preview = Some(Preview {
                        original: Settings {
                            display: display.actual(),
                            ..self.values.clone()
                        },
                        deadline: now + PREVIEW_SECONDS,
                    });
                    display.request(draft.display);
                    self.notice = Message::new("settings_notice.preview");
                }
                3 if action == SettingsAction::Confirm => {
                    self.draft = None;
                    self.notice = Message::new("settings_notice.cancelled");
                    return true;
                }
                4 if action == SettingsAction::Confirm => {
                    draft = Settings::default();
                    display.normalize_pacing(&mut draft.pacing);
                    self.notice = Message::new("settings_notice.defaults");
                }
                5 | 6 if action == SettingsAction::Confirm => {
                    self.page = Some(if self.selection == 5 {
                        Page::Quality
                    } else {
                        Page::Pacing
                    });
                    self.selection = 0;
                }
                7 => {
                    let selected = Locale::ALL
                        .iter()
                        .position(|locale| *locale == draft.locale)
                        .unwrap();
                    if action == SettingsAction::Confirm {
                        self.language_selection = Some(selected);
                    } else {
                        draft.locale = cycle(
                            draft.locale,
                            &Locale::ALL,
                            action == SettingsAction::Previous,
                        );
                    }
                }
                _ => {}
            },
            SettingsAction::Open => {}
        }
        self.draft = Some(draft);
        false
    }

    pub fn text(&self, now: f64, display: &DisplayState, locale: Locale) -> String {
        let Some(draft) = self.draft.as_ref() else {
            return self.notice.render(locale);
        };
        if self.language_selection.is_some() {
            return locale.text("settings.language_title").into();
        }
        if let Some(selected) = self.resolution_selection {
            let rows = display
                .resolution_options(draft.display.fullscreen)
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
                "{}\n{rows}\n{}\n{}",
                locale.text("settings.resolution_title"),
                locale.text("settings.choice_controls"),
                locale.text("settings.draft_hint")
            );
        }
        if let Some(preview) = &self.preview {
            return format!(
                "{}\n{}\n{}\n{}\n{}\n{}",
                Message::with(
                    "settings.keep_title",
                    [(
                        "seconds",
                        ((preview.deadline - now).max(0.0).ceil() as u32).to_string()
                    )]
                )
                .render(locale),
                observed(display, locale),
                locale.text(if display.pending {
                    "settings.waiting"
                } else {
                    "settings.confirm_actual"
                }),
                locale.text("settings.keep_controls"),
                self.notice.render(locale),
                display.notice.render(locale),
            );
        }
        if let Some(page) = self.page {
            let on_off = |enabled| {
                locale.text(if enabled {
                    "settings.on"
                } else {
                    "settings.off"
                })
            };
            let value_row =
                |key, value: &str| Message::with(key, [("value", value.into())]).render(locale);
            let (title, rows) = match page {
                Page::Quality => (
                    "settings.quality_title",
                    vec![
                        value_row(
                            "settings.preset",
                            locale.text(match draft.quality.preset {
                                QualityPreset::Low => "settings.preset_low",
                                QualityPreset::Medium => "settings.preset_medium",
                                QualityPreset::High => "settings.preset_high",
                                QualityPreset::Custom => "settings.preset_custom",
                            }),
                        ),
                        value_row(
                            "settings.antialiasing",
                            match draft.quality.antialiasing {
                                AntiAliasing::Off => locale.text("settings.off"),
                                AntiAliasing::Msaa2 => "MSAA 2×",
                                AntiAliasing::Msaa4 => "MSAA 4×",
                            },
                        ),
                        value_row(
                            "settings.rain",
                            match draft.quality.rain {
                                RainAmount::Off => locale.text("settings.off"),
                                RainAmount::Quarter => "25%",
                                RainAmount::Half => "50%",
                                RainAmount::Full => "100%",
                            },
                        ),
                        value_row("settings.fog", on_off(draft.quality.fog)),
                        value_row("settings.shadows", on_off(draft.quality.shadows)),
                        value_row("settings.bloom", on_off(draft.quality.bloom)),
                        locale.text("settings.back").into(),
                    ],
                ),
                Page::Pacing => {
                    let limit = if draft.pacing.frame_limit == FrameLimit::Display {
                        display
                            .frame_rates()
                            .into_iter()
                            .rev()
                            .find(|rate| matches!(rate, FrameLimit::Limited(_)))
                            .unwrap()
                    } else {
                        draft.pacing.frame_limit
                    };
                    let value = match limit {
                        FrameLimit::Limited(rate) => {
                            let value = format!("{:.3}", f64::from(rate) / 1000.0);
                            value_row(
                                "settings.frame_rate",
                                value.trim_end_matches('0').trim_end_matches('.'),
                            )
                        }
                        FrameLimit::Unlimited => locale.text("settings.unlimited").into(),
                        FrameLimit::Display => unreachable!(),
                    };
                    (
                        "settings.pacing_title",
                        vec![
                            value_row("settings.frame_limit", &value),
                            value_row("settings.vsync", on_off(draft.pacing.vsync)),
                            locale.text("settings.back").into(),
                        ],
                    )
                }
            };
            return format!(
                "{}\n{}",
                locale.text(title),
                rows_text(&rows, self.selection)
            );
        }
        let size = if draft.display.fullscreen {
            draft.display.fullscreen_size
        } else {
            draft.display.window_size
        };
        let custom = !display
            .resolution_options(draft.display.fullscreen)
            .contains(&size);
        let rows = [
            Message::with(
                "settings.resolution",
                [
                    ("width", size[0].to_string()),
                    ("height", size[1].to_string()),
                    (
                        "custom",
                        if custom {
                            locale.text("settings.custom")
                        } else {
                            ""
                        }
                        .into(),
                    ),
                    (
                        "managed",
                        if !draft.display.fullscreen && display.window_managed {
                            locale.text("settings.managed")
                        } else {
                            ""
                        }
                        .into(),
                    ),
                ],
            )
            .render(locale),
            Message::with(
                "settings.fullscreen",
                [(
                    "mark",
                    if draft.display.fullscreen { "x" } else { " " }.into(),
                )],
            )
            .render(locale),
            locale.text("settings.apply").into(),
            locale.text("settings.cancel").into(),
            locale.text("settings.restore_defaults").into(),
            locale.text("settings.quality").into(),
            locale.text("settings.pacing").into(),
        ];
        let rows = rows_text(&rows, self.selection);
        format!("{}\n{rows}", locale.text("settings.title"))
    }

    pub fn footer(&self, display: &DisplayState, locale: Locale) -> String {
        if self.draft.is_none() || self.resolution_selection.is_some() || self.preview.is_some() {
            return String::new();
        }
        if self.language_selection.is_some() || self.page.is_some() {
            return format!(
                "{}\n{}",
                locale.text(if self.page.is_some() {
                    "settings.page_controls"
                } else {
                    "settings.choice_controls"
                }),
                locale.text("settings.draft_hint")
            );
        }
        format!(
            "{}\n{}\n{}\n{}",
            locale.text("settings.controls"),
            observed(display, locale),
            self.notice.render(locale),
            display.notice.render(locale)
        )
    }
}

fn cycle<T: Copy + PartialEq>(current: T, options: &[T], previous: bool) -> T {
    let index = options.iter().position(|option| *option == current);
    let next = if previous {
        index.map_or(options.len() - 1, |index| {
            (index + options.len() - 1) % options.len()
        })
    } else {
        index.map_or(0, |index| (index + 1) % options.len())
    };
    options[next]
}

fn rows_text(rows: &[String], selection: usize) -> String {
    rows.iter()
        .enumerate()
        .map(|(index, row)| format!("{} {row}", if index == selection { ">" } else { " " }))
        .collect::<Vec<_>>()
        .join("\n")
}

fn observed(display: &DisplayState, locale: Locale) -> String {
    let actual = display.actual();
    let size = if actual.fullscreen {
        actual.fullscreen_size
    } else {
        actual.window_size
    };
    Message::with(
        "settings.observed",
        [
            ("width", size[0].to_string()),
            ("height", size[1].to_string()),
            (
                "mode",
                locale
                    .text(if actual.fullscreen {
                        "settings.mode_fullscreen"
                    } else {
                        "settings.mode_windowed"
                    })
                    .into(),
            ),
            ("observed_width", display.physical_size[0].to_string()),
            ("observed_height", display.physical_size[1].to_string()),
        ],
    )
    .render(locale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::DisplaySettings;

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
        assert!(
            menu.text(0.0, &display, Locale::EnUs)
                .starts_with("RESOLUTION\n")
        );
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.handle(SettingsAction::Back, 0.0, &mut display);
        assert_eq!(menu.draft.as_ref().unwrap().display, original);
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
        assert_eq!(menu.notice.key, "settings_notice.save_failed");
        assert_eq!(std::fs::read(&path).unwrap(), persisted);
        menu.tick(41.0, &mut display);
        assert!(display.actual().fullscreen);
        std::fs::write(&path, b"corrupt").unwrap();
        let recovered = SettingsMenu::from_path(path);
        assert_eq!(recovered.values, Settings::default());
        assert_eq!(recovered.notice.key, "settings_notice.read_failed");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn language_choices_and_display_preview_commit_as_one_transaction() {
        let root =
            std::env::temp_dir().join(format!("cocobeat-settings-language-{}", std::process::id()));
        let path = root.join("settings.json");
        settings::save(
            &path,
            &Settings {
                locale: Locale::EnUs,
                ..Settings::default()
            },
        )
        .unwrap();
        let mut menu = SettingsMenu::from_path(path.clone());
        let mut display = DisplayState::new(menu.values.display);
        display.set_headless_surface([1920, 1080]);
        menu.begin(&display);
        menu.handle(SettingsAction::Up, 0.0, &mut display);
        assert_eq!(menu.language_row(), Some((Locale::EnUs, true)));
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        assert_eq!(menu.language_choices(), Some(1));
        assert!(menu.language_row().is_none());
        assert_eq!(menu.text(0.0, &display, Locale::EnUs), "LANGUAGE");
        assert!(
            menu.footer(&display, Locale::EnUs)
                .contains("draft until Apply")
        );
        menu.handle(SettingsAction::Previous, 0.0, &mut display);
        menu.handle(SettingsAction::Previous, 0.0, &mut display);
        assert_eq!(menu.language_choices(), Some(12));
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        assert_eq!(menu.language_choices(), Some(0));
        menu.handle(SettingsAction::Back, 0.0, &mut display);
        assert_eq!(menu.draft.as_ref().unwrap().locale, Locale::EnUs);
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        assert_eq!(menu.draft.as_ref().unwrap().locale, Locale::EnGb);
        assert_eq!(menu.values.locale, Locale::EnUs);
        menu.selection = 2;
        assert!(menu.handle(SettingsAction::Confirm, 1.0, &mut display));
        assert!(menu.preview.is_none());
        assert_eq!(menu.values.locale, Locale::EnGb);
        assert_eq!(settings::load(&path).unwrap(), menu.values);
        let persisted = std::fs::read(&path).unwrap();

        for timeout in [false, true] {
            menu.begin(&display);
            menu.selection = 7;
            menu.handle(SettingsAction::Next, 2.0, &mut display);
            menu.selection = 1;
            menu.handle(SettingsAction::Confirm, 2.0, &mut display);
            menu.selection = 2;
            assert!(!menu.handle(SettingsAction::Confirm, 3.0, &mut display));
            assert_eq!(menu.values.locale, Locale::EnGb);
            assert_eq!(menu.draft.as_ref().unwrap().locale, Locale::Ja);
            assert!(display.actual().fullscreen);
            assert_eq!(std::fs::read(&path).unwrap(), persisted);
            assert!(menu.language_row().is_none());
            assert!(menu.footer(&display, Locale::EnUs).is_empty());
            if timeout {
                menu.tick(18.0, &mut display);
            } else {
                assert!(!menu.handle(SettingsAction::Back, 4.0, &mut display));
            }
            assert_eq!(menu.draft.as_ref().unwrap(), &menu.values);
            assert!(!display.actual().fullscreen);
            assert_eq!(std::fs::read(&path).unwrap(), persisted);
        }

        menu.selection = 7;
        menu.handle(SettingsAction::Next, 20.0, &mut display);
        menu.selection = 1;
        menu.handle(SettingsAction::Confirm, 20.0, &mut display);
        menu.selection = 2;
        assert!(!menu.handle(SettingsAction::Confirm, 21.0, &mut display));
        assert!(menu.handle(SettingsAction::Confirm, 22.0, &mut display));
        assert_eq!(menu.values.locale, Locale::Ja);
        assert!(menu.values.display.fullscreen);
        assert_eq!(settings::load(&path).unwrap(), menu.values);
        let persisted = std::fs::read(&path).unwrap();

        for mixed in [false, true] {
            menu.begin(&display);
            menu.selection = 7;
            menu.handle(SettingsAction::Next, 23.0, &mut display);
            if mixed {
                menu.selection = 1;
                menu.handle(SettingsAction::Confirm, 23.0, &mut display);
            }
            menu.path = Some(root.clone());
            menu.selection = 2;
            assert!(!menu.handle(SettingsAction::Confirm, 24.0, &mut display));
            if mixed {
                assert!(!menu.handle(SettingsAction::Confirm, 25.0, &mut display));
            }
            assert_eq!(menu.notice.key, "settings_notice.save_failed");
            assert_eq!(menu.values.locale, Locale::Ja);
            assert_eq!(menu.draft.as_ref().unwrap().locale, Locale::Ko);
            assert_eq!(std::fs::read(&path).unwrap(), persisted);
            if mixed {
                menu.tick(39.0, &mut display);
                assert_eq!(menu.draft.as_ref().unwrap(), &menu.values);
                assert!(display.actual().fullscreen);
            }
            assert!(menu.handle(SettingsAction::Back, 40.0, &mut display));
            assert_eq!(menu.values.locale, Locale::Ja);
        }
        menu.begin(&display);
        menu.selection = 4;
        menu.handle(SettingsAction::Confirm, 41.0, &mut display);
        let mut defaults = Settings::default();
        display.normalize_pacing(&mut defaults.pacing);
        assert_eq!(menu.draft, Some(defaults));
        assert_eq!(menu.values.locale, Locale::Ja);
        assert_eq!(std::fs::read(&path).unwrap(), persisted);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quality_pacing_and_display_share_the_saved_transaction() {
        let root =
            std::env::temp_dir().join(format!("cocobeat-settings-quality-{}", std::process::id()));
        let path = root.join("settings.json");
        let mut menu = SettingsMenu::from_path(path.clone());
        let mut display = DisplayState::new(menu.values.display);
        display.set_headless_surface([1920, 1080]);
        menu.sync_pacing(&display);
        let original = menu.values.clone();
        menu.begin(&display);
        menu.selection = 6;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        assert!(menu.language_row().is_none());
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.handle(SettingsAction::Down, 0.0, &mut display);
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        assert_eq!(
            menu.draft.as_ref().unwrap().pacing.frame_limit,
            FrameLimit::Unlimited
        );
        assert!(menu.draft.as_ref().unwrap().pacing.vsync);
        menu.handle(SettingsAction::Back, 0.0, &mut display);
        assert_eq!(menu.selection, 6);
        menu.draft.as_mut().unwrap().locale = Locale::Ja;
        menu.selection = 5;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        for (action, preset, aa, rain, effects) in [
            (
                SettingsAction::Next,
                QualityPreset::High,
                AntiAliasing::Msaa4,
                RainAmount::Full,
                true,
            ),
            (
                SettingsAction::Next,
                QualityPreset::Low,
                AntiAliasing::Off,
                RainAmount::Quarter,
                false,
            ),
            (
                SettingsAction::Next,
                QualityPreset::Medium,
                AntiAliasing::Msaa2,
                RainAmount::Half,
                true,
            ),
            (
                SettingsAction::Previous,
                QualityPreset::Low,
                AntiAliasing::Off,
                RainAmount::Quarter,
                false,
            ),
        ] {
            menu.handle(action, 0.0, &mut display);
            let draft = menu.draft.as_ref().unwrap();
            assert_eq!(
                draft.quality,
                settings::QualitySettings {
                    preset,
                    antialiasing: aa,
                    rain,
                    fog: true,
                    shadows: effects,
                    bloom: effects,
                }
            );
            assert_eq!(draft.pacing.frame_limit, FrameLimit::Unlimited);
            assert!(draft.pacing.vsync);
            assert_eq!(draft.locale, Locale::Ja);
            assert_eq!(draft.display, original.display);
        }
        menu.handle(SettingsAction::Down, 0.0, &mut display);
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.handle(SettingsAction::Previous, 0.0, &mut display);
        assert_eq!(
            menu.draft.as_ref().unwrap().quality.preset,
            QualityPreset::Custom
        );
        assert_eq!(
            menu.draft.as_ref().unwrap().quality.antialiasing,
            AntiAliasing::Off
        );
        assert_eq!(menu.values, original);
        menu.handle(SettingsAction::Back, 0.0, &mut display);
        assert_eq!(menu.selection, 5);
        menu.selection = 2;
        assert!(menu.handle(SettingsAction::Confirm, 0.0, &mut display));
        assert_eq!(settings::load(&path).unwrap(), menu.values);
        let saved = menu.values.clone();
        let bytes = std::fs::read(&path).unwrap();

        for fail_save in [false, true] {
            menu.begin(&display);
            let draft = menu.draft.as_mut().unwrap();
            draft.display.fullscreen = true;
            draft.quality.set_preset(QualityPreset::High);
            draft.pacing.frame_limit = FrameLimit::Limited(60000);
            draft.pacing.vsync = false;
            menu.selection = 2;
            assert!(!menu.handle(SettingsAction::Confirm, 1.0, &mut display));
            if fail_save {
                menu.path = Some(root.clone());
                assert!(!menu.handle(SettingsAction::Confirm, 2.0, &mut display));
                assert_eq!(menu.notice.key, "settings_notice.save_failed");
            }
            assert_eq!(menu.values, saved);
            menu.tick(16.0, &mut display);
            assert_eq!(menu.draft.as_ref().unwrap(), &saved);
            assert_eq!(display.actual(), saved.display);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        menu.handle(SettingsAction::Back, 17.0, &mut display);
        menu.begin(&display);
        menu.selection = 4;
        menu.handle(SettingsAction::Confirm, 18.0, &mut display);
        assert_eq!(
            menu.draft.as_ref().unwrap().quality,
            settings::QualitySettings::default()
        );
        assert!(!menu.draft.as_ref().unwrap().pacing.vsync);
        assert_eq!(menu.values, saved);
        assert!(menu.handle(SettingsAction::Back, 19.0, &mut display));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }
}
