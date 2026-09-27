use super::*;

use crate::project::model::{BinderNode, BinderNodeKind};
use crate::session::{SessionState, WindowGeometry};

impl SmaragdApp {
    /// Everything `restore_session` can bring back, as of right now. Window
    /// geometry is taken from `ctx`'s viewport info; while the window is
    /// maximized or fullscreen (including Focus Mode's own maximize) its
    /// normal size isn't knowable from there, so `previous`'s is carried
    /// over instead.
    pub(super) fn capture_session(
        &self,
        ctx: &egui::Context,
        previous: Option<&SessionState>,
    ) -> SessionState {
        let mut session = SessionState {
            window: capture_window_geometry(ctx, previous.and_then(|p| p.window)),
            ..Default::default()
        };
        let Some(project) = &self.project else {
            return session;
        };
        let mut history = self.document_history.clone();
        if let Some(open) = &self.editor.open_path {
            history.record_cursor(open, self.editor.cursor_byte);
        }
        let (entries, position, cursor_positions) = history.snapshot();
        let mut collapsed_folders = Vec::new();
        collect_collapsed_folders(ctx, &project.tree.root, &mut collapsed_folders);
        session.project_path = Some(project.root.clone());
        session.open_document = self.editor.open_path.clone();
        session.cursor_byte = self.editor.cursor_byte;
        session.selected_path = self.selected_path.clone();
        session.history_entries = entries;
        session.history_position = position;
        session.cursor_positions = cursor_positions;
        session.collapsed_folders = collapsed_folders;
        session.focus_mode = self.focus_mode;
        session
    }

    /// Save `capture_session` to `session.json`, whether or not
    /// `Settings::reopen_last_project` is on — the same "so turning it on
    /// later works immediately" reasoning as `Settings::last_project_path`.
    /// Called from the same close-requested hook as `persist_dock_layout`.
    pub(super) fn persist_session(&mut self, ctx: &egui::Context) {
        let Some(path) = crate::session::session_file_path() else {
            return;
        };
        let previous = SessionState::load_from_path(&path);
        let session = self.capture_session(ctx, previous.as_ref());
        // Not a toast: the app is closing, so nobody would see it.
        let _ = session.save_to_path(&path);
    }

    /// Put back what `capture_session` saved, onto the project that just
    /// reopened. A no-op unless `session` belongs to that project. Anything
    /// that no longer exists (a document or folder deleted outside smaragd
    /// since) is skipped rather than reported.
    pub(super) fn restore_session(&mut self, ctx: &egui::Context, session: SessionState) {
        let Some(project) = &self.project else {
            return;
        };
        if session.project_path.as_deref() != Some(project.root.as_path()) {
            return;
        }
        let store = project.store.clone();

        for folder in &session.collapsed_folders {
            if store.is_dir(folder) {
                ui::binder_panel::set_folder_open(ctx, folder, false);
            }
        }

        self.document_history = DocumentHistory::from_snapshot(
            session.history_entries,
            session.history_position,
            session.cursor_positions,
        );
        for path in self.document_history.known_paths() {
            if !store.exists(&path) {
                self.document_history.remove_subtree(&path);
            }
        }

        if let Some(document) = session.open_document.filter(|path| store.exists(path)) {
            self.document_history
                .record_cursor(&document, session.cursor_byte);
            self.load_document(&document);
        }
        if let Some(selected) = session.selected_path.filter(|path| store.exists(path)) {
            self.selected_path = Some(selected);
        }
        if session.focus_mode && self.editor.open_path.is_some() {
            self.set_focus_mode(ctx, true);
        }
    }
}

fn collect_collapsed_folders(ctx: &egui::Context, node: &BinderNode, out: &mut Vec<PathBuf>) {
    if let BinderNodeKind::Folder { children } = &node.kind {
        if !ui::binder_panel::is_folder_open(ctx, &node.path) {
            out.push(node.path.clone());
        }
        for child in children {
            collect_collapsed_folders(ctx, child, out);
        }
    }
}

/// The window's current geometry, or `None` if the viewport hasn't reported
/// a size yet. See `capture_session` on why a maximized/fullscreen window
/// keeps `previous`'s size and position.
fn capture_window_geometry(
    ctx: &egui::Context,
    previous: Option<WindowGeometry>,
) -> Option<WindowGeometry> {
    let viewport = ctx.input(|i| i.viewport().clone());
    let maximized = viewport.maximized.unwrap_or(false);
    let fullscreen = viewport.fullscreen.unwrap_or(false);
    let (inner_size, position) = match previous {
        Some(previous) if maximized || fullscreen => (previous.inner_size, previous.position),
        _ => (
            viewport.inner_rect?.size().into(),
            viewport.outer_rect.map(|rect| rect.min.into()),
        ),
    };
    Some(WindowGeometry {
        inner_size,
        position,
        maximized,
        fullscreen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_project() -> (tempfile::TempDir, SmaragdApp) {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter").unwrap();
        project.create_document(&chapter, "one").unwrap();
        project.create_document(&chapter, "two").unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        (dir, app)
    }

    fn doc(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        dir.path().join("Chapter").join(format!("{name}.md"))
    }

    #[test]
    fn a_captured_session_restores_the_open_document_cursor_history_and_selection() {
        let (dir, mut app) = app_with_project();
        let ctx = egui::Context::default();
        app.open_document(&doc(&dir, "two"));
        app.open_document(&doc(&dir, "one"));
        app.editor.cursor_byte = 3;
        app.selected_path = Some(dir.path().join("Chapter"));
        ui::binder_panel::set_folder_open(&ctx, &dir.path().join("Chapter"), false);

        let session = app.capture_session(&ctx, None);

        let mut reopened = SmaragdApp::test_fixture();
        reopened.project = Some(Project::load_from_folder(dir.path()).unwrap());
        let fresh_ctx = egui::Context::default();
        reopened.restore_session(&fresh_ctx, session);

        assert_eq!(reopened.editor.open_path, Some(doc(&dir, "one")));
        assert_eq!(reopened.editor.pending_cursor, Some(3));
        assert_eq!(reopened.selected_path, Some(dir.path().join("Chapter")));
        assert_eq!(
            reopened.document_history.previous(),
            Some(doc(&dir, "two").as_path())
        );
        assert!(!ui::binder_panel::is_folder_open(
            &fresh_ctx,
            &dir.path().join("Chapter")
        ));
    }

    #[test]
    fn a_session_from_a_different_project_is_ignored() {
        let (dir, mut app) = app_with_project();
        let ctx = egui::Context::default();
        app.open_document(&doc(&dir, "one"));
        let mut session = app.capture_session(&ctx, None);
        session.project_path = Some(PathBuf::from("/somewhere/else"));

        let mut reopened = SmaragdApp::test_fixture();
        reopened.project = Some(Project::load_from_folder(dir.path()).unwrap());
        reopened.restore_session(&ctx, session);

        assert_eq!(reopened.editor.open_path, None);
    }

    #[test]
    fn documents_deleted_since_the_session_was_saved_are_skipped() {
        let (dir, mut app) = app_with_project();
        let ctx = egui::Context::default();
        app.open_document(&doc(&dir, "two"));
        app.open_document(&doc(&dir, "one"));
        let session = app.capture_session(&ctx, None);
        std::fs::remove_file(doc(&dir, "one")).unwrap();
        std::fs::remove_file(doc(&dir, "two")).unwrap();

        let mut reopened = SmaragdApp::test_fixture();
        reopened.project = Some(Project::load_from_folder(dir.path()).unwrap());
        reopened.restore_session(&ctx, session);

        assert_eq!(reopened.editor.open_path, None);
        assert_eq!(reopened.document_history.previous(), None);
        assert!(reopened.document_history.known_paths().is_empty());
    }

    #[test]
    fn a_maximized_window_keeps_the_previous_normal_size() {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .maximized = Some(true);
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .inner_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(2560.0, 1440.0),
        ));
        crate::egui_test_support::run_ui_and_discard(&ctx, input, |_| {});
        let previous = WindowGeometry {
            inner_size: [1200.0, 800.0],
            position: Some([40.0, 30.0]),
            maximized: false,
            fullscreen: false,
        };

        let captured = capture_window_geometry(&ctx, Some(previous)).unwrap();

        assert_eq!(captured.inner_size, [1200.0, 800.0]);
        assert_eq!(captured.position, Some([40.0, 30.0]));
        assert!(captured.maximized);
    }
}
