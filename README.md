# Tachylite

A native desktop authoring tool for writers, built in Rust with [egui](https://github.com/emilk/egui)/[eframe](https://github.com/emilk/egui/tree/main/crates/eframe) — no Electron. It combines ideas from a few different tools:


A project is just a folder of `.md` files and subfolders — no proprietary bundle format. An optional `.tachylite/project.json` stores manuscript ordering that the filesystem can't express; if it's missing or corrupt, tachylite falls back to alphabetical order rather than erroring.

## Features

- Binder tree view of a project folder (gitignore-aware, via the `ignore` crate)
- Markdown text editor with save-on-`Ctrl+S` and save-on-focus-loss
- Create new documents/folders from the binder
- Native folder picker for opening a project (`File > Open Project`)
- Glow-CLI-styled markdown preview (`View > Toggle preview`): colored heading hierarchy, barred blockquotes, boxed code blocks
- Obsidian-style `[[Topic]]` / `[[Topic|Alias]]` wikilinks: rendered as clickable links in preview, resolved by filename within the project
- Wikilink autocomplete while typing `[[`: filtered suggestions, arrow-key/Tab/Enter navigation, mouse click
- Settings window (`File > Settings`) with a "Reopen project on launch" toggle, persisted to `tachylite.toml` in the platform's standard config directory (`~/.config/tachylite` on Linux, `~/Library/Application Support/tachylite` on macOS, `%APPDATA%\tachylite\config` on Windows)

## Not yet implemented

Menu items present but stubbed: `File > Close Project`, `Edit > Cut/Copy/Paste`, `Help > About`, the whole `Tools` menu. Also deferred: the Excalidraw-style canvas, backlink index, compile/export, Longform-style scene metadata (POV, status, word-count targets), multi-tab editing, drag-and-drop reorder.

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
  autocomplete.rs         wikilink-autocomplete query/filter/completion logic
  settings.rs             app-wide preferences: load/save tachylite.toml
  editor/mod.rs          EditorState: open document, dirty tracking, save
  project/
    model.rs              BinderTree/BinderNode data model
    scan.rs                folder -> BinderTree via ignore::WalkBuilder
    mod.rs                 Project: load/save, metadata, create file/folder
  ui/
    binder_panel.rs        binder tree rendering
    editor_panel.rs         text editor + wikilink autocomplete popup
    markdown_preview.rs     glow-style preview rendering
    settings_panel.rs       settings window rendering
```
