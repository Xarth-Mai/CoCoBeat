use crate::{
    display::DisplayState,
    feedback_audio::FeedbackTimbre,
    i18n::{Locale, Message},
    input::{MenuKind, MenuPresentation, MenuRow, MenuRowRole, SettingsAction},
    presentation::{MotionStyle, PresentationOverrides, PresentationVisual, WorldTheme},
    settings::{self, AntiAliasing, FrameLimit, QualityPreset, RainAmount, Settings},
};
use bevy::prelude::Resource;
use std::{io, path::PathBuf};

const PREVIEW_SECONDS: f64 = 15.0;

#[derive(Clone, Copy)]
enum Page {
    Quality,
    Pacing,
    AudioAccess,
    Presentation,
}

struct Preview {
    original: Settings,
    deadline: f64,
}

// Choosing the current value still protects that field from later native readback
#[derive(Default)]
struct DisplayEdits {
    fullscreen: bool,
    window_size: bool,
    fullscreen_size: bool,
}

#[derive(Resource, Default)]
pub(crate) struct SettingsMenu {
    pub values: Settings,
    pub notice: Message,
    path: Option<PathBuf>,
    draft: Option<Settings>,
    display_edits: DisplayEdits,
    selection: usize,
    resolution_selection: Option<usize>,
    language_selection: Option<usize>,
    page: Option<Page>,
    preview: Option<Preview>,
    current_song: Option<String>,
    recommendation: PresentationOverrides,
}

impl SettingsMenu {
    /// Only Ready songs expose editable presentation preferences
    pub fn set_song(&mut self, content_id: Option<String>, recommendation: PresentationOverrides) {
        self.current_song = content_id;
        self.recommendation = recommendation;
        if self.current_song.is_none() && matches!(self.page, Some(Page::Presentation)) {
            self.page = None;
            self.selection = 0;
        }
    }

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
        self.display_edits = DisplayEdits::default();
        self.selection = 0;
        self.resolution_selection = None;
        self.language_selection = None;
        self.preview = None;
        self.page = None;
    }

    pub fn sync_display(&mut self, display: &DisplayState) {
        if self.preview.is_some() || display.pending {
            return;
        }
        if let Some(draft) = &mut self.draft {
            let actual = display.actual();
            if !self.display_edits.fullscreen {
                draft.display.fullscreen = actual.fullscreen;
            }
            if !self.display_edits.window_size {
                draft.display.window_size = actual.window_size;
            }
            if !self.display_edits.fullscreen_size {
                draft.display.fullscreen_size = actual.fullscreen_size;
            }
        }
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

    fn rollback(&mut self, display: &mut DisplayState, notice: &'static str) {
        if let Some(preview) = self.preview.take() {
            display.request(preview.original.display);
            self.draft = Some(preview.original);
            self.display_edits = DisplayEdits::default();
            self.selection = 2;
        }
        self.notice = Message::new(notice);
    }

    pub fn tick(&mut self, now: f64, display: &mut DisplayState) -> bool {
        if self.preview.as_ref().is_some_and(|p| now >= p.deadline) {
            self.rollback(display, "settings_notice.preview_expired");
            return true;
        }
        false
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
                let message = self.notice.render(self.values.locale);
                self.selection = self
                    .presentation(0.0, display, self.values.locale)
                    .unwrap()
                    .rows
                    .iter()
                    .position(|row| row.text == message)
                    .unwrap();
                false
            }
        }
    }

    /// Returns true only when the settings menu closes, never to resume a song
    pub fn handle(&mut self, action: SettingsAction, now: f64, display: &mut DisplayState) -> bool {
        self.sync_display(display);
        self.tick(now, display);
        let Some(mut draft) = self.draft.clone() else {
            return false;
        };
        let rows = self
            .presentation(now, display, self.values.locale)
            .unwrap()
            .rows
            .len();
        let selected = self
            .language_selection
            .as_mut()
            .or(self.resolution_selection.as_mut())
            .unwrap_or(&mut self.selection);
        *selected = (*selected).min(rows.saturating_sub(1));
        if let Some(selected) = self.language_selection {
            match action {
                SettingsAction::Up | SettingsAction::Previous => {
                    self.language_selection = Some((selected + rows - 1) % rows);
                }
                SettingsAction::Down | SettingsAction::Next => {
                    self.language_selection = Some((selected + 1) % rows);
                }
                SettingsAction::Confirm if selected < Locale::ALL.len() => {
                    draft.locale = Locale::ALL[selected];
                    self.draft = Some(draft);
                    self.language_selection = None;
                }
                SettingsAction::Back => self.language_selection = None,
                SettingsAction::Open | SettingsAction::Confirm => {}
            }
            return false;
        }
        if let Some(selected) = self.resolution_selection {
            let options = display.resolution_options(draft.display.fullscreen);
            if options.is_empty() || (!draft.display.fullscreen && display.window_managed) {
                self.resolution_selection = None;
                return false;
            }
            match action {
                SettingsAction::Up | SettingsAction::Previous => {
                    self.resolution_selection = Some((selected + rows - 1) % rows);
                }
                SettingsAction::Down | SettingsAction::Next => {
                    self.resolution_selection = Some((selected + 1) % rows);
                }
                SettingsAction::Confirm if selected < options.len() => {
                    if draft.display.fullscreen {
                        draft.display.fullscreen_size = options[selected];
                        self.display_edits.fullscreen_size = true;
                    } else {
                        draft.display.window_size = options[selected];
                        self.display_edits.window_size = true;
                    }
                    self.draft = Some(draft);
                    self.resolution_selection = None;
                }
                SettingsAction::Back => self.resolution_selection = None,
                SettingsAction::Open | SettingsAction::Confirm => {}
            }
            return false;
        }
        if self.preview.is_some() {
            match action {
                SettingsAction::Confirm if !display.pending => {
                    return self.save_actual(draft, display);
                }
                SettingsAction::Back => self.rollback(display, "settings_notice.display_cancelled"),
                SettingsAction::Up => self.selection = (self.selection + rows - 1) % rows,
                SettingsAction::Down => self.selection = (self.selection + 1) % rows,
                _ => {}
            }
            return false;
        }
        if let Some(page) = self.page {
            let action_rows = match page {
                Page::Quality => 7,
                Page::Pacing => 3,
                Page::AudioAccess => 5,
                Page::Presentation => 5,
            };
            match action {
                SettingsAction::Back | SettingsAction::Confirm
                    if action == SettingsAction::Back || self.selection == action_rows - 1 =>
                {
                    self.page = None;
                    self.selection = match page {
                        Page::Quality => 5,
                        Page::Pacing => 6,
                        Page::AudioAccess => 8,
                        Page::Presentation => 9,
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
                        Page::AudioAccess => match self.selection {
                            0 => draft.music_volume = adjust_volume(draft.music_volume, previous),
                            1 => {
                                draft.feedback_volume =
                                    adjust_volume(draft.feedback_volume, previous)
                            }
                            2 => draft.quality.reduced_motion = !draft.quality.reduced_motion,
                            3 => draft.quality.reduced_flashes = !draft.quality.reduced_flashes,
                            _ => {}
                        },
                        Page::Presentation => {
                            if let Some(song) = &self.current_song {
                                let mut choices = draft
                                    .song_presentations
                                    .get(song)
                                    .copied()
                                    .unwrap_or_default();
                                match self.selection {
                                    0 => {
                                        choices.world = cycle(
                                            choices.world,
                                            &[
                                                None,
                                                Some(WorldTheme::Neon),
                                                Some(WorldTheme::Forest),
                                                Some(WorldTheme::Candy),
                                                Some(WorldTheme::StarSea),
                                            ],
                                            previous,
                                        )
                                    }
                                    1 => {
                                        choices.timbre = cycle(
                                            choices.timbre,
                                            &[
                                                None,
                                                Some(FeedbackTimbre::Wood),
                                                Some(FeedbackTimbre::Crisp),
                                                Some(FeedbackTimbre::Drums),
                                                Some(FeedbackTimbre::Plucks),
                                                Some(FeedbackTimbre::Glass),
                                                Some(FeedbackTimbre::Elastic),
                                            ],
                                            previous,
                                        )
                                    }
                                    2 => {
                                        choices.motion = cycle(
                                            choices.motion,
                                            &[
                                                None,
                                                Some(MotionStyle::Gentle),
                                                Some(MotionStyle::Playful),
                                                Some(MotionStyle::Energetic),
                                            ],
                                            previous,
                                        )
                                    }
                                    3 if action == SettingsAction::Confirm => {
                                        choices = PresentationOverrides::default()
                                    }
                                    _ => {}
                                }
                                if choices == PresentationOverrides::default() {
                                    draft.song_presentations.remove(song);
                                } else {
                                    draft.song_presentations.insert(song.clone(), choices);
                                }
                            }
                        }
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
            SettingsAction::Up => self.selection = (self.selection + rows - 1) % rows,
            SettingsAction::Down => self.selection = (self.selection + 1) % rows,
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
                        if draft.display.fullscreen {
                            self.display_edits.fullscreen_size = true;
                        } else {
                            self.display_edits.window_size = true;
                        }
                    }
                }
                1 => {
                    draft.display.fullscreen = !draft.display.fullscreen;
                    self.display_edits.fullscreen = true;
                }
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
                    self.selection = 1;
                    self.notice = Message::new("settings_notice.preview");
                }
                3 if action == SettingsAction::Confirm => {
                    self.draft = None;
                    self.notice = Message::new("settings_notice.cancelled");
                    return true;
                }
                4 if action == SettingsAction::Confirm => {
                    draft = Settings::default();
                    self.display_edits = DisplayEdits {
                        fullscreen: true,
                        window_size: true,
                        fullscreen_size: true,
                    };
                    display.normalize_pacing(&mut draft.pacing);
                    self.notice = Message::new("settings_notice.defaults");
                }
                5 | 6 | 8 if action == SettingsAction::Confirm => {
                    self.page = Some(match self.selection {
                        5 => Page::Quality,
                        6 => Page::Pacing,
                        _ => Page::AudioAccess,
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
                9 if action == SettingsAction::Confirm && self.current_song.is_some() => {
                    self.page = Some(Page::Presentation);
                    self.selection = 0;
                }
                _ => {}
            },
            SettingsAction::Open => {}
        }
        self.draft = Some(draft);
        false
    }

    pub fn presentation(
        &self,
        now: f64,
        display: &DisplayState,
        locale: Locale,
    ) -> Option<MenuPresentation> {
        let draft = self.draft.as_ref()?;
        let notices = [self.notice.render(locale), display.notice.render(locale)]
            .into_iter()
            .filter(|notice| !notice.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let footer = |controls, detail: String| {
            [locale.text(controls).into(), detail, notices.clone()]
                .into_iter()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let presentation = |title, mut rows: Vec<MenuRow>, footer: String, selected: usize| {
            rows.extend(footer.lines().map(|text| MenuRow {
                text: text.into(),
                role: MenuRowRole::Information,
                ..MenuRow::default()
            }));
            let selected = selected.min(rows.len().saturating_sub(1));
            for (index, row) in rows.iter_mut().enumerate() {
                row.selected = index == selected;
            }
            Some(MenuPresentation {
                title,
                rows,
                kind: MenuKind::Settings,
                ..Default::default()
            })
        };
        if let Some(selected) = self.language_selection {
            return presentation(
                locale.text("settings.language_title").into(),
                Locale::ALL
                    .into_iter()
                    .map(|language| MenuRow {
                        text: "{language}".into(),
                        language: Some(language),
                        selected: false,
                        ..MenuRow::default()
                    })
                    .collect(),
                footer(
                    "settings.choice_controls",
                    locale.text("settings.draft_hint").into(),
                ),
                selected,
            );
        }
        if let Some(selected) = self.resolution_selection {
            return presentation(
                locale.text("settings.resolution_title").into(),
                text_rows(
                    display
                        .resolution_options(draft.display.fullscreen)
                        .iter()
                        .map(|size| format!("{} x {}", size[0], size[1])),
                ),
                footer(
                    "settings.choice_controls",
                    locale.text("settings.draft_hint").into(),
                ),
                selected,
            );
        }
        if let Some(preview) = &self.preview {
            return presentation(
                Message::with(
                    "settings.keep_title",
                    [(
                        "seconds",
                        ((preview.deadline - now).max(0.0).ceil() as u32).to_string(),
                    )],
                )
                .render(locale),
                text_rows([
                    observed(display, locale),
                    locale
                        .text(if display.pending {
                            "settings.waiting"
                        } else {
                            "settings.confirm_actual"
                        })
                        .into(),
                ]),
                footer("settings.keep_controls", String::new()),
                self.selection,
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
                Page::Presentation => {
                    let choices = self
                        .current_song
                        .as_ref()
                        .and_then(|song| draft.song_presentations.get(song))
                        .copied()
                        .unwrap_or_default();
                    let recommendation = choices
                        .world
                        .map(PresentationVisual::recommendation)
                        .unwrap_or(self.recommendation);
                    let selected =
                        |choice: Option<&'static str>, recommended: Option<&'static str>| {
                            choice
                                .map(|key| locale.text(key).to_string())
                                .unwrap_or_else(|| {
                                    recommended
                                        .map(|key| {
                                            Message::with(
                                                "settings.auto_recommended",
                                                [("value", locale.text(key).into())],
                                            )
                                            .render(locale)
                                        })
                                        .unwrap_or_else(|| locale.text("settings.auto").into())
                                })
                        };
                    (
                        "settings.presentation_title",
                        vec![
                            value_row(
                                "settings.world",
                                &selected(
                                    choices.world.map(world_key),
                                    self.recommendation.world.map(world_key),
                                ),
                            ),
                            value_row(
                                "settings.timbre",
                                &selected(
                                    choices.timbre.map(timbre_key),
                                    recommendation.timbre.map(timbre_key),
                                ),
                            ),
                            value_row(
                                "settings.motion",
                                &selected(
                                    choices.motion.map(motion_key),
                                    recommendation.motion.map(motion_key),
                                ),
                            ),
                            locale.text("settings.restore_auto").into(),
                            locale.text("settings.back").into(),
                        ],
                    )
                }
                Page::AudioAccess => (
                    "settings.audio_access_title",
                    vec![
                        value_row("settings.music_volume", &format!("{}%", draft.music_volume)),
                        value_row(
                            "settings.feedback_volume",
                            &format!("{}%", draft.feedback_volume),
                        ),
                        value_row(
                            "settings.reduced_motion",
                            on_off(draft.quality.reduced_motion),
                        ),
                        value_row(
                            "settings.reduced_flashes",
                            on_off(draft.quality.reduced_flashes),
                        ),
                        locale.text("settings.back").into(),
                    ],
                ),
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
            return presentation(
                locale.text(title).into(),
                text_rows(rows),
                footer(
                    "settings.page_controls",
                    locale.text("settings.draft_hint").into(),
                ),
                self.selection,
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
        let mut rows = text_rows(rows);
        rows.push(MenuRow {
            text: locale.text("settings.language").into(),
            language: Some(draft.locale),
            selected: false,
            ..MenuRow::default()
        });
        rows.extend(text_rows([locale.text("settings.audio_access").into()]));
        if self.current_song.is_some() {
            rows.extend(text_rows([locale.text("settings.presentation").into()]));
        }
        presentation(
            locale.text("settings.title").into(),
            rows,
            footer("settings.controls", observed(display, locale)),
            self.selection,
        )
    }
}

fn world_key(world: WorldTheme) -> &'static str {
    match world {
        WorldTheme::Neon => "presentation.world_neon",
        WorldTheme::Forest => "presentation.world_forest",
        WorldTheme::Candy => "presentation.world_candy",
        WorldTheme::StarSea => "presentation.world_star_sea",
    }
}

fn timbre_key(timbre: FeedbackTimbre) -> &'static str {
    match timbre {
        FeedbackTimbre::Wood => "presentation.timbre_wood",
        FeedbackTimbre::Crisp => "presentation.timbre_crisp",
        FeedbackTimbre::Drums => "presentation.timbre_drums",
        FeedbackTimbre::Plucks => "presentation.timbre_plucks",
        FeedbackTimbre::Glass => "presentation.timbre_glass",
        FeedbackTimbre::Elastic => "presentation.timbre_elastic",
    }
}

fn motion_key(motion: MotionStyle) -> &'static str {
    match motion {
        MotionStyle::Gentle => "presentation.motion_gentle",
        MotionStyle::Playful => "presentation.motion_playful",
        MotionStyle::Energetic => "presentation.motion_energetic",
    }
}

fn adjust_volume(value: u8, previous: bool) -> u8 {
    if previous {
        value.saturating_sub(5)
    } else {
        value.saturating_add(5).min(100)
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

fn text_rows(rows: impl IntoIterator<Item = String>) -> Vec<MenuRow> {
    rows.into_iter()
        .map(|text| MenuRow {
            text,
            ..MenuRow::default()
        })
        .collect()
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

    fn selected_row(menu: &SettingsMenu, display: &DisplayState) -> MenuRow {
        let presentation = menu.presentation(0.0, display, menu.values.locale).unwrap();
        assert_eq!(
            presentation.rows.iter().filter(|row| row.selected).count(),
            1
        );
        presentation
            .rows
            .into_iter()
            .find(|row| row.selected)
            .unwrap()
    }

    #[test]
    fn observed_window_changes_do_not_turn_language_apply_into_a_display_preview() {
        let root = std::env::temp_dir().join(format!(
            "cocobeat-settings-observed-window-{}",
            std::process::id()
        ));
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
        // Model a confirmed external resize after the settings draft was opened
        display.request(DisplaySettings {
            window_size: [900, 700],
            ..display.actual()
        });
        menu.sync_display(&display);
        let presentation = menu.presentation(0.0, &display, Locale::EnUs).unwrap();
        assert!(presentation.rows[0].text.contains("900"));
        assert!(presentation.rows[0].text.contains("700"));
        assert!(
            presentation.rows[0]
                .text
                .contains(Locale::EnUs.text("settings.custom"))
        );
        menu.selection = 7;
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.selection = 2;
        assert!(menu.handle(SettingsAction::Confirm, 1.0, &mut display));
        assert!(menu.preview.is_none());
        assert_eq!(display.actual().window_size, [900, 700]);
        let saved = settings::load(&path).unwrap();
        assert_eq!(saved.display.window_size, [900, 700]);
        assert_eq!(saved.locale, Locale::EnGb);

        // Confirming the current preset is still an explicit choice
        display.request(DisplaySettings::default());
        menu.begin(&display);
        menu.handle(SettingsAction::Confirm, 2.0, &mut display);
        menu.handle(SettingsAction::Confirm, 2.0, &mut display);
        display.request(DisplaySettings {
            window_size: [900, 700],
            fullscreen_size: [1024, 640],
            ..display.actual()
        });
        menu.sync_display(&display);
        let draft = menu.draft.as_ref().unwrap().display;
        assert_eq!(draft.window_size, [1280, 800]);
        assert_eq!(draft.fullscreen_size, [1024, 640]);

        // Fullscreen edits leave the unedited window size following actual readback
        menu.begin(&display);
        menu.selection = 1;
        menu.handle(SettingsAction::Confirm, 3.0, &mut display);
        menu.selection = 0;
        menu.handle(SettingsAction::Next, 3.0, &mut display);
        display.request(DisplaySettings {
            window_size: [1000, 700],
            ..display.actual()
        });
        menu.sync_display(&display);
        let draft = menu.draft.as_ref().unwrap().display;
        assert!(draft.fullscreen);
        assert_eq!(draft.fullscreen_size, [1280, 720]);
        assert_eq!(draft.window_size, [1000, 700]);

        menu.selection = 4;
        menu.handle(SettingsAction::Confirm, 4.0, &mut display);
        display.request(DisplaySettings {
            fullscreen: true,
            window_size: [800, 600],
            fullscreen_size: [640, 480],
        });
        menu.sync_display(&display);
        assert_eq!(
            menu.draft.as_ref().unwrap().display,
            DisplaySettings::default()
        );

        display.request(saved.display);
        menu.begin(&display);
        menu.selection = 1;
        menu.handle(SettingsAction::Confirm, 5.0, &mut display);
        menu.selection = 2;
        assert!(!menu.handle(SettingsAction::Confirm, 5.0, &mut display));
        let preview_draft = menu.draft.clone();
        let mut unsettled = DisplayState::new(DisplaySettings {
            window_size: [800, 600],
            fullscreen_size: [640, 480],
            ..display.actual()
        });
        menu.sync_display(&unsettled);
        assert_eq!(menu.draft, preview_draft);
        unsettled.set_headless_surface([1920, 1080]);
        menu.sync_display(&unsettled);
        assert_eq!(menu.draft, preview_draft);

        // Keep the old observed mode until the rollback request is acknowledged
        let mut restoring = DisplayState::new(unsettled.actual());
        assert!(menu.tick(20.0, &mut restoring));
        assert!(restoring.pending);
        assert!(restoring.actual().fullscreen);
        menu.sync_display(&restoring);
        assert_eq!(menu.draft.as_ref().unwrap().display, saved.display);
        restoring.set_headless_surface([1000, 700]);
        menu.sync_display(&restoring);
        assert_eq!(menu.draft.as_ref().unwrap().display, restoring.actual());
        assert!(!menu.draft.as_ref().unwrap().display.fullscreen);
        assert_eq!(menu.values, saved);
        assert_eq!(settings::load(&path).unwrap(), saved);
        std::fs::remove_dir_all(root).unwrap();
    }

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
        assert_eq!(
            menu.presentation(0.0, &display, Locale::EnUs)
                .unwrap()
                .title,
            "RESOLUTION"
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
        assert!(!menu.tick(15.0, &mut display));
        assert!(menu.tick(16.0, &mut display));
        assert!(!menu.tick(16.0, &mut display));
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
        for _ in 0..7 {
            menu.handle(SettingsAction::Down, 0.0, &mut display);
        }
        assert_eq!(selected_row(&menu, &display).language, Some(Locale::EnUs));
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        let presentation = menu.presentation(0.0, &display, Locale::EnUs).unwrap();
        assert_eq!(presentation.title, "LANGUAGE");
        assert_eq!(
            presentation
                .rows
                .iter()
                .filter(|row| row.language.is_some())
                .count(),
            13
        );
        assert!(
            presentation
                .rows
                .iter()
                .any(|row| row.text.contains("draft until Apply"))
        );
        assert_eq!(selected_row(&menu, &display).language, Some(Locale::EnUs));
        menu.handle(SettingsAction::Previous, 0.0, &mut display);
        menu.handle(SettingsAction::Previous, 0.0, &mut display);
        assert!(
            selected_row(&menu, &display)
                .text
                .contains("draft until Apply")
        );
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        assert_eq!(selected_row(&menu, &display).language, Some(Locale::ZhCn));
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
            let presentation = menu.presentation(3.0, &display, Locale::EnUs).unwrap();
            assert!(presentation.rows.iter().all(|row| row.language.is_none()));
            assert!(
                presentation
                    .rows
                    .iter()
                    .any(|row| row.text == Locale::EnUs.text("settings.keep_controls"))
            );
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
        assert_eq!(
            menu.presentation(0.0, &display, Locale::EnUs)
                .unwrap()
                .title,
            "FRAME RATE AND VSYNC"
        );
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
                    reduced_motion: false,
                    reduced_flashes: false,
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
            menu.selection = 1;
            menu.handle(SettingsAction::Confirm, 1.0, &mut display);
            let draft = menu.draft.as_mut().unwrap();
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

    #[test]
    fn every_settings_page_exposes_focusable_rows_and_read_only_information() {
        let mut menu = SettingsMenu::default();
        menu.values.locale = Locale::EnUs;
        let mut display = DisplayState::new(menu.values.display);
        display.set_headless_surface([1920, 1080]);
        assert!(menu.presentation(0.0, &display, Locale::EnUs).is_none());
        for (entry, title, actions) in [
            (None, "SETTINGS", 9),
            (
                Some(0),
                "RESOLUTION",
                display.resolution_options(false).len(),
            ),
            (Some(5), "GRAPHICS", 7),
            (Some(6), "FRAME RATE AND VSYNC", 3),
            (Some(7), "LANGUAGE", 13),
            (Some(8), "AUDIO AND ACCESSIBILITY", 5),
        ] {
            menu.begin(&display);
            if let Some(entry) = entry {
                menu.selection = entry;
                menu.handle(SettingsAction::Confirm, 0.0, &mut display);
            }
            let presentation = menu.presentation(0.0, &display, Locale::EnUs).unwrap();
            assert_eq!(presentation.title, title);
            assert!(presentation.rows.len() > actions);
            let draft = menu.draft.clone();
            let start = presentation
                .rows
                .iter()
                .position(|row| row.selected)
                .unwrap();
            for offset in 0..presentation.rows.len() {
                let index = (start + offset) % presentation.rows.len();
                assert_eq!(
                    selected_row(&menu, &display),
                    MenuRow {
                        selected: true,
                        ..presentation.rows[index].clone()
                    }
                );
                if index >= actions {
                    assert!(!menu.handle(SettingsAction::Confirm, 0.0, &mut display));
                    assert_eq!(menu.draft, draft);
                }
                menu.handle(SettingsAction::Down, 0.0, &mut display);
            }
            assert_eq!(menu.draft, draft);
        }
        menu.begin(&display);
        menu.selection = 1;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        menu.selection = 2;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        assert_eq!(
            selected_row(&menu, &display).text,
            Locale::EnUs.text("settings.confirm_actual")
        );
        display.pending = true;
        assert!(!menu.handle(SettingsAction::Confirm, 1.0, &mut display));
        assert_eq!(
            selected_row(&menu, &display).text,
            Locale::EnUs.text("settings.waiting")
        );
        display.pending = false;
        menu.handle(SettingsAction::Down, 1.0, &mut display);
        assert_eq!(
            selected_row(&menu, &display).text,
            Locale::EnUs.text("settings.keep_controls")
        );
        assert!(!menu.handle(SettingsAction::Confirm, 1.0, &mut display));
        assert_eq!(menu.notice.key, "settings_notice.save_failed");
        let presentation = menu.presentation(1.0, &display, Locale::EnUs).unwrap();
        let error = presentation
            .rows
            .iter()
            .position(|row| row.text == Locale::EnUs.text("settings_notice.save_failed"))
            .unwrap();
        while menu.selection != error {
            menu.handle(SettingsAction::Down, 1.0, &mut display);
        }
        assert_eq!(
            selected_row(&menu, &display).text,
            Locale::EnUs.text("settings_notice.save_failed")
        );
        assert!(!menu.handle(SettingsAction::Back, 2.0, &mut display));
        assert!(!display.actual().fullscreen);
        assert_eq!(menu.selection, 2);
        assert!(menu.handle(SettingsAction::Back, 2.0, &mut display));
        assert!(menu.presentation(2.0, &display, Locale::EnUs).is_none());
    }

    #[test]
    fn audio_and_per_song_choices_apply_together_and_restore_only_current_song() {
        let root = std::env::temp_dir().join(format!(
            "cocobeat-settings-presentation-{}",
            std::process::id()
        ));
        let path = root.join("settings.json");
        let mut menu = SettingsMenu::from_path(path.clone());
        menu.values.locale = Locale::EnUs;
        let mut display = DisplayState::new(menu.values.display);
        display.set_headless_surface([1920, 1080]);
        let forest = PresentationOverrides {
            world: Some(WorldTheme::Forest),
            ..Default::default()
        };
        menu.values
            .song_presentations
            .insert("other-song".into(), forest);
        menu.set_song(Some("current-song".into()), forest);
        menu.begin(&display);
        menu.selection = 8;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        assert_eq!(menu.draft.as_ref().unwrap().music_volume, 100);
        menu.handle(SettingsAction::Previous, 0.0, &mut display);
        menu.selection = 1;
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.selection = 2;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        menu.selection = 3;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        menu.handle(SettingsAction::Back, 0.0, &mut display);
        menu.selection = 9;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        assert!(
            selected_row(&menu, &display)
                .text
                .contains("Luminous forest")
        );
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.selection = 1;
        menu.handle(SettingsAction::Previous, 0.0, &mut display);
        menu.selection = 2;
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        assert!(!menu.values.song_presentations.contains_key("current-song"));
        menu.handle(SettingsAction::Back, 0.0, &mut display);
        menu.selection = 2;
        assert!(menu.handle(SettingsAction::Confirm, 0.0, &mut display));
        assert_eq!(menu.values.music_volume, 95);
        assert_eq!(menu.values.feedback_volume, 85);
        assert!(menu.values.quality.reduced_motion && menu.values.quality.reduced_flashes);
        assert_eq!(
            menu.values.song_presentations["current-song"],
            PresentationOverrides {
                world: Some(WorldTheme::Neon),
                timbre: Some(FeedbackTimbre::Elastic),
                motion: Some(MotionStyle::Gentle)
            }
        );
        assert_eq!(settings::load(&path).unwrap(), menu.values);

        menu.begin(&display);
        menu.selection = 9;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        menu.selection = 3;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        assert!(
            !menu
                .draft
                .as_ref()
                .unwrap()
                .song_presentations
                .contains_key("current-song")
        );
        assert_eq!(
            menu.draft.as_ref().unwrap().song_presentations["other-song"],
            forest
        );
        menu.handle(SettingsAction::Back, 0.0, &mut display);
        menu.handle(SettingsAction::Back, 0.0, &mut display);
        assert!(menu.values.song_presentations.contains_key("current-song"));
        menu.set_song(None, PresentationOverrides::default());
        menu.begin(&display);
        assert!(
            !menu
                .presentation(0.0, &display, Locale::EnUs)
                .unwrap()
                .rows
                .iter()
                .any(|row| row.text == "This song's presentation")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn draft_world_updates_automatic_timbre_and_motion_labels() {
        let mut menu = SettingsMenu::default();
        menu.values.locale = Locale::EnUs;
        let mut display = DisplayState::new(menu.values.display);
        display.set_headless_surface([1920, 1080]);
        menu.set_song(
            Some("current-song".into()),
            PresentationVisual::recommendation(WorldTheme::Neon),
        );
        menu.begin(&display);
        menu.selection = 9;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        let rows = menu.presentation(0.0, &display, Locale::EnUs).unwrap().rows;
        assert!(rows[1].text.contains("Auto · Elastic electronic"));
        assert!(rows[2].text.contains("Auto · Energetic"));
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        menu.handle(SettingsAction::Next, 0.0, &mut display);
        let rows = menu.presentation(0.0, &display, Locale::EnUs).unwrap().rows;
        assert!(rows[0].text.contains("Luminous forest"));
        assert!(rows[1].text.contains("Auto · Wood percussion"));
        assert!(rows[2].text.contains("Auto · Gentle"));
        menu.selection = 3;
        menu.handle(SettingsAction::Confirm, 0.0, &mut display);
        let rows = menu.presentation(0.0, &display, Locale::EnUs).unwrap().rows;
        assert!(rows[0].text.contains("Auto · Neon city"));
        assert!(rows[1].text.contains("Auto · Elastic electronic"));
    }
}
