pub mod app;
pub mod autocomplete;
pub mod clipboard;
pub mod color_theme;
pub mod double_tap;
pub mod editor_font;
pub mod notifications;
pub mod plugins;
pub mod pomodoro;
pub mod session;
pub mod settings;
pub mod shortcuts;
pub mod ui;

#[cfg(test)]
pub(crate) mod egui_test_support;

pub use app::SmaragdApp;
