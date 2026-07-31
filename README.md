# Smaragd

A native desktop authoring tool for writers, built in Rust with [egui](https://github.com/emilk/egui)/[eframe](https://github.com/emilk/egui/tree/main/crates/eframe) — no Electron.

A project is a folder of `.md` files and subfolders marked with a `.smaragd/project.json` file — no proprietary bundle format, but not just any folder either. `File > New Project` creates one from scratch; `File > Open Project` on a folder that hasn't been used by smaragd before offers to set it up in place rather than refusing outright. `.smaragd/project.json` stores manuscript ordering and folder roles that the filesystem can't express; if its *contents* are corrupt (as opposed to the marker being absent, which instead means "not a project yet") smaragd falls back to defaults rather than erroring.

See [`docs/user-manual.md`](docs/user-manual.md) for a full user-facing guide to every feature below.

## Installing

Prebuilt binaries for Linux, Windows, and macOS are on the [Releases page](https://github.com/ljantzen/smaragd/releases/latest). They aren't signed with a paid code-signing certificate, so Windows and macOS show a first-run warning — expected, not a broken download. See [Installation](docs/user-manual.md#installation) in the user manual for how to get past it on each OS.

## Features

- Binder tree view of a project folder (gitignore-aware, via the `ignore` crate); documents are shown without their `.md` extension. Drag-and-drop a file or folder onto another folder to move it into that folder (appended at the end); drag one onto another *document* row instead to reorder it to sit immediately before that document, within the same folder or a different one. Keyboard-navigable: click a row (or Tab to it) then Up/Down moves between rows, Left/Right collapses/expands a folder, and Enter opens the focused document. The remappable "Toggle Binder/Editor Focus" shortcut (`F6`) jumps keyboard focus between the binder and the editor and back
- The top menu bar (File/Edit/View/Tools/Versions/Window/Help) drops down on a click or an Alt+letter mnemonic (Versions is Alt+S, since View already claims Alt+V), and is arrow-navigable once open: Up/Down moves within the open dropdown, Left/Right switches between the seven top-level menus, both wrapping at the ends. The three nested submenus (View > Theme, Window > Layouts, File > Export Manuscript with multiple folders) stay mouse/hover-only for now
- Binder, Backlinks, Tags, Document Metadata, and the Editor/Preview/Corkboard central views are *all* one shared dockable layout (via `egui_dock`) rather than fixed panels, modals, or mutually-exclusive view modes — drag a tab's title to float it in its own window, tab it together with another, split it against any other tab, or dock it to an edge, Visual-Basic-Properties-window style. Toggling Preview/Corkboard (`View` menu or their shortcuts) opens/closes that tab next to the editor rather than switching to an exclusive "view mode" — any combination can be open and arranged at once. The layout persists across restarts; `Window > Save Current Layout…` names and saves the current arrangement, `Window > Layouts` switches back to a saved one, and `Window > Restore Default Layout` resets to the original Binder-left/Editor-right split
- Markdown text editor with save-on-`Ctrl+S` and save-on-focus-loss; borderless and filling the whole Editor tab (`desired_rows` sized to the tab's currently available height, since a `TextEdit`'s own frame and hit-testable area both otherwise size to content only, not the `ScrollArea` around it) — clicking anywhere in the tab, even below a short document's last line, places the cursor there
- `File > Open Document…` (`Ctrl+P`) opens an fzf-style quick-switcher: fuzzy-filters every document in the project by its relative path as you type (subsequence matching via `nucleo-matcher`, the engine behind the Helix editor's picker — not the plain prefix/substring match the command prompt's own `:open` completion uses), Enter or click opens the highlighted result directly. `File > Close Document` (`Ctrl+W`) saves if dirty and clears the editor back to its empty placeholder
- Focus Mode (`View > Focus Mode` / `F9`): maximizes the window and hides the menu bar, status bar, and every other dock tab, leaving just the editor centered in a comfortable column; the paragraph containing the cursor renders at full strength with every other paragraph dimmed ("typewriter" focus). Escape exits, same as toggling it again
- Right-click context menu in the binder: New File/New Folder/New From Template (folders — see below), Rename, Delete (native confirmation dialog, worded as a Trash move when one's configured — see below), Restore (for a trashed item), Folder Role/Dropdown Source/Empty Trash (folders) — New File/Folder and Rename each prompt for a name (Enter to confirm), and renaming a document updates any `[[wikilinks]]` to it elsewhere in the project
- `File > New Project` opens a template picker (Blank/Novel/Nonfiction/Screenplay, plus any custom templates — see below) before the usual native folder picker + name prompt, scaffolding the chosen template's folders and starter documents into the new project. `File > Open Project` opens a native folder picker, offering to adopt a folder smaragd hasn't opened before
- Scrivener-style project templates: Blank, Novel, Nonfiction, and Screenplay ship built-in, each stamping a starter folder/document layout (and Research/Trash folder roles, where applicable) into a freshly created project — Blank reproduces the old "just an empty project" behavior exactly. `File > Save Project as Template…` saves the *current* project's own structure as a reusable custom template (excluding Trash's contents and narrative state — story cards, protagonist Desire/Misbelief, book/export/git metadata), stored in `smaragd/project_templates/` in the platform config directory alongside custom themes/styles/plugins. Unlike those, a hand-dropped custom template needs an app restart to show up — there's no "Reload Custom Templates" button yet
- Designated Research/Trash/Templates/Manuscript folders (right-click a folder's "Folder Role" submenu, shown as a leading icon 🔍/🗑/📋/📖 in the binder), Scrivener-style: Research/Trash/Templates stay exclusive (at most one folder per role project-wide), but Manuscript isn't — multiple folders can hold it at once (e.g. one per book in a series).
  - Deleting something moves it into the designated Trash folder instead of removing it from disk; right-click the Trash folder for "Empty Trash" (permanent, confirmed) or a trashed item for "Restore" (moves it back to its original folder, offering to recreate that folder via a dialog if it's gone since).
  - The Templates folder's direct child documents show up in every folder's "New From Template" submenu — picking one creates a new document from a copy of the template (frontmatter included), prompting for its name first; the template itself is untouched. Two placeholders are substituted in the copy: `${{name}}` (the typed name) and `${{date}}` (today's date, formatted per a chrono-strftime pattern set in `File > Settings`, defaulting to `%Y-%m-%d`) — an unparseable custom format falls back to the default rather than failing document creation.
  - Research is currently just a marker with no behavior of its own yet — an extension point for future features like word-count rollups. Unlike Trash/Templates, Export does *not* skip Research-role folders
  - Manuscript designates a folder as primary manuscript content, mirroring Scrivener's Draft folder. `File > Export Manuscript…` compiles straight from it — from the whole project if none is assigned yet, or via a submenu to pick among them when more than one folder holds the role
- Per-document YAML frontmatter (`type`/`status`/`pov`/`word_count_target`/`tags` — Longform/Scrivener-style manuscript metadata): parsed on demand, stripped from the markdown preview so it doesn't render as a garbled paragraph, and editable live through a dockable form (`Edit > Document Metadata`) that only ever touches those five keys — any other hand-added YAML key in the block survives a save. Edits apply as you type, no Save/Cancel step. The `Type`/`Status`/`POV` fields switch from free text to a dropdown once a folder is checked as that field's "Dropdown Source" (any folder's right-click menu) — its direct child documents' titles become the options, independently per field and independent of `FolderRole` (a folder keeps whatever role, or none, it already has, and stays in export exactly as before). The panel also shows a live word count, recomputed every frame from the open buffer (not just the last save)
- Story cards (`View > Corkboard`, Lisa Cron "Story Genius" style): a wrapping grid of scene cards, each with an Alpha Point, Cause, Effect, Why It Matters, Realization, and "And so?", plus optional subplot tags and an optional soft link to a manuscript document by title. Cards are independent of the binder tree — reorderable on their own, and a card can exist with no linked document yet (a pure plotting artifact) or be linked to a document that's since been renamed or deleted without breaking. The Corkboard also has a project-wide Desire/Misbelief pair (Cron's "Third Rail") that every card's Why It Matters is meant to test or advance
- Glow-CLI-styled markdown preview (`View > Preview`): colored heading hierarchy, barred blockquotes, boxed code blocks, striped GFM tables, and images — both standard `![alt](src)` and Obsidian-style `![[image.png]]` embeds — loaded via `egui_extras` (relative paths resolve against the open document's folder and are required to stay inside the project; remote `http(s)://` images aren't fetched)
- Obsidian-style `[[Topic]]` / `[[Topic|Alias]]` wikilinks: rendered as clickable links in preview, resolved by filename within the project. Ctrl+Click a link in preview (or place the cursor on one and press the remappable "Activate Wikilink" shortcut, `Ctrl+Enter` by default, in the editor) to create the missing document, in the same folder as the note the link was in
- Wikilink autocomplete while typing `[[`: filtered suggestions, arrow-key/Tab/Enter navigation, mouse click
- Backlinks (`View > Backlinks` / `Cmd+Shift+B`, a dockable tool window like Binder/Metadata above): every other document that `[[links]]` to the one currently open, each entry showing the linking document's title plus a short snippet of surrounding text, click to open it — one entry per link occurrence (a document linking twice gets two entries), grouped visually under a shared title when a document links more than once
- Document tags (`View > Tags` / `Cmd+Shift+T`, a dockable tool window like Backlinks above): combines frontmatter `tags:` with inline `#tag` mentions written directly in a document's body (letters/digits/`_`/`-`/`/` after the `#`, at least one letter required so `#42` isn't mistaken for a tag, `/` supports Obsidian-style nesting like `#projects/smaragd`). By default lists the open document's own tags, each paired with every other project document sharing it; clicking a tag (or typing into the panel's own search box) switches to a project-wide "every document with this tag" list instead. Tag matching is case-insensitive; the `:tag <name>` command prompt command opens the panel pre-filtered. Recomputed on demand by scanning the project, same as Backlinks — no persisted index
- Find and Replace (`Edit > Find and Replace`): plain-text search across the current file, current directory, unsaved (modified) files, or the whole project, with replace-one/replace-all
- Command prompt (`Tools > Command Prompt`, Vim-style `:` commands): `:w`/`:write`, `:q`/`:quit`, `:wq`/`:x`, `:o`/`:open <title>`, `:new <title>`, `:dmode <dark|light|system>`, `:theme <id>`, `:find <text>`, `:tag <name>` (opens the Tags dock filtered to that tag), `:git <enable|commit [message]|push|pull|backup [message]>`, and any `:` command a loaded plugin registers (see below) — with tab-completable arguments (note titles, theme ids, plugin command names)
- User-contributed plugins (`Tools > Reload Plugins`): `.rhai` scripts — the [Rhai](https://rhai.rs) embedded scripting language — that can register custom `:` commands, a default keyboard shortcut for one (`register_shortcut`, remappable/unbindable from `File > Settings` like any built-in shortcut — see `shortcuts::ShortcutTarget`), and an `on_save` hook that transforms a document's text before it's written to disk. Scripts talk to the app through host functions to read/replace the open document's text, read its basename or its path relative to the project root, set the status message, and shell out to any program on `PATH` (capturing its stdout/stderr/exit code) — so a loaded plugin has the same reach as anything else run under your own account, not a sandboxed one. Loaded from two places: a global, always-on directory (`smaragd/plugins/*.rhai` in the platform config directory) and a project's own `.smaragd/plugins/*.rhai`, which only loads once that project explicitly turns it on (`Tools > Enable Project Plugins`) — since a project's plugin folder could otherwise arrive via a shared/pulled git repo and run unreviewed code the moment it's opened
- Git integration (`Versions` menu, modeled on the Obsidian Git plugin): opt-in per project (`Enable Git Support`, offered once when a project is opened or available manually), then Commit/Commit and Push/Push/Pull, shelling out to the system `git`. Push and pull run on a background thread so a slow or hung network operation never freezes the UI
- 15 built-in Helix-inspired color themes (`View > Theme`) — Gruvbox, Dracula, Nord, Solarized, Catppuccin, One Dark/Light, Tokyo Night, Everforest, Ayu, and more — plus custom themes: a `.toml` file (background/foreground/accent, and optionally overrides for the markdown preview's heading/wikilink/quote-bar colors) dropped into `smaragd/themes/` in the platform config directory shows up alongside the built-ins (`View > Theme > Reload Custom Themes` to pick up new/edited files without restarting). Both layer on top of a separate Dark/Light/System appearance switch (`File > Settings`, applied immediately, System following the OS preference)
- Editor/Preview font (`File > Settings`) — one shared font + size for both, not independent per-view settings. A curated, bundled set of 4 rather than a live system-font picker: egui's own Proportional/Monospace, plus Libertinus Serif and DejaVu Sans Mono (the same font files already embedded for print-PDF export — see `editor_font.rs`), registered a second time with egui's own font system for on-screen use. Code blocks in the Preview always stay monospace regardless of this setting.
- Optional typewriter-quotes pass (`File > Settings > Editor`, off by default): rewrites straight `"`/`'`/`--`/`...` into curly quotes, an em dash, and an ellipsis wherever markdown is rendered from — the Preview pane and every export format — without ever touching the source `.md` text itself.
- Export (right-click a binder folder > Export…) compiles it and its subfolders, in manuscript order, to DOCX, EPUB, or a print-ready PDF — skipping any nested Trash/Templates-role folder. All three are driven by one shared typesetting Style (fonts, page size/margins, running headers, drop caps): 2 built-ins (Manuscript, Trade Paperback) plus custom `.toml` styles dropped into `smaragd/styles/` in the platform config directory, the same files-only-authoring pattern as color themes (`Reload Custom Styles` in the export dialog). The PDF target is real typesetting via the [Typst](https://typst.app) compiler embedded directly (no install, no network): genuine page layout with automatic widow/orphan avoidance, a running header showing the current chapter, a raised drop cap on each chapter's first paragraph, and an estimated spine width reported for the resulting page count. DOCX gets real named heading styles and a running header with page numbers; EPUB gets a generated stylesheet with a CSS `::first-letter` drop cap. Wikilinks resolve to real links within an EPUB when the target is also part of the export, otherwise render as plain text; DOCX doesn't attempt drop caps or wikilink resolution
- Pomodoro timer (`Tools > Pomodoro Timer`, remappable shortcut, default `Ctrl+Alt+T`): a dockable tab (Start/Pause/Skip/Reset) plus a status bar countdown segment that stays visible whether or not the tab is open, since the timer itself lives in app state and keeps ticking regardless. Classic cadence — work, then a short break, with every *n*th work session followed by a long break instead — defaulting to 25/5/15 minutes and 4 sessions, configurable in `File > Settings`. Each phase completing pauses rather than auto-continuing, so starting the next one is always a deliberate click/keypress; no sound or OS notification on phase change
- Word Count targets (`Tools > Word Count`, remappable shortcut, default `Ctrl+Alt+W`), Scrivener-style: a dockable tab with a Draft Target (overall manuscript goal) and a Session Target (today's writing goal), each shown as a progress bar against the project's current word count. A per-project scope toggle picks what counts toward that total — Manuscript-role folder(s) only (falling back to the whole project, minus Trash/Templates, if none is assigned yet) or the whole project minus just Trash — with Trash and Templates always excluded either way. The total recomputes on a background thread (never blocking the UI) on a handful of triggers — opening a project, a git pull, a folder-role or scope change, an actual save, or the remappable "Refresh Word Count" shortcut (default `F5`) — rather than every frame or on every document change; a status bar segment mirrors the Draft Target's progress whenever one is set, the same way the Pomodoro countdown does. The panel also shows a target-less "characters typed this session" activity counter — every character inserted *or* deleted in a tracked document counts, so typing 100 characters then deleting them all reads 200, not a net 0 — kept only in memory (not persisted) and reset when a project opens or "Reset Session" is clicked
- Fully remappable keyboard shortcuts (`File > Settings`), including a fullscreen toggle
- Error-severity notifications surface as toasts, not status-bar text: a stack of auto-dismissing boxes in the top-right corner (each with its own × to close early), used for anything that represents an actual problem — a failed save/export/git operation, invalid frontmatter YAML, and the like — so they can't be missed the way status-bar text can (easy to glance past, and gone the instant an unrelated action overwrites it). Routine confirmations ("Committed", "Exported to ...") still use the plain status bar, which now also auto-clears itself after a few seconds rather than sitting there until the next status update happens to replace it. Both durations are configurable in `File > Settings > General`
- `File > Settings` is an IntelliJ-style modal dialog: a left-hand category list (General, Appearance, Editor, Templates, Pomodoro, Shortcuts) with Up/Down keyboard navigation and a per-category content pane, rather than one long scrolling column. Settings persist to `smaragd.toml` in the platform's standard config directory (`~/.config/smaragd` on Linux, `~/Library/Application Support/smaragd` on macOS, `%APPDATA%\smaragd\config` on Windows); General has "Reopen project on launch", "Ensure Research and Trash folders exist in every project" (off by default — on every project open, creates each one independently if no folder holds that role yet, or recreates it at its original path if it was deleted from disk since), and a Notifications section with the error-toast and status-bar-message durations mentioned above (1–60 seconds each, defaulting to 6 and 8 respectively)

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

## Releases

Pushing a semantic-version tag (`v1.2.3` or `1.2.3`, prerelease suffixes like `-rc.1` allowed) triggers [`.github/workflows/release.yml`](.github/workflows/release.yml), which builds:

- **Linux**: an x86_64 release binary and an AppImage (via `linuxdeploy`, using [`packaging/smaragd.desktop`](packaging/smaragd.desktop) and the app icon — see below).
- **Windows**: an x86_64 build, packaged as a zip.
- **macOS**: arm64 and x86_64 cross-compiled on a single arm64 runner, lipo'd into a universal binary, assembled into a `Smaragd.app` bundle (via [`packaging/macos/Info.plist.template`](packaging/macos/Info.plist.template)) and ad-hoc signed (required for arm64 under Gatekeeper).

All three, plus a `SHA256SUMS` file per platform, are published to a GitHub release. See [RELEASENOTES.md](RELEASENOTES.md) for what's changed release to release — update its Unreleased section as changes land, and roll it into a new version header when cutting a release.

## Project layout

Pure, unit-tested logic is kept separate from egui rendering code, which is verified manually rather than with automated tests:

```
src/
  main.rs                 entry point
  app.rs                  SmaragdApp: dock layout, menu bar, event routing
  build.rs                (repo root) captures git commit/build date as compile-time env vars for Help > About, and rasterizes assets/smaragd-icon.svg into the compiled-in window icon
  markdown.rs             markdown -> Block/Span parser (pulldown-cmark + wikilinks + inline #tag scanning)
  frontmatter.rs          YAML frontmatter parsing (DocumentMeta) + write-back + stripping for preview
  autocomplete.rs         wikilink-autocomplete query/filter/completion logic (plain prefix/substring match)
  fuzzy.rs                fzf-style subsequence fuzzy matching (nucleo-matcher) for the Open Document quick-switcher
  search.rs               plain-text find/replace across a chosen SearchScope
  git.rs                  thin wrapper over the system `git` binary (init/commit/push/pull)
  plugins.rs              loads/runs .rhai plugins: custom : commands + the on_save hook
  pomodoro.rs             Pomodoro work/break state machine (pure, ticked once per frame regardless of dock-tab visibility)
  color_theme.rs          built-in + loaded-from-.toml color themes, egui::Visuals application
  shortcuts.rs            ShortcutAction <-> egui::KeyboardShortcut map, load/save, guards against binding a shortcut that would make some character untypable
  settings.rs             app-wide preferences: load/save smaragd.toml
  templates.rs            `${{name}}`/`${{date}}` substitution for New From Template
  project_template.rs     Scrivener-style New Project templates: built-in Blank/Novel/Nonfiction/Screenplay + loaded-from-disk custom ones, apply()/save_from_project()
  editor/mod.rs           EditorState: open/close document, dirty tracking, save
  editor_font.rs          the curated Editor/Preview font set, and registering the two custom ones with egui
  export/
    mod.rs                 gather() (binder walk, Trash/Templates-skipping) + shared ExportDoc/BookMeta/ExportError
    style.rs                TypesetStyle: built-in + loaded-from-.toml typesetting styles shared by all 3 formats
    docx.rs                 DOCX rendering (docx_rs)
    epub.rs                 EPUB rendering (epub_builder)
    pdf.rs                  print-PDF rendering via the embedded Typst compiler (typst-as-lib)
  project/
    model.rs              BinderTree/BinderNode data model
    scan.rs                folder -> BinderTree via ignore::WalkBuilder
    mod.rs                 Project: load/initialize, metadata, folder roles, trash/restore, create/rename/delete/reorder, story cards, backlinks scan, tag index/search, word count (WordCountScope-aware tree walk) + Draft/Session target persistence
  ui/
    about_panel.rs          Help > About modal: version + build info
    backlinks_panel.rs      backlinks list rendering (dockable tab)
    tags_panel.rs           tags list + tag search rendering (dockable tab)
    binder_panel.rs        binder tree rendering + right-click context menu + drag-and-drop move/reorder (dockable tab)
    editor_panel.rs         text editor + wikilink autocomplete popup + Focus Mode's paragraph-dimming layouter (dockable tab)
    markdown_preview.rs     glow-style preview rendering (dockable tab)
    corkboard_panel.rs      story-card grid + card editor modal (dockable tab)
    metadata_panel.rs       document-metadata form editor, live-binding (dockable tab)
    open_document_prompt.rs fzf-style quick-switcher modal for Open Document
    find_replace_panel.rs   find/replace panel rendering
    command_prompt.rs       `:` command parsing, completion, and prompt rendering
    settings_panel.rs       settings dialog rendering: category nav + per-category content (incl. shortcut remapping)
    name_prompt.rs          new file/folder/new-from-template/rename/new-project name-prompt modal rendering
    new_project_template_prompt.rs  template-choice step shown before the New Project name prompt
    export_panel.rs         export dialog: Title/Author/Style + DOCX/EPUB/Print PDF buttons
    pomodoro_panel.rs       Pomodoro dock tab: countdown + Start/Pause/Skip/Reset
    word_count_panel.rs     Word Count dock tab: scope toggle, Draft/Session Target progress bars, characters-typed counter
```

Binder, Backlinks, Tags, Metadata, Editor, Preview, Corkboard, Pomodoro, and Word Count all dock together in one shared area via [`egui_dock`](https://github.com/Adanos020/egui_dock), wired up in `app.rs`'s `DockTab`/`AppTabViewer`.

## License

Smaragd is licensed under the [GNU GPL-3.0-or-later](LICENSE). Contributions require agreeing to the [Contributor License Agreement](CLA.md) — see [`CONTRIBUTING.md`](CONTRIBUTING.md).

## The name

Smaragd is the germanic name for Emerald. A small play on Obsidian.  A working name for a long time was Tachylite, but i think Smaragd works better.
