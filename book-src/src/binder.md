# The Binder

The left-hand panel is the **binder** — a tree view of your project folder, one of the dockable tool windows described above. It's `.gitignore`-aware, and documents are shown without their `.md` extension.

- **Navigate by keyboard**: click a row (or Tab to it) to give it focus, then:
  - `Up`/`Down` moves between rows
  - `Left`/`Right` collapses/expands a focused folder
  - `Enter` opens the focused document
- **Drag and drop** a file or folder *onto* another folder to move it there. Drag it *onto another document* instead to reorder — dropping it just before that document, within the same folder — without changing which folder it's in.
- **`F6`** (the remappable "Toggle Binder/Editor Focus" shortcut) jumps keyboard focus back and forth between the binder and the editor/preview, without touching the mouse.
- **Right-click** a row for a context menu:
  - **New File** / **New Folder** / **New From Template** (folders only — see [Templates](folder-roles.md)) — each prompts for a name (`Enter` to confirm)
  - **Rename** — also prompts for a name, and updates any `[[wikilinks]]` elsewhere in the project that pointed at the old name
  - **Delete** — shows a native confirmation dialog; if a Trash folder is configured, it's worded as a move to Trash rather than a permanent delete
  - **Restore** (on a trashed item) — moves it back to its original folder, offering to recreate that folder if it's gone since
  - **Folder Role** / **Dropdown Source** / **Empty Trash** (folders only) — see [Folder Roles](folder-roles.md) and [Dropdown Source Folders](document-metadata.md#dropdown-source-folders)
- A folder with a role assigned shows a leading icon instead of a text label: 🔍 Research, 🗑 Trash, 📋 Templates, 📖 Manuscript
- **Click the root row** (the project itself, at the very top of the tree) to switch the Metadata dock over to project-wide fields — see [Project Metadata](project-metadata.md). The row gets the same persistent highlight a selected document gets, and clicking it also toggles the whole tree open/closed, the same as clicking any other folder row does
- **Click any other folder row** to switch the Metadata dock to that folder's own metadata instead — the same `Type`/`Status`/`POV`/`Word Count Target`/`Tags` fields and form documents use (see [Document Metadata](document-metadata.md)), just without a live word count of its own since a folder has no body to count. The form's heading reads "Folder Metadata" instead of the document's title, so it's always clear which kind of row you're editing

## Binder Background Coloring

Binder rows — documents and folders alike — can be background-colored to make status, POV, or progress toward a word count target visible at a glance without opening the Metadata dock for each one. Four modes are available, switchable via **`View > Color Binder By`**, the remappable **"Cycle Binder Color Mode"** shortcut (default `Ctrl+Shift+C`, cycling through the modes below in order), or by clicking the mode indicator that appears in the status bar once a mode other than `Off` is active (clicking it cycles too):

- **Off** — no background coloring at all. The default, so a new project's binder starts uncolored until you opt in.
- **Status** — colors each row by its own `status` value, using whatever color you've assigned that status (see below).
- **POV** — colors each row by its own `pov` value the same way.
- **Word Count Progress** — a red→yellow→green gradient toward `word_count_target`: a document uses its own word count against its own target; a folder uses the *combined* word count of every document nested inside it (computed on a background thread alongside the [Word Count](word-count.md) panel's own total, so it never blocks the UI) against the folder's own target.

A row with nothing relevant to the active mode — no status/POV assigned, or no target set — simply shows no color; there's no fallback to a different mode.

Status and POV colors themselves are assigned from the Metadata dock: next to the `Status:`/`POV:` row (on both the document and folder forms), a color swatch button appears as soon as that field isn't blank — click it to open a color picker. Each status/POV value's color is shared project-wide, so coloring "draft" or "Alice" once colors every row carrying that value, document or folder alike.

A file with uncommitted git changes (when [git integration](git-integration.md) is on, both app-wide and for the project) also gets a trailing "•" marker after its name — a folder shows the same marker if anything nested inside it is dirty. This is independent of the coloring modes above: it's a plain text suffix, not a color, so it shows alongside whichever `Color Binder By` mode is active rather than competing with it.

## Document Stats

**Settings > Appearance > Show document stats in binder** (off by default) adds a right-aligned `lines/words/chars` readout to every document row — e.g. `53/418/2345` for a 53-line, 418-word, 2,345-character document. The open document's numbers update live as you type; every other document's come from its file on disk, re-read only when it actually changes. Toggle it without opening Settings via the remappable **"Toggle Document Stats in Binder"** shortcut (default `Ctrl+Alt+D`).
