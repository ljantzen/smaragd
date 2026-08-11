use super::*;

/// What a `NamePromptState` modal should do with the name once confirmed.
pub(super) enum PromptAction {
    NewFile {
        parent: PathBuf,
    },
    NewFolder {
        parent: PathBuf,
    },
    NewFileFromTemplate {
        parent: PathBuf,
        template_path: PathBuf,
    },
    Rename {
        path: PathBuf,
    },
    NewProject {
        location: PathBuf,
        template_id: String,
    },
    /// Commit with the (editable) message the prompt was confirmed with; `push_after`
    /// carries through whether this was "Commit" or "Commit and Push".
    GitCommit {
        push_after: bool,
    },
    /// Save the current dock layout under the confirmed name (see
    /// `save_named_layout`).
    SaveLayout,
    /// Join a collaboration session using the pasted connection code (see
    /// `start_collab_join`).
    JoinCollabSession,
    /// Save the current project's structure as a new custom template under the
    /// confirmed name (see `save_project_as_template`).
    SaveProjectAsTemplate,
}

pub(super) struct PendingPrompt {
    pub(super) action: PromptAction,
    pub(super) state: NamePromptState,
}

impl SmaragdApp {
    /// Open the "New File" name-prompt modal for a file to be created inside
    /// `parent`. Shared by the binder's right-click menu and the New File keyboard
    /// shortcut.
    pub(super) fn prompt_new_file(&mut self, parent: PathBuf) {
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewFile { parent },
            state: NamePromptState::new("New File", "Create", ""),
        });
    }

    /// Open the "New Folder" name-prompt modal for a folder to be created inside
    /// `parent`. Shared by the binder's right-click menu and the New Folder keyboard
    /// shortcut.
    pub(super) fn prompt_new_folder(&mut self, parent: PathBuf) {
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewFolder { parent },
            state: NamePromptState::new("New Folder", "Create", ""),
        });
    }

    /// Open the "New From Template" name-prompt modal for a document to be created
    /// inside `parent`, copying `template_path`'s content — pre-filled with the
    /// template's own stem, same as `prompt_rename` pre-fills from the renamed
    /// item's current name.
    pub(super) fn prompt_new_file_from_template(
        &mut self,
        parent: PathBuf,
        template_path: PathBuf,
    ) {
        let name = template_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        self.prompt = Some(PendingPrompt {
            action: PromptAction::NewFileFromTemplate {
                parent,
                template_path,
            },
            state: NamePromptState::new("New From Template", "Create", name),
        });
    }

    /// Open the "Rename" name-prompt modal, pre-filled with `path`'s current stem.
    /// Shared by the binder's right-click menu and the Rename keyboard shortcut.
    pub(super) fn prompt_rename(&mut self, path: PathBuf) {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        self.prompt = Some(PendingPrompt {
            action: PromptAction::Rename { path },
            state: NamePromptState::new("Rename", "Rename", name),
        });
    }

    /// New File/Folder triggered by keyboard shortcut: targets the currently
    /// selected document's parent folder, or the project root if nothing's
    /// selected. No-op if no project is open (rather than popping up a modal that
    /// would silently go nowhere on confirm).
    pub(super) fn keyboard_new_file(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        let parent = self
            .selected_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| project.root.clone());
        self.prompt_new_file(parent);
    }

    /// See `keyboard_new_file`.
    pub(super) fn keyboard_new_folder(&mut self) {
        let Some(project) = &self.project else {
            return;
        };
        let parent = self
            .selected_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| project.root.clone());
        self.prompt_new_folder(parent);
    }

    /// Move a trashed item back to the folder it was deleted from. If that folder no
    /// longer exists, offers via a native Yes/No dialog to recreate it — matching
    /// `open_project_or_offer_to_adopt`'s "try, then offer, then retry" shape —
    /// leaving the item in Trash if declined.
    pub(super) fn restore_node(&mut self, path: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.restore_from_trash(path, false) {
            Ok(_) => self.document_status_cache.clear(),
            Err(RestoreError::OriginalFolderMissing(folder)) => {
                let recreate = rfd::MessageDialog::new()
                    .set_title("Restore")
                    .set_description(format!(
                        "\"{}\" no longer exists. Recreate it and restore here?",
                        folder.display()
                    ))
                    .set_level(rfd::MessageLevel::Info)
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show();
                if recreate == rfd::MessageDialogResult::Yes
                    && let Some(project) = &mut self.project
                {
                    match project.restore_from_trash(path, true) {
                        Ok(_) => self.document_status_cache.clear(),
                        Err(err) => {
                            self.push_error_toast(format!("Couldn't restore: {err}"));
                        }
                    }
                }
            }
            Err(err) => self.push_error_toast(format!("Couldn't restore: {err}")),
        }
    }

    pub(super) fn set_folder_role(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        role: Option<crate::project::FolderRole>,
    ) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.set_folder_role(path, role) {
            Ok(()) => self.spawn_word_count_recompute(ctx),
            Err(err) => self.push_error_toast(format!("Couldn't set folder role: {err}")),
        }
    }

    pub(super) fn set_picklist_folder(
        &mut self,
        field: crate::project::PicklistField,
        path: Option<PathBuf>,
    ) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.set_picklist_folder(field, path.as_deref()) {
            self.push_error_toast(format!("Couldn't set dropdown source: {err}"));
        }
    }

    /// Ask for confirmation, then permanently delete everything inside the
    /// designated Trash folder at `path`.
    pub(super) fn empty_trash_folder(&mut self, path: &Path) {
        let confirmed = rfd::MessageDialog::new()
            .set_title("Empty Trash")
            .set_description("Permanently delete everything in Trash? This cannot be undone.")
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if confirmed != rfd::MessageDialogResult::Yes {
            return;
        }

        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.empty_trash() {
            self.push_error_toast(format!("Couldn't empty Trash: {err}"));
            return;
        }
        self.document_status_cache.clear();
        if self
            .selected_path
            .as_deref()
            .is_some_and(|selected| selected.starts_with(path))
        {
            self.editor = EditorState::default();
            self.selected_path = None;
        }
    }

    pub(super) fn finish_prompt(&mut self, ctx: &egui::Context, outcome: NamePromptOutcome) {
        let Some(pending) = self.prompt.take() else {
            return;
        };
        let NamePromptOutcome::Confirmed(name) = outcome else {
            return;
        };
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        match pending.action {
            PromptAction::NewFile { parent } => self.create_document(&parent, name),
            PromptAction::NewFolder { parent } => self.create_folder(&parent, name),
            PromptAction::NewFileFromTemplate {
                parent,
                template_path,
            } => self.create_document_from_template(&parent, name, &template_path),
            PromptAction::Rename { path } => self.rename_node(&path, name),
            PromptAction::NewProject {
                location,
                template_id,
            } => self.create_project(ctx, &location, name, &template_id),
            PromptAction::GitCommit { push_after } => self.run_git_commit(ctx, name, push_after),
            PromptAction::SaveLayout => self.save_named_layout(ctx, name),
            PromptAction::SaveProjectAsTemplate => self.save_project_as_template(name),
            PromptAction::JoinCollabSession => self.start_collab_join(ctx, name),
        }
    }

    pub(super) fn rename_node(&mut self, path: &Path, new_name: &str) {
        // If `path` is the open document and it's dirty, save it *before* the
        // physical rename below — `project.rename` does an immediate `fs::rename`,
        // and letting `EditorState::open`'s own save-if-dirty run afterward (from
        // `open_document`, once the item's reopened under its new name) would try to
        // save to `editor.open_path`, which is still the pre-rename path and no
        // longer exists — silently resurrecting a stray file there with the unsaved
        // content while the visible buffer quietly reverts to the pre-edit version.
        // Saving first means the rename carries the up-to-date content along.
        if self.editor.open_path.as_deref() == Some(path)
            && self.editor.dirty
            && let Err(err) = self.editor.save()
        {
            self.push_error_toast(format!("Couldn't save before renaming: {err}"));
            return;
        }

        let Some(project) = &mut self.project else {
            return;
        };
        match project.rename(path, new_name) {
            Ok(new_path) => {
                if self.metadata.target == MetadataTarget::Folder(path.to_path_buf()) {
                    self.metadata.target = MetadataTarget::Folder(new_path.clone());
                    self.metadata.folder_computed_for = None;
                }
                self.document_status_cache.clear();
                self.document_history.rename_path(path, &new_path);
                if self.selected_path.as_deref() == Some(path) {
                    self.open_document_internal(&new_path);
                } else if !self.editor.dirty
                    && let Some(open_path) = self.editor.open_path.clone()
                {
                    // The rename may have rewritten a `[[wikilink]]` to this document
                    // on disk; reload it so the editor reflects that. Skipped while
                    // dirty so we don't clobber unsaved edits with the disk version.
                    let _ = self.editor.open(&open_path);
                }
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't rename: {err}"));
            }
        }
    }

    /// Ask for confirmation via a native dialog, then delete the file or folder at
    /// `path`, closing it in the editor first if it (or its containing folder) was
    /// open. Worded accurately depending on whether this will move `path` into a
    /// designated Trash folder or remove it from disk outright.
    pub(super) fn delete_node(&mut self, path: &Path) {
        let to_trash = self
            .project
            .as_ref()
            .is_some_and(|project| project.deletes_to_trash(path));
        let confirmed = if to_trash {
            rfd::MessageDialog::new()
                .set_title("Move to Trash")
                .set_description(format!("Move \"{}\" to Trash?", path.display()))
                .set_level(rfd::MessageLevel::Info)
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
        } else {
            rfd::MessageDialog::new()
                .set_title("Delete")
                .set_description(format!(
                    "Delete \"{}\"? This cannot be undone.",
                    path.display()
                ))
                .set_level(rfd::MessageLevel::Warning)
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
        };
        if confirmed != rfd::MessageDialogResult::Yes {
            return;
        }

        let Some(project) = &mut self.project else {
            return;
        };
        match project.delete(path) {
            Ok(()) => {
                if self
                    .selected_path
                    .as_deref()
                    .is_some_and(|selected| selected == path || selected.starts_with(path))
                {
                    self.editor = EditorState::default();
                    self.selected_path = None;
                }
                if let MetadataTarget::Folder(target) = &self.metadata.target
                    && (target.as_path() == path || target.starts_with(path))
                {
                    self.metadata.target = MetadataTarget::Document;
                    self.metadata.folder_computed_for = None;
                }
                self.document_history.remove_subtree(path);
                self.document_status_cache.clear();
            }
            Err(err) => {
                self.push_error_toast(format!("Couldn't delete {}: {err}", path.display()));
            }
        }
    }

    pub(super) fn create_document(&mut self, parent: &Path, name: &str) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.create_document(parent, name) {
            Ok(path) => self.open_document(&path),
            Err(err) => self.push_error_toast(format!("Couldn't create file: {err}")),
        }
    }

    fn create_folder(&mut self, parent: &Path, name: &str) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.create_folder(parent, name) {
            self.push_error_toast(format!("Couldn't create folder: {err}"));
        }
    }

    fn create_document_from_template(&mut self, parent: &Path, name: &str, template_path: &Path) {
        let Some(project) = &mut self.project else {
            return;
        };
        match project.create_document_from_template(
            parent,
            name,
            template_path,
            &self.settings.template_date_format,
        ) {
            Ok(path) => self.open_document(&path),
            Err(err) => self.push_error_toast(format!("Couldn't create file: {err}")),
        }
    }
}
