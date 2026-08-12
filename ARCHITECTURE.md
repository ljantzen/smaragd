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
  fuzzy.rs                fzf-style subsequence fuzzy matching (nucleo-matcher) for the Open Document quick-switcher
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
  export/
    mod.rs                 gather() (binder walk, Trash/Templates-skipping) + shared ExportDoc/BookMeta/ExportError
    style.rs                TypesetStyle: built-in + loaded-from-.toml typesetting styles shared by all 3 formats
    docx.rs                 DOCX rendering (docx_rs)
    epub.rs                 EPUB rendering (epub_builder)
    pdf.rs                  print-PDF rendering via the embedded Typst compiler (typst-as-lib)
  project/
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
    find_replace_panel.rs   find/replace panel rendering
    command_prompt.rs       `:` command parsing, completion, and prompt rendering
    settings_panel.rs       settings dialog rendering: category nav + per-category content (incl. shortcut remapping)
    name_prompt.rs          new file/folder/new-from-template/rename/new-project name-prompt modal rendering
    new_project_template_prompt.rs  template-choice step shown before the New Project name prompt
    export_panel.rs         export dialog: Title/Subtitle/Author/Style + DOCX/EPUB/Print PDF buttons
    pomodoro_panel.rs       Pomodoro dock tab: countdown + Start/Pause/Skip/Reset
    word_count_panel.rs     Word Count dock tab: scope toggle, Draft/Session Target progress bars, characters-typed counter
    collab_panel.rs         Collaborate dock tab: connection code / peer fingerprint + Host/Join/End
    streak_panel.rs         Streak dock tab: Streak/Configure inner tabs, traffic-light badge, weekly schedule editing
```

Binder, Backlinks, Tags, Metadata, Editor, Preview, Corkboard, Story Grid, Belief Timeline, Pomodoro, Word Count, Collaborate, and Streak all dock together in one shared area via [`egui_dock`](https://github.com/Adanos020/egui_dock), wired up in `app.rs`'s `DockTab`/`AppTabViewer`.
