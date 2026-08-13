# Bookmarks

A bookmark marks a specific line in a document so you can jump straight back to it later — project-wide, not scoped to whichever file happens to be open.

## Setting a bookmark

Two equivalent ways:

- **`Ctrl+F2`** toggles a bookmark at the cursor's current line.
- Click a line's icon slot in the Editor's [line-number gutter](writing-and-editor.md#line-numbers) (**Settings > Editor > Show line numbers** must be on for this — the shortcut works either way). A bookmarked line shows a ◆ there; clicking it again removes the bookmark.

Only one bookmark per line — toggling on an already-bookmarked line removes it.

## The Bookmarks dock

**`View > Bookmarks`** (or `Ctrl+Alt+B`) opens a dock listing every bookmark in the project, sorted by document then line. Each row is a clickable link (like a Backlinks or Tags row) — click it to open that document and jump straight to the line — plus a **Delete** button.

If a bookmarked file has since been renamed, moved, or deleted, its row shows **(not found)** instead of a link: the bookmark isn't automatically kept in sync with the file it points to, so it's left dangling rather than silently dropped. It's still listed (and deletable) even though there's nowhere left to jump to — the same tolerant-of-drift behavior a Story Card's linked document already has if that document goes away.

## Navigating between bookmarks

**`Alt+Down`**/**`Alt+Up`** step to the next/previous bookmark, in the same document-then-line order the dock lists them, wrapping around at either end — deliberately paralleling **`Alt+Left`**/**`Alt+Right`**'s [Go Back/Go Forward](writing-and-editor.md) history navigation. A bookmark whose document can't be found is skipped.
