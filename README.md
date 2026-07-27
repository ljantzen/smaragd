# Tachylite

A native desktop authoring tool for writers, built in Rust with [egui](https://github.com/emilk/egui)/[eframe](https://github.com/emilk/egui/tree/main/crates/eframe) — no Electron.

A project is a folder of `.md` files and subfolders marked with a `.tachylite/project.json` file — no proprietary bundle format, but not just any folder either. `File > New Project` creates one from scratch; `File > Open Project` on a folder that hasn't been used by tachylite before offers to set it up in place rather than refusing outright. `.tachylite/project.json` stores manuscript ordering and folder roles that the filesystem can't express; if its *contents* are corrupt (as opposed to the marker being absent, which instead means "not a project yet") tachylite falls back to defaults rather than erroring.

See [`docs/user-manual.md`](docs/user-manual.md) for a full user-facing guide to every feature below.

## Features

- Binder tree view of a project folder (gitignore-aware, via the `ignore` crate); documents are shown without their `.md` extension. Drag-and-drop a file or folder onto another folder to move it. Keyboard-navigable: click a row (or Tab to it) then Up/Down moves between rows, Left/Right collapses/expands a folder, and Enter opens the focused document. Binder, Backlinks (see below), and Document Metadata (see below) are all dockable tool windows (via `egui_dock`) rather than fixed panels or modals — drag a tab's title to float it in its own window, tab it together with another, or dock it to an edge, Visual-Basic-Properties-window style
- Markdown text editor with save-on-`Ctrl+S` and save-on-focus-loss
- Right-click context menu in the binder: New File/New Folder/New From Template (folders — see below), Rename, Delete (native confirmation dialog, worded as a Trash move when one's configured — see below), Restore (for a trashed item), Folder Role/Empty Trash (folders) — New File/Folder and Rename each prompt for a name (Enter to confirm), and renaming a document updates any `[[wikilinks]]` to it elsewhere in the project
- `File > New Project` (native folder picker + name prompt) and `File > Open Project` (native folder picker, offering to adopt a folder tachylite hasn't opened before)
- Designated Research/Trash/Templates folders (right-click a folder's "Folder Role" submenu), Scrivener-style: at most one folder per role project-wide.
  - Deleting something moves it into the designated Trash folder instead of removing it from disk; right-click the Trash folder for "Empty Trash" (permanent, confirmed) or a trashed item for "Restore" (moves it back to its original folder, offering to recreate that folder via a dialog if it's gone since).
  - The Templates folder's direct child documents show up in every folder's "New From Template" submenu — picking one creates a new document that's a verbatim copy of the template (frontmatter included), prompting for its name first; the template itself is untouched.
  - Research is currently just a marker with no behavior of its own yet — an extension point for future features like compile or word-count rollups
- Per-document YAML frontmatter (`type`/`status`/`pov`/`word_count_target`/`tags` — Longform/Scrivener-style manuscript metadata): parsed on demand, stripped from the markdown preview so it doesn't render as a garbled paragraph, and editable live through a dockable form (`Edit > Document Metadata`) that only ever touches those five keys — any other hand-added YAML key in the block survives a save. Edits apply as you type, no Save/Cancel step
- Story cards (`View > Corkboard`, Lisa Cron "Story Genius" style): a wrapping grid of scene cards, each with an Alpha Point, Cause, Effect, Realization, and "And so?", plus optional subplot tags and an optional soft link to a manuscript document by title. Cards are independent of the binder tree — reorderable on their own, and a card can exist with no linked document yet (a pure plotting artifact) or be linked to a document that's since been renamed or deleted without breaking
- Glow-CLI-styled markdown preview (`View > Preview`): colored heading hierarchy, barred blockquotes, boxed code blocks, striped GFM tables, and images — both standard `![alt](src)` and Obsidian-style `![[image.png]]` embeds — loaded via `egui_extras` (relative paths resolve against the open document's folder and are required to stay inside the project; remote `http(s)://` images aren't fetched)
- Obsidian-style `[[Topic]]` / `[[Topic|Alias]]` wikilinks: rendered as clickable links in preview, resolved by filename within the project. Ctrl+Click a link in preview (or place the cursor on one and press the remappable "Activate Wikilink" shortcut, `Ctrl+Enter` by default, in the editor) to create the missing document, in the same folder as the note the link was in
- Wikilink autocomplete while typing `[[`: filtered suggestions, arrow-key/Tab/Enter navigation, mouse click
- Backlinks (`View > Backlinks` / `Cmd+Shift+B`, a dockable tool window like Binder/Metadata above): every other document that `[[links]]` to the one currently open, each entry showing the linking document's title plus a short snippet of surrounding text, click to open it — one entry per link occurrence (a document linking twice gets two entries), grouped visually under a shared title when a document links more than once
- Find and Replace (`Edit > Find and Replace`): plain-text search across the current file, current directory, unsaved (modified) files, or the whole project, with replace-one/replace-all
- Command prompt (`Tools > Command Prompt`, Vim-style `:` commands): `:w`/`:write`, `:q`/`:quit`, `:wq`/`:x`, `:o`/`:open <title>`, `:new <title>`, `:dmode <dark|light|system>`, `:theme <id>`, `:find <text>`, `:git <enable|commit [message]|push|pull|backup [message]>`, and any `:` command a loaded plugin registers (see below) — with tab-completable arguments (note titles, theme ids, plugin command names)
- User-contributed plugins (`Tools > Reload Plugins`): `.rhai` scripts — the [Rhai](https://rhai.rs) embedded scripting language — that can register custom `:` commands, a default keyboard shortcut for one (`register_shortcut`, remappable/unbindable from `File > Settings` like any built-in shortcut — see `shortcuts::ShortcutTarget`), and an `on_save` hook that transforms a document's text before it's written to disk. Scripts talk to the app through host functions to read/replace the open document's text, set the status message, and shell out to any program on `PATH` (capturing its stdout/stderr/exit code) — so a loaded plugin has the same reach as anything else run under your own account, not a sandboxed one. Loaded from two places: a global, always-on directory (`tachylite/plugins/*.rhai` in the platform config directory) and a project's own `.tachylite/plugins/*.rhai`, which only loads once that project explicitly turns it on (`Tools > Enable Project Plugins`) — since a project's plugin folder could otherwise arrive via a shared/pulled git repo and run unreviewed code the moment it's opened
- Git integration (`Versions` menu, modeled on the Obsidian Git plugin): opt-in per project (`Enable Git Support`, offered once when a project is opened or available manually), then Commit/Commit and Push/Push/Pull, shelling out to the system `git`. Push and pull run on a background thread so a slow or hung network operation never freezes the UI
- 15 Helix-inspired color themes (`View > Theme`) — Gruvbox, Dracula, Nord, Solarized, Catppuccin, One Dark/Light, Tokyo Night, Everforest, Ayu, and more — layered on top of a separate Dark/Light/System appearance switch (`File > Settings`, applied immediately, System following the OS preference)
- Fully remappable keyboard shortcuts (`File > Settings`), including a fullscreen toggle
- Settings persisted to `tachylite.toml` in the platform's standard config directory (`~/.config/tachylite` on Linux, `~/Library/Application Support/tachylite` on macOS, `%APPDATA%\tachylite\config` on Windows); `File > Settings` also has "Reopen project on launch" and "Ensure Research and Trash folders exist in every project" (off by default — on every project open, creates each one independently if no folder holds that role yet, or recreates it at its original path if it was deleted from disk since)

## Not yet implemented

Menu items present but stubbed: `File > Close Project`, `Help > About`. Also deferred: the Excalidraw-style canvas, compile/export, multi-tab editing, and template folders/subfolders beyond a flat list (only documents directly inside the Templates folder are offered, not ones nested in a subfolder of it). A plugin's `on_save` hook only runs on the explicit save actions (`:w`/`Ctrl+S`/`:wq`) — not the focus-loss autosave or the save-before-switching-documents path, both of which stay plugin-agnostic in v1. In the markdown parser itself: raw HTML (blocks and inline) is dropped rather than passed through; table column alignment (`:---:`) is parsed but not yet reflected visually; GFM extras other than strikethrough/tables (task lists, footnotes) aren't enabled; mixing container types (e.g. a list inside a blockquote) doesn't preserve proper nesting; and `![[Note]]` embeds only actually embed when `Note` has an image extension — embedding another note's rendered content (transclusion) isn't implemented, so those fall back to behaving like a plain `[[Note]]` link — see `src/markdown.rs`'s doc comment.

## Running

```sh
cargo run
```

## Development

```sh
cargo test                                    # unit tests
cargo clippy --all-targets --all-features     # must be warning-free before committing
cargo fmt                                     # must be applied before committing
```

Version control uses [jj (Jujutsu)](https://github.com/jj-vcs/jj) with the git backend (colocated).

## Project layout

Pure, unit-tested logic is kept separate from egui rendering code, which is verified manually rather than with automated tests:

```
src/
  main.rs                 entry point
  app.rs                  TachyliteApp: panel layout, menu bar, event routing
  markdown.rs             markdown -> Block/Span parser (pulldown-cmark + wikilinks)
  frontmatter.rs          YAML frontmatter parsing (DocumentMeta) + write-back + stripping for preview
  autocomplete.rs         wikilink-autocomplete query/filter/completion logic
  search.rs               plain-text find/replace across a chosen SearchScope
  git.rs                  thin wrapper over the system `git` binary (init/commit/push/pull)
  plugins.rs              loads/runs .rhai plugins: custom : commands + the on_save hook
  color_theme.rs          Helix-style color theme definitions + egui::Visuals application
  shortcuts.rs            ShortcutAction <-> egui::KeyboardShortcut map, load/save, guards against binding a shortcut that would make some character untypable
  settings.rs             app-wide preferences: load/save tachylite.toml
  editor/mod.rs           EditorState: open document, dirty tracking, save
  project/
    model.rs              BinderTree/BinderNode data model
    scan.rs                folder -> BinderTree via ignore::WalkBuilder
    mod.rs                 Project: load/initialize, metadata, folder roles, trash/restore, create/rename/delete, story cards, backlinks scan
  ui/
    backlinks_panel.rs      backlinks list rendering (dockable tab)
    binder_panel.rs        binder tree rendering + right-click context menu + drag-and-drop (dockable tab)
    editor_panel.rs         text editor + wikilink autocomplete popup
    markdown_preview.rs     glow-style preview rendering
    corkboard_panel.rs      story-card grid + card editor modal
    metadata_panel.rs       document-metadata form editor, live-binding (dockable tab)
    find_replace_panel.rs   find/replace panel rendering
    command_prompt.rs       `:` command parsing, completion, and prompt rendering
    settings_panel.rs       settings window rendering (incl. shortcut remapping)
    name_prompt.rs          new file/folder/new-from-template/rename/new-project name-prompt modal rendering
```

Binder/Backlinks/Metadata dock together via [`egui_dock`](https://github.com/Adanos020/egui_dock), wired up in `app.rs`'s `DockTab`/`AppTabViewer`.
