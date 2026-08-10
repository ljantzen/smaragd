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

    /// Handle an outcome from the export dialog: Docx/Epub/Pdf opens a native
    /// save dialog and runs the export; Reload re-scans custom styles; Close
    /// dismisses it. Title/Subtitle/Author/Style edits are persisted to the
    /// project regardless of which button was pressed, since the fields may
    /// have changed even if the user just closes the dialog.
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
            ui::export_panel::ExportAction::Docx => {
                if let Some(out_path) = rfd::FileDialog::new()
                    .set_file_name(format!("{}.docx", meta.filename_stem()))
                    .add_filter("Word Document", &["docx"])
                    .save_file()
                {
                    self.run_export(&source, &meta, &style, &out_path);
                }
            }
            ui::export_panel::ExportAction::Epub => {
                if let Some(out_path) = rfd::FileDialog::new()
                    .set_file_name(format!("{}.epub", meta.filename_stem()))
                    .add_filter("EPUB", &["epub"])
                    .save_file()
                {
                    self.run_export_epub(&source, &meta, &style, &out_path);
                }
            }
            ui::export_panel::ExportAction::Pdf => {
                if let Some(out_path) = rfd::FileDialog::new()
                    .set_file_name(format!("{}.pdf", meta.filename_stem()))
                    .add_filter("PDF", &["pdf"])
                    .save_file()
                {
                    self.run_export_pdf(&source, &meta, &style, &out_path);
                }
            }
        }
    }

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
