use super::*;
use std::collections::HashSet;

/// A user-written note pinned to a specific position — not just a line, a
/// byte offset *within* it — in a specific document, project-wide (one list
/// spanning every document in `ProjectMeta::notes`, not scoped to whichever
/// file happens to be open), set from the Editor's line-number gutter or its
/// keyboard shortcut (`ShortcutAction::AddNoteAtCursor`). Unlike a
/// [`Bookmark`], a line can carry more than one `Note` (different columns),
/// so `(path, line)` alone isn't a unique key — `(path, line, column)` is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    pub id: Uuid,
    /// Project-root-relative `/`-joined key (via `relative_key`) — same
    /// portable, rename/move-surviving convention `Bookmark::path` uses. Kept
    /// in sync via `Project::rewrite_note_paths`/`remove_notes_under_prefix`,
    /// called from the same sites the bookmark equivalents are.
    pub path: String,
    /// 1-based logical line — a run of text ending in a real `\n`, the same
    /// definition `Bookmark::line` uses.
    pub line: usize,
    /// 0-based byte offset from the start of `line` to where the note was
    /// placed. Like `line`, not remapped as surrounding text changes —
    /// `Project::goto_note`-equivalent callers clamp it to the line's own
    /// current end rather than panicking if the line has since shrunk.
    pub column: usize,
    /// The note's free text.
    pub text: String,
}

/// A `Note` resolved against the live `BinderTree`, for the Notes dock
/// (`ui::notes_panel::show`) to render directly without touching `Project`
/// itself — same pre-resolved-before-rendering shape `ResolvedBookmark`
/// already uses.
pub struct ResolvedNote {
    pub id: Uuid,
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub text: String,
    /// `None` when the noted document no longer resolves (renamed, moved, or
    /// deleted since) — the dock shows "(not found)" and disables "Goto" for
    /// that row, but "Delete" still works.
    pub document_stem: Option<String>,
}

impl Project {
    /// Every line in `path` carrying one or more notes, for the Editor's
    /// gutter to paint a marker on — a line is in the set if *any* note sits
    /// on it, regardless of column. `path` is matched by its resolved
    /// project-relative key, not string-compared directly, so it works
    /// whether the caller passes an absolute or already-relative path.
    pub fn noted_lines_for(&self, path: &Path) -> HashSet<usize> {
        let key = relative_key(&self.root, path);
        self.meta
            .notes
            .iter()
            .filter(|n| n.path == key)
            .map(|n| n.line)
            .collect()
    }

    /// Every note on `line` in `path`, ordered by ascending `column` — for a
    /// gutter click, which knows the line but not a column (see
    /// `ui::editor_panel::paint_gutter`'s doc comment on why a line-gutter
    /// icon can't carry one).
    pub fn notes_at(&self, path: &Path, line: usize) -> Vec<&Note> {
        let key = relative_key(&self.root, path);
        let mut notes: Vec<&Note> = self
            .meta
            .notes
            .iter()
            .filter(|n| n.path == key && n.line == line)
            .collect();
        notes.sort_by_key(|n| n.column);
        notes
    }

    /// Adds a note at `(path, line, column)` if none exists there yet,
    /// otherwise replaces its text — the shared upsert both the gutter's
    /// edit-existing-note path and the keyboard shortcut drive. An empty
    /// `text` deletes the note instead of leaving a blank one behind.
    pub fn upsert_note(
        &mut self,
        path: &Path,
        line: usize,
        column: usize,
        text: String,
    ) -> io::Result<()> {
        let key = relative_key(&self.root, path);
        let existing = self
            .meta
            .notes
            .iter()
            .position(|n| n.path == key && n.line == line && n.column == column);
        if text.is_empty() {
            if let Some(index) = existing {
                self.meta.notes.remove(index);
            }
        } else {
            match existing {
                Some(index) => self.meta.notes[index].text = text,
                None => self.meta.notes.push(Note {
                    id: Uuid::new_v4(),
                    path: key,
                    line,
                    column,
                    text,
                }),
            }
        }
        self.save_metadata()
    }

    pub fn delete_note(&mut self, id: Uuid) -> io::Result<()> {
        self.meta.notes.retain(|n| n.id != id);
        self.save_metadata()
    }

    /// Follow a document/folder rename or move — the note counterpart of
    /// `rewrite_bookmark_paths`. Doesn't persist — callers already call
    /// `save_metadata` after the rest of the move.
    pub(super) fn rewrite_note_paths(&mut self, old_prefix: &str, new_prefix: &str) {
        let nested_prefix = format!("{old_prefix}/");
        for note in self.meta.notes.iter_mut() {
            if note.path == old_prefix {
                note.path = new_prefix.to_string();
            } else if let Some(rest) = note.path.strip_prefix(&nested_prefix) {
                note.path = format!("{new_prefix}/{rest}");
            }
        }
    }

    /// Drop every note whose `path` is `prefix` itself or nested under it —
    /// the note counterpart of `remove_bookmarks_under_prefix`, called once
    /// the underlying file or folder is gone for good.
    pub(super) fn remove_notes_under_prefix(&mut self, prefix: &str) {
        let nested_prefix = format!("{prefix}/");
        self.meta
            .notes
            .retain(|n| n.path != prefix && !n.path.starts_with(&nested_prefix));
    }

    /// Every note in the project, resolved against the current binder tree
    /// and sorted by `(resolved path, line, column)` for a stable, scannable
    /// dock order and stepping sequence.
    pub fn resolved_notes(&self) -> Vec<ResolvedNote> {
        let mut resolved: Vec<ResolvedNote> = self
            .meta
            .notes
            .iter()
            .map(|n| {
                let path = self.root.join(&n.path);
                let document_stem = self
                    .tree
                    .find_by_path(&path)
                    .filter(|node| matches!(node.kind, BinderNodeKind::Document))
                    .and_then(|node| node.path.file_stem())
                    .and_then(|stem| stem.to_str())
                    .map(str::to_string);
                ResolvedNote {
                    id: n.id,
                    path,
                    line: n.line,
                    column: n.column,
                    text: n.text.clone(),
                    document_stem,
                }
            })
            .collect();
        resolved.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.column.cmp(&b.column))
        });
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_note_adds_then_replaces_text_at_the_same_position() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();

        project
            .upsert_note(&doc, 3, 5, "first draft".to_string())
            .unwrap();
        assert_eq!(project.notes_at(&doc, 3).len(), 1);
        assert_eq!(project.notes_at(&doc, 3)[0].text, "first draft");

        project
            .upsert_note(&doc, 3, 5, "revised".to_string())
            .unwrap();
        let notes = project.notes_at(&doc, 3);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].text, "revised");
    }

    #[test]
    fn upsert_note_with_empty_text_deletes_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.upsert_note(&doc, 2, 0, "temp".to_string()).unwrap();

        project.upsert_note(&doc, 2, 0, String::new()).unwrap();

        assert!(project.notes_at(&doc, 2).is_empty());
    }

    #[test]
    fn notes_at_different_columns_on_the_same_line_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();

        project.upsert_note(&doc, 4, 10, "b".to_string()).unwrap();
        project.upsert_note(&doc, 4, 2, "a".to_string()).unwrap();

        let notes = project.notes_at(&doc, 4);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].text, "a"); // sorted by column
        assert_eq!(notes[1].text, "b");
    }

    #[test]
    fn notes_on_different_lines_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();

        project.upsert_note(&doc, 2, 0, "a".to_string()).unwrap();
        project.upsert_note(&doc, 5, 0, "b".to_string()).unwrap();

        assert_eq!(project.noted_lines_for(&doc), HashSet::from([2, 5]));
    }

    #[test]
    fn delete_note_removes_only_the_matching_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.upsert_note(&doc, 1, 0, "a".to_string()).unwrap();
        project.upsert_note(&doc, 2, 0, "b".to_string()).unwrap();
        let keep_id = project.resolved_notes()[0].id;
        let delete_id = project.resolved_notes()[1].id;

        project.delete_note(delete_id).unwrap();

        let remaining: Vec<Uuid> = project.resolved_notes().iter().map(|n| n.id).collect();
        assert_eq!(remaining, vec![keep_id]);
    }

    #[test]
    fn notes_persist_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project
            .upsert_note(&doc, 4, 7, "remember this".to_string())
            .unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();

        let notes = reloaded.notes_at(&doc, 4);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].column, 7);
        assert_eq!(notes[0].text, "remember this");
    }

    #[test]
    fn note_json_without_a_notes_key_loads_as_empty() {
        // Guards `#[serde(default)]` on `ProjectMeta::notes`: a project.json
        // written before this field existed has no "notes" key at all.
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join(METADATA_DIR);
        fs::create_dir_all(&meta_dir).unwrap();
        fs::write(
            meta_dir.join(METADATA_FILE),
            r#"{ "version": 1, "node_order": {} }"#,
        )
        .unwrap();

        let project = Project::load_from_folder(dir.path()).unwrap();

        assert!(project.meta.notes.is_empty());
    }

    #[test]
    fn permanently_deleting_the_noted_document_removes_its_notes() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.upsert_note(&doc, 1, 0, "a".to_string()).unwrap();

        project.delete(&doc).unwrap();

        assert!(project.meta.notes.is_empty());
    }

    #[test]
    fn moving_a_noted_document_to_trash_keeps_its_note_resolving() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.upsert_note(&doc, 3, 2, "a".to_string()).unwrap();

        project.delete(&doc).unwrap();

        let trashed = trash.join("Scene 1.md");
        let notes = project.notes_at(&trashed, 3);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].column, 2);
        let resolved = project.resolved_notes();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].document_stem, Some("Scene 1".to_string()));
    }

    #[test]
    fn renaming_a_noted_document_keeps_its_note_resolving() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.upsert_note(&doc, 2, 4, "a".to_string()).unwrap();

        let renamed = project.rename(&doc, "Scene 1 Renamed").unwrap();

        assert_eq!(project.notes_at(&renamed, 2).len(), 1);
        assert!(project.notes_at(&doc, 2).is_empty());
    }

    #[test]
    fn resolved_notes_is_sorted_by_document_then_line_then_column() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_document(dir.path(), "A").unwrap();
        let b = project.create_document(dir.path(), "B").unwrap();
        project.upsert_note(&b, 1, 0, "b1".to_string()).unwrap();
        project.upsert_note(&a, 5, 0, "a5".to_string()).unwrap();
        project.upsert_note(&a, 2, 9, "a2-9".to_string()).unwrap();
        project.upsert_note(&a, 2, 1, "a2-1".to_string()).unwrap();

        let texts: Vec<String> = project
            .resolved_notes()
            .into_iter()
            .map(|n| n.text)
            .collect();

        assert_eq!(texts, vec!["a2-1", "a2-9", "a5", "b1"]);
    }
}
