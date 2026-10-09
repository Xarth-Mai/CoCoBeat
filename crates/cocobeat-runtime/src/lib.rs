//! Bevy input/presentation and Kira audio adapt to the shared deterministic rules

mod app;
mod audio;
mod brand_audio;
mod brand_intro;
pub mod clock;
mod content;
pub mod dev_song;
mod display;
mod display_area;
mod feedback_audio;
mod i18n;
mod input;
mod library;
mod online;
mod presentation;
pub mod probe;
mod replay_playback;
mod scene;
mod session;
mod settings;
mod settings_menu;
mod timing_diagnostic;
mod ui_assets;
mod view;

pub use app::run;
pub use audio::{AudioOutput, sound_data};
pub use i18n::{Locale, Message};
pub use input::{InputSource, MenuAccess, menu_access};
pub use settings::configured_locale;
pub use ui_assets::{UiAssets, install as install_ui_assets};
