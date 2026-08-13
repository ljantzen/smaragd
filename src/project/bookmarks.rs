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
    /// cloned elsewhere, unlike an absolute path. Deliberately *not* kept
    /// in sync on a document rename/move/delete (v1 scope): a bookmark
    /// whose document no longer resolves is left dangling, the same
    /// tolerant-of-drift behavior `StoryCard::linked_document_stems`
    /// already established (see `deleting_the_linked_document_leaves_a_dangling_but_harmless_link`
    /// in `story_cards.rs`).
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
    fn deleting_the_bookmarked_document_leaves_a_dangling_but_harmless_bookmark() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::initialize(dir.path()).unwrap();
        let doc = project.create_document(dir.path(), "Scene 1").unwrap();
        project.toggle_bookmark(&doc, 1).unwrap();

        project.delete(&doc).unwrap();

        // The bookmark survives untouched; only resolution against the
        // (now-gone) tree fails, mirroring how a dangling linked story-card
        // document behaves elsewhere.
        assert_eq!(project.meta.bookmarks.len(), 1);
        let resolved = project.resolved_bookmarks();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].document_stem, None);
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
