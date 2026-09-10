use super::*;

/// State for the open Export dialog (`ui::export_panel`), from the binder's
/// "Export…" context-menu entry — which folder to compile and the book
/// title/subtitle/author fields being edited live.
pub(super) struct ExportState {
    pub(super) source: PathBuf,
    pub(super) source_label: String,
    pub(super) meta: crate::export::BookMeta,
    pub(super) style_id: String,
}

impl SmaragdApp {
    /// The project's currently effective export style id: `ProjectMeta::book_style`
    /// if it still resolves in `self.typeset_styles`, else the first loaded style
    /// (built-in "Manuscript"), else empty if somehow no styles loaded at all or no
    /// project is open. Shared by `open_export` (pre-filling the Export dialog's
    /// Style field) and the Preview tab's inline Style picker (`ui::markdown_preview`),
    /// so both always start from the same resolved style.
    pub(super) fn resolve_book_style_id(&self) -> String {
        let Some(project) = &self.project else {
            return String::new();
        };
        project
            .meta
            .book_style
            .clone()
            .filter(|id| crate::export::style::find(&self.typeset_styles, id).is_some())
            .or_else(|| self.typeset_styles.first().map(|s| s.id.clone()))
            .unwrap_or_default()
    }

    /// Open the Export dialog for `path` (a binder folder) — pre-fills the
    /// Title/Subtitle/Author fields from `ProjectMeta::book_title`/
    /// `book_subtitle`/`book_author` and the Style choice from
    /// `ProjectMeta::book_style`, falling back to the first loaded style
    /// (built-in "Manuscript") if unset or no longer resolves.
    pub(super) fn open_export(&mut self, path: PathBuf) {
        let Some(project) = &self.project else {
            return;
        };
        let source_label = project
            .tree
            .find_by_path(&path)
            .map(|node| node.name.clone())
            .unwrap_or_else(|| path.display().to_string());
        let style_id = self.resolve_book_style_id();
        self.export = Some(ExportState {
            source: path,
            source_label,
            meta: crate::export::BookMeta {
                title: project.meta.book_title.clone().unwrap_or_default(),
                subtitle: project.meta.book_subtitle.clone().unwrap_or_default(),
                author: project.meta.book_author.clone().unwrap_or_default(),
            },
            style_id,
        });
    }

    /// Handle an outcome from the export dialog: Docx/Epub/Pdf opens a save
    /// dialog (native, blocking; a browser download picker on wasm32 — see
    /// the arms below) and runs the export; Reload re-scans custom styles;
    /// Close dismisses it. Title/Subtitle/Author/Style edits are persisted
    /// to the project regardless of which button was pressed, since the
    /// fields may have changed even if the user just closes the dialog.
    pub(super) fn finish_export(
        &mut self,
        ctx: &egui::Context,
        action: ui::export_panel::ExportAction,
    ) {
        let Some(state) = &self.export else {
            return;
        };
        let source = state.source.clone();
        let meta = state.meta.clone();
        let style_id = state.style_id.clone();

        if let Some(project) = &mut self.project
            && let Err(err) = project.set_book_meta(
                meta.title.clone(),
                meta.subtitle.clone(),
                meta.author.clone(),
                style_id.clone(),
            )
        {
            self.push_error_toast(format!("Couldn't save settings: {err}"));
        }

        let Some(style) = crate::export::style::find(&self.typeset_styles, &style_id).cloned()
        else {
            match action {
                ui::export_panel::ExportAction::Close => self.export = None,
                ui::export_panel::ExportAction::ReloadStyles => self.reload_typeset_styles(ctx),
                _ => self.push_error_toast("No typesetting style selected"),
            }
            return;
        };

        match action {
            ui::export_panel::ExportAction::Close => {
                self.export = None;
            }
            ui::export_panel::ExportAction::ReloadStyles => {
                self.reload_typeset_styles(ctx);
            }
            // Export currently writes straight to a path chosen via a native
            // save dialog; the web build needs an in-memory-buffer + browser
            // download rework instead (see the wasm feasibility plan). Cut
            // for now rather than reworked.
            #[cfg(not(target_arch = "wasm32"))]
            ui::export_panel::ExportAction::Docx => {
                if let Some(out_path) = rfd::FileDialog::new()
                    .set_file_name(format!("{}.docx", meta.filename_stem()))
                    .add_filter("Word Document", &["docx"])
                    .save_file()
                {
                    self.run_export(&source, &meta, &style, &out_path);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            ui::export_panel::ExportAction::Epub => {
                if let Some(out_path) = rfd::FileDialog::new()
                    .set_file_name(format!("{}.epub", meta.filename_stem()))
                    .add_filter("EPUB", &["epub"])
                    .save_file()
                {
                    self.run_export_epub(&source, &meta, &style, &out_path);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            ui::export_panel::ExportAction::Pdf => {
                if let Some(out_path) = rfd::FileDialog::new()
                    .set_file_name(format!("{}.pdf", meta.filename_stem()))
                    .add_filter("PDF", &["pdf"])
                    .save_file()
                {
                    self.run_export_pdf(&source, &meta, &style, &out_path);
                }
            }
            // Same rendering as the native arms above (export_*_bytes is the
            // shared core export_* itself now calls to write to a path — see
            // export/{docx,epub,pdf}.rs), but the save step is a browser
            // download instead of a native dialog + fs::write, and rfd's own
            // wasm32 backend makes that unavoidably async (see
            // spawn_browser_download's own doc comment).
            #[cfg(target_arch = "wasm32")]
            ui::export_panel::ExportAction::Docx => {
                let Some(project) = &self.project else { return };
                let Some(folder) = project.tree.find_by_path(&source) else {
                    return;
                };
                let docs = crate::export::gather(project, folder, self.settings.typewriter_quotes);
                match crate::export::docx::export_docx_bytes(&docs, &meta, &style, &project.root) {
                    Ok(bytes) => {
                        self.spawn_browser_download(
                            ctx,
                            format!("{}.docx", meta.filename_stem()),
                            bytes,
                        );
                    }
                    Err(err) => self.push_error_toast(format!("Export failed: {err}")),
                }
            }
            #[cfg(target_arch = "wasm32")]
            ui::export_panel::ExportAction::Epub => {
                let Some(project) = &self.project else { return };
                let Some(folder) = project.tree.find_by_path(&source) else {
                    return;
                };
                let docs = crate::export::gather(project, folder, self.settings.typewriter_quotes);
                match crate::export::epub::export_epub_bytes(&docs, &meta, &style, &project.root) {
                    Ok(bytes) => {
                        self.spawn_browser_download(
                            ctx,
                            format!("{}.epub", meta.filename_stem()),
                            bytes,
                        );
                    }
                    Err(err) => self.push_error_toast(format!("Export failed: {err}")),
                }
            }
            #[cfg(target_arch = "wasm32")]
            ui::export_panel::ExportAction::Pdf => {
                let Some(project) = &self.project else { return };
                let Some(folder) = project.tree.find_by_path(&source) else {
                    return;
                };
                let docs = crate::export::gather(project, folder, self.settings.typewriter_quotes);
                match crate::export::pdf::export_pdf_bytes(&docs, &meta, &style, &project.root) {
                    Ok((bytes, _spine_width_in)) => {
                        self.spawn_browser_download(
                            ctx,
                            format!("{}.pdf", meta.filename_stem()),
                            bytes,
                        );
                    }
                    Err(err) => self.push_error_toast(format!("Export failed: {err}")),
                }
            }
        }
    }

    /// Kicks off a browser save-file pick, then writes `bytes` to it once
    /// chosen — the wasm32 counterpart of a native save dialog + `fs::write`.
    /// Unavoidably async here: per `rfd`'s own docs, `save_file` on wasm32
    /// returns immediately with no dialog at all, and the browser only
    /// actually prompts the user for a save location when
    /// `FileHandle::write` is called — so the pick-and-write can't be split
    /// into "get a path synchronously, write later" the way every native
    /// backend allows. A no-op while a download is already in flight.
    #[cfg(target_arch = "wasm32")]
    fn spawn_browser_download(&mut self, ctx: &egui::Context, filename: String, bytes: Vec<u8>) {
        if self.pending_browser_download.is_some() {
            self.push_error_toast("An export is already in progress");
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let repaint_ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .set_file_name(&filename)
                .save_file()
                .await
            else {
                let _ = sender.send(None);
                repaint_ctx.request_repaint();
                return;
            };
            let result = handle
                .write(&bytes)
                .await
                .map(|()| filename)
                .map_err(|err| err.to_string());
            let _ = sender.send(Some(result));
            repaint_ctx.request_repaint();
        });
        self.pending_browser_download = Some(receiver);
    }

    /// Check whether `pending_browser_download` (if any) has finished, and
    /// apply its result. Called every frame; a no-op whenever nothing is
    /// pending or the download hasn't finished yet.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn poll_browser_download(&mut self) {
        let Some(receiver) = &self.pending_browser_download else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_browser_download = None;
                return;
            }
        };
        self.pending_browser_download = None;
        match result {
            Some(Ok(filename)) => self.set_status_message(format!("Exported {filename}")),
            Some(Err(err)) => self.push_error_toast(format!("Export failed: {err}")),
            // The user closed the save dialog without picking anywhere —
            // an ordinary cancel, not a failure worth a toast.
            None => {}
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_export(
        &mut self,
        source: &Path,
        meta: &crate::export::BookMeta,
        style: &crate::export::style::TypesetStyle,
        out_path: &Path,
    ) {
        let Some(project) = &self.project else {
            return;
        };
        let Some(folder) = project.tree.find_by_path(source) else {
            return;
        };
        let docs = crate::export::gather(project, folder, self.settings.typewriter_quotes);
        match crate::export::docx::export_docx(&docs, meta, style, &project.root, out_path) {
            Ok(()) => {
                self.set_status_message(format!("Exported to {}", out_path.display()));
            }
            Err(err) => {
                self.push_error_toast(format!("Export failed: {err}"));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_export_epub(
        &mut self,
        source: &Path,
        meta: &crate::export::BookMeta,
        style: &crate::export::style::TypesetStyle,
        out_path: &Path,
    ) {
        let Some(project) = &self.project else {
            return;
        };
        let Some(folder) = project.tree.find_by_path(source) else {
            return;
        };
        let docs = crate::export::gather(project, folder, self.settings.typewriter_quotes);
        match crate::export::epub::export_epub(&docs, meta, style, &project.root, out_path) {
            Ok(()) => {
                self.set_status_message(format!("Exported to {}", out_path.display()));
            }
            Err(err) => {
                self.push_error_toast(format!("Export failed: {err}"));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_export_pdf(
        &mut self,
        source: &Path,
        meta: &crate::export::BookMeta,
        style: &crate::export::style::TypesetStyle,
        out_path: &Path,
    ) {
        let Some(project) = &self.project else {
            return;
        };
        let Some(folder) = project.tree.find_by_path(source) else {
            return;
        };
        let docs = crate::export::gather(project, folder, self.settings.typewriter_quotes);
        match crate::export::pdf::export_pdf(&docs, meta, style, &project.root, out_path) {
            Ok(spine_width_in) => {
                self.set_status_message(format!(
                    "Exported to {} — estimated spine width: {spine_width_in:.2}in",
                    out_path.display()
                ));
            }
            Err(err) => {
                self.push_error_toast(format!("Export failed: {err}"));
            }
        }
    }

    /// Handle the card-editor modal closing this frame, whether by Save, Delete, or
    /// Cancel — always clears `card_draft` either way, since the modal is done either
    /// way once an outcome is produced.
    pub(super) fn finish_card_editor(&mut self, outcome: CardEditorOutcome) {
        let Some(draft) = self.card_draft.take() else {
            return;
        };
        let Some(project) = &mut self.project else {
            return;
        };
        match outcome {
            CardEditorOutcome::Save => {
                if let Err(err) = project.upsert_story_card(draft.finalize()) {
                    self.push_error_toast(format!("Couldn't save card: {err}"));
                }
            }
            CardEditorOutcome::Delete(id) => {
                if let Err(err) = project.delete_story_card(id) {
                    self.push_error_toast(format!("Couldn't delete card: {err}"));
                }
            }
            CardEditorOutcome::Cancel => {}
        }
    }
}
