//! A form editor for a document's YAML frontmatter (`frontmatter::DocumentMeta`) — a
//! dockable, always-live tool window (Visual-Basic-Properties-window style) rather
//! than a raw YAML text box or a modal with an explicit Save step: the whole point is
//! that a user never has to hand-edit the `---` block, and edits here take effect as
//! soon as they're typed. `app.rs`'s `apply_metadata_edits_if_changed` is what
//! actually notices a change and writes it back into the open document's buffer —
//! this module only renders the fields and mutates the draft.

use crate::frontmatter::DocumentMeta;
use crate::project::Project;

/// Plain-text editing buffers for a `DocumentMeta`'s fields, owned by `app.rs` for as
/// long as the modal is open. Deriving `Default` gives exactly the same blank state
/// as `from_meta(&DocumentMeta::default())` — every field is a `String`, and
/// `DocumentMeta::default()` is all-`None`/empty, so `from_meta` would just fill
/// each field with `unwrap_or_default()`'s own empty string anyway.
#[derive(Default)]
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

/// The three fields' picklist options, bundled so `show` doesn't need a separate
/// parameter per field — each is a project's `PicklistField`-assigned folder's
/// document titles (see `Project::picklist_documents`), or empty if that field has
/// no folder assigned. Built fresh by the caller (`app.rs`) every frame.
pub struct MetadataPicklists<'a> {
    pub types: &'a [String],
    pub statuses: &'a [String],
    pub povs: &'a [String],
}

/// Renders the metadata form directly into `ui` (a dock tab's content area, not a
/// modal overlay). `open_path` is only used to show an empty-state message when no
/// document is open — `draft` itself is kept in sync with whatever document is open
/// by the caller (`app.rs`'s `refresh_metadata_if_needed`). Edits land directly in
/// `draft`'s buffers via the `text_edit_singleline` calls below; there's no Save/
/// Cancel here, since `apply_metadata_edits_if_changed` picks up any change after
/// this renders each frame.
///
/// `picklists` holds each field's assigned picklist folder's document titles (see
/// `Project::picklist_documents`/`PicklistField`), computed fresh by the caller each
/// frame. A field whose list is empty (no folder assigned to it, or the assigned
/// folder holds no documents yet) keeps that field exactly as free text, unchanged
/// from before this mechanism existed — only a non-empty list switches it to a
/// dropdown. The dropdown never clobbers an existing value that isn't one of the
/// options (e.g. typed before a folder was assigned, or since renamed away): it's
/// shown as-is via `selected_text` until the user actually picks a different entry.
///
/// `word_count` is `frontmatter::count_words` run over the open document's *live*
/// buffer (not necessarily saved yet), recomputed by the caller every frame — so it's
/// a read-only label, not another `draft` buffer, and stays current as the user types
/// with no extra plumbing beyond this whole panel already re-rendering each frame.
pub fn show(
    ui: &mut egui::Ui,
    open_path: Option<&std::path::Path>,
    draft: &mut MetadataDraft,
    picklists: &MetadataPicklists,
    word_count: usize,
) {
    ui.heading("Metadata");
    ui.separator();

    if open_path.is_none() {
        ui.label("Open a document to edit its metadata.");
        return;
    }

    egui::Grid::new("document_metadata_grid")
        .num_columns(2)
        .show(ui, |ui| {
            picklist_or_text_row(
                ui,
                "Type:",
                "metadata_type_combo",
                &mut draft.section_type,
                picklists.types,
            );
            picklist_or_text_row(
                ui,
                "Status:",
                "metadata_status_combo",
                &mut draft.status,
                picklists.statuses,
            );
            picklist_or_text_row(
                ui,
                "POV:",
                "metadata_pov_combo",
                &mut draft.pov,
                picklists.povs,
            );

            ui.label("Word count:");
            ui.label(word_count.to_string());
            ui.end_row();

            ui.label("Word count target:");
            ui.text_edit_singleline(&mut draft.word_count_target_text);
            ui.end_row();

            ui.label("Tags:");
            ui.text_edit_singleline(&mut draft.tags_text);
            ui.end_row();
        });
}

/// One `Type:`/`Status:`/`POV:`-style grid row: a free-text field when `options` is
/// empty, or a dropdown offering `options` plus a `"(none)"` clearing entry
/// otherwise. `combo_id` must be unique across the whole UI (egui's `ComboBox` id
/// requirement) — the three call sites above each pass their own literal.
fn picklist_or_text_row(
    ui: &mut egui::Ui,
    label: &str,
    combo_id: &str,
    value: &mut String,
    options: &[String],
) {
    ui.label(label);
    if options.is_empty() {
        ui.text_edit_singleline(value);
    } else {
        let selected_text = if value.is_empty() {
            "(none)"
        } else {
            value.as_str()
        };
        egui::ComboBox::new(combo_id, "")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                ui.selectable_value(value, String::new(), "(none)");
                for option in options {
                    ui.selectable_value(value, option.clone(), option);
                }
            });
    }
    ui.end_row();
}

/// Outcomes of editing the project-wide fields `show_project` renders — the same
/// "mutate a local copy, raise an event on change" pattern `corkboard_panel`'s
/// Desire/Misbelief fields use, rather than `MetadataDraft`'s buffer-diff
/// machinery: these are plain `ProjectMeta` fields with no document-frontmatter
/// text to round-trip through, so there's nothing for a draft to stay in sync
/// with beyond the project itself.
pub enum ProjectMetaEvent {
    SetTitle(String),
    SetSubtitle(String),
    SetAuthor(String),
    SetLogline(String),
    SetPoint(String),
    SetSynopsis(String),
    SetWhatIf(String),
}

/// Renders the project-wide metadata form shown in place of `show` (a
/// document's frontmatter) when the binder's root project row is selected
/// instead of a document — see `app::MetadataState::project_selected`. Title/
/// Author reuse `ProjectMeta::book_title`/`book_author` (the Export dialog's
/// own fields) rather than duplicating them under project-specific keys.
///
/// No `Grid` here (unlike `show`'s Type/Status/POV rows): a label-column-plus-
/// field-column layout would leave each field narrower than the tab, and every
/// field here is meant to fill it — so each is a label directly above a
/// full-width field instead. Logline/What if/Synopsis then evenly split
/// whatever vertical space is left after Title/Subtitle/Author/Point, rather
/// than Synopsis alone getting a fixed height while the other two stay
/// single-line. Point, like Title/Subtitle/Author before it, is always a
/// single line, so it's grouped with them rather than counted as a fourth
/// even share of the split.
pub fn show_project(ui: &mut egui::Ui, project: &Project) -> Option<ProjectMetaEvent> {
    let mut event = None;
    ui.heading("Project");
    ui.separator();

    let width = ui.available_width();

    ui.label("Title:");
    let mut title = project.meta.book_title.clone().unwrap_or_default();
    if ui
        .add(egui::TextEdit::singleline(&mut title).desired_width(width))
        .changed()
    {
        event = Some(ProjectMetaEvent::SetTitle(title));
    }

    ui.label("Subtitle:");
    let mut subtitle = project.meta.book_subtitle.clone().unwrap_or_default();
    if ui
        .add(egui::TextEdit::singleline(&mut subtitle).desired_width(width))
        .changed()
    {
        event = Some(ProjectMetaEvent::SetSubtitle(subtitle));
    }

    ui.label("Author:");
    let mut author = project.meta.book_author.clone().unwrap_or_default();
    if ui
        .add(egui::TextEdit::singleline(&mut author).desired_width(width))
        .changed()
    {
        event = Some(ProjectMetaEvent::SetAuthor(author));
    }

    ui.label("Point:");
    let mut point = project.meta.point.clone();
    if ui
        .add(egui::TextEdit::singleline(&mut point).desired_width(width))
        .changed()
    {
        event = Some(ProjectMetaEvent::SetPoint(point));
    }

    // Split whatever vertical space Title/Subtitle/Author/Point left behind
    // three ways for Logline/What if/Synopsis. `row_height` doubles as the
    // label line-height estimate and the multiline boxes' own row height,
    // since both render in the default `TextStyle::Body`.
    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let spacing = ui.spacing().item_spacing.y;
    let section_height = ((ui.available_height() - 3.0 * (row_height + spacing)) / 3.0).max(0.0);

    ui.label("Logline:");
    let mut logline = project.meta.logline.clone();
    if project_text_area(
        ui,
        "logline",
        &mut logline,
        width,
        section_height,
        row_height,
        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
    ) {
        event = Some(ProjectMetaEvent::SetLogline(logline));
    }

    ui.label("What if:");
    let mut what_if = project.meta.what_if.clone();
    if project_text_area(
        ui,
        "what_if",
        &mut what_if,
        width,
        section_height,
        row_height,
        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
    ) {
        event = Some(ProjectMetaEvent::SetWhatIf(what_if));
    }

    ui.label("Synopsis:");
    let mut synopsis = project.meta.synopsis.clone();
    // Always-visible (not just when overflowing, like Logline/What if above):
    // Synopsis is the field most likely to run past its box, so the scrollbar
    // stays a visible affordance rather than only appearing after the fact.
    if project_text_area(
        ui,
        "synopsis",
        &mut synopsis,
        width,
        section_height,
        row_height,
        egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
    ) {
        event = Some(ProjectMetaEvent::SetSynopsis(synopsis));
    }

    event
}

/// One Logline/What-if/Synopsis box: a fixed-`height`, always-scrollable
/// region (rather than letting the `TextEdit` grow with its content, the way
/// `show`'s old single `Synopsis` field used to) so the three boxes stay
/// exactly evenly split regardless of how much text is in any one of them.
/// `desired_rows` is sized to `height`, not left at `TextEdit`'s own default,
/// for the same reason `editor_panel::show` sizes the main editor's: the
/// widget's actual clickable area needs to match what it visually looks like
/// it covers, all the way to the bottom of its third of the panel.
fn project_text_area(
    ui: &mut egui::Ui,
    id_salt: &str,
    value: &mut String,
    width: f32,
    height: f32,
    row_height: f32,
    scroll_bar_visibility: egui::scroll_area::ScrollBarVisibility,
) -> bool {
    let desired_rows = ((height / row_height).floor() as usize).max(1);
    let mut changed = false;
    ui.allocate_ui(egui::vec2(width, height), |ui| {
        egui::ScrollArea::vertical()
            .id_salt(id_salt)
            .auto_shrink([false, false])
            .scroll_bar_visibility(scroll_bar_visibility)
            .show(ui, |ui| {
                changed = ui
                    .add(
                        egui::TextEdit::multiline(value)
                            .desired_width(f32::INFINITY)
                            .desired_rows(desired_rows),
                    )
                    .changed();
            });
    });
    changed
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
