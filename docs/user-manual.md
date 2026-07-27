# Tachylite User Manual

Tachylite is a desktop writing tool for long-form fiction and other manuscripts organized as a folder of Markdown files. It borrows ideas from Scrivener (binder, folder roles, templates), Longform/Obsidian (frontmatter metadata, wikilinks), Lisa Cron's *Story Genius* (structured story cards), and Helix (color themes, `:` command prompt).

This manual covers what the app does and how to use it. For internals (source layout, build/test commands), see the main [README](../README.md).

## Contents

- [Projects](#projects)
- [Dockable Tool Windows](#dockable-tool-windows)
- [The Binder](#the-binder)
- [Writing and the Editor](#writing-and-the-editor)
- [Markdown Preview](#markdown-preview)
- [Wikilinks](#wikilinks)
- [Backlinks](#backlinks)
- [Document Metadata (Frontmatter)](#document-metadata-frontmatter)
- [Folder Roles: Research, Trash, Templates](#folder-roles-research-trash-templates)
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

The **Binder**, **Backlinks**, and **Document Metadata** panels are dockable tool windows, not fixed panels or modals — similar to the Properties window in Visual Basic's IDE. Each shows up as a tab on the left by default, but you can:

- **Drag a tab's title** onto empty space to pop it out into its own floating window
- **Drag a floating window's title back** onto the dock area to re-dock it
- **Drag one tab onto another** to group them together, switching between them like browser tabs
- **Resize** the dock area, or a floating window, by dragging its edge

Binder is present from the moment a project is open; Backlinks and Metadata start closed. All three can be closed via their tab's × button, and reopened again from **`View > Binder`**, **`View > Backlinks`**, or **`Edit > Document Metadata`** (Backlinks and Metadata also have shortcuts — see below).

## The Binder

The left-hand panel is the **binder** — a tree view of your project folder, one of the dockable tool windows described above. It's `.gitignore`-aware, and documents are shown without their `.md` extension.

- **Navigate by keyboard**: click a row (or Tab to it) to give it focus, then:
  - `Up`/`Down` moves between rows
  - `Left`/`Right` collapses/expands a focused folder
  - `Enter` opens the focused document
- **Drag and drop** a file or folder onto another folder to move it.
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
- **Research**: currently just a marker with no behavior yet attached — reserved for future features like compile or word-count rollups.

## Story Cards (Corkboard)

**`View > Corkboard`** opens a wrapping grid of scene cards, modeled on Lisa Cron's *Story Genius* method — a structured cause-and-effect breakdown rather than a freeform synopsis. Each card has:

- **Alpha Point** — the scene's core moment
- **Cause** — the external event, and why it matters given the protagonist's current goal
- **Effect** — the external and internal consequence of the cause
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

Available theme ids:

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

Since the editor is a single plain-text field with no syntax-highlighting pipeline, each theme reproduces its palette's overall look (background, body text, one accent color for selection/links) rather than full per-token syntax highlighting.

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

Two shortcuts can never overlap — rebinding one to a combo another action already owns automatically un-assigns it from the previous owner. This holds across built-ins and plugin shortcuts alike: if a loaded plugin registered a `:` command with its own shortcut (see [Plugins](#plugins)), it shows up in its own "Plugin Shortcuts" section further down the same window, remappable/unbindable the same way.

## Settings

Settings are stored as `tachylite.toml` in the platform's config directory (the same base path as the global plugins folder — see [Plugins](#plugins)). Available in **`File > Settings`**:

- **Reopen project on launch** — automatically reopens the last project you had open (off by default)
- **Ensure Research and Trash folders exist in every project** — off by default; see [Projects](#projects)
- **Appearance** (Dark/Light/System) and **Color Theme** — see [Themes](#themes-and-appearance)
- **Keyboard shortcuts** — remap or unbind any action, including a fullscreen toggle

If the settings file is missing or its contents can't be parsed, tachylite falls back to defaults rather than failing to start.

## Current Limitations

Menu items present but not yet functional: `File > Close Project`, `Help > About`.

Not yet implemented: an Excalidraw-style canvas, compile/export, multi-tab editing, and template folders/subfolders beyond a flat list (only documents directly inside the Templates folder are offered — not ones nested in a subfolder of it).

In the Markdown preview specifically: raw HTML is dropped rather than rendered; table column alignment (`:---:`) is parsed but not yet reflected visually; GFM task lists and footnotes aren't enabled; mixing container types (e.g. a list inside a blockquote) doesn't preserve proper nesting; and `![[Note]]` only actually embeds when `Note` has an image extension — embedding another note's rendered content (transclusion) falls back to behaving like a plain `[[Note]]` link.
