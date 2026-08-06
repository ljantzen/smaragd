# Smaragd User Manual

Smaragd is a desktop writing tool for long-form fiction and other manuscripts organized as a folder of Markdown files. It borrows ideas from Scrivener (binder, folder roles, templates), Longform/Obsidian (frontmatter metadata, wikilinks), Lisa Cron's *Story Genius* (structured story cards), and Helix (color themes, `:` command prompt).

This manual covers what the app does and how to use it. For internals (source layout, build/test commands), see the main [README](../README.md) and [ARCHITECTURE.md](../ARCHITECTURE.md).

## Contents

- [Installation](#installation)
- [Projects](#projects)
- [Project Templates](#project-templates)
- [The Menu Bar](#the-menu-bar)
- [Dockable Tool Windows](#dockable-tool-windows)
- [The Binder](#the-binder)
- [Writing and the Editor](#writing-and-the-editor)
- [Focus Mode](#focus-mode)
- [Markdown Preview](#markdown-preview)
- [Wikilinks](#wikilinks)
- [Backlinks](#backlinks)
- [Document Metadata (Frontmatter)](#document-metadata-frontmatter)
- [Project Metadata](#project-metadata)
- [Tags](#tags)
- [Folder Roles: Research, Trash, Templates, Manuscript](#folder-roles-research-trash-templates-manuscript)
- [Export](#export)
- [Story Cards (Corkboard)](#story-cards-corkboard)
- [Find and Replace](#find-and-replace)
- [The Command Prompt](#the-command-prompt)
- [Pomodoro Timer](#pomodoro-timer)
- [Word Count](#word-count)
- [Writing Streak](#writing-streak)
- [Collaboration](#collaboration)
- [Plugins](#plugins)
- [Git Integration](#git-integration)
- [Themes and Appearance](#themes-and-appearance)
- [Keyboard Shortcuts](#keyboard-shortcuts)
- [Notifications](#notifications)
- [Settings](#settings)

## Installation

Prebuilt binaries for Linux, Windows, and macOS are on the [Releases page](https://github.com/ljantzen/smaragd/releases/latest). They're built by CI, not signed with a paid code-signing certificate, so Windows and macOS both show a first-run warning — this is expected, not a sign of a broken or tampered download.

### Windows

An unsigned `.exe` downloaded from a browser gets flagged by SmartScreen with a "Windows protected your PC" dialog. It's a soft block:

1. Click **More info**.
2. Click **Run anyway**.

### macOS

macOS tags anything downloaded via a browser with a quarantine attribute (`com.apple.quarantine`). Launching it then shows "cannot be opened because the developer cannot be verified" (or, on newer macOS, "is damaged and can't be opened"). This is also a soft block, not a hard one:

- **Right-click the app → Open → Open Anyway** — works on most versions, though Apple has tightened this on newer macOS: sometimes it takes a second step, going to **System Settings → Privacy & Security** and clicking **Open Anyway** there after the first failed attempt.
- Or, more reliably, clear the quarantine attribute directly from Terminal:

  ```bash
  xattr -cr /path/to/Smaragd.app
  ```

  This strips the quarantine flag and sidesteps Gatekeeper's warning entirely.

### Linux

The AppImage needs its executable bit set before it will run:

```bash
chmod +x Smaragd-*.AppImage
```

## Projects

A **project** is just a folder on disk containing `.md` files and subfolders, marked with a `.smaragd/project.json` file. There's no proprietary bundle format — you can open the folder in any other editor, sync it with any tool, and everything still works.

- **`File > New Project`** opens a [template picker](#project-templates), then a native folder picker; it creates the folder, marks it as a project, and scaffolds in whatever the chosen template provides. If the folder you pick is already empty, the project is created directly in it — there's no separate name prompt, since the folder's own name already says what the project is called. Pick a non-empty folder instead (to hold the new project as a subfolder of it) and you'll get the usual name prompt.
- **`File > Open Project`** opens a native folder picker. If you point it at a folder smaragd hasn't used before, it offers to adopt the folder in place (writing the `.smaragd` marker) rather than refusing.
- **`File > Close Project`** (or **`Ctrl+Shift+W`**) saves the current document (and any open Story Card draft) if it has unsaved changes, then closes the project entirely — no save/discard/cancel prompt, same silent-autosave convention as Close Document. The Binder and every other dock tab return to their empty, no-project state, and a later "Reopen project on launch" won't bring this project back, since closing it is a deliberate choice to leave it behind. Only enabled while a project is open.
- With no project open, the Binder panel shows **New Project** / **Open Project** buttons in place of the empty binder — the first time you've ever opened a project in smaragd, New Project defaults to the **World-Building** template (see below) instead of Blank.
- `.smaragd/project.json` stores things the filesystem can't express on its own — manuscript ordering, folder roles, whether plugins/git are enabled for this project. If that file's *contents* ever get corrupted, smaragd falls back to defaults rather than erroring; only a missing marker means "this isn't a project yet."

`File > Settings` has a **"Reopen project on launch"** option, and a separate **"Ensure Research and Trash folders exist in every project"** option (off by default) that creates those two role folders automatically whenever you open a project, recreating them at their original path if they were deleted since.

## Project Templates

**`File > New Project`** shows a template picker before the usual folder picker (and name prompt, for a non-empty folder) — pick a starting scaffold, then locate the new project as before. Five templates ship built-in:

| Template | What it scaffolds |
|---|---|
| **Blank** (default*) | Nothing — an empty project, exactly like `File > New Project` behaved before templates existed |
| **Novel** | A `Manuscript` folder with two starter chapters, a `Characters` folder with a Protagonist document (Desire/Misbelief/Arc headings), plus Research and Trash folders (roles already assigned) |
| **Nonfiction** | A `Manuscript` folder with an Introduction and a "Part One" subfolder containing a first chapter, plus Research and Trash |
| **Screenplay** | A `Screenplay` folder with Act One/Two/Three starter documents, plus Research and Trash. Smaragd's editor is plain Markdown, not Fountain — this reproduces a screenplay draft's *look* with headings, not a real screenplay-format pipeline |
| **World-Building** | A `Manuscript` folder with a starter chapter, Research, a `World` folder (`Characters` with Main/Supporting subfolders, `Locations`, `Items`), and a Templates folder with Character/Location stationery documents (`${{name}}` placeholder, "New From Template" — see [Template Variables](#template-variables)), plus Trash — all roles already assigned |

\* The very first time you've ever opened a project in smaragd, the picker instead starts on **World-Building** — see [Projects](#projects).

**`File > Save Project as Template…`** turns your *current* project's own folder/document structure into a reusable custom template, prompting for a name. It excludes:
- Whatever's currently inside the project's Trash folder (if one's configured)
- Narrative state that belongs to one specific project, not a reusable shape: story cards, the protagonist Desire/Misbelief pair, and book/export/git metadata

Custom templates are stored in `smaragd/project_templates/` in the platform config directory (the same base path as custom themes/styles/plugins — see [Plugins](#plugins)):

- Linux: `~/.config/smaragd/project_templates`
- macOS: `~/Library/Application Support/smaragd/project_templates`
- Windows: `%APPDATA%\smaragd\config\project_templates`

Each is a subfolder containing a `template.toml` (label, description) and a `content/` folder mirroring the structure to stamp out. Unlike custom themes/styles, there's no "Reload Custom Templates" button — a hand-dropped or hand-edited template only shows up in the picker after restarting the app (saving one via **Save Project as Template…** refreshes the list immediately, since the app already knows it just wrote it).

## The Menu Bar

The top menu bar — File, Edit, View, Tools, Versions, Window, Help — can be driven entirely from the keyboard:

- **Alt+letter mnemonics** drop a menu down without touching the mouse: `Alt+F` File, `Alt+E` Edit, `Alt+V` View, `Alt+T` Tools, `Alt+S` Versions (Versions uses `S` since View already claims `V`), `Alt+W` Window, `Alt+H` Help.
- **Arrow-key navigation** once a menu is open: `Up`/`Down` moves the highlighted item within the open dropdown, wrapping at the ends; `Left`/`Right` switches to the adjacent top-level menu, also wrapping, and auto-focuses its first item.
- Three nested submenus — **View > Theme**, **Window > Layouts**, and **File > Export Manuscript…** when more than one folder holds the Manuscript role — stay mouse/hover-only for now; their trigger row is keyboard-navigable, just not their flyout contents.

## Dockable Tool Windows

The **Binder**, **Backlinks**, **Tags**, **Document Metadata**, **Editor**, **Preview**, **Corkboard**, **Story Grid**, **Belief Timeline**, **Pomodoro**, **Word Count**, **Collaborate**, and **Streak** views are all one shared dockable layout — similar to the Properties window in Visual Basic's IDE — rather than a mix of fixed panels, modals, and mutually-exclusive view modes. You can:

- **Drag a tab's title** onto empty space to pop it out into its own floating window
- **Drag a floating window's title back** onto the dock area to re-dock it
- **Drag one tab onto another** to group them together, switching between them like browser tabs
- **Drag a tab to an edge** of another tab or the dock area to split the layout and place it side by side
- **Resize** the dock area, or a floating window, by dragging its edge

Binder and Editor are present from the moment a project is open; Backlinks, Tags, Metadata, Preview, Corkboard, Story Grid, Belief Timeline, Pomodoro, Word Count, Collaborate, and Streak start closed. Any tab can be closed via its × button, and reopened again from **`View > Binder`**, **`View > Backlinks`**, **`View > Tags`**, **`View > Preview`**, **`View > Corkboard`**, **`View > Story Grid`**, **`View > Belief Timeline`**, **`View > Metadata`**, **`Collaborate > Collaboration Panel`**, or (for Pomodoro, Word Count, and Streak) **`Tools > Pomodoro Timer`**/**`Tools > Word Count`**/**`Tools > Streak`** (most also have shortcuts — see [Keyboard Shortcuts](#keyboard-shortcuts)). Toggling Preview, Corkboard, Story Grid, or Belief Timeline just opens or closes that tab next to the Editor rather than switching to an exclusive "view mode" — any combination of tabs can be open and arranged at once.

The whole arrangement — which tabs are open, how they're split or floated, and window position/size — persists across restarts. **`Window`** menu:

- **Save Current Layout…** — names and saves the current arrangement
- **Layouts** — lists saved layouts; pick one to switch to it
- **Restore Default Layout** — resets to the default layout: Binder on the left, Editor in the center (occupying the majority of the space), and Metadata/Backlinks stacked on the right

## The Binder

The left-hand panel is the **binder** — a tree view of your project folder, one of the dockable tool windows described above. It's `.gitignore`-aware, and documents are shown without their `.md` extension.

- **Navigate by keyboard**: click a row (or Tab to it) to give it focus, then:
  - `Up`/`Down` moves between rows
  - `Left`/`Right` collapses/expands a focused folder
  - `Enter` opens the focused document
- **Drag and drop** a file or folder *onto* another folder to move it there. Drag it *onto another document* instead to reorder — dropping it just before that document, within the same folder — without changing which folder it's in.
- **`F6`** (the remappable "Toggle Binder/Editor Focus" shortcut) jumps keyboard focus back and forth between the binder and the editor/preview, without touching the mouse.
- **Right-click** a row for a context menu:
  - **New File** / **New Folder** / **New From Template** (folders only — see [Templates](#folder-roles-research-trash-templates-manuscript)) — each prompts for a name (`Enter` to confirm)
  - **Rename** — also prompts for a name, and updates any `[[wikilinks]]` elsewhere in the project that pointed at the old name
  - **Delete** — shows a native confirmation dialog; if a Trash folder is configured, it's worded as a move to Trash rather than a permanent delete
  - **Restore** (on a trashed item) — moves it back to its original folder, offering to recreate that folder if it's gone since
  - **Folder Role** / **Dropdown Source** / **Empty Trash** (folders only) — see [Folder Roles](#folder-roles-research-trash-templates-manuscript) and [Dropdown Source Folders](#dropdown-source-folders)
- A folder with a role assigned shows a leading icon instead of a text label: 🔍 Research, 🗑 Trash, 📋 Templates, 📖 Manuscript
- **Click the root row** (the project itself, at the very top of the tree) to switch the Metadata dock over to project-wide fields — see [Project Metadata](#project-metadata). The row gets the same persistent highlight a selected document gets, and clicking it also toggles the whole tree open/closed, the same as clicking any other folder row does
- **Click any other folder row** to switch the Metadata dock to that folder's own metadata instead — the same `Type`/`Status`/`POV`/`Word Count Target`/`Tags` fields and form documents use (see [Document Metadata](#document-metadata-frontmatter)), just without a live word count of its own since a folder has no body to count. The form's heading reads "Folder Metadata" instead of the document's title, so it's always clear which kind of row you're editing

### Binder Background Coloring

Binder rows — documents and folders alike — can be background-colored to make status, POV, or progress toward a word count target visible at a glance without opening the Metadata dock for each one. Four modes are available, switchable via **`View > Color Binder By`**, the remappable **"Cycle Binder Color Mode"** shortcut (default `Ctrl+Shift+C`, cycling through the modes below in order), or by clicking the mode indicator that appears in the status bar once a mode other than `Off` is active (clicking it cycles too):

- **Off** — no background coloring at all. The default, so a new project's binder starts uncolored until you opt in.
- **Status** — colors each row by its own `status` value, using whatever color you've assigned that status (see below).
- **POV** — colors each row by its own `pov` value the same way.
- **Word Count Progress** — a red→yellow→green gradient toward `word_count_target`: a document uses its own word count against its own target; a folder uses the *combined* word count of every document nested inside it (computed on a background thread alongside the [Word Count](#word-count) panel's own total, so it never blocks the UI) against the folder's own target.

A row with nothing relevant to the active mode — no status/POV assigned, or no target set — simply shows no color; there's no fallback to a different mode.

Status and POV colors themselves are assigned from the Metadata dock: next to the `Status:`/`POV:` row (on both the document and folder forms), a color swatch button appears as soon as that field isn't blank — click it to open a color picker. Each status/POV value's color is shared project-wide, so coloring "draft" or "Alice" once colors every row carrying that value, document or folder alike.

## Writing and the Editor

The main panel is a plain-text Markdown editor, borderless and filling the whole Editor tab — clicking anywhere in it, including below the last line of a short document, places the cursor there.

- **`Ctrl+S`** (or **`Cmd+S`** on macOS) saves explicitly. The document also saves automatically when it loses focus (e.g. you click into the binder or another panel).
- There's currently no multi-tab editing — opening a document replaces whatever's currently open (saving it first if it has unsaved changes).
- **`File > Open Document…`** (or **`Ctrl+P`**) opens an fzf-style quick-switcher: type a few letters and it fuzzy-matches against every document's path, best match first — a query doesn't need to be a contiguous substring, so e.g. "ch1sc2" can match "Chapter 1/Scene 2". Use `Up`/`Down` to change the highlighted result, `Enter` or a click to open it, `Escape` to cancel.
- **`File > Close Document`** (or **`Ctrl+W`**) saves the current document if it has unsaved changes, then closes it — there's no save/discard/cancel prompt, matching the same silent-autosave behavior as opening a different document.
- Exiting the app (**`File > Exit`**, **`Ctrl+Q`**, or closing the window) is different: if the open document has unsaved changes, or a Story Card editor is open with an uncommitted draft (see [Story Cards](#story-cards-corkboard)), a **Save / Discard / Cancel** prompt appears instead of closing immediately. Save writes the document (and the card draft, if any) before exiting; Discard drops both without writing them; Cancel leaves the app open exactly as it was.

## Focus Mode

**`Tools > Focus Mode`** (or **`F9`**) is a distraction-free writing mode, similar to Scrivener's Composition Mode: the window maximizes and all chrome — menu bar, binder, other dock tabs — disappears, leaving just the current document centered in the available width. The paragraph your cursor is in stays at full brightness while other paragraphs dim, a typewriter-style aid for keeping your eye on the sentence you're actually writing.

Focus Mode needs an open document to enter — with nothing open there's nothing to focus on. Press `Escape` or `F9` again to exit and return to the normal layout.

## Markdown Preview

**`View > Preview`** (or the Toggle Preview shortcut) renders the current document in a Glow-CLI-inspired style: a colored heading hierarchy, barred blockquotes, boxed code blocks, striped GFM tables, and images.

Images work two ways:
- Standard Markdown: `![alt](path/to/image.png)`
- Obsidian-style embeds: `![[image.png]]`

Relative image paths resolve against the open document's own folder, and must stay inside the project (a path that tries to escape the project root — via `..` or a symlink — is refused). Remote `http(s)://` images are never fetched.

### Typewriter Quotes

**`File > Settings > Editor`** has a **"Typewriter quotes in Preview and export"** checkbox, off by default. When it's on, straight typewriter punctuation is rewritten wherever markdown gets rendered *from* — the Preview pane here, and every [Export](#export) format:

| Typed | Rendered |
|---|---|
| `"straight double quotes"` | "curly double quotes" |
| `'straight single quotes'` | 'curly single quotes' |
| `--` | — (em dash) |
| `...` | … (ellipsis) |

Your `.md` file on disk is never touched — the source text you type stays exactly as typed, straight quotes and all. Only the rendered *view* of it (Preview, or a compiled DOCX/EPUB/PDF) changes, so switching the setting off later shows your original punctuation again, nothing was lost. Quote direction (opening vs. closing) is inferred from context, the same simple heuristic most word processors use — it isn't guaranteed correct in every edge case (e.g. deeply nested quotes), but handles ordinary dialogue and contractions correctly.

## Wikilinks

Type `[[Topic]]` or `[[Topic|Alias]]` to link to another document by its filename (no path needed — resolution is by name, project-wide).

- In the preview, wikilinks render as clickable links.
- **Ctrl+Click** a link in the preview — or place your cursor on one in the editor and press **Ctrl+Enter** (the remappable "Activate Wikilink" shortcut — see [Keyboard Shortcuts](#keyboard-shortcuts)) — to jump to it. If the target document doesn't exist yet, this creates it, in the same folder as the note you linked from.
- While typing `[[` in the editor, an autocomplete popup filters matching document titles as you type. Navigate it with arrow keys or Tab, and press Enter (or click) to accept.

## Backlinks

**`View > Backlinks`** (or **`Ctrl+Shift+B`**) opens a dockable tool window (see [Dockable Tool Windows](#dockable-tool-windows)) listing every other document that `[[links]]` to whichever one is currently open — the reverse of a wikilink.

Each entry shows the linking document's title and a short snippet of the surrounding text, so you can tell *why* it links here without opening it. Click a title to jump to that document. A document that links more than once gets one entry per occurrence, grouped under its title. A **Refresh** button re-scans on demand, for the rare case where a file changed outside the app (e.g. a git pull) while your current document stayed open — otherwise the list updates automatically whenever you switch documents.

If no document is open, or nothing links to the current one yet, the panel says so instead of showing an empty list.

## Document Metadata (Frontmatter)

Each document can carry a YAML frontmatter block (Longform/Scrivener-style manuscript metadata) at the very top of the file:

```yaml
---
type: Chapter
status: draft
pov: Alex
word_count_target: 2500
tags: [action, chapter-3]
---
```

Open **`View > Metadata`** (or **`Ctrl+Shift+M`**) to edit these fields through a dockable form (see [Dockable Tool Windows](#dockable-tool-windows)) instead of hand-editing YAML. Unlike a typical dialog, there's no Save/Cancel step — edits apply as you type, the same way typing in the main editor does. Smaragd only ever reads/writes these five keys:

| Field | Meaning |
|---|---|
| `type` | Free-form section type — "Chapter", "Scene", "Part", or anything you want. Not tied to folder nesting. |
| `status` | Free-form drafting status — "draft", "revised", "final", or anything you want. |
| `pov` | Point-of-view character, free text. |
| `word_count_target` | A target word count for this document. |
| `tags` | A list of free-form tags — see [Tags](#tags) for combining these with inline `#tag` mentions and searching by tag. |

Any other YAML key you've hand-added to the block (or that some other tool wrote) is left alone — Smaragd never round-trips the whole block through its own data model, so unrelated keys survive a save untouched. The frontmatter block is stripped from the Markdown preview so it doesn't render as a garbled paragraph.

The Metadata panel also shows a **Word count** — a live, read-only count of the open document's body (frontmatter excluded), recomputed continuously from whatever's currently in the editor, not just what was last saved.

The `Status:` and `POV:` rows each show a color-swatch button once that field isn't blank — click it to assign that status/POV value a project-wide binder background color. See [Binder Background Coloring](#binder-background-coloring) for what these colors are used for and how to switch which one (if any) the Binder actually displays.

### Dropdown Source Folders

By default, `type`/`status`/`pov` are free text — nothing stops "Scene" and "scene" and "seen" from all being typed for the same field across a project. To turn one of them into a closed dropdown instead, right-click any folder and check it under **Dropdown Source** for **Type**, **Status**, or **POV**. That folder's direct child documents' titles (not documents in a subfolder of it) become the dropdown's options for that field; the Metadata panel's `Type:`/`Status:`/`POV:` row switches from a text box to a dropdown automatically as soon as a field has at least one folder assigned and one document in it.

A few things worth knowing:

- **Independent per field, and independent of Folder Role.** Type, Status, and POV each have their own separate folder assignment — the same folder can drive more than one field, or each can point somewhere different. Checking a folder here doesn't touch whatever [Folder Role](#folder-roles-research-trash-templates-manuscript) it already has (or lack of one), and doesn't exclude it from [Export](#export) — so an existing Research folder of character bios can double as the POV dropdown's source without anything else about it changing.
- **Never destroys an existing value.** If a document's `pov: Alice` was typed before you ever assigned a POV folder — or Alice's document has since been renamed or removed from that folder — the field still shows "Alice" as-is; it just isn't one of the clickable options until you pick something else from the dropdown.
- **"(none)"** is always the first dropdown entry, for clearing the field.
- Not recursive: only documents placed directly inside the assigned folder count, the same limitation [Templates](#folder-roles-research-trash-templates-manuscript) has.

## Project Metadata

Alongside per-document frontmatter, a project itself carries a handful of project-wide fields — a Title/Subtitle/Author, a one-line **Point**, a **Logline**, a **What if** premise question, and a longer **Synopsis** — for the book as a whole rather than any one document.

Rather than opening yet another dock tab, these reuse the same Metadata dock [Document Metadata](#document-metadata-frontmatter) already uses: click the root row at the very top of the [Binder](#the-binder) (the project itself) and the Metadata dock switches over to this form instead of a document's frontmatter. Clicking any document row switches it back.

- **Title** and **Author** are the same fields the [Export](#export) dialog's Title/Author use — editing them here or there keeps both in sync.
- **Subtitle** feeds into export the same way — see [Export](#export) for exactly where it shows up in each output format.
- **Point**, **Logline**, **What if**, and **Synopsis** are new fields with no other home yet; they don't currently appear anywhere in an exported DOCX/EPUB/PDF.

Like Document Metadata, there's no Save/Cancel step — edits apply as you type. **Point** is a single-line field, same as Title/Subtitle/Author; Logline/What if/Synopsis evenly split whatever vertical space is left in the tab below it. Synopsis (the field most likely to run long) keeps its scrollbar always visible, while Logline/What if only show theirs once there's actually more text than fits.

## Tags

Beyond the `tags:` list in [Document Metadata](#document-metadata-frontmatter), you can tag a document by writing `#tag` directly in its body — e.g. "Alice discovers the #mystery behind her mother's disappearance." A tag needs at least one letter (so `#42`, a footnote- or issue-style reference, is never mistaken for one), can include digits/`_`/`-`/`/` after the first letter, and ends at the first character outside that set — `/` lets you nest tags, e.g. `#projects/smaragd`.

**`View > Tags`** (or **`Ctrl+Shift+T`**) opens a dockable tool window (see [Dockable Tool Windows](#dockable-tool-windows)) showing the *combined* tags — frontmatter `tags:` plus every inline `#tag` mention — of whichever document is currently open. Each tag is listed with every other document in the project that also carries it; click a document's title to jump to it. A tag with no other matching document yet is still shown, so you can confirm what's actually on the open document. A **Refresh** button re-scans on demand, the same as [Backlinks](#backlinks)' own.

Click a tag heading itself, or type into the panel's own **Search** box, to switch from "tags on this document" to a project-wide list of every document carrying that tag, regardless of what's currently open — a **Clear** button empties the search box and returns to the current document's own tags. Tag matching is case-insensitive throughout (`#Mystery` and `#mystery` are the same tag), though whichever casing a given document actually used is what's displayed.

The **`:tag <name>`** [command prompt](#the-command-prompt) command opens the Tags window pre-filled with a project-wide search for `<name>`, without needing to open the window or type into its search box first.

## Folder Roles: Research, Trash, Templates, Manuscript

Right-click a folder and choose **Folder Role** to designate it as one of four special folders. A folder with a role assigned shows a leading icon in the binder (🔍/🗑/📋/📖) instead of a text label. Research, Trash, and Templates are exclusive — at most one folder per role, project-wide — but Manuscript isn't: several folders can hold it at once, e.g. one per book in a series.

- **Trash**: deleting a file or folder moves it here instead of removing it from disk. Right-click the Trash folder for **Empty Trash** (permanent, with confirmation), or right-click a trashed item for **Restore**.
- **Templates**: any document placed directly inside this folder (not in a subfolder of it) shows up in every other folder's right-click **"New From Template"** submenu. Picking one creates a new document from a copy of it — frontmatter included, with [template variables](#template-variables) substituted — after prompting you for a name. The template itself is never modified.
- **Research**: currently just a marker with no behavior yet attached — reserved for future features like word-count rollups. Unlike Trash and Templates, [Export](#export) does *not* skip a Research-role folder — right-clicking one to export it exports it like any other folder.
- **Manuscript**: designates a folder as your project's primary manuscript content, mirroring Scrivener's Draft folder. **`File > Export Manuscript…`** compiles straight from it instead of you having to right-click and find the folder yourself: if no folder has the role yet, it falls back to exporting the whole project; if exactly one does, it exports that folder directly; if more than one does, it opens a submenu to pick which one. It's also what the [Word Count](#word-count) panel's "Manuscript folders only" tracking scope sums.

### Template Variables

A template document can use two placeholders, substituted when a new document is created from it:

| Placeholder | Substituted with |
|---|---|
| `${{name}}` | The name you typed when creating the document (without the `.md` extension) |
| `${{date}}` | Today's date, formatted per the date format set in **`File > Settings`** |

For example, a template starting with:

```markdown
---
type: Scene
---
# ${{name}}

Started ${{date}}.
```

typed into the "New From Template" prompt as "Aria" would produce a document starting with `# Aria` and today's date in place of `${{date}}`.

The date format is a single format string, shared by every template — it's a [strftime](https://en.wikipedia.org/wiki/Strftime) pattern, the same mini-language used by `date`, `printf`, and most other tools that format dates from a code. It defaults to `%Y-%m-%d` (e.g. `2026-07-28`) when left blank. Some common formats:

| Format | Example output |
|---|---|
| `%Y-%m-%d` | `2026-07-28` |
| `%d/%m/%Y` | `28/07/2026` |
| `%m/%d/%Y` | `07/28/2026` |
| `%d %B %Y` | `28 July 2026` |
| `%B %-d, %Y` | `July 28, 2026` |
| `%A, %d %B %Y` | `Tuesday, 28 July 2026` |
| `%Y%m%d` | `20260728` |

A format that isn't a valid strftime pattern falls back to `%Y-%m-%d` automatically, both in a created document and in Settings' own live preview of the format — a typo here never blocks document creation.

## Export

Right-click any folder in the binder and choose **Export…** to compile it — and everything nested inside it, in the same top-to-bottom order shown in the binder — into a single DOCX, EPUB, or print-ready PDF file. A nested folder whose role is **Trash** or **Templates** is skipped automatically, so deleted or template content never accidentally ends up in a compiled manuscript. If [Typewriter Quotes](#typewriter-quotes) is turned on, the exported file gets curly quotes/em dashes/ellipses too, same as the Preview pane.

**`File > Export Manuscript…`** is a shortcut to the same dialog for whichever folder(s) hold the [Manuscript role](#folder-roles-research-trash-templates-manuscript) — see there for how it behaves with zero, one, or several such folders.

The export dialog has:

- **Title** / **Subtitle** / **Author** — plain book metadata, remembered for next time (the same fields shown in [Project Metadata](#project-metadata) — editing them in either place keeps both in sync). Subtitle is optional; leave it blank if your book doesn't have one.
- **Style** — a dropdown of typesetting styles (see below). Fonts, page size, running headers, and drop caps all come from whichever style is selected, not from anything typed into this dialog.
- **Export as DOCX…** / **Export as EPUB…** / **Export as Print PDF…** — each opens a native "Save As" dialog (defaulting to a filename built from Title/Subtitle — `"Title - Subtitle.docx"`, falling back to whichever one is set, or `manuscript.docx` if neither is), then compiles.

Title/Subtitle/Author show up differently per format:

- **DOCX/PDF** get a centered title page before the manuscript — Title, then Subtitle (if set), then Author.
- **EPUB** has no title-page concept (it's reflowable text, not paginated), so Subtitle instead folds into the book's title metadata as `"Title: Subtitle"` — that's what shows up as the book's title in a reader/library view.
- A custom style's **running header** (see below) can reference `{subtitle}` alongside `{title}`/`{author}`/`{chapter}`.

All three formats read from the *same* style, so switching styles changes DOCX, EPUB, and PDF output alike — closer to how a book-design tool like Deckle Studio treats "one style set drives every output" than to a plain markdown-to-Word converter.

### Typesetting styles

Two built-in styles ship with smaragd:

| id | Label | What it looks like |
|---|---|---|
| `manuscript` | Manuscript | Plain submission format: US Letter, 1in margins, double-spaced, ragged-right (not justified), no running header or drop cap |
| `trade_paperback` | Trade Paperback | 6×9in trim, justified body text, a running header (author's name / current chapter), and a drop cap on each chapter's first paragraph |

Like [color themes](#custom-themes) and plugins, custom styles are `.toml` files you author or drop into `smaragd/styles/` inside smaragd's config directory (no in-app style editor):

- Linux: `~/.config/smaragd/styles`
- macOS: `~/Library/Application Support/smaragd/styles`
- Windows: `%APPDATA%\smaragd\config\styles`

A minimal custom style:

```toml
id = "novella"
label = "Novella"

[page]
width_mm = 139.7   # 5.5in
height_mm = 215.9  # 8.5in
margin_mm = 15.0

[body]
font = "Libertinus Serif"
size_pt = 11
line_height = 1.2
justify = true

[headings]
font = "Libertinus Serif"
sizes_pt = [22, 19, 16, 14, 13, 12]

[blockquote]
font = "Libertinus Serif"
size_pt = 11
italic = true

[code]
font = "DejaVu Sans Mono"
size_pt = 10
```

`id` and `label` are required, along with the `[page]`/`[body]`/`[headings]`/`[blockquote]`/`[code]` tables — `id` is what selects the style and is lowercased automatically. `sizes_pt` needs all six sizes (one per heading level, `h1`–`h6`). Two more tables are optional:

```toml
[drop_cap]
scale = 3.0  # first letter renders at 3x body size

[running_header]
left = "{author}"
right = "{chapter}"
```

`{title}`/`{subtitle}`/`{author}` are substituted with whatever's typed into the export dialog; `{chapter}` (supported as a whole side's content, not mixed with other text) shows the current chapter on the print PDF specifically — DOCX and EPUB don't have a per-page "current chapter" concept, so a `{chapter}` token is just left blank there.

**"Libertinus Serif" and "DejaVu Sans Mono"** (the built-in styles' fonts) aren't arbitrary choices — they're guaranteed available to the PDF renderer specifically, bundled with smaragd itself rather than depending on what's installed on your system. A custom style naming some other font still works for DOCX/EPUB (which just reference a font by name, the same way any other document does — Word/an e-reader substitutes if it's not installed), and for PDF too if that font happens to be installed locally; if not, the PDF falls back to *some* available font rather than failing the export.

Use **Reload Custom Styles** in the export dialog to pick up a new or edited `.toml` file without restarting. A style file that fails to parse, or whose `id` collides with an already-loaded style (built-in or another custom one — whichever loaded first wins), is skipped with an error message rather than stopping other styles from loading.

### The print PDF specifically

Unlike DOCX/EPUB (which place text on the page or in an XHTML flow), the PDF target is real typesetting: smaragd embeds the [Typst](https://typst.app) compiler directly (no separate install, no network access) and generates a Typst document from your manuscript and the chosen style, then lets Typst do the actual page layout — the same category of tool as LaTeX or InDesign, not a "print to PDF" of a web page.

That gets you, for free or close to it: automatic widow/orphan avoidance (Typst's default), a running header that tracks which chapter you're actually on per page, and a drop cap rendered as an oversized inline initial letter (a *raised* cap — it doesn't wrap subsequent lines around it the way a true sunk drop cap does; that needs either a Typst package fetched over the network, which smaragd deliberately avoids, or more elaborate manual layout math than a v1 warrants).

After a successful PDF export, the status bar reports an estimated spine width for the resulting page count — useful for sizing a paperback cover, but a rough estimate based on a standard white-paper thickness constant, not a print-broker-grade figure. Confirm against your printer's own spine-width calculator (e.g. KDP's) before sending a cover to print.

### What export doesn't do (yet)

- No per-block styling for verse, dialogue, or other special block types — the markdown parser has no such concept today, only headings/paragraphs/quotes/lists/tables/code/images.
- EPUB output is one general-purpose file, not separately tuned per e-reader (Kindle/Apple Books/Kobo).
- Wikilinks resolve to a real in-book link in EPUB, when the target document is also part of the same export — otherwise (and always, in DOCX) they render as plain text.

## Story Cards (Corkboard)

**`View > Corkboard`** opens a wrapping grid of scene cards. A card isn't just a Lisa Cron *Story Genius*-style cause-and-effect breakdown — it also tracks the psychological change a scene represents: a character's belief going in, and what it becomes coming out.

At the top of the Corkboard, two project-wide fields capture what Cron calls the "Third Rail" — the protagonist's driving force, not tied to any one scene:

- **Desire** — the external/internal want the protagonist is pursuing
- **Misbelief** — the flawed, usually childhood-formed belief standing in its way

Every scene card below is meant to test or advance this pair. Each card has a header, always visible, and three tabs underneath it for everything else.

The header:

- **Scene #** — a free-text label, independent of manuscript order
- **Alpha Point** — the scene's core moment
- **Subplots** — optional, comma-separated tags
- **POV Character** — becomes a dropdown once you've designated a Dropdown Source folder for POV (see [Dropdown Source Folders](#dropdown-source-folders)), otherwise free text
- **Linked documents** — comma-separated, with autocomplete as you type. A card can link to more than one manuscript document (spanning several scenes), and more than one card can link to the same document. Only documents under a Manuscript-role folder are suggested (see [Folder Roles](#folder-roles-research-trash-templates-manuscript)) — falling back to every non-Trash/Templates document if the project has no Manuscript folder designated yet. Picking a suggestion appends a comma automatically, so it's clear you can keep typing to add another

The three tabs:

- **Plot** — **Cause** (the external event that occurs) and **Effect** (its external and internal consequence)
- **Belief and Knowledge** — **Prior Belief** (what the POV Character believes going into this card), **New Belief** (what they believe as a result of it), **Value Shift** (a short label for the value at stake, e.g. "Trust -> Distrust"), and **Knowledge Gained** (comma-separated facts the character learns)
- **Third Rail** — **Why It Matters** (the scene's link back to the protagonist's Desire/Misbelief — why these events matter to them personally), **Realization** (what the protagonist comes to understand), and **And So?** (what they do next, as a result of that realization)

Cards are independent of the binder tree: you can reorder them freely, create a card with no linked document yet (pure plotting, before you've drafted the scene), or link a card to a document that later gets renamed or deleted — the link just resolves to "not found" rather than breaking anything, the same way a dangling `[[wikilink]]` behaves.

### Story Grid

**`View > Story Grid`** opens a second, read-only view of the same cards as a table — one row per card, in whatever order its earliest linked document sits in the binder today, rather than the freeform order you set on the Corkboard.

Each row shows a computed manuscript position (`#`), the card's own `Scene #` label (unchanged, shown alongside rather than replaced), every one of its linked documents' titles, POV, and a word count summed across all of them (read live from disk, the same way the Metadata and Word Count panels do), and every field from the card — Cause, Effect, Why It Matters, Realization, And So, Prior Belief, New Belief, Value Shift, and subplot tags. The POV column prefers the card's own POV Character when it's set, falling back to the linked document's frontmatter POV otherwise.

Cards with no linked document, or where every link is stale, group into an **Unplaced** section — a toggle at the top of the panel puts that section above or below the placed rows. Unlike everything else on this page, that toggle is an app-wide preference, not a per-project one: it's remembered across every project you open, the same way UI Scale or your theme choice is. Clicking a linked document's title opens it in the Editor, same as Corkboard's own 🔗 link; clicking a row's Scene # opens the card editor.

The **POV** and **Words** columns are colored the same way the [Binder](#binder-background-coloring) colors its own rows: a colored dot next to the POV name whenever that POV has an assigned color, and the word count itself tinted along the same red→yellow→green gradient toward the (first resolved) document's word count target. Unlike the Binder, this coloring isn't mode-switched — it's always on, independent of whatever `Color Binder By` mode is currently active.

The Story Grid never reorders the manuscript itself — its row order is always a reflection of the binder, not something you can drag to change from here. To reorder scenes, reorder the documents in the Binder.

### Belief Timeline

**`View > Belief Timeline`** (`Ctrl+Shift+E`) shows one character's arc across the whole manuscript: pick a POV Character from the dropdown (populated from whatever names story cards have set in their own POV Character field — not the Metadata panel's POV dropdown source, since a card can describe a belief shift before any scene exists for it) and see their cards, in manuscript order, chained as Prior Belief → New Belief. A repeated belief that just restates the previous card's New Belief is skipped, so the chain reads as one continuous arc rather than restating itself. Cards with no resolvable linked document trail at the end. Clicking a card's linked scene opens it in the Editor, same as Story Grid.

If no story card has a POV Character set yet, the panel just says so — set one from the Corkboard card editor's header to start populating this view.

## Find and Replace

**`Edit > Find and Replace`** searches plain text across a chosen scope:

- Current file
- Current directory
- All unsaved (modified) files
- The whole project

Supports replace-one and replace-all.

## The Command Prompt

**`Tools > Command Prompt`** opens a Vim/Helix-style `:` command line. Arguments tab-complete where it makes sense (note titles, theme ids, plugin command names).

| Command | Effect |
|---|---|
| `:w` / `:write` | Save |
| `:q` / `:quit` | Quit |
| `:wq` / `:x` | Save and quit |
| `:o <title>` / `:open <title>` | Open a document by title |
| `:new <title>` | Create a new document |
| `:dmode <dark\|light\|system>` | Set the dark/light/system appearance |
| `:theme <id>` | Apply a color theme (see [Themes](#themes-and-appearance)) — no argument clears back to plain dark/light |
| `:find <text>` | Open Find and Replace pre-filled with `<text>` |
| `:tag <name>` | Open [Tags](#tags) pre-filtered to documents carrying `<name>` |
| `:git enable` | Turn on git support for this project |
| `:git commit [message]` | Commit; prompts for a message if omitted |
| `:git push` | Push |
| `:git pull` | Pull |
| `:git backup [message]` | Commit and push in one step |

Any `:` command a loaded plugin has registered also works here (see below) — plugin commands can never override a built-in name.

## Pomodoro Timer

**`Tools > Pomodoro Timer`** (or the remappable shortcut, `Ctrl+Alt+T` by default) opens a Pomodoro dock tab — the classic interval-timer technique for focused writing sessions, alternating fixed blocks of work and rest.

- **Start** / **Pause** / **Skip** / **Reset** control the current phase. Skip jumps to the next phase immediately, whatever's left on the clock; Reset returns to a fresh Work phase (it keeps your completed-session count for the day, it just stops the clock and rewinds the current phase).
- When a phase's time runs out, smaragd automatically switches to the next one — Work leads to a Short Break, except every *n*th Work session (configurable, default every 4th) leads to a Long Break instead — **and pauses**, rather than continuing to run unattended. Starting the next phase is always a deliberate action, not something that happens silently while you're away from the keyboard.
- **"Show a desktop notification when a phase completes"** ([Settings](#settings) > Pomodoro, off by default) fires an OS-level desktop notification the moment a phase ends on its own — useful if the app isn't in focus. It only fires on an automatic completion, never on a manual Skip, since you already know about that one. There's still no audible chime.

The timer keeps running whether or not its dock tab is open or visible (it's part of the app's state, not something tied to a window being shown), and a compact countdown — `⏱ Work 12:34` — shows in the status bar at the bottom of the window any time a session has been started, so it's visible at a glance without needing to switch tabs. It doesn't appear during [Focus Mode](#focus-mode), which hides the whole status bar; the dock tab itself is unaffected and still works there.

Durations default to the traditional 25 minutes of work, a 5-minute short break, and a 15-minute long break every 4 sessions — all four are adjustable in [Settings](#settings).

## Word Count

**`Tools > Word Count`** (or the remappable shortcut, `Ctrl+Alt+W` by default) opens a Word Count dock tab, Scrivener-style: a **Draft Target** for the whole manuscript and a **Session Target** for today's writing, each shown as a progress bar against the project's current word count.

- **Track** — a per-project toggle for what counts toward the total: **Manuscript folders only** (any folder holding the [Manuscript role](#folder-roles-research-trash-templates-manuscript), falling back to the whole project if none is assigned yet) or **Everything except Trash** (every document except Trash and Templates content). Trash and Templates are excluded either way — the toggle only changes whether tracking is scoped to your Manuscript folder(s) or opened up to the whole project.
- **Draft Target** — type a target word count to see a progress bar (current / target); leave it blank to hide the bar.
- **Session Target** — a separate, smaller target for today's writing, measured against words written since the session's baseline rather than the Draft Target's running total. The baseline rolls forward automatically at the start of a new calendar day, or immediately if you click **Reset Session**.
- **Characters typed this session** — a plain informational count, no target: every character you insert *or* delete in a tracked document adds to it, so typing 100 characters and then deleting them all reads 200, not a net 0. It only counts keystrokes in documents inside the current Track scope, resets when a project is opened or you click Reset Session, and isn't saved to disk — it doesn't survive quitting and relaunching smaragd.
- **Refresh** — recomputes the current word count immediately. There's also a dedicated remappable shortcut for this, **Refresh Word Count** (`F5` by default), so you don't need the panel open to force an update.

The total doesn't recompute on every keystroke or every frame — recomputing means re-reading every tracked document from disk, so it only happens on a handful of triggers (opening a project, a git pull, a folder-role or Track-scope change, an actual save, or an explicit Refresh) and always runs on a background thread so it never freezes the UI. Creating, deleting, moving, or renaming a document doesn't trigger a recompute on its own — click Refresh (or its shortcut) if the count looks stale after one of those.

A compact `340 : 12,345 / 50,000 words` segment (characters typed this session, then current/target words) shows in the status bar, next to the Pomodoro countdown, any time a Draft Target is set — the Session Target is dock-tab-only, not mirrored in the status bar.

## Writing Streak

**`Tools > Streak`** (or the remappable shortcut, `Ctrl+Alt+S` by default) opens a Streak dock tab that tracks whether you're keeping up with a self-set weekly writing schedule. Everything about it — including whether it's on at all — is configured **per project**, right inside the tab itself, not in the global Settings dialog: different projects can reasonably want different paces (or none at all).

The tab itself has two inner tabs, **Streak** and **Configure**, switchable freely at any time. Opening the tab picks a sensible starting one for you — **Streak** if the project already has tracking on, **Configure** if it doesn't (e.g. a project you've never set this up for) — but that's just the default; nothing snaps you back to it afterward.

- **Configure**: an enable checkbox ("Track a writing streak for this project"), a word target for each day of the week (0 for a day off), how a week counts as "met," and how many consecutive missed weeks turn the light red (default 2). All shown regardless of the enable flag, so you can set everything up before switching it on.
- **Streak**: once enabled, shows **Last completed week** — a traffic-light badge (green/yellow/red, or gray until there's a full week of history) judged only from fully completed Monday–Sunday weeks. It never reacts to today or the still-in-progress current week, so it can't turn red on a Monday morning before you've had a chance to write anything — and **Progress this week**, a live, purely informational progress bar for the current week's actual words vs. target that never changes the badge's color above it. If the project doesn't have tracking on yet, this tab instead shows a short message and a button that jumps to Configure.
- Two ways to judge whether a week was "met" (set in Configure): **Cumulative weekly total** (the week's total words meets the sum of that week's targets — a big Saturday can cover a missed Tuesday) or **Every day individually** (each day with a nonzero target must be met on its own).
- A compact traffic-light dot plus a live percentage (e.g. `● 45%`) mirrors both readings in the status bar (once enabled for the open project); clicking either opens the Streak tab.

**Streak counts the exact same words as the [Word Count](#word-count) panel's Track scope** — by default, **Manuscript folders only**. Words written in a folder outside your Manuscript folder(s) (Research, a Characters note, a loose file at the project root, etc.) won't count toward your streak at all, even though they're saved to disk — easy to miss the first time you test it. Switch Track to **Everything except Trash** in the Word Count panel if you want every document to count toward the streak too.

## Collaboration

The **`Collaborate`** menu (and its dockable **Collaboration Panel**, `Ctrl+Shift+L`) lets two people edit the same document together in real time, peer-to-peer — no server, no account, no third-party service ever holds the manuscript text.

### Hosting a session

1. Open the document you want to collaborate on.
2. **`Collaborate > Host Session`** (needs a document open; disabled otherwise).
3. Smaragd generates a one-time **connection code** and shows it in the Collaboration Panel. **Copy** it and send it to your collaborator through whatever channel you'd already trust with the document itself — chat, email, whatever.
4. The panel shows "Waiting for a peer to join…" until they do.

### Joining a session

1. **`Collaborate > Join Session…`** (or **Join Session…** in the panel itself) — needs *no* document currently open, since the shared document a join receives replaces whatever was there, not merges with one of your own files. Close your current document first if one's open.
2. Paste the code your collaborator sent you and confirm.
3. Once paired, the host's document appears in your editor and either side can type — edits from both sides merge automatically.

### While connected

Both sides just type normally in the Editor tab; there's no separate "collaboration mode" to the editing experience itself. Under the hood, each side's edits are diffed against a shared baseline and merged with a CRDT (the same category of algorithm behind Google Docs/Yjs), so concurrent edits from both people — even to the same paragraph — converge to the same result on both sides without overwriting each other or needing a manual conflict resolution step. When a remote edit comes in, your local cursor position is adjusted to stay put relative to the surrounding text rather than jumping.

The panel shows **Connected to peer `<fingerprint>`** once pairing completes — a short id derived from the peer's network identity, useful for confirming you're connected to who you think you are, not a name either side chooses. **End Session** stops collaborating; the document itself is unaffected and stays open normally afterward.

What opening a different document does depends on which side you're on. If you're **hosting**, switching to another document keeps the session running — your collaborator's view follows along to the new document automatically, with a status message ("Your collaborator switched documents") to explain why their editor content just changed. If you're the one who **joined**, opening one of your own documents has nowhere to put the shared one, so you're asked to confirm first: decline and the shared document keeps showing with the session still live, confirm and the session ends before your document opens. Either side **closing** the current document, or your collaborator's connection dropping (network loss, or they closed their side), still ends the session immediately with no prompt — the panel then shows **"Lost connection to your collaborator"** for a drop. There's no automatic reconnection: start a fresh **Host Session**/**Join Session…** to resume.

### Privacy and security

- **No server holds your text.** Peers connect directly to each other via [iroh](https://iroh.computer) (falling back to iroh's relay infrastructure only to help establish that direct connection when needed, the same way most peer-to-peer / video-call tools do) — the manuscript itself is never uploaded anywhere or stored by a third party.
- **End-to-end encrypted**, on top of iroh's own transport encryption: every edit exchanged between peers is additionally encrypted with a key derived from a secret that exists only inside the connection code itself, so even iroh's own relay infrastructure can't read the content it's helping relay.
- **The connection code is the credential.** Whoever holds it can join the session — treat it like a password for as long as the session is open, and don't post it somewhere public. Joining requires proving you hold the secret from the code before the other side ever reports you as connected, so a stranger who reaches the host's network endpoint without the code can neither read the session nor block the real collaborator from pairing.
- Each session's encryption keys are freshly derived per session and tied to that specific connection code — an old code from a past session can't be reused to rejoin a new one.

## Plugins

Smaragd can be extended with small scripts written in [Rhai](https://rhai.rs), an embedded scripting language. A plugin script can:

1. Register a custom `:` command
2. Define an `on_save(text)` hook that transforms a document's text right before an explicit save

### Where plugins live

- **Global**, always loaded: `plugins/` inside smaragd's config directory
  - Linux: `~/.config/smaragd/plugins`
  - macOS: `~/Library/Application Support/smaragd/plugins`
  - Windows: `%APPDATA%\smaragd\config\plugins`
- **Per-project**: `.smaragd/plugins/` inside the project folder. This only loads once you explicitly turn it on for that project via **`Tools > Enable Project Plugins`** — a project folder shared or pulled from somewhere else could otherwise run unreviewed code the moment you open it.

Use **`Tools > Reload Plugins`** to pick up new or edited scripts without restarting the app. A script that fails to compile or run, or that tries to register a `:` command another plugin already owns, is skipped with an error message — it never stops other plugins from loading.

### ⚠️ No sandbox

A loaded plugin can shell out to any program on your system, with the same access as anything else run under your own user account — there's no restricted execution environment. Only load plugins whose code you trust, and treat the project-plugin opt-in as a real trust decision, not a formality.

### Host functions available to a script

- `smaragd_status(msg)` — show `msg` in the status bar
- `smaragd_document_text()` — returns the open document's current text
- `smaragd_document_basename()` — returns the open document's file name (without its `.md` extension), or an empty string if nothing's open
- `smaragd_document_filename()` — returns the open document's path relative to the project root, `.md` extension included (e.g. `Part 1/Scene 5.md`), or an empty string if nothing's open
- `smaragd_set_document_text(text)` — replaces the open document's text
- `smaragd_run_command(cmd, args)` — runs `cmd` (an array of string `args`) as a subprocess, waits for it to finish, and returns a map with `stdout`, `stderr`, `exit_code`, and `success`. Runs in the open project's root, and blocks the app's UI until the process exits — avoid anything long-running.
- `register_command(name, fn_name)` — called once at script load time to expose a `:` command
- `register_shortcut(name, key_spec)` — called at script load time to give a registered `:` command a default keyboard shortcut, e.g. `register_shortcut("hello", "ctrl+shift+h")`. `key_spec` is `+`-separated modifiers (`ctrl`/`cmd`/`command`, `shift`, `alt`/`option` — case-insensitive) followed by a key name (`k`, `F2`, `Enter`, `Colon`, ...). A bare key with no modifier is rejected unless it's a function key or Escape, same rule as built-in shortcuts.

### Example: a custom `:` command

```rhai
fn say_hello(arg) {
    smaragd_status("Hello, " + arg + "!");
}
register_command("hello", "say_hello");
register_shortcut("hello", "ctrl+shift+h");
```

Typing `:hello world` in the command prompt calls `say_hello("world")` and shows "Hello, world!" in the status bar. Everything after the command name is passed as a single string argument. Pressing `Ctrl+Shift+H` runs the same command with an empty argument.

Whatever shortcut a script asks for is just a *default*: **`File > Settings`** lists every plugin command that registered one, alongside the built-in shortcuts, and lets you remap or unbind it exactly the same way. If a script's requested combo is already in use by a built-in action or another plugin command, it's simply left unbound (with a message explaining why) rather than stealing it — you can still assign it a free combo yourself from Settings.

### Example: shelling out to a tool

```rhai
fn wordcount(arg) {
    let result = smaragd_run_command("wc", ["-w"]);
    smaragd_status("Words: " + result.stdout);
}
register_command("wordcount", "wordcount");
```

### Example: an `on_save` hook

```rhai
fn on_save(text) {
    text.trim() + "\n"
}
```

This runs before every explicit save (`:w` / `Ctrl+S` / `:wq`), in plugin-load order, each hook's output feeding the next. Return a `String` to replace the saved text; return anything else (typically nothing) to leave it unchanged. If a hook throws, that plugin's change is dropped and an error is shown — a broken plugin can never block a save.

Note: `on_save` only runs on those explicit save actions — not the focus-loss autosave, and not the save-before-switching-documents path.

## Git Integration

Modeled on the Obsidian Git plugin. **`Versions` menu**, or **`:git`** commands:

- Opt-in per project — you're offered "Enable Git Support" once when a project is opened, or you can trigger it manually (`Versions > Enable Git Support` or `:git enable`)
- Commit / Commit and Push / Push / Pull — shells out to the system `git` binary
- Push and pull run on a background thread, so a slow or hung network operation never freezes the UI

## Themes and Appearance

Two independent settings:

- **Appearance** (`File > Settings`, or `:dmode <dark|light|system>`): plain Dark/Light/System styling. System follows your OS preference and updates immediately.
- **Color Theme** (`View > Theme`, or `:theme <id>`): a full Helix-inspired palette layered on top of the appearance base. `:theme` with no argument clears back to plain appearance styling.

Since the editor is a single plain-text field with no syntax-highlighting pipeline, each theme reproduces its palette's overall look (background, body text, one accent color for selection/links) rather than full per-token syntax highlighting.

### Built-in themes

| id | Label |
|---|---|
| `gruvbox` | Gruvbox |
| `gruvbox_light` | Gruvbox Light |
| `dracula` | Dracula |
| `nord` | Nord |
| `nord_light` | Nord Light |
| `solarized_dark` | Solarized Dark |
| `solarized_light` | Solarized Light |
| `catppuccin_mocha` | Catppuccin Mocha |
| `catppuccin_latte` | Catppuccin Latte |
| `onedark` | One Dark |
| `onelight` | One Light |
| `tokyonight` | Tokyo Night |
| `everforest_dark` | Everforest Dark |
| `everforest_light` | Everforest Light |
| `ayu_dark` | Ayu Dark |

### Custom themes

You can add your own themes as `.toml` files — no in-app editor, the same "drop a file in a folder" model as [Plugins](#plugins). Custom themes live in `smaragd/themes/` inside smaragd's config directory (the same base path as the global plugins folder):

- Linux: `~/.config/smaragd/themes`
- macOS: `~/Library/Application Support/smaragd/themes`
- Windows: `%APPDATA%\smaragd\config\themes`

A minimal custom theme:

```toml
id = "my_theme"
label = "My Theme"
dark = true
background = "#1e1e2e"
foreground = "#cdd6f4"
accent = "#cba6f7"
```

`id`, `label`, `dark`, and the three colors are required; colors are `"#RRGGBB"` hex strings (the `#` is optional). `id` is what you'd type as `:theme my_theme` — it's lowercased automatically, so casing in the file doesn't matter.

You can optionally also override the markdown preview's heading-color ladder, wikilink color, and quote-bar color (otherwise a fixed dark/light pair used by every theme, built-in or custom, that doesn't specify its own):

```toml
[preview]
heading = ["#f38ba8", "#89b4fa", "#a6e3a1", "#cba6f7", "#f9e2af", "#fab387"]
wikilink = "#a6e3a1"
quote_bar = "#6c7086"
```

`heading` needs all six colors (one per heading level, `h1`–`h6`); `wikilink` and `quote_bar` are independent of each other and of `heading` — include only the ones you want to override.

Use **`View > Theme > Reload Custom Themes`** to pick up a new or edited file without restarting the app. A theme file that fails to parse, has an invalid color, or whose `id` collides with an already-loaded theme (built-in or another custom one — whichever loaded first wins) is skipped with an error message rather than stopping other themes from loading. If the theme you currently have active stops resolving after a reload (for instance, you just introduced a mistake into the file you're editing), smaragd falls back to the default appearance rather than leaving a stale palette applied with nothing in the menu showing as selected.

### Editor and Preview Font

**`File > Settings`** has a **Font** section with one shared font and size, used by both the Editor and the Preview — not independent settings for each, so what you write in looks the same as what you preview.

| Font | What it looks like |
|---|---|
| Proportional | egui's built-in sans-serif — the default |
| Monospace | egui's built-in fixed-width face |
| Libertinus Serif | A literary serif text font — the same one used by the [Trade Paperback export style](#typesetting-styles) |
| DejaVu Sans Mono | A fixed-width face — the same one used by the [Manuscript export style](#typesetting-styles) |

These four are the only choices — not a live picker over every font installed on your system, so the app looks and behaves identically on every platform. Code blocks in the Preview always render in a fixed-width face regardless of this setting, matching how virtually every other markdown renderer treats code.

## Keyboard Shortcuts

All shortcuts are fully remappable in **`File > Settings`**, listed with a Category column (Application, Project, Files & Folders, Editing, View, Git, Tools) and sorted by category, then alphabetically within each. Defaults below use `Ctrl` (shown as `Cmd` on macOS):

| Action | Default shortcut |
|---|---|
| New Project | `Ctrl+Alt+N` |
| Open Project | `Ctrl+O` |
| Close Project | `Ctrl+Shift+W` |
| Settings | `Ctrl+,` |
| Exit | `Ctrl+Q` |
| Toggle Preview | `Ctrl+Shift+P` |
| Save | `Ctrl+S` |
| New File | `Ctrl+N` |
| New Folder | `Ctrl+Shift+F` |
| Rename | `F2` |
| Delete | `Ctrl+Shift+Backspace` |
| Restore | `Ctrl+Shift+R` |
| Toggle Dark/Light Mode | `Ctrl+Shift+D` |
| Toggle Full Screen | `F11` |
| Find and Replace | `Ctrl+F` |
| Toggle Corkboard | `Ctrl+Shift+K` |
| Toggle Story Grid | `Ctrl+Shift+G` |
| Toggle Belief Timeline | `Ctrl+Shift+E` |
| Cycle Binder Color Mode | `Ctrl+Shift+C` |
| Toggle Backlinks | `Ctrl+Shift+B` |
| Toggle Tags | `Ctrl+Shift+T` |
| Command Prompt | `Ctrl+:` |
| Commit (Git) | `Ctrl+Alt+C` |
| Push (Git) | `Ctrl+Alt+P` |
| Metadata | `Ctrl+Shift+M` |
| Activate Wikilink | `Ctrl+Enter` |
| Toggle Binder/Editor Focus | `F6` |
| Toggle Focus Mode | `F9` |
| Open Document | `Ctrl+P` |
| Close Document | `Ctrl+W` |
| Toggle Pomodoro Timer | `Ctrl+Alt+T` |
| Toggle Word Count | `Ctrl+Alt+W` |
| Refresh Word Count | `F5` |
| Toggle Collaboration Panel | `Ctrl+Shift+L` |
| Toggle Streak Tracking | `Ctrl+Alt+S` |

Two shortcuts can never overlap — rebinding one to a combo another action already owns automatically un-assigns it from the previous owner. This holds across built-ins and plugin shortcuts alike: if a loaded plugin registered a `:` command with its own shortcut (see [Plugins](#plugins)), it shows up in its own "Plugin Shortcuts" section further down the same window, remappable/unbindable the same way.

## Notifications

Smaragd tells you about things two different ways, depending on how much attention they need:

- **Toasts** — a stack of boxes in the top-right corner of the window, each with its own **×** to dismiss early, that fade away on their own after a few seconds otherwise. Used for anything that represents an actual problem: a failed save, export, or git operation; invalid frontmatter YAML (see [Document Metadata](#document-metadata-frontmatter)); a plugin error; and so on. Several can stack up at once if more than one thing goes wrong in quick succession.
- **The status bar**, at the bottom of the window — a single line for routine confirmations that don't need to grab your attention: "Committed", "Exported to ...", "Replaced 3 occurrence(s)", and the like. It now clears itself automatically after a few seconds, rather than sitting there until the next unrelated status update happens to overwrite it.

Both durations are configurable — see **Notifications** under [Settings](#settings) below.

## Settings

**`File > Settings`** (or **`Ctrl+,`**) is a two-pane dialog, IntelliJ-style: a category list on the left (General, Appearance, Editor, Templates, Pomodoro, Shortcuts), `Up`/`Down` to move between categories, and that category's controls on the right. Settings are stored as `smaragd.toml` in the platform's config directory (the same base path as the global plugins, custom-themes, custom-styles, and custom-project-templates folders — see [Plugins](#plugins), [Custom themes](#custom-themes), [Typesetting styles](#typesetting-styles), and [Project Templates](#project-templates)). Writing Streak is *not* here — it's configured per project, inside its own dock tab (see [Writing Streak](#writing-streak)).

- **General**: **Reopen project on launch** (off by default), **Ensure Research and Trash folders exist in every project** (off by default; see [Projects](#projects)), and a **Notifications** section with **Error toast duration** and **Status bar message duration** (1–60 seconds each, defaulting to 6 and 8 respectively — see [Notifications](#notifications) above)
- **Appearance**: Dark/Light/System and Color Theme (see [Themes](#themes-and-appearance)), plus **UI Scale** — a manual multiplier (50%–300%, default 100%) on top of whatever scaling your OS/display server already reports, for the rare case that comes back wrong (some Wayland compositors don't report a scale winit picks up, leaving the whole UI tiny with no apparent way to fix it). Takes effect immediately; **Reset** clears it back to 100%
- **Editor**: font + size (see [Editor and Preview Font](#editor-and-preview-font)) and **Typewriter quotes in Preview and export** (off by default; see [Typewriter Quotes](#typewriter-quotes))
- **Templates**: date format for `${{date}}` — see [Template Variables](#template-variables)
- **Pomodoro**: durations (work/short break/long break minutes, and sessions before a long break), plus a desktop-notification toggle for automatic phase completions — see [Pomodoro Timer](#pomodoro-timer)
- **Shortcuts**: remap or unbind any action, including a fullscreen toggle

If the settings file is missing or its contents can't be parsed, smaragd falls back to defaults rather than failing to start.
