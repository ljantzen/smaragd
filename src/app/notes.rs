use super::bookmarks::{line_at_byte, line_start_byte_offset};
use super::*;
use crate::project::ResolvedNote;
use crate::ui::note_prompt::{NotePromptOutcome, NotePromptState};
use uuid::Uuid;

impl SmaragdApp {
    /// Jump to the next/previous note, project-wide (see `step_note`) —
    /// `ShortcutAction::NextNote`/`PreviousNote`.
    pub(super) fn goto_next_note(&mut self) {
        self.step_note(true);
    }

    pub(super) fn goto_previous_note(&mut self) {
        self.step_note(false);
    }

    /// Jump to the next (`forward`) or previous note, ordered the same way
    /// `Project::resolved_notes` sorts (by document, then line, then
    /// column), wrapping around at either end — see `step_note_index`.
    /// Dangling notes (no resolved document) are skipped: there's nowhere to
    /// jump to. A silent no-op with no project open or no non-dangling notes
    /// to step through.
    fn step_note(&mut self, forward: bool) {
        let Some(project) = &self.project else {
            return;
        };
        let resolved: Vec<ResolvedNote> = project
            .resolved_notes()
            .into_iter()
            .filter(|n| n.document_stem.is_some())
            .collect();
        if resolved.is_empty() {
            return;
        }
        let current = self.editor.open_path().map(|path| {
            let line = line_at_byte(self.editor.buffer(), self.editor.cursor_byte());
            let column = self.editor.cursor_byte()
                - line_start_byte_offset(self.editor.buffer(), line);
            (path, line, column)
        });
        let index = step_note_index(&resolved, current, forward);
        let target = &resolved[index];
        self.goto_note(target.path.clone(), target.line, target.column);
    }

    /// Open the Note prompt for `line` in whichever document is currently
    /// open. `column`, when `Some`, is the exact spot the caller knows (the
    /// `AddNoteAtCursor` shortcut, converted from the live `TextEdit` cursor)
    /// — pre-fills from an existing note there if one exists, else starts
    /// blank at that column. `None` (a gutter click, which only knows the
    /// line — see `ui::editor_panel::paint_gutter`'s doc comment) edits the
    /// first (lowest-column) note already on that line, or else starts a
    /// blank one at column 0. A silent no-op with no open document —
    /// unreachable from the UI in that state anyway.
    pub(super) fn open_note_prompt(&mut self, line: usize, column: Option<usize>) {
        let Some(path) = self.editor.open_path().map(Path::to_path_buf) else {
            return;
        };
        let Some(project) = &self.project else {
            return;
        };
        let (column, text) = match column {
            Some(column) => {
                let text = project
                    .notes_at(&path, line)
                    .iter()
                    .find(|n| n.column == column)
                    .map(|n| n.text.clone())
                    .unwrap_or_default();
                (column, text)
            }
            None => project
                .notes_at(&path, line)
                .first()
                .map(|n| (n.column, n.text.clone()))
                .unwrap_or((0, String::new())),
        };
        self.note_prompt = Some(NotePromptState::new(path, line, column, text));
    }

    pub(super) fn handle_note_prompt_outcome(&mut self, outcome: NotePromptOutcome) {
        let Some(prompt) = self.note_prompt.take() else {
            return;
        };
        let Some(project) = &mut self.project else {
            return;
        };
        match outcome {
            NotePromptOutcome::Saved(text) => {
                if let Err(err) =
                    project.upsert_note(&prompt.path, prompt.line, prompt.column, text)
                {
                    self.push_error_toast(format!("Couldn't save note: {err}"));
                }
            }
            NotePromptOutcome::Deleted => {
                if let Some(id) = project
                    .notes_at(&prompt.path, prompt.line)
                    .iter()
                    .find(|n| n.column == prompt.column)
                    .map(|n| n.id)
                {
                    self.delete_note(id);
                }
            }
            NotePromptOutcome::Cancelled => {}
        }
    }

    pub(super) fn handle_notes_event(&mut self, event: ui::notes_panel::NotesEvent) {
        match event {
            ui::notes_panel::NotesEvent::Open { path, line, column } => {
                self.goto_note(path, line, column);
            }
            ui::notes_panel::NotesEvent::Delete(id) => self.delete_note(id),
        }
    }

    /// Open `path` and move the cursor to `line`'s `column` — "goto note".
    /// `open_document` (via `load_document`) already stages
    /// `editor.pending_cursor` from `document_history`'s own last-known
    /// position for `path`; this deliberately overwrites it *afterward* with
    /// the note's own target, and only if the open actually landed on
    /// `path` — see `goto_bookmark`'s identical reasoning.
    fn goto_note(&mut self, path: PathBuf, line: usize, column: usize) {
        self.open_document(&path);
        if self.editor.open_path() == Some(path.as_path()) {
            let start = line_start_byte_offset(self.editor.buffer(), line);
            // Clamped to *this line's* own end, not the whole buffer's —
            // unlike a bookmark's line-start jump, landing past the line
            // (into the next one) if it shrank since the note was made would
            // be visibly wrong, not just imprecise.
            let line_end = self.editor.buffer()[start..]
                .find('\n')
                .map(|offset| start + offset)
                .unwrap_or(self.editor.buffer().len());
            let target = (start + column).min(line_end);
            if let Some(tab) = self.editor.active_tab_mut() {
                tab.pending_cursor = Some(target);
            }
        }
    }

    fn delete_note(&mut self, id: Uuid) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Err(err) = project.delete_note(id) {
            self.push_error_toast(format!("Couldn't delete note: {err}"));
        }
    }
}

/// Which index into `notes` (already sorted the same way
/// `Project::resolved_notes` sorts: by path, then line, then column)
/// stepping forward/backward from `current` (the open document + cursor
/// position, if any) lands on — wrapping around at either end. `notes` must
/// be non-empty; `current: None` (nothing open, or the open document has no
/// notes of its own to be "between") starts forward stepping at the first
/// note and backward stepping at the last.
fn step_note_index(
    notes: &[ResolvedNote],
    current: Option<(&Path, usize, usize)>,
    forward: bool,
) -> usize {
    let found = current.and_then(|(path, line, column)| {
        if forward {
            notes
                .iter()
                .position(|n| (n.path.as_path(), n.line, n.column) > (path, line, column))
        } else {
            notes
                .iter()
                .rposition(|n| (n.path.as_path(), n.line, n.column) < (path, line, column))
        }
    });
    match found {
        Some(index) => index,
        None if forward => 0,
        None => notes.len() - 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `ResolvedNote` for `step_note_index` tests — `id`/`document_stem`/
    /// `text` never affect that function's own logic, so all three are
    /// filled in with an arbitrary present stem and empty text.
    fn nt(path: &str, line: usize, column: usize) -> ResolvedNote {
        ResolvedNote {
            id: Uuid::new_v4(),
            path: PathBuf::from(path),
            line,
            column,
            text: String::new(),
            document_stem: Some("doc".to_string()),
        }
    }

    #[test]
    fn step_note_index_forward_finds_the_next_note_after_the_cursor() {
        let notes = vec![nt("a.md", 1, 0), nt("a.md", 5, 0), nt("b.md", 2, 0)];
        let index = step_note_index(&notes, Some((Path::new("a.md"), 3, 0)), true);
        assert_eq!(index, 1); // a.md:5
    }

    #[test]
    fn step_note_index_forward_wraps_to_the_first_note_past_the_end() {
        let notes = vec![nt("a.md", 1, 0), nt("a.md", 5, 0), nt("b.md", 2, 0)];
        let index = step_note_index(&notes, Some((Path::new("b.md"), 2, 0)), true);
        assert_eq!(index, 0); // wraps to a.md:1
    }

    #[test]
    fn step_note_index_backward_finds_the_previous_note_before_the_cursor() {
        let notes = vec![nt("a.md", 1, 0), nt("a.md", 5, 0), nt("b.md", 2, 0)];
        let index = step_note_index(&notes, Some((Path::new("b.md"), 2, 0)), false);
        assert_eq!(index, 1); // a.md:5
    }

    #[test]
    fn step_note_index_backward_wraps_to_the_last_note_before_the_start() {
        let notes = vec![nt("a.md", 1, 0), nt("a.md", 5, 0), nt("b.md", 2, 0)];
        let index = step_note_index(&notes, Some((Path::new("a.md"), 1, 0)), false);
        assert_eq!(index, 2); // wraps to b.md:2
    }

    #[test]
    fn step_note_index_with_no_open_document_starts_at_either_end() {
        let notes = vec![nt("a.md", 1, 0), nt("b.md", 2, 0)];
        assert_eq!(step_note_index(&notes, None, true), 0);
        assert_eq!(step_note_index(&notes, None, false), 1);
    }

    #[test]
    fn step_note_index_steps_between_two_notes_on_the_same_line_by_column() {
        let notes = vec![nt("a.md", 4, 2), nt("a.md", 4, 10)];
        let forward = step_note_index(&notes, Some((Path::new("a.md"), 4, 2)), true);
        assert_eq!(forward, 1);
        let backward = step_note_index(&notes, Some((Path::new("a.md"), 4, 10)), false);
        assert_eq!(backward, 0);
    }
}
