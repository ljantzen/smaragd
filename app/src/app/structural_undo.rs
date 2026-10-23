use super::*;

/// The most recent binder rename or move, kept so `undo_last_structural_op` can
/// reverse it. Carries the item's path just before (`from`) and just after
/// (`to`) the operation — reversing it is just re-applying the same kind of
/// operation with `to` and `from` swapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StructuralOp {
    /// `from`/`to` differ only in file name, same parent folder (see
    /// `Project::rename`).
    Renamed { from: PathBuf, to: PathBuf },
    /// `from`/`to` differ only in parent folder, same file name (see
    /// `Project::move_item`/`move_item_before` — a drag-and-drop move never
    /// renames, it only ever refuses on a name collision).
    Moved { from: PathBuf, to: PathBuf },
}

impl SmaragdApp {
    /// Reverse the binder rename or move recorded in `self.last_structural_op`,
    /// if any — `ShortcutAction::UndoLastMove`'s handler. A single-level undo:
    /// reversing the operation runs back through `rename_node`/`move_item`,
    /// which record *their own* result as the new `last_structural_op`, so
    /// invoking this again redoes the original change rather than doing
    /// nothing — there's no deeper history than that.
    ///
    /// Best-effort for a move: the item lands back in its original folder, but
    /// at the end of that folder's order rather than necessarily its exact
    /// former position (`move_item`'s own placement rule) — restoring the
    /// folder is the part that matters; exact position isn't tracked.
    pub(super) fn undo_last_structural_op(&mut self) {
        let Some(op) = self.last_structural_op.take() else {
            self.set_status_message("Nothing to undo");
            return;
        };
        match op {
            StructuralOp::Renamed { from, to } => {
                let Some(old_name) = from.file_name().and_then(|n| n.to_str()) else {
                    return;
                };
                self.rename_node(&to, old_name);
            }
            StructuralOp::Moved { from, to } => {
                let Some(old_parent) = from.parent() else {
                    return;
                };
                self.move_item(&to, old_parent);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_project() -> (tempfile::TempDir, SmaragdApp) {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::initialize(dir.path()).unwrap();
        let mut app = SmaragdApp::test_fixture();
        app.project = Some(project);
        (dir, app)
    }

    #[test]
    fn undoing_a_rename_renames_it_back_and_records_the_reverse_as_the_new_last_op() {
        let (_dir, mut app) = app_with_project();
        let root = app.project.as_ref().unwrap().root.clone();
        let path = app
            .project
            .as_mut()
            .unwrap()
            .create_document(&root, "Draft")
            .unwrap();

        app.rename_node(&path, "Renamed.md");
        let renamed = path.with_file_name("Renamed.md");
        assert!(app.project.as_ref().unwrap().store.exists(&renamed));

        app.undo_last_structural_op();

        assert!(app.project.as_ref().unwrap().store.exists(&path));
        assert!(!app.project.as_ref().unwrap().store.exists(&renamed));
        assert_eq!(
            app.last_structural_op,
            Some(StructuralOp::Renamed {
                from: renamed,
                to: path,
            })
        );
    }

    #[test]
    fn undoing_a_move_moves_it_back_to_its_original_folder() {
        let (_dir, mut app) = app_with_project();
        let root = app.project.as_ref().unwrap().root.clone();
        let project = app.project.as_mut().unwrap();
        let path = project.create_document(&root, "Draft").unwrap();
        let folder = project.create_folder(&root, "Chapter One").unwrap();

        app.move_item(&path, &folder);
        let moved = folder.join("Draft.md");
        assert!(app.project.as_ref().unwrap().store.exists(&moved));

        app.undo_last_structural_op();

        assert!(app.project.as_ref().unwrap().store.exists(&path));
        assert!(!app.project.as_ref().unwrap().store.exists(&moved));
    }

    #[test]
    fn undoing_with_nothing_recorded_sets_a_status_message_without_panicking() {
        let (_dir, mut app) = app_with_project();
        app.undo_last_structural_op();
        assert_eq!(app.status_message.as_deref(), Some("Nothing to undo"));
    }
}
