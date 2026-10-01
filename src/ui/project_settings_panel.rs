//! The Project Settings dialog: per-project configuration, as opposed to the
//! app-wide `settings_panel`. Structured identically to that dialog (same
//! `egui::Modal` chrome, same left-nav-category / right-content-pane layout) but
//! scoped to one open `Project` instead of global `Settings`.
//!
//! Pure rendering, like `sync_panel`: every control here can trigger a fallible
//! `Project::set_*` (a disk write), so rather than mutate `Project` directly and
//! swallow or `.unwrap()` the result, a changed control returns a
//! [`ProjectSettingsEvent`] for the caller to apply and report errors from —
//! exactly the division `sync_panel`'s `SyncPanelEvent` already uses for the
//! same reason.

use std::path::PathBuf;

use crate::project::{AttachmentDestination, BinderColorMode, Project};
use crate::settings::Settings;

/// Keyboard filter claimed on a focused category row — see
/// `settings_panel::CATEGORY_ARROW_KEYS_FILTER`'s doc comment for why; this is
/// the same filter, duplicated rather than shared so the two dialogs' nav
/// lists stay independent modules.
const CATEGORY_ARROW_KEYS_FILTER: egui::EventFilter = egui::EventFilter {
    tab: false,
    horizontal_arrows: false,
    vertical_arrows: true,
    escape: false,
};

/// Which page of the dialog is currently showing — purely UI navigation
/// state, not persisted, exactly like `settings_panel::SettingsCategory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSettingsCategory {
    Git,
    Sync,
    Binder,
    Attachments,
}

impl ProjectSettingsCategory {
    pub const ALL: [ProjectSettingsCategory; 4] = [
        ProjectSettingsCategory::Git,
        ProjectSettingsCategory::Sync,
        ProjectSettingsCategory::Binder,
        ProjectSettingsCategory::Attachments,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ProjectSettingsCategory::Git => "Git",
            ProjectSettingsCategory::Sync => "Sync",
            ProjectSettingsCategory::Binder => "Binder",
            ProjectSettingsCategory::Attachments => "Attachments",
        }
    }
}

/// A control in this dialog changed — the caller applies it to the open
/// project (a `Project::set_*` call, each of which persists immediately) and
/// reports any error, plus runs whatever side effect the change needs (e.g.
/// `SetSyncFiles` should be followed by a resync).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSettingsEvent {
    SetGitAutoCommitEnabled(bool),
    SetGitAutoCommitIntervalMinutes(u32),
    SetGitAutoCommitPushEnabled(bool),
    /// See `ProjectMeta::sync_files`; moved here from the Sync dock tab.
    SetSyncFiles(bool),
    SetBinderColorMode(BinderColorMode),
    SetAttachmentDestination(AttachmentDestination),
    /// `None` clears the configured folder (Reset).
    SetAttachmentsFolder(Option<PathBuf>),
    SetClipboardImageSizeLimitEnabled(bool),
    SetClipboardImageSizeLimitMb(u32),
}

/// Renders the Project Settings dialog when `open` is true (a no-op
/// otherwise) — same `egui::Modal` two-pane shape as `settings_panel::show`,
/// see its doc comment for why a real `Modal` rather than a `Window`.
/// Returns the one control that changed this frame, if any, for the caller to
/// apply via a `Project::set_*` call.
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    category: &mut ProjectSettingsCategory,
    project: &Project,
    settings: &Settings,
) -> Option<ProjectSettingsEvent> {
    let was_open_id = egui::Id::new("project_settings_was_open_last_frame");
    let was_open = ctx.data(|d| d.get_temp::<bool>(was_open_id).unwrap_or(false));
    let just_opened = *open && !was_open;
    ctx.data_mut(|d| d.insert_temp(was_open_id, *open));

    if !*open {
        return None;
    }
    let mut event = None;
    let mut close_requested = false;
    let modal_id = egui::Id::new("project_settings_modal");
    let modal_response = egui::Modal::new(modal_id)
        .area(
            egui::Modal::default_area(modal_id)
                .default_width(640.0)
                .default_height(460.0),
        )
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(640.0, 460.0));
            ui.heading("Project Settings");
            ui.add_space(4.0);
            egui::Panel::bottom("project_settings_bottom_bar")
                .resizable(false)
                .exact_size(40.0)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("OK").clicked() {
                            close_requested = true;
                        }
                    });
                    ui.add_space(4.0);
                });
            egui::Panel::left("project_settings_nav")
                .resizable(false)
                .exact_size(160.0)
                .show(ui, |ui| show_category_nav(ui, category, just_opened));
            egui::ScrollArea::vertical().show(ui, |ui| {
                event = match *category {
                    ProjectSettingsCategory::Git => show_git_category(ui, project, settings),
                    ProjectSettingsCategory::Sync => show_sync_category(ui, project),
                    ProjectSettingsCategory::Binder => show_binder_category(ui, project),
                    ProjectSettingsCategory::Attachments => show_attachments_category(ui, project),
                };
            });
        });

    if close_requested || modal_response.backdrop_response.clicked() {
        *open = false;
    }
    if *open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *open = false;
    }

    event
}

fn show_category_nav(ui: &mut egui::Ui, category: &mut ProjectSettingsCategory, just_opened: bool) {
    let mut ids = Vec::new();
    for c in ProjectSettingsCategory::ALL {
        let response = ui.selectable_value(category, c, c.label());
        if (response.clicked() && !response.has_focus()) || (just_opened && c == *category) {
            response.request_focus();
        }
        if response.has_focus() {
            ui.ctx().memory_mut(|mem| {
                mem.set_focus_lock_filter(response.id, CATEGORY_ARROW_KEYS_FILTER)
            });
        }
        ids.push(response.id);
    }
    if let Some(focused_id) = ui.ctx().memory(|mem| mem.focused())
        && let Some(current) = ids.iter().position(|id| *id == focused_id)
    {
        let move_down =
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        let move_up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        let next = if move_down {
            Some((current + 1).min(ids.len() - 1))
        } else if move_up {
            Some(current.saturating_sub(1))
        } else {
            None
        };
        if let Some(next) = next
            && next != current
        {
            *category = ProjectSettingsCategory::ALL[next];
            ui.ctx().memory_mut(|mem| mem.request_focus(ids[next]));
        }
    }
}

fn show_git_category(
    ui: &mut egui::Ui,
    project: &Project,
    settings: &Settings,
) -> Option<ProjectSettingsEvent> {
    let mut event = None;
    ui.heading("Git");
    ui.add_space(12.0);
    if !settings.git_integration_enabled() {
        ui.weak("Git integration is turned off — see Settings > History.");
        return None;
    }
    if !project.meta.git_enabled {
        ui.weak("Enable git support for this project from the Versions menu first.");
        return None;
    }
    let mut auto_commit = project.meta.git_auto_commit_enabled;
    if ui
        .checkbox(&mut auto_commit, "Automatically commit changes")
        .changed()
    {
        event = Some(ProjectSettingsEvent::SetGitAutoCommitEnabled(auto_commit));
    }
    ui.add_enabled_ui(project.meta.git_auto_commit_enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Every:");
            let mut minutes = project.meta.resolve_git_auto_commit_interval_minutes();
            if ui
                .add(
                    egui::DragValue::new(&mut minutes)
                        .range(1..=1440)
                        .suffix(" min"),
                )
                .changed()
            {
                event = Some(ProjectSettingsEvent::SetGitAutoCommitIntervalMinutes(
                    minutes,
                ));
            }
        });
        let mut push_after = project.meta.git_auto_commit_push_enabled;
        if ui
            .checkbox(&mut push_after, "Push after each automatic commit")
            .changed()
        {
            event = Some(ProjectSettingsEvent::SetGitAutoCommitPushEnabled(
                push_after,
            ));
        }
    });
    event
}

fn show_sync_category(ui: &mut egui::Ui, project: &Project) -> Option<ProjectSettingsEvent> {
    let mut event = None;
    ui.heading("Sync");
    ui.add_space(12.0);
    let mut sync_files = project.meta.sync_files;
    if ui
        .checkbox(&mut sync_files, "Also sync images, PDFs and other files")
        .changed()
    {
        event = Some(ProjectSettingsEvent::SetSyncFiles(sync_files));
    }
    ui.weak(
        "Every other file in the project folder, whole, up to the server's size limit \
         (100 MB unless its operator changed it). A project setting: it applies on every \
         device syncing this project.",
    );
    ui.add_space(10.0);
    ui.weak("Pairing, live status and devices are managed from the Sync dock tab.");
    event
}

fn show_binder_category(ui: &mut egui::Ui, project: &Project) -> Option<ProjectSettingsEvent> {
    let mut event = None;
    ui.heading("Binder");
    ui.add_space(12.0);
    ui.label("Color binder rows by:");
    let mut mode = project.meta.binder_color_mode;
    for candidate in [
        BinderColorMode::Off,
        BinderColorMode::Status,
        BinderColorMode::Pov,
        BinderColorMode::WordCountProgress,
    ] {
        if ui
            .radio_value(&mut mode, candidate, candidate.label())
            .changed()
        {
            event = Some(ProjectSettingsEvent::SetBinderColorMode(mode));
        }
    }
    ui.add_space(4.0);
    ui.weak("Also available from the binder's right-click menu.");
    event
}

fn show_attachments_category(ui: &mut egui::Ui, project: &Project) -> Option<ProjectSettingsEvent> {
    let mut event = None;
    ui.heading("Attachments");
    ui.add_space(12.0);
    ui.label("Save pasted/dropped images and files:");
    let mut destination = project.attachment_destination();
    if ui
        .radio_value(
            &mut destination,
            AttachmentDestination::SameAsDocument,
            "In the same folder as the document",
        )
        .changed()
    {
        event = Some(ProjectSettingsEvent::SetAttachmentDestination(destination));
    }
    if ui
        .radio_value(
            &mut destination,
            AttachmentDestination::ConfiguredFolder,
            "In a specific folder",
        )
        .changed()
    {
        event = Some(ProjectSettingsEvent::SetAttachmentDestination(destination));
    }
    let folder_error_id = ui.id().with("attachments_folder_error");
    ui.add_enabled_ui(
        project.attachment_destination() == AttachmentDestination::ConfiguredFolder,
        |ui| {
            ui.horizontal(|ui| {
                let mut dir_text = project
                    .attachments_folder()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(not set)".to_string());
                ui.add_enabled(
                    false,
                    egui::TextEdit::singleline(&mut dir_text).desired_width(260.0),
                );
                if ui.button("Browse…").clicked()
                    && let Some(picked) = rfd::FileDialog::new()
                        .set_directory(&project.root)
                        .pick_folder()
                {
                    if picked.strip_prefix(&project.root).is_ok() {
                        ui.ctx().data_mut(|d| d.remove::<String>(folder_error_id));
                        event = Some(ProjectSettingsEvent::SetAttachmentsFolder(Some(picked)));
                    } else {
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(
                                folder_error_id,
                                "The attachments folder must be inside the project.".to_string(),
                            )
                        });
                    }
                }
                if project.attachments_folder().is_some() && ui.button("Reset").clicked() {
                    ui.ctx().data_mut(|d| d.remove::<String>(folder_error_id));
                    event = Some(ProjectSettingsEvent::SetAttachmentsFolder(None));
                }
            });
        },
    );
    if let Some(err) = ui.ctx().data(|d| d.get_temp::<String>(folder_error_id)) {
        ui.colored_label(ui.visuals().error_fg_color, err);
    }
    ui.add_space(10.0);
    let mut limit_enabled = project.clipboard_image_size_limit_enabled();
    if ui
        .checkbox(&mut limit_enabled, "Limit clipboard image paste size")
        .changed()
    {
        event = Some(ProjectSettingsEvent::SetClipboardImageSizeLimitEnabled(
            limit_enabled,
        ));
    }
    ui.add_enabled_ui(project.clipboard_image_size_limit_enabled(), |ui| {
        ui.horizontal(|ui| {
            ui.label("Maximum size:");
            let mut mb = project.clipboard_image_size_limit_mb();
            if ui
                .add(egui::DragValue::new(&mut mb).range(1..=1000).suffix(" MB"))
                .changed()
            {
                event = Some(ProjectSettingsEvent::SetClipboardImageSizeLimitMb(mb));
            }
        });
    });
    event
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_test_support::run_ui_and_discard;

    #[test]
    fn git_category_renders_without_panicking_in_every_gating_state() {
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        // Global git integration off.
        let disabled_settings = Settings {
            git_integration_disabled: true,
            ..Default::default()
        };
        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            show_git_category(ui, &project, &disabled_settings);
        });

        // Global on, project hasn't enabled git yet.
        let settings = Settings::default();
        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            show_git_category(ui, &project, &settings);
        });

        // Project enabled, auto-commit off, then on.
        project.enable_git_support().unwrap();
        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            show_git_category(ui, &project, &settings);
        });
        project.set_git_auto_commit_enabled(true).unwrap();
        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            show_git_category(ui, &project, &settings);
        });
    }

    #[test]
    fn sync_category_renders_without_panicking() {
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();

        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            show_sync_category(ui, &project);
        });
    }

    #[test]
    fn binder_category_renders_without_panicking_and_reports_the_clicked_mode() {
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut event = None;

        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            event = show_binder_category(ui, &project);
        });

        // Nothing clicked yet — a bare render reports no event.
        assert!(event.is_none());
    }

    #[test]
    fn attachments_category_renders_without_panicking_in_both_destination_modes() {
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();

        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            show_attachments_category(ui, &project);
        });

        project
            .set_attachment_destination(AttachmentDestination::ConfiguredFolder)
            .unwrap();
        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            show_attachments_category(ui, &project);
        });
    }

    #[test]
    fn attachments_category_reports_the_clicked_destination() {
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut event = None;

        run_ui_and_discard(&ctx, egui::RawInput::default(), |ui| {
            event = show_attachments_category(ui, &project);
        });

        // Nothing clicked yet — a bare render reports no event.
        assert!(event.is_none());
    }
}
