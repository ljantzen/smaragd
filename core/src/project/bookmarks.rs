use super::*;
use std::collections::HashSet;

/// A user-defined jump-back-to-here marker at a specific line in a specific
/// document — project-wide (one list spanning every document in
/// `ProjectMeta::bookmarks`, not scoped to whichever file happens to be
/// open), set from the Editor's line-number gutter or its keyboard
/// shortcut (`ShortcutAction::ToggleBookmark`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bookmark {
    pub id: Uuid,
    /// Project-root-relative `/`-joined key (via `relative_key`), the same
    /// portable convention `folder_roles`/`trashed_origins`/`node_order`
    /// already use — survives the whole project folder being relocated or
    /// cloned elsewhere, unlike an absolute path. Kept in sync on a
    /// document/folder rename or move — see `Project::rewrite_bookmark_paths`,
    /// called from the same rename/move sites `Project::rewrite_relative_key_prefix`
    /// is (including a trash/restore round trip, itself just a move) — so a
    /// bookmark follows its document instead of dangling. Only actually
    /// removed once the document is gone for good, via
    /// `Project::remove_bookmarks_under_prefix` — see `permanently_delete`.
    pub path: String,
    /// 1-based logical line — a run of text ending in a real `\n`, the same
    /// definition `ui::editor_panel::paint_gutter` and `search::line_at`
    /// use, not a wrapped visual row.
    pub line: usize,
}

/// A `Bookmark` resolved against the live `BinderTree`, for the Bookmarks
/// dock (`ui::bookmarks_panel::show`) to render directly without touching
/// `Project` itself — same pre-resolved-before-rendering shape
/// `BacklinkEntry`/`TagGroup` already use.
pub struct ResolvedBookmark {
    pub id: Uuid,
    pub path: PathBuf,
    pub line: usize,
    /// `None` when the bookmarked document no longer resolves (renamed,
    /// moved, or deleted since) — the dock shows "(not found)" and disables
    /// "Goto" for that row, but "Delete" still works.
    pub document_stem: Option<String>,
}

impl Project {
    /// Every bookmarked line in `path`, for the Editor's gutter to paint a
    /// diamond on. `path` is matched by its resolved project-relative key,
    /// not string-compared directly, so it works whether the caller passes
    /// an absolute or already-relative path.
    pub fn bookmarked_lines_for(&self, path: &Path) -> HashSet<usize> {
        let key = relative_key(&self.root, path);
        self.meta
            .bookmarks
            .iter()
            .filter(|b| b.path == key)
            .map(|b| b.line)
            .collect()
    }

    /// Adds a bookmark at `(path, line)` if none exists there yet, removes
    /// it otherwise — the shared toggle both the gutter click and the
    /// keyboard shortcut drive. Returns whether the line is bookmarked
    /// after the call.
    pub fn toggle_bookmark(&mut self, path: &Path, line: usize) -> io::Result<bool> {
        let key = relative_key(&self.root, path);
        let existing = self
            .meta
            .bookmarks
            .iter()
            .position(|b| b.path == key && b.line == line);
        let now_bookmarked = match existing {
            Some(index) => {
                self.meta.bookmarks.remove(index);
                false
            }
            None => {
                self.meta.bookmarks.push(Bookmark {
                    id: Uuid::new_v4(),
                    path: key,
                    line,
                });
                true
            }
        };
        self.save_metadata()?;
        Ok(now_bookmarked)
    }

    pub fn delete_bookmark(&mut self, id: Uuid) -> io::Result<()> {
        self.meta.bookmarks.retain(|b| b.id != id);
        self.save_metadata()
    }

    /// Follow a document/folder rename or move: any bookmark whose `path` is
    /// exactly `old_prefix`, or nested under it (`old_prefix/...`, a
    /// descendant inside a moved/renamed folder), is rewritten to sit under
    /// `new_prefix` instead — the same "keep pointing at where the thing
    /// actually is" job `rewrite_relative_key_prefix` does for
    /// `node_order`/`folder_roles`/etc. Called from the same call sites that
    /// one is (`rename`, `move_node_with`), plus the single-document (not
    /// just folder) cases those skip it for, since a bookmark's `path` names
    /// a document directly rather than being keyed by one. Does not persist
    /// — callers already call `save_metadata` after the rest of the move.
    pub(super) fn rewrite_bookmark_paths(&mut self, old_prefix: &str, new_prefix: &str) {
        let nested_prefix = format!("{old_prefix}/");
        for bookmark in self.meta.bookmarks.iter_mut() {
            if bookmark.path == old_prefix {
                bookmark.path = new_prefix.to_string();
            } else if let Some(rest) = bookmark.path.strip_prefix(&nested_prefix) {
                bookmark.path = format!("{new_prefix}/{rest}");
            }
        }
    }

    /// Drop every bookmark whose `path` is `prefix` itself or nested under it
    /// (`prefix/...`) — called once the underlying file or folder is gone for
    /// good (`permanently_delete`), unlike a move/rename/trash round trip
    /// (see `rewrite_bookmark_paths`), where the bookmark should keep
    /// pointing at its new location instead of being dropped.
    pub(super) fn remove_bookmarks_under_prefix(&mut self, prefix: &str) {
        let nested_prefix = format!("{prefix}/");
        self.meta
            .bookmarks
            .retain(|b| b.path != prefix && !b.path.starts_with(&nested_prefix));
    }

    /// Every bookmark in the project, resolved against the current binder
    /// tree and sorted by `(resolved path, line)` for a stable, scannable
    /// dock order — bookmarks have no manual/drag order of their own to
    /// preserve, unlike `story_cards`.
    pub fn resolved_bookmarks(&self) -> Vec<ResolvedBookmark> {
        let mut resolved: Vec<ResolvedBookmark> = self
            .meta
            .bookmarks
            .iter()
            .map(|b| {
                let path = self.root.join(&b.path);
                let document_stem = self
                    .tree
                    .find_by_path(&path)
                    .filter(|node| matches!(node.kind, BinderNodeKind::Document))
                    .and_then(|node| node.path.file_stem())
                    .and_then(|stem| stem.to_str())
                    .map(str::to_string);
                ResolvedBookmark {
                    id: b.id,
                    path,
                    line: b.line,
                    document_stem,
                }
            })
            .collect();
        resolved.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_bookmark_adds_then_removes_at_the_same_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();

        assert!(project.toggle_bookmark(&doc, 3).unwrap());
        assert_eq!(project.bookmarked_lines_for(&doc), HashSet::from([3]));

        assert!(!project.toggle_bookmark(&doc, 3).unwrap());
        assert!(project.bookmarked_lines_for(&doc).is_empty());
    }

    #[test]
    fn toggle_bookmark_at_a_different_line_in_the_same_file_is_independent() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();

        project.toggle_bookmark(&doc, 2).unwrap();
        project.toggle_bookmark(&doc, 5).unwrap();
        project.toggle_bookmark(&doc, 2).unwrap(); // removes only line 2

        assert_eq!(project.bookmarked_lines_for(&doc), HashSet::from([5]));
    }

    #[test]
    fn delete_bookmark_removes_only_the_matching_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.toggle_bookmark(&doc, 1).unwrap();
        project.toggle_bookmark(&doc, 2).unwrap();
        let keep_id = project.resolved_bookmarks()[0].id;
        let delete_id = project.resolved_bookmarks()[1].id;

        project.delete_bookmark(delete_id).unwrap();

        let remaining: Vec<Uuid> = project.resolved_bookmarks().iter().map(|b| b.id).collect();
        assert_eq!(remaining, vec![keep_id]);
    }

    #[test]
    fn bookmarks_persist_across_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.toggle_bookmark(&doc, 4).unwrap();

        let reloaded = Project::load_from_folder(dir.path()).unwrap();

        assert_eq!(reloaded.bookmarked_lines_for(&doc), HashSet::from([4]));
    }

    #[test]
    fn bookmark_json_without_a_bookmarks_key_loads_as_empty() {
        // Guards `#[serde(default)]` on `ProjectMeta::bookmarks`: a
        // project.json written before this field existed has no
        // "bookmarks" key at all.
        let dir = tempfile::tempdir().unwrap();
        let meta_dir = dir.path().join(METADATA_DIR);
        fs::create_dir_all(&meta_dir).unwrap();
        fs::write(
            meta_dir.join(METADATA_FILE),
            r#"{ "version": 1, "node_order": {} }"#,
        )
        .unwrap();

        let project = Project::load_from_folder(dir.path()).unwrap();

        assert!(project.meta.bookmarks.is_empty());
    }

    #[test]
    fn permanently_deleting_the_bookmarked_document_removes_its_bookmark() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.toggle_bookmark(&doc, 1).unwrap();

        // No Trash folder is designated, so this goes straight to
        // `permanently_delete` — the document is gone for good, so its
        // bookmark should be too, not left dangling.
        project.delete(&doc).unwrap();

        assert!(project.meta.bookmarks.is_empty());
    }

    #[test]
    fn moving_a_bookmarked_document_to_trash_keeps_its_bookmark_resolving() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.toggle_bookmark(&doc, 3).unwrap();

        project.delete(&doc).unwrap();

        let trashed = trash.join("Scene 1.md");
        assert_eq!(project.bookmarked_lines_for(&trashed), HashSet::from([3]));
        let resolved = project.resolved_bookmarks();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].document_stem, Some("Scene 1".to_string()));
    }

    #[test]
    fn emptying_trash_removes_bookmarks_of_the_documents_it_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let trash = project.create_folder(dir.path(), "Trash").unwrap();
        project
            .set_folder_role(&trash, Some(FolderRole::Trash))
            .unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.toggle_bookmark(&doc, 3).unwrap();
        project.delete(&doc).unwrap();
        assert_eq!(project.meta.bookmarks.len(), 1);

        project.empty_trash().unwrap();

        assert!(project.meta.bookmarks.is_empty());
    }

    #[test]
    fn renaming_a_bookmarked_document_keeps_its_bookmark_resolving() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.toggle_bookmark(&doc, 2).unwrap();

        let renamed = project.rename(&doc, "Scene 1 Renamed").unwrap();

        assert_eq!(project.bookmarked_lines_for(&renamed), HashSet::from([2]));
        assert!(project.bookmarked_lines_for(&doc).is_empty());
    }

    #[test]
    fn renaming_a_folder_keeps_a_descendant_documents_bookmark_resolving() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();
        let doc = project.create_document(&chapter, "Scene 1").unwrap();
        project.toggle_bookmark(&doc, 4).unwrap();

        let renamed_chapter = project.rename(&chapter, "Chapter One").unwrap();

        let moved_doc = renamed_chapter.join("Scene 1.md");
        assert_eq!(project.bookmarked_lines_for(&moved_doc), HashSet::from([4]));
    }

    #[test]
    fn moving_a_bookmarked_document_to_a_different_folder_keeps_its_bookmark_resolving() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.toggle_bookmark(&doc, 1).unwrap();
        let chapter = project.create_folder(dir.path(), "Chapter 1").unwrap();

        let moved = project.move_item(&doc, &chapter).unwrap();

        assert_eq!(project.bookmarked_lines_for(&moved), HashSet::from([1]));
    }

    #[test]
    fn resolved_bookmarks_is_sorted_by_document_then_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let a = project.create_document(dir.path(), "A").unwrap();
        let b = project.create_document(dir.path(), "B").unwrap();
        project.toggle_bookmark(&b, 1).unwrap();
        project.toggle_bookmark(&a, 5).unwrap();
        project.toggle_bookmark(&a, 2).unwrap();

        let lines: Vec<(Option<String>, usize)> = project
            .resolved_bookmarks()
            .into_iter()
            .map(|r| (r.document_stem, r.line))
            .collect();

        assert_eq!(
            lines,
            vec![
                (Some("A".to_string()), 2),
                (Some("A".to_string()), 5),
                (Some("B".to_string()), 1),
            ]
        );
    }
}
