use super::*;
use crate::project::ResolvedBookmark;
use uuid::Uuid;

impl SmaragdApp {
    /// Jump to the next/previous bookmark, project-wide (see
    /// `step_bookmark`) — `ShortcutAction::NextBookmark`/`PreviousBookmark`.
    pub(super) fn goto_next_bookmark(&mut self) {
        self.step_bookmark(true);
    }

    pub(super) fn goto_previous_bookmark(&mut self) {
        self.step_bookmark(false);
    }

    /// Jump to the next (`forward`) or previous bookmark, ordered the same
    /// way `Project::resolved_bookmarks` sorts (by document, then line),
    /// wrapping around at either end — see `step_bookmark_index`. Dangling
    /// bookmarks (no resolved document) are skipped: there's nowhere to
    /// jump to. A silent no-op with no project open or no non-dangling
    /// bookmarks to step through.
    fn step_bookmark(&mut self, forward: bool) {
        let Some(project) = &self.project else {
            return;
        };
        let resolved: Vec<ResolvedBookmark> = project
            .resolved_bookmarks()
            .into_iter()
            .filter(|b| b.document_stem.is_some())
            .collect();
        if resolved.is_empty() {
            return;
        }
        let current = self.editor.open_path.as_deref().map(|path| {
            (
                path,
                line_at_byte(&self.editor.buffer, self.editor.cursor_byte),
            )
        });
        let index = step_bookmark_index(&resolved, current, forward);
        let target = &resolved[index];
        self.goto_bookmark(target.path.clone(), target.line);
    }

    /// Add/remove a bookmark at `line` in `path` — the shared handler for
    /// both a gutter click and `ShortcutAction::ToggleBookmark`, both
    /// raised as `DockAction::ToggleBookmark`/`EditorEvent::ToggleBookmark`.
    /// A silent no-op with no open project — unreachable from the UI in
    /// that state anyway, since there's no open document to click/shortcut
    /// against.
    pub(super) fn toggle_bookmark(&mut self, path: &Path, line: usize) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.toggle_bookmark(path, line) {
            self.push_error_toast(format!("Couldn't save bookmark: {err}"));
        }
    }

    pub(super) fn handle_bookmarks_event(&mut self, event: ui::bookmarks_panel::BookmarksEvent) {
        match event {
            ui::bookmarks_panel::BookmarksEvent::Open { path, line } => {
                self.goto_bookmark(path, line);
            }
            ui::bookmarks_panel::BookmarksEvent::Delete(id) => self.delete_bookmark(id),
        }
    }

    /// Open `path` and move the cursor to the start of `line` — "goto
    /// bookmark". `open_document` (via `load_document`) already stages
    /// `editor.pending_cursor` from `document_history`'s own last-known
    /// position for `path`; this deliberately overwrites it *afterward*
    /// with the bookmark's own target, and only if the open actually landed
    /// on `path` — a declined collab-session confirmation leaves
    /// `open_path` pointing elsewhere, in which case jumping the cursor
    /// would land in the wrong document.
    fn goto_bookmark(&mut self, path: PathBuf, line: usize) {
        self.open_document(&path);
        if self.editor.open_path.as_deref() == Some(path.as_path()) {
            self.editor.pending_cursor = Some(line_start_byte_offset(&self.editor.buffer, line));
        }
    }

    fn delete_bookmark(&mut self, id: Uuid) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.delete_bookmark(id) {
            self.push_error_toast(format!("Couldn't delete bookmark: {err}"));
        }
    }
}

/// Which index into `bookmarks` (already sorted the same way
/// `Project::resolved_bookmarks` sorts: by path, then line) stepping
/// forward/backward from `current` (the open document + cursor line, if
/// any) lands on — wrapping around at either end. `bookmarks` must be
/// non-empty; `current: None` (nothing open, or the open document has no
/// bookmarks of its own to be "between") starts forward stepping at the
/// first bookmark and backward stepping at the last.
fn step_bookmark_index(
    bookmarks: &[ResolvedBookmark],
    current: Option<(&Path, usize)>,
    forward: bool,
) -> usize {
    let found = current.and_then(|(path, line)| {
        if forward {
            bookmarks
                .iter()
                .position(|b| (b.path.as_path(), b.line) > (path, line))
        } else {
            bookmarks
                .iter()
                .rposition(|b| (b.path.as_path(), b.line) < (path, line))
        }
    });
    match found {
        Some(index) => index,
        None if forward => 0,
        None => bookmarks.len() - 1,
    }
}

/// The 1-based logical line containing byte offset `byte` in `text` — same
/// `\n`-delimited definition `line_start_byte_offset` inverts and
/// `search::line_at`/`ui::editor_panel::paint_gutter` already use. `byte` is
/// clamped to `text.len()` defensively (`EditorState::cursor_byte` should
/// always be in range, but this avoids a slice panic if it somehow isn't).
fn line_at_byte(text: &str, byte: usize) -> usize {
    let byte = byte.min(text.len());
    text[..byte].matches('\n').count() + 1
}

/// The byte offset of the start of 1-based logical `line` in `text` — the
/// inverse of `search::line_at`'s/`ui::editor_panel::paint_gutter`'s line
/// numbering (a run of text ending in a real `\n`). Clamped to `text.len()`
/// if `line` is beyond the text's current line count (e.g. the file shrank
/// since the bookmark was set), rather than panicking.
fn line_start_byte_offset(text: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    text.match_indices('\n')
        .nth(line - 2)
        .map(|(i, _)| i + 1)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `ResolvedBookmark` for `step_bookmark_index` tests — `id`/
    /// `document_stem` never affect that function's own logic, so both are
    /// filled in with an arbitrary present stem.
    fn bm(path: &str, line: usize) -> ResolvedBookmark {
        ResolvedBookmark {
            id: Uuid::new_v4(),
            path: PathBuf::from(path),
            line,
            document_stem: Some("doc".to_string()),
        }
    }

    #[test]
    fn step_bookmark_index_forward_finds_the_next_bookmark_after_the_cursor() {
        let bookmarks = vec![bm("a.md", 1), bm("a.md", 5), bm("b.md", 2)];
        let index = step_bookmark_index(&bookmarks, Some((Path::new("a.md"), 3)), true);
        assert_eq!(index, 1); // a.md:5
    }

    #[test]
    fn step_bookmark_index_forward_wraps_to_the_first_bookmark_past_the_end() {
        let bookmarks = vec![bm("a.md", 1), bm("a.md", 5), bm("b.md", 2)];
        let index = step_bookmark_index(&bookmarks, Some((Path::new("b.md"), 2)), true);
        assert_eq!(index, 0); // wraps to a.md:1
    }

    #[test]
    fn step_bookmark_index_backward_finds_the_previous_bookmark_before_the_cursor() {
        let bookmarks = vec![bm("a.md", 1), bm("a.md", 5), bm("b.md", 2)];
        let index = step_bookmark_index(&bookmarks, Some((Path::new("b.md"), 2)), false);
        assert_eq!(index, 1); // a.md:5
    }

    #[test]
    fn step_bookmark_index_backward_wraps_to_the_last_bookmark_before_the_start() {
        let bookmarks = vec![bm("a.md", 1), bm("a.md", 5), bm("b.md", 2)];
        let index = step_bookmark_index(&bookmarks, Some((Path::new("a.md"), 1)), false);
        assert_eq!(index, 2); // wraps to b.md:2
    }

    #[test]
    fn step_bookmark_index_with_no_open_document_starts_at_either_end() {
        let bookmarks = vec![bm("a.md", 1), bm("b.md", 2)];
        assert_eq!(step_bookmark_index(&bookmarks, None, true), 0);
        assert_eq!(step_bookmark_index(&bookmarks, None, false), 1);
    }

    #[test]
    fn line_at_byte_matches_line_start_byte_offsets_own_numbering() {
        let text = "one\ntwo\nthree\nfour";
        for line in 1..=4 {
            assert_eq!(line_at_byte(text, line_start_byte_offset(text, line)), line);
        }
    }

    #[test]
    fn line_start_byte_offset_finds_the_start_of_each_line() {
        let text = "one\ntwo\nthree\nfour";
        assert_eq!(line_start_byte_offset(text, 1), 0);
        assert_eq!(line_start_byte_offset(text, 2), 4);
        assert_eq!(line_start_byte_offset(text, 3), 8);
        assert_eq!(line_start_byte_offset(text, 4), 14);
    }

    #[test]
    fn line_start_byte_offset_clamps_to_the_buffers_end_for_an_out_of_range_line() {
        let text = "one\ntwo";
        assert_eq!(line_start_byte_offset(text, 5), text.len());
    }
}
