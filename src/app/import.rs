use super::*;

/// One finished `pending_browser_import` outcome: the imported nodes plus
/// the source filename (for `finish_import`'s status message), or an error
/// message.
#[cfg(target_arch = "wasm32")]
pub(super) type BrowserImportResult = Result<(Vec<crate::import::ImportedNode>, String), String>;

// Docx/Epub/Pdf import only ever need a single-file pick, which `rfd`'s web
// backend does support (unlike a folder pick — see import_scrivener's own
// native-only gate below) — but `rfd::AsyncFileDialog` is unavoidably async
// on wasm32 (no dialog appears at all until awaited), so these three each
// have a separate wasm32 implementation using the same
// spawn-then-poll-each-frame pattern as `export::spawn_browser_download`.
#[cfg(not(target_arch = "wasm32"))]
impl SmaragdApp {
    /// Import a `.docx` file, picked via a native file dialog, into the
    /// currently open project — split into one document per Heading 1 (see
    /// `import::docx::parse`).
    pub(super) fn import_docx(&mut self, _ctx: &egui::Context) {
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
    pub(super) fn import_epub(&mut self, _ctx: &egui::Context) {
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
    pub(super) fn import_pdf(&mut self, _ctx: &egui::Context) {
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
    /// validate against). Native-only: folder-picking has no browser
    /// equivalent (see the wasm feasibility plan).
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
}

#[cfg(target_arch = "wasm32")]
impl SmaragdApp {
    pub(super) fn import_docx(&mut self, ctx: &egui::Context) {
        self.spawn_browser_import(ctx, "Word Document", &["docx"], |bytes, filename| {
            let fallback_title = filename
                .strip_suffix(".docx")
                .unwrap_or(&filename)
                .to_string();
            crate::import::docx::parse(&bytes, &fallback_title).map_err(|err| err.to_string())
        });
    }

    pub(super) fn import_epub(&mut self, ctx: &egui::Context) {
        self.spawn_browser_import(ctx, "EPUB", &["epub"], |bytes, _filename| {
            crate::import::epub::parse(&bytes).map_err(|err| err.to_string())
        });
    }

    pub(super) fn import_pdf(&mut self, ctx: &egui::Context) {
        self.spawn_browser_import(ctx, "PDF", &["pdf"], |bytes, filename| {
            let fallback_title = filename
                .strip_suffix(".pdf")
                .unwrap_or(&filename)
                .to_string();
            crate::import::pdf::parse(&bytes, &fallback_title).map_err(|err| err.to_string())
        });
    }

    /// Kicks off a browser open-file pick, reads the chosen file, and parses
    /// it with `parse` — all inside the same background task, so the only
    /// thing that needs to cross back to the main thread (via
    /// `pending_browser_import`, polled by `poll_browser_import`) is the
    /// already-parsed result, not the format-specific parsing step itself.
    /// Unavoidably async here, unlike native's blocking `rfd::FileDialog`
    /// (see `export::spawn_browser_download`'s doc comment for the same
    /// story on the save side). A no-op while an import is already in
    /// flight.
    fn spawn_browser_import(
        &mut self,
        ctx: &egui::Context,
        filter_name: &'static str,
        extensions: &'static [&'static str],
        parse: impl FnOnce(Vec<u8>, String) -> Result<Vec<crate::import::ImportedNode>, String>
        + 'static,
    ) {
        if self.project.is_none() {
            self.push_error_toast("No project open");
            return;
        }
        if self.pending_browser_import.is_some() {
            self.push_error_toast("An import is already in progress");
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let repaint_ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter(filter_name, extensions)
                .pick_file()
                .await
            else {
                let _ = sender.send(None);
                repaint_ctx.request_repaint();
                return;
            };
            let filename = handle.file_name();
            let bytes = handle.read().await;
            let result = parse(bytes, filename.clone()).map(|nodes| (nodes, filename));
            let _ = sender.send(Some(result));
            repaint_ctx.request_repaint();
        });
        self.pending_browser_import = Some(receiver);
    }

    /// Check whether `pending_browser_import` (if any) has finished, and
    /// apply its result via `finish_import` (the same tail end native's
    /// `import_*` methods share). Called every frame; a no-op whenever
    /// nothing is pending or the import hasn't finished yet.
    pub(super) fn poll_browser_import(&mut self) {
        let Some(receiver) = &self.pending_browser_import else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_browser_import = None;
                return;
            }
        };
        self.pending_browser_import = None;
        match result {
            Some(Ok((nodes, filename))) => self.finish_import(nodes, &filename),
            Some(Err(err)) => self.push_error_toast(format!("Import failed: {err}")),
            // The user closed the open-file dialog without picking anything
            // — an ordinary cancel, not a failure worth a toast.
            None => {}
        }
    }
}

impl SmaragdApp {
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
