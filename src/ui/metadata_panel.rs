//! A form editor for a document's YAML frontmatter (`frontmatter::DocumentMeta`) — a
//! dockable, always-live tool window (Visual-Basic-Properties-window style) rather
//! than a raw YAML text box or a modal with an explicit Save step: the whole point is
//! that a user never has to hand-edit the `---` block, and edits here take effect as
//! soon as they're typed. `app.rs`'s `apply_metadata_edits_if_changed` is what
//! actually notices a change and writes it back into the open document's buffer —
//! this module only renders the fields and mutates the draft.

use crate::frontmatter::DocumentMeta;

/// Plain-text editing buffers for a `DocumentMeta`'s fields, owned by `app.rs` for as
/// long as the modal is open.
pub struct MetadataDraft {
    pub section_type: String,
    pub status: String,
    pub pov: String,
    pub word_count_target_text: String,
    /// Comma-separated, matching the story card editor's `subplot_tags_text`.
    pub tags_text: String,
}

impl MetadataDraft {
    pub fn from_meta(meta: &DocumentMeta) -> Self {
        Self {
            section_type: meta.section_type.clone().unwrap_or_default(),
            status: meta.status.clone().unwrap_or_default(),
            pov: meta.pov.clone().unwrap_or_default(),
            word_count_target_text: meta
                .word_count_target
                .map(|target| target.to_string())
                .unwrap_or_default(),
            tags_text: meta.tags.join(", "),
        }
    }

    /// Fold the editing buffers into a `DocumentMeta`. A blank field (or, for word
    /// count target, one that doesn't parse as a number) becomes `None`/empty rather
    /// than an error — there's no invalid state here, just an unset field.
    pub fn to_meta(&self) -> DocumentMeta {
        DocumentMeta {
            section_type: non_empty(&self.section_type),
            status: non_empty(&self.status),
            pov: non_empty(&self.pov),
            word_count_target: self.word_count_target_text.trim().parse().ok(),
            tags: self
                .tags_text
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }
}

fn non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Renders the metadata form directly into `ui` (a dock tab's content area, not a
/// modal overlay). `open_path` is only used to show an empty-state message when no
/// document is open — `draft` itself is kept in sync with whatever document is open
/// by the caller (`app.rs`'s `refresh_metadata_if_needed`). Edits land directly in
/// `draft`'s buffers via the `text_edit_singleline` calls below; there's no Save/
/// Cancel here, since `apply_metadata_edits_if_changed` picks up any change after
/// this renders each frame.
pub fn show(ui: &mut egui::Ui, open_path: Option<&std::path::Path>, draft: &mut MetadataDraft) {
    ui.heading("Metadata");
    ui.separator();

    if open_path.is_none() {
        ui.label("Open a document to edit its metadata.");
        return;
    }

    egui::Grid::new("document_metadata_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Type:");
            ui.text_edit_singleline(&mut draft.section_type);
            ui.end_row();

            ui.label("Status:");
            ui.text_edit_singleline(&mut draft.status);
            ui.end_row();

            ui.label("POV:");
            ui.text_edit_singleline(&mut draft.pov);
            ui.end_row();

            ui.label("Word count target:");
            ui.text_edit_singleline(&mut draft.word_count_target_text);
            ui.end_row();

            ui.label("Tags:");
            ui.text_edit_singleline(&mut draft.tags_text);
            ui.end_row();
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_meta_fills_in_blank_buffers_for_unset_fields() {
        let draft = MetadataDraft::from_meta(&DocumentMeta::default());
        assert_eq!(draft.section_type, "");
        assert_eq!(draft.status, "");
        assert_eq!(draft.pov, "");
        assert_eq!(draft.word_count_target_text, "");
        assert_eq!(draft.tags_text, "");
    }

    #[test]
    fn from_meta_and_to_meta_round_trip() {
        let meta = DocumentMeta {
            section_type: Some("Scene".to_string()),
            status: Some("draft".to_string()),
            pov: Some("Alice".to_string()),
            word_count_target: Some(2500),
            tags: vec!["foo".to_string(), "bar".to_string()],
        };
        let draft = MetadataDraft::from_meta(&meta);
        assert_eq!(draft.to_meta(), meta);
    }

    #[test]
    fn to_meta_treats_blank_fields_as_unset() {
        let draft = MetadataDraft {
            section_type: "  ".to_string(),
            status: String::new(),
            pov: String::new(),
            word_count_target_text: String::new(),
            tags_text: String::new(),
        };
        let meta = draft.to_meta();
        assert_eq!(meta.section_type, None);
        assert_eq!(meta.status, None);
        assert_eq!(meta.pov, None);
        assert_eq!(meta.word_count_target, None);
        assert!(meta.tags.is_empty());
    }

    #[test]
    fn to_meta_ignores_a_non_numeric_word_count_target_rather_than_erroring() {
        let draft = MetadataDraft {
            section_type: String::new(),
            status: String::new(),
            pov: String::new(),
            word_count_target_text: "not a number".to_string(),
            tags_text: String::new(),
        };
        assert_eq!(draft.to_meta().word_count_target, None);
    }

    #[test]
    fn to_meta_trims_and_filters_empty_tags() {
        let draft = MetadataDraft {
            section_type: String::new(),
            status: String::new(),
            pov: String::new(),
            word_count_target_text: String::new(),
            tags_text: " foo ,, bar ,".to_string(),
        };
        assert_eq!(draft.to_meta().tags, vec!["foo", "bar"]);
    }
}
