use super::*;

impl SmaragdApp {
    /// Import a `.docx` file, picked via a native file dialog, into the
    /// currently open project — split into one document per Heading 1 (see
    /// `import::docx::parse`).
    pub(super) fn import_docx(&mut self) {
        if self.project.is_none() {
            self.push_error_toast("No project open");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Word Document", &["docx"])
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.push_error_toast(format!("Couldn't read {}: {err}", path.display()));
                return;
            }
        };
        let fallback_title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported Document")
            .to_string();
        match crate::import::docx::parse(&bytes, &fallback_title) {
            Ok(nodes) => self.finish_import(nodes, &path.display().to_string()),
            Err(err) => self.push_error_toast(format!("Import failed: {err}")),
        }
    }

    /// Import an `.epub` file, picked via a native file dialog, into the
    /// currently open project — one document per spine chapter, titled from
    /// each chapter's own first heading (see `import::epub::parse`).
    pub(super) fn import_epub(&mut self) {
        if self.project.is_none() {
            self.push_error_toast("No project open");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("EPUB", &["epub"])
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.push_error_toast(format!("Couldn't read {}: {err}", path.display()));
                return;
            }
        };
        match crate::import::epub::parse(&bytes) {
            Ok(nodes) => self.finish_import(nodes, &path.display().to_string()),
            Err(err) => self.push_error_toast(format!("Import failed: {err}")),
        }
    }

    /// Import a `.pdf` file, picked via a native file dialog, into the
    /// currently open project — always a single document, with no formatting
    /// or chapter structure recovered (see `import::pdf::parse`'s doc
    /// comment for why: PDF has none to recover in the general case).
    pub(super) fn import_pdf(&mut self) {
        if self.project.is_none() {
            self.push_error_toast("No project open");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.push_error_toast(format!("Couldn't read {}: {err}", path.display()));
                return;
            }
        };
        let fallback_title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported Document")
            .to_string();
        match crate::import::pdf::parse(&bytes, &fallback_title) {
            Ok(nodes) => self.finish_import(nodes, &path.display().to_string()),
            Err(err) => self.push_error_toast(format!("Import failed: {err}")),
        }
    }

    /// Import a Scrivener project, picked as a folder (a `.scriv` project is
    /// a directory, not a single file — no `add_filter` applies) via a
    /// native picker, into the currently open project (see
    /// `import::scrivener::parse`'s doc comment: the highest-risk of the
    /// four importers, since it was written without a real sample project to
    /// validate against).
    pub(super) fn import_scrivener(&mut self) {
        if self.project.is_none() {
            self.push_error_toast("No project open");
            return;
        }
        let Some(path) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        match crate::import::scrivener::parse(&path) {
            Ok(nodes) => self.finish_import(nodes, &path.display().to_string()),
            Err(err) => self.push_error_toast(format!("Import failed: {err}")),
        }
    }

    /// Writes `nodes` into the currently open project (already checked
    /// `Some` by every `import_*` caller above) and reports the outcome —
    /// shared by every format so the "where does it land, how is success/
    /// failure reported" policy exists exactly once.
    fn finish_import(&mut self, nodes: Vec<crate::import::ImportedNode>, source_label: &str) {
        // Computed with plain field reads, before taking `&mut self.project`
        // below — `self.selected_path`'s parent, or the project root, same
        // destination rule `keyboard_new_file`/`keyboard_new_folder` use.
        let project_root = self
            .project
            .as_ref()
            .expect("caller already checked project.is_some()")
            .root
            .clone();
        let destination = self
            .selected_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or(project_root);

        let Some(project) = &mut self.project else {
            return;
        };
        match crate::import::write_imported_tree(project, &destination, &nodes) {
            Ok(summary) => {
                self.set_status_message(format!(
                    "Imported {} document{} from {source_label}",
                    summary.documents,
                    if summary.documents == 1 { "" } else { "s" },
                ));
            }
            Err(err) => {
                self.push_error_toast(format!("Import failed: {err}"));
            }
        }
    }
}
