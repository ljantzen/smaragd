use super::*;

impl SmaragdApp {
    /// The documents a find/replace `scope` covers right now. Empty if no project is
    /// open, or (for the file-relative scopes) if nothing's open in the editor.
    fn search_scope_paths(&self, scope: SearchScope) -> Vec<PathBuf> {
        let Some(project) = &self.project else {
            return Vec::new();
        };
        match scope {
            SearchScope::CurrentFile => self.editor.open_path.clone().into_iter().collect(),
            SearchScope::CurrentDirectory => {
                let Some(dir) = self.editor.open_path.as_deref().and_then(Path::parent) else {
                    return Vec::new();
                };
                project
                    .tree
                    .document_paths()
                    .into_iter()
                    .filter(|path| path.parent() == Some(dir))
                    .collect()
            }
            SearchScope::ModifiedFiles => self.editor.modified_paths.iter().cloned().collect(),
            SearchScope::AllFiles => project.tree.document_paths(),
        }
    }

    pub(super) fn handle_find_replace_event(
        &mut self,
        ctx: &egui::Context,
        event: FindReplaceEvent,
    ) {
        match event {
            FindReplaceEvent::Search => self.run_search(),
            FindReplaceEvent::ReplaceAll => self.run_replace_all(),
            FindReplaceEvent::OpenResult(index) => self.open_search_result(ctx, index),
        }
    }

    fn run_search(&mut self) {
        let paths = self.search_scope_paths(self.find_replace.scope);
        let live = self
            .editor
            .open_path
            .as_deref()
            .map(|path| (path, self.editor.buffer.as_str()));
        self.find_replace.results = search::search_paths(
            &paths,
            &self.find_replace.query,
            self.find_replace.case_sensitive,
            live,
        );
    }

    /// Replace every match in scope. The currently open document (if in scope) is
    /// updated in its live buffer and marked dirty, matching how the rest of the app
    /// treats it as unsaved until focus leaves the editor or the user saves
    /// explicitly; every other file in scope is read, replaced, and written back to
    /// disk immediately, since there's no in-memory buffer for it to land in.
    fn run_replace_all(&mut self) {
        let paths = self.search_scope_paths(self.find_replace.scope);
        let mut total = 0usize;
        for path in paths {
            let is_open = self.editor.open_path.as_deref() == Some(path.as_path());
            let content = if is_open {
                self.editor.buffer.clone()
            } else {
                match fs::read_to_string(&path) {
                    Ok(content) => content,
                    Err(_) => continue,
                }
            };

            let (new_content, count) = search::replace_all(
                &content,
                &self.find_replace.query,
                &self.find_replace.replacement,
                self.find_replace.case_sensitive,
            );
            if count == 0 {
                continue;
            }
            total += count;

            if is_open {
                self.editor.buffer = new_content;
                self.editor.mark_dirty();
            } else if let Err(err) = fs::write(&path, &new_content) {
                self.push_error_toast(format!("Couldn't update {}: {err}", path.display()));
            }
        }
        self.set_status_message(format!("Replaced {total} occurrence(s)"));
        self.run_search();
    }

    /// Open the result's document (if it isn't already open) and move the editor
    /// cursor to where the match starts.
    fn open_search_result(&mut self, ctx: &egui::Context, index: usize) {
        let Some(result) = self.find_replace.results.get(index).cloned() else {
            return;
        };
        if self.editor.open_path.as_deref() != Some(result.path.as_path()) {
            self.open_document(&result.path);
        }
        if self.editor.open_path.as_deref() == Some(result.path.as_path()) {
            ui::editor_panel::move_cursor_to(
                ctx,
                ui::editor_panel::editor_text_edit_id(),
                &self.editor.buffer,
                result.byte_start,
            );
        }
    }
}
