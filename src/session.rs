//! The working state "Reopen last project on launch" (`Settings::
//! reopen_last_project`) brings back beyond the project itself: which
//! document was open and where its cursor was, the binder selection and
//! collapsed folders, Back/Forward history, Focus Mode, and the window's size
//! and position. Saved to `session.json` whenever the app closes (see
//! `SmaragdApp::persist_session`) and read back at the next launch — the
//! window geometry by `main` before the window exists
//! ([`restored_viewport`]), everything else once the project has reopened
//! (`SmaragdApp::restore_session`). The dock layout and `Settings` aren't
//! here: both already persist on their own (`dock_layout.json`,
//! `smaragd.toml`).
//!
//! Kept out of the project's own `.smaragd/project.json`: this changes on
//! every close, and a project folder is often a git repo — cursor positions
//! and window sizes don't belong in its history.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    /// The project the rest of these fields belong to — restored only when
    /// it's the one that actually reopened. `None` when the app closed with
    /// no project open (the window geometry is still worth keeping).
    pub project_path: Option<PathBuf>,
    pub open_document: Option<PathBuf>,
    /// Byte offset of the cursor in `open_document`.
    pub cursor_byte: usize,
    /// The binder selection, which can differ from `open_document` (a folder
    /// whose metadata was showing, say).
    pub selected_path: Option<PathBuf>,
    /// `DocumentHistory`'s entries, current position and per-document cursor
    /// offsets — what Back/Forward and "Recent Files > Opened" work from.
    pub history_entries: Vec<PathBuf>,
    pub history_position: Option<usize>,
    pub cursor_positions: Vec<(PathBuf, usize)>,
    /// Folders collapsed in the binder. Only the collapsed ones are listed,
    /// since expanded is the binder's default.
    pub collapsed_folders: Vec<PathBuf>,
    pub focus_mode: bool,
    pub window: Option<WindowGeometry>,
}

/// The window's size and position in logical points, as `egui::ViewportInfo`
/// reports them. `inner_size`/`position` describe the window's normal
/// (unmaximized, windowed) placement even when `maximized`/`fullscreen` is
/// set, so leaving either after a restore lands back at the right size —
/// see `SmaragdApp::persist_session`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub inner_size: [f32; 2],
    /// `None` where the platform doesn't report one (Wayland never lets a
    /// client know or choose its own position).
    pub position: Option<[f32; 2]>,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl SessionState {
    /// `None` if the file is missing or doesn't parse — never an error: a
    /// lost session just means starting with nothing open, as before this
    /// existed.
    pub fn load_from_path(path: &Path) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    pub fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

/// The full path to the saved session, e.g. `~/.config/smaragd/session.json`
/// on Linux, alongside `dock_layout.json`. `None` if the platform's config
/// directory can't be determined.
#[cfg(not(target_arch = "wasm32"))]
pub fn session_file_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "smaragd")
        .map(|dirs| dirs.config_dir().join("session.json"))
}

/// No config directory in a browser — the browser edition reopens its
/// project its own way (`SmaragdApp::spawn_browser_project_load`), and has
/// no window to size.
#[cfg(target_arch = "wasm32")]
pub fn session_file_path() -> Option<PathBuf> {
    None
}

/// `builder` with the previous session's window size, position and
/// maximized/fullscreen state applied — when `Settings::reopen_last_project`
/// is on and there's a saved session to take them from; unchanged otherwise.
/// Called by `main` before the window is created, since eframe only takes
/// initial geometry through its `ViewportBuilder`.
pub fn restored_viewport(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    let reopen = crate::settings::config_file_path()
        .map(|path| crate::settings::Settings::load_from_path(&path).reopen_last_project)
        .unwrap_or(false);
    let window = session_file_path()
        .and_then(|path| SessionState::load_from_path(&path))
        .and_then(|session| session.window);
    match window {
        Some(window) if reopen => apply_window_geometry(builder, window),
        _ => builder,
    }
}

fn apply_window_geometry(
    builder: egui::ViewportBuilder,
    window: WindowGeometry,
) -> egui::ViewportBuilder {
    let mut builder = builder
        .with_inner_size(window.inner_size)
        .with_maximized(window.maximized)
        .with_fullscreen(window.fullscreen);
    if let Some(position) = window.position {
        builder = builder.with_position(position);
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trips_through_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let session = SessionState {
            project_path: Some(PathBuf::from("/home/author/novel")),
            open_document: Some(PathBuf::from("/home/author/novel/Manuscript/one.md")),
            cursor_byte: 42,
            selected_path: Some(PathBuf::from("/home/author/novel/Manuscript")),
            history_entries: vec![
                PathBuf::from("/home/author/novel/Manuscript/two.md"),
                PathBuf::from("/home/author/novel/Manuscript/one.md"),
            ],
            history_position: Some(1),
            cursor_positions: vec![(PathBuf::from("/home/author/novel/Manuscript/two.md"), 7)],
            collapsed_folders: vec![PathBuf::from("/home/author/novel/Research")],
            focus_mode: true,
            window: Some(WindowGeometry {
                inner_size: [1200.0, 800.0],
                position: Some([40.0, 30.0]),
                maximized: false,
                fullscreen: false,
            }),
        };

        session.save_to_path(&path).unwrap();

        assert_eq!(SessionState::load_from_path(&path), Some(session));
    }

    #[test]
    fn a_missing_or_corrupt_session_file_loads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        assert_eq!(SessionState::load_from_path(&path), None);

        std::fs::write(&path, "not json").unwrap();
        assert_eq!(SessionState::load_from_path(&path), None);
    }
}
