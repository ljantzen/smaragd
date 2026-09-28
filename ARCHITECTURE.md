# Architecture

A map of Smaragd's codebase for contributors — what lives where, and how the pieces fit together. For what the app *does*, see the [README](README.md) and the [user manual](docs/user-manual.md); for how to build/test/release it, see the README's [Development](README.md#development) and [Releases](README.md#releases) sections.

Pure, unit-tested logic is kept separate from egui rendering code, which is verified manually rather than with automated tests:

```
src/
  main.rs                 entry point
  app.rs                  SmaragdApp: dock layout, menu bar, event routing
  build.rs                (repo root) captures git commit/build date as compile-time env vars for Help > About, and rasterizes assets/smaragd-icon.svg into the compiled-in window icon
  markdown.rs             markdown -> Block/Span parser (pulldown-cmark + wikilinks + inline #tag scanning)
  frontmatter.rs          YAML frontmatter parsing (DocumentMeta) + write-back + stripping for preview
  autocomplete.rs         wikilink-autocomplete query/filter/completion logic (plain prefix/substring match)
  fuzzy.rs                fzf-style subsequence fuzzy matching (nucleo-matcher) for the Open Document quick-switcher and Search Everywhere
  double_tap.rs           double-tap-Shift gesture detector (pure state machine over egui ModifiersChanged events) that opens Search Everywhere
  search.rs               plain-text find/replace across a chosen SearchScope
  git.rs                  thin wrapper over the system `git` binary (init/commit/push/pull)
  plugins.rs              loads/runs .rhai plugins: custom : commands + the on_save hook
  pomodoro.rs             Pomodoro work/break state machine (pure, ticked once per frame regardless of dock-tab visibility)
  notifications.rs        thin wrapper over notify-rust for OS-level desktop notifications (currently just Pomodoro phase changes)
  streak.rs               Writing Streak evaluation (pure): WeeklySchedule, evaluate_streak (judges only completed Mon-Sun weeks), prune_daily_history
  color_theme.rs          built-in + loaded-from-.toml color themes, egui::Visuals application
  shortcuts.rs            ShortcutAction <-> egui::KeyboardShortcut map, load/save, guards against binding a shortcut that would make some character untypable
  settings.rs             app-wide preferences: load/save smaragd.toml
  spellcheck.rs           Hunspell-compatible spell-check (spellbook): misspelled_word_spans (pure tokenizer) + dictionary lookup, memoized and invalidatable; real, individually license-reviewed dictionaries are hosted in the separate github.com/ljantzen/smaragd-dictionaries repo, <code>/ (own LICENSE+SOURCE per language; indexed by dictionaries/catalog.json here) and not compiled into the binary -- fetched at runtime via download_dictionary (app/dictionary_download.rs, ui/settings_panel.rs's Dictionaries list) with SHA-256 verification against the catalog; English/Norwegian additionally fall back to a tiny bundled placeholder before either is downloaded
  templates.rs            `${{name}}`/`${{date}}` substitution for New From Template
  project_template.rs     Scrivener-style New Project templates: built-in Blank/Novel/Nonfiction/Screenplay/World-Building + loaded-from-disk custom ones, apply()/save_from_project()
  editor/mod.rs           EditorState: open/close document, dirty tracking, save
  editor_font.rs          the curated Editor/Preview font set, and registering the three bundled ones with egui
  collab/
    mod.rs                 CollabSession: the SmaragdApp-facing surface tying crdt/diff to a running net session
    crdt.rs                 CRDT document (Yjs/yrs), proven convergent against in-process documents
    diff.rs                 text diffing (old vs. new buffer -> TextChange) + cursor adjustment on remote edits
    ticket.rs                the pasteable connection code: iroh EndpointAddr + session secret, postcard + base58
    crypto.rs                app-level end-to-end encryption layered on top of iroh's transport security (directional keys, implicit counter nonces, host identity folded into key derivation)
    net.rs                   iroh networking on its own background thread/tokio runtime: pairing handshake, encrypted frame exchange
  sync/
    mod.rs                 module overview; the sync core is pure and wasm-compatible, only client.rs is native-only
    crypto.rs               passphrase -> key (NFKC + Argon2id + blake3 subkey) and sealing/opening updates (XChaCha20-Poly1305, random nonce, AAD bound to vault + doc + key version)
    crdt.rs                 FileDoc: one synced markdown file as a CRDT (frontmatter keys as per-key registers + body as text)
    manifest.rs             ManifestDoc: doc id -> {path, kind (file/folder), deleted} CRDT (renames/deletes merge; deletes are tombstones) + path-safety checks for untrusted paths
    meta_crdt.rs            project.json (ProjectMeta) as a CRDT: SyncedFields (id-keyed snapshot; per-device fields like plugins_enabled never sync; paths <-> stable ids via PathIds) + MetaDoc (settings as registers, prose as text, order/cards/bookmarks as arrays)
    engine.rs               SyncEngine::sync_once: one reconcile pass between a project folder and a vault (files, folders and project.json: capture local edits, pull/merge, apply renames/deletes, push, then replace a document's accumulated updates with a snapshot once it has ~64); transport-agnostic
    engine/binaries.rs      non-Markdown files (images, PDFs, ...) when the project's sync_files is on: synced whole, not merged — each version a header (id, lineage, size, BLAKE3 hash) plus <=4 MiB chunks in the document's update log; the latest *complete* version wins, the losing author keeps a conflict copy, and older versions are trimmed with a snapshot
    transport.rs            the SyncTransport trait: the engine's blocking, data-plane-only view of the server (pulls are paged: follow `more`)
    state.rs                StateStore (+ DirStateStore over ProjectStore, MemoryStateStore for tests): local CRDT state between runs
    client.rs               native-only ureq HttpClient/HttpTransport: control plane (create vault, pairing, devices) + data plane
    pairing.rs              native-only one-off server operations behind the panel (create vault with admin-token fallback, join with a ticket, make ticket, list/revoke devices, leave); plain functions so they are testable against a real server
    link.rs                 ProjectLink (.smaragd/sync.json: non-secret server/vault/salt) + DeviceCredentials (token, kept in the OS data dir, never in the project)
    runner.rs               native-only SyncRunner: the engine on a background thread (key derivation happens there too), interval + on-demand passes, SyncEvents to the UI; unreachable = Offline (retries), wrong passphrase / revoked = Halted
    fake.rs                 (tests only) in-memory server for simulating several devices
  export/
    mod.rs                 gather() (binder walk, Trash/Templates-skipping) + shared ExportDoc/BookMeta/ExportError
    style.rs                TypesetStyle: built-in + loaded-from-.toml typesetting styles shared by all 3 formats
    docx.rs                 DOCX rendering (docx_rs)
    epub.rs                 EPUB rendering (epub_builder)
    pdf.rs                  print-PDF rendering via the embedded Typst compiler (typst-as-lib)
  project/
    store.rs               ProjectStore trait (read/write/list/rename/delete by path) + NativeStore (std::fs); the I/O boundary all of project/, settings.rs, backup.rs, plugins.rs, and spellcheck.rs go through instead of touching the filesystem directly
    browser_store.rs        BrowserStore: the wasm32 ProjectStore impl, an in-memory HashMap<PathBuf, Entry> synced to IndexedDB (browser_store/idb.rs, via rexie) on every mutation
    model.rs              BinderTree/BinderNode data model
    scan.rs                folder -> BinderTree via ignore::WalkBuilder
    mod.rs                 Project: type defs (FolderRole, ProjectMeta, StoryCard, ...) + core lifecycle/CRUD (load/initialize/rescan, create/rename/delete/move)
    roles.rs               folder-role assignment/lookup, trash_path/deletes_to_trash
    trash.rs                restore-from-trash/empty-trash/permanent-delete
    story_cards.rs          story cards (Cause/Effect/Why It Matters/Realization/And So, plus Prior/New Belief, Value Shift, Knowledge Gained, and many-to-many linked documents) + protagonist Desire/Misbelief + manuscript_document_stems (the Manuscript-role-restricted linking candidate list)
    queries.rs              backlinks + tag index/search
    word_count.rs           word count (WordCountScope-aware tree walk), is_path_tracked, Draft/Session target persistence, daily_word_counts rollover (feeds Streak)
    streak.rs                per-project Streak config setters (enable flag, weekly schedule, evaluation mode, red threshold)
    picklists.rs            Type/Status/POV dropdown-source folders
  ui/
    about_panel.rs          Help > About modal: version + build info
    backlinks_panel.rs      backlinks list rendering (dockable tab)
    tags_panel.rs           tags list + tag search rendering (dockable tab)
    binder_panel.rs        binder tree rendering + right-click context menu + drag-and-drop move/reorder (dockable tab)
    editor_panel.rs         text editor + wikilink autocomplete popup + Focus Mode's paragraph-dimming layouter (dockable tab)
    markdown_preview.rs     style-driven manuscript preview rendering — same `TypesetStyle` export uses (dockable tab)
    corkboard_panel.rs      story-card grid + tabbed card editor modal (Plot / Belief and Knowledge / Third Rail) (dockable tab)
    story_grid_panel.rs     read-only, manuscript-ordered table view of the same story cards, resolved against multiple linked documents per card (dockable tab)
    belief_timeline_panel.rs  a chosen character's story cards chained in manuscript order as Prior Belief -> New Belief (dockable tab)
    metadata_panel.rs       document-metadata form editor, live-binding; also renders the project-wide Title/Subtitle/Author/Logline/What-if/Synopsis form shown when the binder's root row is selected (dockable tab)
    open_document_prompt.rs fzf-style quick-switcher modal for Open Document
    search_everywhere.rs    IntelliJ-style Search Everywhere modal: documents, text, actions, and settings (settings_panel::SETTINGS_INDEX) in tabs
    find_replace_panel.rs   find/replace panel rendering
    command_prompt.rs       `:` command parsing, completion, and prompt rendering
    settings_panel.rs       settings dialog rendering: category nav + per-category content (incl. shortcut remapping)
    name_prompt.rs          new file/folder/new-from-template/rename/new-project name-prompt modal rendering
    new_project_template_prompt.rs  template-choice step shown before the New Project name prompt
    export_panel.rs         export dialog: Title/Subtitle/Author/Style + DOCX/EPUB/Print PDF buttons
    pomodoro_panel.rs       Pomodoro dock tab: countdown + Start/Pause/Skip/Reset
    word_count_panel.rs     Word Count dock tab: scope toggle, Draft/Session Target progress bars, characters-typed counter
    collab_panel.rs         Collaborate dock tab: connection code / peer fingerprint + Host/Join/End
    sync_panel.rs           Sync dock tab: status, last sync, pairing ticket, device list + revoke, stop syncing (pure rendering; app/sync.rs derives its data and handles its events)
    streak_panel.rs         Streak dock tab: Streak/Configure inner tabs, traffic-light badge, weekly schedule editing
```

Binder, Backlinks, Tags, Metadata, Editor, Preview, Corkboard, Story Grid, Belief Timeline, Pomodoro, Word Count, Collaborate, Sync, and Streak all dock together in one shared area via [`egui_dock`](https://github.com/Adanos020/egui_dock), wired up in `app.rs`'s `DockTab`/`AppTabViewer`.

## Sync (self-hosted server)

**Experimental:** the vault format, envelope layout and HTTP API are not yet stable and may change without a migration path.

Sync keeps a project identical across a user's own devices through a **blind** server: it stores and relays sealed CRDT updates and can read none of them (contrast `collab/`, which is live, peer-to-peer and serverless). All merging happens on the clients. Files are CRDT documents (`sync/crdt.rs`); a manifest CRDT (`sync/manifest.rs`) maps stable document ids to paths, so renames and deletes merge too. Binary files, when a project opts in, are manifest entries of kind `file` whose update logs hold whole encrypted versions instead of CRDT updates (`sync/engine/binaries.rs`); the server can't tell the two apart. See the module docs in `src/sync/engine.rs` for the reconcile pass, the join/adoption rules, and the safety rails.

The repository is a Cargo workspace with **three deliberately separate lockfiles**:

```
Cargo.toml / Cargo.lock       the desktop app (+ crates/smaragd-sync-protocol as a workspace member)
crates/
  smaragd-sync-protocol/      wire types shared by client and server: ids, encrypted-envelope layout + AAD, the SyncTicket pairing code, HTTP API request/response types (wasm32-compatible; its rustdoc is the protocol reference)
  smaragd-sync-server/        the axum + SQLite server, its Dockerfile/compose file and self-hosting README. Its OWN workspace and Cargo.lock. Also runs unattended housekeeping (`maintenance.rs`: expired pairing codes, vaults with no devices left after a retention period, vacuum when worthwhile) and has an operator CLI (`admin.rs`: list/delete-vault/purge-empty/vacuum/maintenance, destructive ones need --yes)
  smaragd-sync-e2e/           end-to-end tests: the real client + engine (from the app crate) against the real server. Its OWN workspace and Cargo.lock
```

The server and e2e crates are `exclude`d from the root workspace because `scripts/update-flatpak-sources.sh` feeds the *entire* root `Cargo.lock` to the flatpak generator (CI diffs the result): server dependencies (axum, rusqlite, ...) must not enter it. The e2e crate is separate from the server so the server's Docker build (which cannot see the app) never has to resolve a path dependency on the app. `just check` runs all three; `.github/workflows/sync-server.yml` runs the server and e2e jobs and builds/publishes the image.

Where state lives: nothing secret goes inside the project folder (`git.rs` runs `git add -A`; `backup.rs` zips `.smaragd/`). The engine's CRDT state goes through `sync::state::StateStore` (an OS data-dir location, chosen by the app), keys are derived in memory from the passphrase and never written to disk, and the server holds only hashes of device tokens and pairing codes.

## The wasm32 (browser) build

The same crate also targets `wasm32-unknown-unknown`, built with [`trunk`](https://trunkrs.dev/) from the repo-root `index.html` (`trunk build`/`trunk serve`; the [Pages workflow](.github/workflows/pages.yml) publishes a release build to https://ljantzen.github.io/smaragd/app/ each time a version is tagged). `Cargo.toml`'s `[target.'cfg(...)'.dependencies]` tables split native-only deps (iroh, tokio, directories, notify-rust, ureq, rfd's native dialogs) from wasm32-only ones (rexie, serde-wasm-bindgen, wasm-bindgen(-futures), console_error_panic_hook). Features with no browser equivalent — git, p2p collaboration, the HTTP sync client (the rest of `src/sync/` is pure and compiles for wasm32; a `fetch`-based `SyncTransport` is future work), native notifications, plugin subprocess execution, Scrivener import — are `#[cfg(not(target_arch = "wasm32"))]`-gated out of the UI rather than attempted; see `project/store.rs`/`browser_store.rs` above for the storage side of that split. Synchronous fs-shaped call sites keep working unchanged on both targets through the `ProjectStore` trait; the few genuinely async boundaries (project bundle load, browser file pick/save, IndexedDB persistence) use a `wasm_bindgen_futures::spawn_local` + `std::sync::mpsc::channel` + poll-once-per-frame pattern instead of threading async through the whole app (see `app/project_lifecycle.rs`'s `spawn_browser_project_load`/`poll_browser_project_load` for the shape).

### Sync in the app

`app/sync.rs` (with `app/sync_stub.rs` as the browser build's no-op twin, selected by `#[cfg_attr(..., path = ...)]`) owns everything app-side, modeled on `app/collab.rs`: a `SyncState` polled once per frame by `poll_sync`. It notices project open/close, loads the project's `ProjectLink` and this device's credentials, and starts/stops/restarts the runner whenever the settings, project or pairing change (a `Signature` of root + vault + passphrase + server; a runner that halted for a signature is not restarted until that changes). One-off server calls (create/join/ticket/devices/revoke/leave/test connection) run on short-lived threads and report back through a channel. Two integration rules worth knowing: when the engine reports `meta_written`, the app calls `Project::reload_metadata` (otherwise its next `save_metadata` would overwrite the merged file); and the app tells the engine which file has unsaved edits (`SyncRunner::set_held_paths`) so it is never overwritten on disk — the engine diffs the saved file against the version it was last in sync with, so the edit is merged with whatever arrived meanwhile rather than reverting it. A *clean* open file is just rewritten on disk and reloaded by the existing 2-second external-change scan (`app/external_watch.rs`).
