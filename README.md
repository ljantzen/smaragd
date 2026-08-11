# Smaragd

Smaragd is a native desktop authoring tool for writers.

A project is a folder of `.md` files and subfolders marked with a `.smaragd/project.json` file. There is no proprietary bundle format, but not just any folder either. `File > New Project` creates one from scratch; `File > Open Project` on a folder that hasn't been used by smaragd before offers to set it up in place rather than refusing outright. `.smaragd/project.json` stores manuscript ordering and folder roles that the filesystem can't express; if its *contents* are corrupt (as opposed to the marker being absent, which instead means "not a project yet") smaragd falls back to defaults rather than erroring.

See the [User Manual](https://ljantzen.github.io/smaragd/manual/) for a full user-facing guide to every feature below.

## Installing

Prebuilt binaries for Linux, Windows, and macOS are on the [Releases page](https://github.com/ljantzen/smaragd/releases/latest). They aren't signed with a paid code-signing certificate, so Windows and macOS show a first-run warning — expected, not a broken download. See [Installation](https://ljantzen.github.io/smaragd/manual/installation.html) in the user manual for how to get past it on each OS.

## Features

- Dockable views that can be moved freely around
- Binder tree view of the writing project
- Markdown text editor 
- fzf-style quick-switcher between documents 
- Project templates 
- Custom file templates 
- Folders can serve different roles   
- Per-document YAML frontmatter
- Project-wide metadata (Title/Subtitle/Author/Logline/What if/Synopsis)
- Story cards, Lisa Cron "Story Genius" style
- Story Grid : A read-only view of the Story Cards as a table
- Project export to DOCX, EPUB, or PDF, using one of 12 built-in (or your own custom) typesetting styles 
- Import an existing manuscript from DOCX, EPUB, a Scrivener project, or PDF
- Live manuscript-styled markdown preview, tied to the selected export typesetting style
- Obsidian-style `[[Topic]]` / `[[Topic|Alias]]` wikilinks
- Wikilink autocomplete while typing `[[`: filtered suggestions, arrow-key/Tab/Enter navigation, mouse click
- Backlinks. A dockable tool window like Binder/Metadata above): every other document that `[[links]]` to the one currently open
- Document annotations with #tags in the text and/or in the document frontmatter. 
- Find and Replace 
- Helix/vim-style command prompt 
- Support for user-contributed plugins
- Support for version control using Git
- 15 built-in Helix-inspired color themes in addition to your own custom themes 
- UI Scaling 
- A Pomodoro timer 
- Word Count targets 
- Writing Streak tracker 
- Fully remappable keyboard shortcuts 
- Real-time peer-to-peer private collaborative editing with no shared server infrastructure 

## Running

```sh
cargo run
```

## Development

A [`justfile`](justfile) wraps the common commands (`just --list` to see all of them):

```sh
just check      # fmt-check + clippy + test — same as CI, run before committing
just test       # cargo test --all-targets --all-features
just clippy     # cargo clippy --all-targets --all-features -- -D warnings
just fmt        # cargo fmt
```

(Equivalent plain `cargo` commands work too, if you don't have [`just`](https://github.com/casey/just) installed — see the justfile for the exact invocations.)

Version control uses [jj (Jujutsu)](https://github.com/jj-vcs/jj) with the git backend (colocated).  You can use plain git, but some of the scripts may not work. Anyway, I suggest you take a look at jj, it is really nice! 

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs the same fmt/clippy/test checks on every push/PR to `main`, plus a separate `cargo llvm-cov` job that uploads an `lcov.info` coverage report as a build artifact (informational — nothing is currently gated on a coverage threshold).

## Releases

Pushing a semantic-version tag (`v1.2.3` or `1.2.3`, prerelease suffixes like `-rc.1` allowed) triggers [`.github/workflows/release.yml`](.github/workflows/release.yml), which builds:

- **Linux**: an x86_64 release binary and an AppImage (via `linuxdeploy`, using [`packaging/smaragd.desktop`](packaging/smaragd.desktop) and the app icon — see below).
- **Windows**: an x86_64 build, packaged as a zip.
- **macOS**: arm64 and x86_64 cross-compiled on a single arm64 runner, lipo'd into a universal binary, assembled into a `Smaragd.app` bundle (via [`packaging/macos/Info.plist.template`](packaging/macos/Info.plist.template)) and ad-hoc signed (required for arm64 under Gatekeeper).

All three, plus a `SHA256SUMS` file per platform, are published to a GitHub release. See [RELEASENOTES.md](RELEASENOTES.md) for what's changed release to release.

`just release <version>` (e.g. `just release 0.6.2`) automates cutting one — [`scripts/release.sh`](scripts/release.sh) bumps `Cargo.toml`/`Cargo.lock`, rolls RELEASENOTES.md's Unreleased section into a dated `## vX.Y.Z` header, runs the same checks CI does, then (after a confirmation prompt) commits, pushes `main`, tags, and pushes the tag. Requires a clean jj working copy. `--dry-run` stops right before the push/tag step; `--yes` skips the confirmation prompt.

## Project layout

See [ARCHITECTURE.md](ARCHITECTURE.md) for a full module-by-module map of the codebase and how the dockable UI is wired together.

## License

Smaragd is licensed under the [GNU GPL-3.0-or-later](LICENSE). Contributions require agreeing to the [Contributor License Agreement](CLA.md) — see [`CONTRIBUTING.md`](CONTRIBUTING.md).

## The name

Smaragd is the germanic name for Emerald. A small play on Obsidian.  A working name for a long time was Tachylite, but i think Smaragd works better.
