# Tachylite

A native desktop authoring tool for writers, built in Rust with [egui](https://github.com/emilk/egui)/[eframe](https://github.com/emilk/egui/tree/main/crates/eframe) — no Electron.

A project is a folder of `.md` files and subfolders marked with a `.tachylite/project.json` file — no proprietary bundle format, but not just any folder either. `File > New Project` creates one from scratch; `File > Open Project` on a folder that hasn't been used by tachylite before offers to set it up in place rather than refusing outright. `.tachylite/project.json` stores manuscript ordering and folder roles that the filesystem can't express; if its *contents* are corrupt (as opposed to the marker being absent, which instead means "not a project yet") tachylite falls back to defaults rather than erroring.

## Features

- Binder tree view of a project folder (gitignore-aware, via the `ignore` crate)
- Markdown text editor with save-on-`Ctrl+S` and save-on-focus-loss
- Right-click context menu in the binder: New File/New Folder (folders), Rename, Delete (native confirmation dialog, worded as a Trash move when one's configured — see below), Restore (for a trashed item), Folder Role/Empty Trash (folders) — New File/Folder and Rename each prompt for a name (Enter to confirm), and renaming a document updates any `[[wikilinks]]` to it elsewhere in the project
- `File > New Project` (native folder picker + name prompt) and `File > Open Project` (native folder picker, offering to adopt a folder tachylite hasn't opened before)
- Designated Research/Trash folders (right-click a folder's "Folder Role" submenu), Scrivener-style: at most one folder per role project-wide. Deleting something moves it into the designated Trash folder instead of removing it from disk; right-click the Trash folder for "Empty Trash" (permanent, confirmed) or a trashed item for "Restore" (moves it back to its original folder, offering to recreate that folder via a dialog if it's gone since). Research is currently just a marker with no behavior of its own yet — an extension point for future features like compile or word-count rollups
- Per-document YAML frontmatter (`type`/`status`/`pov`/`word_count_target`/`tags` — Longform/Scrivener-style manuscript metadata) parsed on demand and stripped from the markdown preview so it doesn't render as a garbled paragraph. Read-only for now: there's no metadata-editing UI yet, so it's hand-edited as a `---`-delimited YAML block at the top of the file
- Glow-CLI-styled markdown preview (`View > Toggle preview`): colored heading hierarchy, barred blockquotes, boxed code blocks, striped GFM tables, and images — both standard `![alt](src)` and Obsidian-style `![[image.png]]` embeds — loaded via `egui_extras` (relative paths resolve against the open document's folder, remote `http(s)://` images aren't fetched)
- Obsidian-style `[[Topic]]` / `[[Topic|Alias]]` wikilinks: rendered as clickable links in preview, resolved by filename within the project. Ctrl+Click a link in preview (or place the cursor on one and press Ctrl+Enter in the editor) to create the missing document, in the same folder as the note the link was in
- Wikilink autocomplete while typing `[[`: filtered suggestions, arrow-key/Tab/Enter navigation, mouse click
- Settings window (`File > Settings`): "Reopen project on launch", and "Create Research and Trash folders in new projects" (off by default — pre-seeds `File > New Project` with empty, role-assigned Research/Trash folders when on), persisted to `tachylite.toml` in the platform's standard config directory (`~/.config/tachylite` on Linux, `~/Library/Application Support/tachylite` on macOS, `%APPDATA%\tachylite\config` on Windows)

## Not yet implemented

Menu items present but stubbed: `File > Close Project`, `Edit > Cut/Copy/Paste`, `Help > About`, the whole `Tools` menu. Also deferred: the Excalidraw-style canvas, backlink index, compile/export, a metadata-editing UI for the frontmatter fields above (index cards, binder badges — parsing exists, editing doesn't), multi-tab editing, and moving a file/folder between arbitrary folders (drag-and-drop or otherwise) — Trash/Restore's moves are the only place the app relocates something on its own. In the markdown parser itself: raw HTML (blocks and inline) is dropped rather than passed through; table column alignment (`:---:`) is parsed but not yet reflected visually; GFM extras other than strikethrough/tables (task lists, footnotes) aren't enabled; mixing container types (e.g. a list inside a blockquote) doesn't preserve proper nesting; and `![[Note]]` embeds only actually embed when `Note` has an image extension — embedding another note's rendered content (transclusion) isn't implemented, so those fall back to behaving like a plain `[[Note]]` link — see `src/markdown.rs`'s doc comment.

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
  main.rs               entry point
  app.rs                 TachyliteApp: panel layout, menu bar, event routing
  markdown.rs            markdown -> Block/Span parser (pulldown-cmark + wikilinks)
  frontmatter.rs          YAML frontmatter parsing (DocumentMeta) + stripping for preview
  autocomplete.rs         wikilink-autocomplete query/filter/completion logic
  settings.rs             app-wide preferences: load/save tachylite.toml
  editor/mod.rs          EditorState: open document, dirty tracking, save
  project/
    model.rs              BinderTree/BinderNode data model
    scan.rs                folder -> BinderTree via ignore::WalkBuilder
    mod.rs                 Project: load/initialize, metadata, folder roles, trash/restore, create/rename/delete
  ui/
    binder_panel.rs        binder tree rendering + right-click context menu
    editor_panel.rs         text editor + wikilink autocomplete popup
    markdown_preview.rs     glow-style preview rendering
    settings_panel.rs       settings window rendering
    name_prompt.rs          new file/folder/rename/new-project name-prompt modal rendering
```
