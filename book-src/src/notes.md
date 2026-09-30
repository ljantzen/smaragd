# Notes

A note pins a short piece of free text to a specific position — line *and* column, not just the line — in a document, project-wide, not scoped to whichever file happens to be open. Unlike a [bookmark](bookmarks.md), which only marks a line, a note carries its own text, and a single line can hold more than one.

## Adding a note

Two equivalent ways:

- **`Ctrl+Shift+J`** opens a small prompt for a note at the cursor's exact position. Type the text, then **`Ctrl+Enter`** (or the **Save** button) to save; **Escape** or **Cancel** discards it.
- Click a line's note icon slot — to the left of the bookmark diamond — in the Editor's [line-number gutter](writing-and-editor.md#line-numbers) (**Settings > Editor > Show line numbers** must be on for this — the shortcut works either way). A noted line shows a ● there. Clicking it opens the same prompt, pre-filled, to edit or delete the first note on that line; clicking an unmarked line's note icon starts a blank one at the start of the line.

Saving an empty note deletes it instead of leaving a blank one behind.

## The Notes dock

**`Tools > Notes`** (or `Ctrl+Shift+N`) opens a dock listing every note in the project, sorted by document, then line, then column. Each row is a clickable link (like a Bookmarks row) showing `line:column`, followed by a short excerpt of the note's text — click the link to open that document and jump straight to the note's exact position — plus a **Delete** button.

A note follows its document through a rename, a drag-and-drop move, and a move to [Trash](folder-roles.md) — including the round trip back out via **Restore** — the same as a bookmark. It's only actually removed once the document is truly gone: permanently deleted (no Trash configured), or via **Empty Trash**. If a noted document somehow can't be resolved anyway, its row shows **(not found)** instead of a link — still listed (and deletable) even though there's nowhere left to jump to.

## Navigating between notes

**`Ctrl+Alt+Down`**/**`Ctrl+Alt+Up`** step to the next/previous note, in the same document-then-line-then-column order the dock lists them, wrapping around at either end — one modifier up from **`Alt+Down`**/**`Alt+Up`**'s [bookmark stepping](bookmarks.md#navigating-between-bookmarks), since bookmarks already own the unmodified pair. A note whose document can't be found is skipped.
