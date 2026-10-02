//! Bevy input/presentation and Kira audio adapt to the shared deterministic rules

mod app;
mod audio;
mod brand_audio;
mod brand_intro;
pub mod clock;
pub mod dev_song;
mod display;
mod display_area;
mod i18n;
mod input;
pub mod probe;
mod session;
mod settings;
mod settings_menu;
mod ui_assets;
mod view;

pub use app::run;
