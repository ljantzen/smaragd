//! App-wide preferences, stored separately from the real egui app's own
//! settings file — see core::settings's module doc for why. Stateless like
//! every other command module so far (reload from disk, mutate, save) since
//! settings changes are rare enough that the IPC round trip is irrelevant.

use smaragd_core::color_theme::{ColorTheme, built_in_themes};
use smaragd_core::project::store::NativeStore;
use smaragd_core::settings::Settings;

fn path() -> Result<std::path::PathBuf, String> {
    smaragd_core::settings::config_file_path()
        .ok_or_else(|| "couldn't resolve the platform config directory".to_string())
}

#[tauri::command]
pub fn settings_load() -> Result<Settings, String> {
    Ok(Settings::load_from_path(&NativeStore, &path()?))
}

#[tauri::command]
pub fn settings_save(settings: Settings) -> Result<(), String> {
    settings
        .save_to_path(&NativeStore, &path()?)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn color_themes_list() -> Vec<ColorTheme> {
    built_in_themes()
}
