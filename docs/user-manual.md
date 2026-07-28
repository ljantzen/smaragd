# Tachylite User Manual

Tachylite is a desktop writing tool for long-form fiction and other manuscripts organized as a folder of Markdown files. It borrows ideas from Scrivener (binder, folder roles, templates), Longform/Obsidian (frontmatter metadata, wikilinks), Lisa Cron's *Story Genius* (structured story cards), and Helix (color themes, `:` command prompt).

This manual covers what the app does and how to use it. For internals (source layout, build/test commands), see the main [README](../README.md).

## Contents

- [Projects](#projects)
- [Dockable Tool Windows](#dockable-tool-windows)
- [The Binder](#the-binder)
- [Writing and the Editor](#writing-and-the-editor)
- [Focus Mode](#focus-mode)
- [Markdown Preview](#markdown-preview)
- [Wikilinks](#wikilinks)
- [Backlinks](#backlinks)
- [Document Metadata (Frontmatter)](#document-metadata-frontmatter)
- [Folder Roles: Research, Trash, Templates](#folder-roles-research-trash-templates)
- [Export](#export)
- [Story Cards (Corkboard)](#story-cards-corkboard)
- [Find and Replace](#find-and-replace)
- [The Command Prompt](#the-command-prompt)
- [Plugins](#plugins)
- [Git Integration](#git-integration)
- [Themes and Appearance](#themes-and-appearance)
- [Keyboard Shortcuts](#keyboard-shortcuts)
- [Settings](#settings)
- [Current Limitations](#current-limitations)

## Projects

A **project** is just a folder on disk containing `.md` files and subfolders, marked with a `.tachylite/project.json` file. There's no proprietary bundle format — you can open the folder in any other editor, sync it with any tool, and everything still works.

- **`File > New Project`** opens a native folder picker and asks for a name; it creates the folder and marks it as a project.
- **`File > Open Project`** opens a native folder picker. If you point it at a folder tachylite hasn't used before, it offers to adopt the folder in place (writing the `.tachylite` marker) rather than refusing.
- `.tachylite/project.json` stores things the filesystem can't express on its own — manuscript ordering, folder roles, whether plugins/git are enabled for this project. If that file's *contents* ever get corrupted, tachylite falls back to defaults rather than erroring; only a missing marker means "this isn't a project yet."

`File > Settings` has a **"Reopen project on launch"** option, and a separate **"Ensure Research and Trash folders exist in every project"** option (off by default) that creates those two role folders automatically whenever you open a project, recreating them at their original path if they were deleted since.

## Dockable Tool Windows

The **Binder**, **Backlinks**, **Document Metadata**, **Editor**, **Preview**, and **Corkboard** views are all one shared dockable layout — similar to the Properties window in Visual Basic's IDE — rather than a mix of fixed panels, modals, and mutually-exclusive view modes. You can:

- **Drag a tab's title** onto empty space to pop it out into its own floating window
- **Drag a floating window's title back** onto the dock area to re-dock it
- **Drag one tab onto another** to group them together, switching between them like browser tabs
- **Drag a tab to an edge** of another tab or the dock area to split the layout and place it side by side
- **Resize** the dock area, or a floating window, by dragging its edge

Binder and Editor are present from the moment a project is open; Backlinks, Metadata, Preview, and Corkboard start closed. Any tab can be closed via its × button, and reopened again from **`View > Binder`**, **`View > Backlinks`**, **`View > Preview`**, **`View > Corkboard`**, or **`Edit > Document Metadata`** (most also have shortcuts — see [Keyboard Shortcuts](#keyboard-shortcuts)). Toggling Preview or Corkboard just opens or closes that tab next to the Editor rather than switching to an exclusive "view mode" — any combination of tabs can be open and arranged at once.

The whole arrangement — which tabs are open, how they're split or floated, and window position/size — persists across restarts. **`Window`** menu:

- **Save Current Layout…** — names and saves the current arrangement
- **Layouts** — lists saved layouts; pick one to switch to it
- **Restore Default Layout** — resets to the original Binder-left/Editor-right split, with the Editor occupying the majority of the space

## The Binder

The left-hand panel is the **binder** — a tree view of your project folder, one of the dockable tool windows described above. It's `.gitignore`-aware, and documents are shown without their `.md` extension.

- **Navigate by keyboard**: click a row (or Tab to it) to give it focus, then:
  - `Up`/`Down` moves between rows
  - `Left`/`Right` collapses/expands a focused folder
  - `Enter` opens the focused document
- **Drag and drop** a file or folder *onto* another folder to move it there. Drag it *onto another document* instead to reorder — dropping it just before that document, within the same folder — without changing which folder it's in.
- **`F6`** (the remappable "Toggle Binder/Editor Focus" shortcut) jumps keyboard focus back and forth between the binder and the editor/preview, without touching the mouse.
- **Right-click** a row for a context menu:
  - **New File** / **New Folder** / **New From Template** (folders only — see [Templates](#folder-roles-research-trash-templates)) — each prompts for a name (`Enter` to confirm)
  - **Rename** — also prompts for a name, and updates any `[[wikilinks]]` elsewhere in the project that pointed at the old name
  - **Delete** — shows a native confirmation dialog; if a Trash folder is configured, it's worded as a move to Trash rather than a permanent delete
  - **Restore** (on a trashed item) — moves it back to its original folder, offering to recreate that folder if it's gone since
  - **Folder Role** / **Empty Trash** (folders only) — see below

## Writing and the Editor

The main panel is a plain-text Markdown editor.

- **`Ctrl+S`** (or **`Cmd+S`** on macOS) saves explicitly. The document also saves automatically when it loses focus (e.g. you click into the binder or another panel).
- There's currently no multi-tab editing — opening a document replaces whatever's currently open (saving it first if it has unsaved changes).
- **`File > Open Document…`** (or **`Ctrl+P`**) opens an fzf-style quick-switcher: type a few letters and it fuzzy-matches against every document's path, best match first — a query doesn't need to be a contiguous substring, so e.g. "ch1sc2" can match "Chapter 1/Scene 2". Use `Up`/`Down` to change the highlighted result, `Enter` or a click to open it, `Escape` to cancel.
- **`File > Close Document`** (or **`Ctrl+W`**) saves the current document if it has unsaved changes, then closes it — there's no save/discard/cancel prompt, matching the same silent-autosave behavior as opening a different document.

## Focus Mode

**`View > Focus Mode`** (or **`F9`**) is a distraction-free writing mode, similar to Scrivener's Composition Mode: the window maximizes and all chrome — menu bar, binder, other dock tabs — disappears, leaving just the current document centered in the available width. The paragraph your cursor is in stays at full brightness while other paragraphs dim, a typewriter-style aid for keeping your eye on the sentence you're actually writing.

Focus Mode needs an open document to enter — with nothing open there's nothing to focus on. Press `Escape` or `F9` again to exit and return to the normal layout.

## Markdown Preview

**`View > Preview`** (or the Toggle Preview shortcut) renders the current document in a Glow-CLI-inspired style: a colored heading hierarchy, barred blockquotes, boxed code blocks, striped GFM tables, and images.

Images work two ways:
- Standard Markdown: `![alt](path/to/image.png)`
- Obsidian-style embeds: `![[image.png]]`

Relative image paths resolve against the open document's own folder, and must stay inside the project (a path that tries to escape the project root — via `..` or a symlink — is refused). Remote `http(s)://` images are never fetched.

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

Open **`Edit > Document Metadata`** (or **`Ctrl+Shift+M`**) to edit these fields through a dockable form (see [Dockable Tool Windows](#dockable-tool-windows)) instead of hand-editing YAML. Unlike a typical dialog, there's no Save/Cancel step — edits apply as you type, the same way typing in the main editor does. Tachylite only ever reads/writes these five keys:

| Field | Meaning |
|---|---|
| `type` | Free-form section type — "Chapter", "Scene", "Part", or anything you want. Not tied to folder nesting. |
| `status` | Free-form drafting status — "draft", "revised", "final", or anything you want. |
| `pov` | Point-of-view character, free text. |
| `word_count_target` | A target word count for this document. |
| `tags` | A list of free-form tags. |

Any other YAML key you've hand-added to the block (or that some other tool wrote) is left alone — Tachylite never round-trips the whole block through its own data model, so unrelated keys survive a save untouched. The frontmatter block is stripped from the Markdown preview so it doesn't render as a garbled paragraph.

## Folder Roles: Research, Trash, Templates

Right-click a folder and choose **Folder Role** to designate it as one of three special folders. At most one folder can hold each role, project-wide.

- **Trash**: deleting a file or folder moves it here instead of removing it from disk. Right-click the Trash folder for **Empty Trash** (permanent, with confirmation), or right-click a trashed item for **Restore**.
- **Templates**: any document placed directly inside this folder (not in a subfolder of it) shows up in every other folder's right-click **"New From Template"** submenu. Picking one creates a new document that's a verbatim copy — frontmatter included — after prompting you for a name. The template itself is never modified.
- **Research**: currently just a marker with no behavior yet attached — reserved for future features like word-count rollups. Unlike Trash and Templates, [Export](#export) does *not* skip a Research-role folder — right-clicking one to export it exports it like any other folder.

## Export

Right-click any folder in the binder and choose **Export…** to compile it — and everything nested inside it, in the same top-to-bottom order shown in the binder — into a single DOCX, EPUB, or print-ready PDF file. A nested folder whose role is **Trash** or **Templates** is skipped automatically, so deleted or template content never accidentally ends up in a compiled manuscript.

The export dialog has:

- **Title** / **Author** — plain book metadata, remembered for next time.
- **Style** — a dropdown of typesetting styles (see below). Fonts, page size, running headers, and drop caps all come from whichever style is selected, not from anything typed into this dialog.
- **Export as DOCX…** / **Export as EPUB…** / **Export as Print PDF…** — each opens a native "Save As" dialog, then compiles.

All three formats read from the *same* style, so switching styles changes DOCX, EPUB, and PDF output alike — closer to how a book-design tool like Deckle Studio treats "one style set drives every output" than to a plain markdown-to-Word converter.

### Typesetting styles

Two built-in styles ship with tachylite:

| id | Label | What it looks like |
|---|---|---|
| `manuscript` | Manuscript | Plain submission format: US Letter, 1in margins, double-spaced, ragged-right (not justified), no running header or drop cap |
| `trade_paperback` | Trade Paperback | 6×9in trim, justified body text, a running header (author's name / current chapter), and a drop cap on each chapter's first paragraph |

Like [color themes](#custom-themes) and plugins, custom styles are `.toml` files you author or drop into `tachylite/styles/` inside tachylite's config directory (no in-app style editor):

- Linux: `~/.config/tachylite/styles`
- macOS: `~/Library/Application Support/tachylite/styles`
- Windows: `%APPDATA%\tachylite\config\styles`

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

`{title}`/`{author}` are substituted with whatever's typed into the export dialog; `{chapter}` (supported as a whole side's content, not mixed with other text) shows the current chapter on the print PDF specifically — DOCX and EPUB don't have a per-page "current chapter" concept, so a `{chapter}` token is just left blank there.

**"Libertinus Serif" and "DejaVu Sans Mono"** (the built-in styles' fonts) aren't arbitrary choices — they're guaranteed available to the PDF renderer specifically, bundled with tachylite itself rather than depending on what's installed on your system. A custom style naming some other font still works for DOCX/EPUB (which just reference a font by name, the same way any other document does — Word/an e-reader substitutes if it's not installed), and for PDF too if that font happens to be installed locally; if not, the PDF falls back to *some* available font rather than failing the export.

Use **Reload Custom Styles** in the export dialog to pick up a new or edited `.toml` file without restarting. A style file that fails to parse, or whose `id` collides with an already-loaded style (built-in or another custom one — whichever loaded first wins), is skipped with an error message rather than stopping other styles from loading.

### The print PDF specifically

Unlike DOCX/EPUB (which place text on the page or in an XHTML flow), the PDF target is real typesetting: tachylite embeds the [Typst](https://typst.app) compiler directly (no separate install, no network access) and generates a Typst document from your manuscript and the chosen style, then lets Typst do the actual page layout — the same category of tool as LaTeX or InDesign, not a "print to PDF" of a web page.

That gets you, for free or close to it: automatic widow/orphan avoidance (Typst's default), a running header that tracks which chapter you're actually on per page, and a drop cap rendered as an oversized inline initial letter (a *raised* cap — it doesn't wrap subsequent lines around it the way a true sunk drop cap does; that needs either a Typst package fetched over the network, which tachylite deliberately avoids, or more elaborate manual layout math than a v1 warrants).

After a successful PDF export, the status bar reports an estimated spine width for the resulting page count — useful for sizing a paperback cover, but a rough estimate based on a standard white-paper thickness constant, not a print-broker-grade figure. Confirm against your printer's own spine-width calculator (e.g. KDP's) before sending a cover to print.

### What export doesn't do (yet)

- No per-block styling for verse, dialogue, or other special block types — the markdown parser has no such concept today, only headings/paragraphs/quotes/lists/tables/code/images.
- EPUB output is one general-purpose file, not separately tuned per e-reader (Kindle/Apple Books/Kobo).
- Wikilinks resolve to a real in-book link in EPUB, when the target document is also part of the same export — otherwise (and always, in DOCX) they render as plain text.

## Story Cards (Corkboard)

**`View > Corkboard`** opens a wrapping grid of scene cards, modeled on Lisa Cron's *Story Genius* method — a structured cause-and-effect breakdown rather than a freeform synopsis.

At the top of the Corkboard, two project-wide fields capture what Cron calls the "Third Rail" — the protagonist's driving force, not tied to any one scene:

- **Desire** — the external/internal want the protagonist is pursuing
- **Misbelief** — the flawed, usually childhood-formed belief standing in its way

Every scene card below is meant to test or advance this pair. Each card has:

- **Alpha Point** — the scene's core moment
- **Cause** — the external event that occurs
- **Effect** — the external and internal consequence of the cause
- **Why It Matters** — the scene's link back to the protagonist's Desire/Misbelief — why these events matter to them personally
- **Realization** — what the protagonist comes to understand
- **And so?** — what the protagonist does next, as a result of that realization
- Optional **subplot tags**
- An optional soft link to a manuscript document, by title

Cards are independent of the binder tree: you can reorder them freely, create a card with no linked document yet (pure plotting, before you've drafted the scene), or link a card to a document that later gets renamed or deleted — the link just resolves to "not found" rather than breaking anything, the same way a dangling `[[wikilink]]` behaves.

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
| `:git enable` | Turn on git support for this project |
| `:git commit [message]` | Commit; prompts for a message if omitted |
| `:git push` | Push |
| `:git pull` | Pull |
| `:git backup [message]` | Commit and push in one step |

Any `:` command a loaded plugin has registered also works here (see below) — plugin commands can never override a built-in name.

## Plugins

Tachylite can be extended with small scripts written in [Rhai](https://rhai.rs), an embedded scripting language. A plugin script can:

1. Register a custom `:` command
2. Define an `on_save(text)` hook that transforms a document's text right before an explicit save

### Where plugins live

- **Global**, always loaded: `plugins/` inside tachylite's config directory
  - Linux: `~/.config/tachylite/plugins`
  - macOS: `~/Library/Application Support/tachylite/plugins`
  - Windows: `%APPDATA%\tachylite\config\plugins`
- **Per-project**: `.tachylite/plugins/` inside the project folder. This only loads once you explicitly turn it on for that project via **`Tools > Enable Project Plugins`** — a project folder shared or pulled from somewhere else could otherwise run unreviewed code the moment you open it.

Use **`Tools > Reload Plugins`** to pick up new or edited scripts without restarting the app. A script that fails to compile or run, or that tries to register a `:` command another plugin already owns, is skipped with an error message — it never stops other plugins from loading.

### ⚠️ No sandbox

A loaded plugin can shell out to any program on your system, with the same access as anything else run under your own user account — there's no restricted execution environment. Only load plugins whose code you trust, and treat the project-plugin opt-in as a real trust decision, not a formality.

### Host functions available to a script

- `tachylite_status(msg)` — show `msg` in the status bar
- `tachylite_document_text()` — returns the open document's current text
- `tachylite_set_document_text(text)` — replaces it
- `tachylite_run_command(cmd, args)` — runs `cmd` (an array of string `args`) as a subprocess, waits for it to finish, and returns a map with `stdout`, `stderr`, `exit_code`, and `success`. Runs in the open project's root, and blocks the app's UI until the process exits — avoid anything long-running.
- `register_command(name, fn_name)` — called once at script load time to expose a `:` command
- `register_shortcut(name, key_spec)` — called at script load time to give a registered `:` command a default keyboard shortcut, e.g. `register_shortcut("hello", "ctrl+shift+h")`. `key_spec` is `+`-separated modifiers (`ctrl`/`cmd`/`command`, `shift`, `alt`/`option` — case-insensitive) followed by a key name (`k`, `F2`, `Enter`, `Colon`, ...). A bare key with no modifier is rejected unless it's a function key or Escape, same rule as built-in shortcuts.

### Example: a custom `:` command

```rhai
fn say_hello(arg) {
    tachylite_status("Hello, " + arg + "!");
}
register_command("hello", "say_hello");
register_shortcut("hello", "ctrl+shift+h");
```

Typing `:hello world` in the command prompt calls `say_hello("world")` and shows "Hello, world!" in the status bar. Everything after the command name is passed as a single string argument. Pressing `Ctrl+Shift+H` runs the same command with an empty argument.

Whatever shortcut a script asks for is just a *default*: **`File > Settings`** lists every plugin command that registered one, alongside the built-in shortcuts, and lets you remap or unbind it exactly the same way. If a script's requested combo is already in use by a built-in action or another plugin command, it's simply left unbound (with a message explaining why) rather than stealing it — you can still assign it a free combo yourself from Settings.

### Example: shelling out to a tool

```rhai
fn wordcount(arg) {
    let result = tachylite_run_command("wc", ["-w"]);
    tachylite_status("Words: " + result.stdout);
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

You can add your own themes as `.toml` files — no in-app editor, the same "drop a file in a folder" model as [Plugins](#plugins). Custom themes live in `tachylite/themes/` inside tachylite's config directory (the same base path as the global plugins folder):

- Linux: `~/.config/tachylite/themes`
- macOS: `~/Library/Application Support/tachylite/themes`
- Windows: `%APPDATA%\tachylite\config\themes`

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

Use **`View > Theme > Reload Custom Themes`** to pick up a new or edited file without restarting the app. A theme file that fails to parse, has an invalid color, or whose `id` collides with an already-loaded theme (built-in or another custom one — whichever loaded first wins) is skipped with an error message rather than stopping other themes from loading. If the theme you currently have active stops resolving after a reload (for instance, you just introduced a mistake into the file you're editing), tachylite falls back to the default appearance rather than leaving a stale palette applied with nothing in the menu showing as selected.

## Keyboard Shortcuts

All shortcuts are fully remappable in **`File > Settings`**, listed with a Category column (Application, Project, Files & Folders, Editing, View, Git, Tools) and sorted by category, then alphabetically within each. Defaults below use `Ctrl` (shown as `Cmd` on macOS):

| Action | Default shortcut |
|---|---|
| New Project | `Ctrl+Alt+N` |
| Open Project | `Ctrl+O` |
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
| Toggle Backlinks | `Ctrl+Shift+B` |
| Command Prompt | `Ctrl+:` |
| Commit (Git) | `Ctrl+Alt+C` |
| Push (Git) | `Ctrl+Alt+P` |
| Document Metadata | `Ctrl+Shift+M` |
| Activate Wikilink | `Ctrl+Enter` |
| Toggle Binder/Editor Focus | `F6` |
| Toggle Focus Mode | `F9` |
| Open Document | `Ctrl+P` |
| Close Document | `Ctrl+W` |

Two shortcuts can never overlap — rebinding one to a combo another action already owns automatically un-assigns it from the previous owner. This holds across built-ins and plugin shortcuts alike: if a loaded plugin registered a `:` command with its own shortcut (see [Plugins](#plugins)), it shows up in its own "Plugin Shortcuts" section further down the same window, remappable/unbindable the same way.

## Settings

Settings are stored as `tachylite.toml` in the platform's config directory (the same base path as the global plugins, custom-themes, and custom-styles folders — see [Plugins](#plugins), [Custom themes](#custom-themes), and [Typesetting styles](#typesetting-styles)). Available in **`File > Settings`**:

- **Reopen project on launch** — automatically reopens the last project you had open (off by default)
- **Ensure Research and Trash folders exist in every project** — off by default; see [Projects](#projects)
- **Appearance** (Dark/Light/System) and **Color Theme** — see [Themes](#themes-and-appearance)
- **Keyboard shortcuts** — remap or unbind any action, including a fullscreen toggle

If the settings file is missing or its contents can't be parsed, tachylite falls back to defaults rather than failing to start.

## Current Limitations

Menu items present but not yet functional: `File > Close Project`.

Not yet implemented: an Excalidraw-style canvas, multi-tab editing, and template folders/subfolders beyond a flat list (only documents directly inside the Templates folder are offered — not ones nested in a subfolder of it). [Export](#export)'s own gaps are listed at the end of that section.

In the Markdown preview specifically: raw HTML is dropped rather than rendered; table column alignment (`:---:`) is parsed but not yet reflected visually; GFM task lists and footnotes aren't enabled; mixing container types (e.g. a list inside a blockquote) doesn't preserve proper nesting; and `![[Note]]` only actually embeds when `Note` has an image extension — embedding another note's rendered content (transclusion) falls back to behaving like a plain `[[Note]]` link.
