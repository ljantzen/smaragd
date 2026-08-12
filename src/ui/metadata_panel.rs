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
    /// Comma-separated, matching the story card editor's `subplot_tags_text`. The
    /// actual stored value `to_meta()`/`from_meta` round-trip through — `tags_chip_editor`
    /// renders it as removable chips rather than a raw text box, but never lets the
    /// user edit this buffer directly.
    pub tags_text: String,
    /// The chip editor's transient "typing a new tag" buffer — not itself part of
    /// `tags_text`/`DocumentMeta`, so it's not touched by `from_meta`/`to_meta`.
    /// Naturally resets to empty along with the rest of the draft whenever the open
    /// document/folder changes, the same way a half-finished edit anywhere else in
    /// this form is abandoned rather than carried over.
    pub tag_input: String,
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
            tag_input: String::new(),
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
            tags: parse_tags_text(&self.tags_text),
        }
    }
}

fn non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Split `tags_text` (`MetadataDraft`'s comma-separated storage format) into its
/// individual tags — trimmed, with empty entries (from a stray/doubled/trailing
/// comma) dropped. Shared by `to_meta` and `tags_chip_editor`'s chip rendering, so
/// both agree on exactly what counts as "one tag".
fn parse_tags_text(tags_text: &str) -> Vec<String> {
    tags_text
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Append `candidate` to `tags_text` as a new chip, unless a case-insensitively
/// matching tag is already present (in which case `tags_text` comes back unchanged)
/// — the logic behind committing a typed or suggestion-clicked tag in
/// `tags_chip_editor`, pulled out pure so it's unit-testable without an egui
/// context. A blank `candidate` (or one that's only whitespace) is a no-op.
fn add_tag(tags_text: &str, candidate: &str) -> String {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return tags_text.to_string();
    }
    let mut tags = parse_tags_text(tags_text);
    if tags.iter().any(|tag| tag.eq_ignore_ascii_case(candidate)) {
        return tags_text.to_string();
    }
    tags.push(candidate.to_string());
    tags.join(", ")
}

/// Remove the tag at `index` (as [`parse_tags_text`] would enumerate them) from
/// `tags_text` — the logic behind a chip's "×" button, pulled out pure for the same
/// reason as `add_tag`. An out-of-range `index` is a no-op.
fn remove_tag(tags_text: &str, index: usize) -> String {
    let mut tags = parse_tags_text(tags_text);
    if index < tags.len() {
        tags.remove(index);
    }
    tags.join(", ")
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
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    open_path: Option<&std::path::Path>,
    draft: &mut MetadataDraft,
    picklists: &MetadataPicklists,
    word_count: usize,
    status_color: Option<egui::Color32>,
    pov_color: Option<egui::Color32>,
    known_tags: &[String],
) -> Option<MetadataFormEvent> {
    ui.heading("Metadata");
    ui.separator();

    if open_path.is_none() {
        ui.label("Open a document to edit its metadata.");
        return None;
    }

    metadata_fields_grid(
        ui,
        "document_metadata_grid",
        draft,
        picklists,
        status_color,
        pov_color,
        Some(word_count),
        known_tags,
    )
}

/// The Type/Status/POV/Word-Count-Target/Tags `egui::Grid` body shared by
/// `show` (a document's frontmatter, which also shows a live "Word count:"
/// readout — `word_count: Some(_)`) and `show_folder` (a folder's own
/// metadata, which has no single open buffer to compute one from —
/// `word_count: None`). `id_salt` must be unique per caller (egui's `Grid`/
/// `ComboBox` id requirement), so each passes its own literal.
#[allow(clippy::too_many_arguments)]
fn metadata_fields_grid(
    ui: &mut egui::Ui,
    id_salt: &str,
    draft: &mut MetadataDraft,
    picklists: &MetadataPicklists,
    status_color: Option<egui::Color32>,
    pov_color: Option<egui::Color32>,
    word_count: Option<usize>,
    known_tags: &[String],
) -> Option<MetadataFormEvent> {
    let mut event = None;
    egui::Grid::new(id_salt).num_columns(2).show(ui, |ui| {
        picklist_or_text_row(
            ui,
            "Type:",
            &format!("{id_salt}_type_combo"),
            &mut draft.section_type,
            picklists.types,
        );
        event = status_row(
            ui,
            &format!("{id_salt}_status_combo"),
            &mut draft.status,
            picklists.statuses,
            status_color,
        );
        // Only one of the two swatches can be clicked in a given frame, so
        // overwriting `event` here whenever `pov_row` fires safely combines
        // whichever one (if either) did — there's never a real collision.
        if let Some(pov_event) = pov_row(
            ui,
            &format!("{id_salt}_pov_combo"),
            &mut draft.pov,
            picklists.povs,
            pov_color,
        ) {
            event = Some(pov_event);
        }

        if let Some(word_count) = word_count {
            ui.label("Word count:");
            ui.label(word_count.to_string());
            ui.end_row();
        }

        ui.label("Word count target:");
        ui.text_edit_singleline(&mut draft.word_count_target_text);
        ui.end_row();

        ui.label("Tags:");
        tags_chip_editor(
            ui,
            id_salt,
            &mut draft.tags_text,
            &mut draft.tag_input,
            known_tags,
        );
        ui.end_row();
    });
    event
}

/// Renders `tags_text` (see `MetadataDraft::tags_text`) as a row of removable chips
/// plus a free-text input for adding a new one — Obsidian-style, replacing what used
/// to be a bare comma-separated text box (#31). `tag_input` is the transient
/// "typing a new tag" buffer (see its doc comment on `MetadataDraft`); `known_tags`
/// (`Project::all_tags`) drives a prefix-matched suggestion list shown under the
/// input while typing, via the same `autocomplete::filter_candidates` logic behind
/// the Editor's own `#tag` autocomplete popup and the `:tag` command's completion.
///
/// Typing a comma commits everything before it as chips (so pasting
/// `"foo, bar, baz"` in one go produces three chips, not one long tag), leaving only
/// the text after the last comma as the still-in-progress `tag_input`; Enter (while
/// the input has focus) or clicking a suggestion commits the current `tag_input` as
/// one more chip. A candidate that already matches an existing tag
/// case-insensitively is silently ignored rather than added as a duplicate (see
/// `add_tag`).
fn tags_chip_editor(
    ui: &mut egui::Ui,
    id_salt: &str,
    tags_text: &mut String,
    tag_input: &mut String,
    known_tags: &[String],
) {
    ui.vertical(|ui| {
        let tags = parse_tags_text(tags_text);
        if !tags.is_empty() {
            ui.horizontal_wrapped(|ui| {
                let mut remove_index = None;
                for (index, tag) in tags.iter().enumerate() {
                    egui::Frame::group(ui.style())
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(format!("#{tag}"));
                                if ui.small_button("×").clicked() {
                                    remove_index = Some(index);
                                }
                            });
                        });
                }
                if let Some(index) = remove_index {
                    *tags_text = remove_tag(tags_text, index);
                }
            });
        }

        let response = ui.add(
            egui::TextEdit::singleline(tag_input)
                .hint_text("Add tag…")
                .id_salt(format!("{id_salt}_tag_input")),
        );

        // A comma commits every complete tag typed/pasted before it, leaving only
        // the still-in-progress fragment after the last comma as the new `tag_input`.
        if tag_input.contains(',') {
            let mut parts: Vec<String> = tag_input.split(',').map(str::to_string).collect();
            let remainder = parts.pop().unwrap_or_default();
            for part in parts {
                *tags_text = add_tag(tags_text, part.trim());
            }
            *tag_input = remainder;
        }

        let mut commit: Option<String> = None;
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            commit = Some(tag_input.clone());
        }

        let query = tag_input.trim();
        if !query.is_empty() {
            let suggestions: Vec<&str> = crate::autocomplete::filter_candidates(known_tags, query)
                .into_iter()
                .take(8)
                .collect();
            if !suggestions.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for suggestion in suggestions {
                        if ui.small_button(suggestion).clicked() {
                            commit = Some(suggestion.to_string());
                        }
                    }
                });
            }
        }

        if let Some(candidate) = commit {
            *tags_text = add_tag(tags_text, &candidate);
            tag_input.clear();
        }
    });
}

/// Renders a folder's own metadata form — structurally identical to `show`
/// minus the "Word count:" live readout (a folder has no single open buffer
/// to compute one from, unlike a document; `word_count_target` itself is
/// still kept, only the live count is dropped). Shown in place of `show` when
/// the binder's non-root folder row is selected — see
/// `app::MetadataState::target`/`ui::binder_panel::BinderEvent::SelectFolder`.
pub fn show_folder(
    ui: &mut egui::Ui,
    draft: &mut MetadataDraft,
    picklists: &MetadataPicklists,
    status_color: Option<egui::Color32>,
    pov_color: Option<egui::Color32>,
    known_tags: &[String],
) -> Option<MetadataFormEvent> {
    ui.heading("Folder Metadata");
    ui.separator();
    metadata_fields_grid(
        ui,
        "folder_metadata_grid",
        draft,
        picklists,
        status_color,
        pov_color,
        None,
        known_tags,
    )
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

/// Outcomes of editing a Status field's color swatch — see `status_row`.
/// Separate from `ProjectMetaEvent`/`MetadataDraft`'s own buffer-diff
/// mechanism: a color assignment lives on `Project::meta.status_colors`
/// directly (keyed by status text, not by document/folder path), with no
/// draft of its own to round-trip through — raised the moment the swatch is
/// used, same as `ProjectMetaEvent`.
pub enum MetadataFormEvent {
    SetStatusColor {
        status: String,
        color: egui::Color32,
    },
    SetPovColor {
        pov: String,
        color: egui::Color32,
    },
}

/// The "Status:" row: the same free-text-vs-dropdown toggle
/// `picklist_or_text_row` gives Type/POV, plus a small color swatch button
/// (disabled when the field is blank — there'd be nothing to key a color by)
/// that opens a popup to assign/edit whatever status is currently typed or
/// selected. Its own function rather than a `picklist_or_text_row` call
/// because that one unconditionally ends the row after exactly two widgets;
/// this needs a third.
fn status_row(
    ui: &mut egui::Ui,
    combo_id: &str,
    value: &mut String,
    options: &[String],
    current_color: Option<egui::Color32>,
) -> Option<MetadataFormEvent> {
    ui.label("Status:");
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

    let mut event = None;
    let status = value.trim().to_string();
    ui.add_enabled_ui(!status.is_empty(), |ui| {
        let mut color = current_color.unwrap_or(ui.visuals().weak_text_color());
        if egui::color_picker::color_edit_button_srgba(
            ui,
            &mut color,
            egui::color_picker::Alpha::Opaque,
        )
        .changed()
        {
            event = Some(MetadataFormEvent::SetStatusColor { status, color });
        }
    });
    ui.end_row();
    event
}

/// The "POV:" row — an exact structural mirror of `status_row`, just keyed by
/// `ProjectMeta::pov_colors` instead of `status_colors` and raising
/// `SetPovColor` instead of `SetStatusColor`. Kept as its own function rather
/// than parameterizing `status_row` over the field/label/event-constructor:
/// the two are small and simple enough that a shared abstraction would cost
/// more to read than the duplication it removes.
fn pov_row(
    ui: &mut egui::Ui,
    combo_id: &str,
    value: &mut String,
    options: &[String],
    current_color: Option<egui::Color32>,
) -> Option<MetadataFormEvent> {
    ui.label("POV:");
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

    let mut event = None;
    let pov = value.trim().to_string();
    ui.add_enabled_ui(!pov.is_empty(), |ui| {
        let mut color = current_color.unwrap_or(ui.visuals().weak_text_color());
        if egui::color_picker::color_edit_button_srgba(
            ui,
            &mut color,
            egui::color_picker::Alpha::Opaque,
        )
        .changed()
        {
            event = Some(MetadataFormEvent::SetPovColor { pov, color });
        }
    });
    ui.end_row();
    event
}

/// Outcomes of editing the project-wide fields `show_project` renders — the same
/// "mutate a local copy, raise an event on change" pattern `corkboard_panel`'s
/// Desire/Misbelief fields use, rather than `MetadataDraft`'s buffer-diff
/// machinery: these are plain `ProjectMeta` fields with no document-frontmatter
/// text to round-trip through, so there's nothing for a draft to stay in sync
/// with beyond the project itself.
/// Extra vertical gap before every project-metadata field's label except the
/// very first (Title) — separates one label/field pair from the next a bit
/// more than `Ui`'s default `item_spacing.y` alone does, without needing a
/// full `ui.separator()` line between each.
const LEADING_GAP: f32 = 6.0;

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

    ui.label("Title");
    let mut title = project.meta.book_title.clone().unwrap_or_default();
    if ui.add(project_text_field(&mut title, width)).changed() {
        event = Some(ProjectMetaEvent::SetTitle(title));
    }

    ui.add_space(LEADING_GAP);
    ui.label("Subtitle");
    let mut subtitle = project.meta.book_subtitle.clone().unwrap_or_default();
    if ui.add(project_text_field(&mut subtitle, width)).changed() {
        event = Some(ProjectMetaEvent::SetSubtitle(subtitle));
    }

    ui.add_space(LEADING_GAP);
    ui.label("Author");
    let mut author = project.meta.book_author.clone().unwrap_or_default();
    if ui.add(project_text_field(&mut author, width)).changed() {
        event = Some(ProjectMetaEvent::SetAuthor(author));
    }

    ui.add_space(LEADING_GAP);
    ui.label("Point");
    let mut point = project.meta.point.clone();
    if ui.add(project_text_field(&mut point, width)).changed() {
        event = Some(ProjectMetaEvent::SetPoint(point));
    }

    // Split whatever vertical space Title/Subtitle/Author/Point left behind
    // three ways for Logline/What if/Synopsis. `row_height` doubles as the
    // label line-height estimate and the multiline boxes' own row height,
    // since both render in the default `TextStyle::Body`; `LEADING_GAP` is
    // subtracted too since each of the three still gets its own leading gap
    // below, which otherwise wouldn't be accounted for and would push
    // Synopsis's box past the bottom of the tab.
    let row_height = ui.text_style_height(&egui::TextStyle::Body);
    let spacing = ui.spacing().item_spacing.y;
    let section_height =
        ((ui.available_height() - 3.0 * (row_height + spacing + LEADING_GAP)) / 3.0).max(0.0);

    ui.add_space(LEADING_GAP);
    ui.label("Logline");
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

    ui.add_space(LEADING_GAP);
    ui.label("What if");
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

    ui.add_space(LEADING_GAP);
    ui.label("Synopsis");
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

/// A single-line Title/Subtitle/Author/Point field, flush against the left
/// edge of the label above it. `TextEdit`'s default inner margin
/// (`Margin::symmetric(4, 2)`) otherwise indents the displayed text a few
/// pixels right of the label's own left edge, reading as a misalignment
/// between header and field even though both widgets start at the same `x`
/// — zeroing just the horizontal margin (keeping the vertical one, so the
/// field's height/click area is unchanged) flushes the two to match.
fn project_text_field(value: &mut String, width: f32) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(value)
        .desired_width(width)
        .margin(egui::Margin::symmetric(0, 2))
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
                            .desired_rows(desired_rows)
                            // Same flush-left-margin reasoning as
                            // `project_text_field` — keeps Logline/What
                            // if/Synopsis's text aligned with their labels.
                            .margin(egui::Margin::symmetric(0, 2)),
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
            tag_input: String::new(),
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
            tag_input: String::new(),
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
            tag_input: String::new(),
        };
        assert_eq!(draft.to_meta().tags, vec!["foo", "bar"]);
    }

    #[test]
    fn add_tag_appends_a_new_chip_to_an_existing_list() {
        assert_eq!(add_tag("foo, bar", "baz"), "foo, bar, baz");
    }

    #[test]
    fn add_tag_to_an_empty_list_produces_a_single_chip() {
        assert_eq!(add_tag("", "foo"), "foo");
    }

    #[test]
    fn add_tag_trims_the_candidate() {
        assert_eq!(add_tag("foo", "  bar  "), "foo, bar");
    }

    #[test]
    fn add_tag_ignores_a_case_insensitive_duplicate() {
        assert_eq!(add_tag("foo, Bar", "bar"), "foo, Bar");
    }

    #[test]
    fn add_tag_is_a_noop_for_a_blank_candidate() {
        assert_eq!(add_tag("foo", "   "), "foo");
    }

    #[test]
    fn remove_tag_drops_the_chip_at_the_given_index() {
        assert_eq!(remove_tag("foo, bar, baz", 1), "foo, baz");
    }

    #[test]
    fn remove_tag_at_the_first_index_drops_the_first_chip() {
        assert_eq!(remove_tag("foo, bar", 0), "bar");
    }

    #[test]
    fn remove_tag_with_an_out_of_range_index_is_a_noop() {
        assert_eq!(remove_tag("foo, bar", 5), "foo, bar");
    }

    #[test]
    fn remove_tag_the_only_entry_leaves_an_empty_string() {
        assert_eq!(remove_tag("foo", 0), "");
    }

    #[test]
    fn parse_tags_text_drops_empty_entries_from_stray_commas() {
        assert_eq!(
            parse_tags_text(" foo ,, bar ,"),
            vec!["foo".to_string(), "bar".to_string()]
        );
    }
}
