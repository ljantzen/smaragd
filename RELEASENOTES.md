# Release Notes

Notable changes to Smaragd, most recent first. Versions before 0.5.1 predate
this file.

## Unreleased

- Added **paste/drop attachments** into the editor: paste a clipboard image
  (Ctrl+V), paste a file copied in a file manager, or drag a file onto the
  editor, and it's saved to disk with a markdown image embed or link inserted
  at the cursor — like Obsidian. A new **Attachments** page in
  `File > Project Settings…` controls where files are saved (next to the
  document, or a specific project folder) and an optional clipboard-image
  size cap.
- The **Version Activity** panel's push/pull entries now list the files
  that were actually sent or brought in, under a collapsible "N files" row.
- Added an Obsidian-style **☰ menu** to the Editor pane's top-right corner:
  toggle the Backlinks panel, switch between Source mode and Reading view
  (without leaving the Preview tab behind), Rename, Move file to…, Find &
  Replace, Open in default app, Show in system explorer, Reveal file in
  navigation, and Delete file.

## v1.4.1 — 2026-10-01

- Added **auto-commit**: a project can commit its own changes on an interval
  (`File > Project Settings > Git`, off by default, 15 minutes when turned
  on), with an independent "push after each automatic commit" toggle. Reuses
  the same commit path a manual Commit does, so a "nothing to commit" tick or
  a failure behaves identically either way.
- Added a **commit message template** (`File > Settings > History`,
  multi-line, with a live preview and a placeholder quick-reference):
  `{{date}}`, `{{time}}`, `{{numFiles}}`, `{{linesAdded}}`, `{{linesDeleted}}`,
  `{{linesChanged}}`, and `{{fileList}}` (one `A`/`M`/`D` line per
  added/changed/deleted file). The default template now lists every changed
  file in the commit body. The manual Commit dialog is also a real fix: it
  used to always pre-fill the literal text "Smaragd backup" regardless of any
  configured template — it now renders the template like every automatic
  commit does, in a new multi-line dialog (`Ctrl+Enter` to confirm) instead of
  the single-line rename-style prompt it used to reuse.
- Added a **Version Activity** dock tab (`Versions > Version Activity`,
  `Ctrl+Alt+V`): current dirty files, the last 20 commits (`git log`), and a
  running log of every commit/push/pull smaragd itself triggered this
  session — manual or automatic — each with its outcome. A **Refresh** button
  re-reads git state on demand.
- Added **`File > Project Settings…`** (`F4`, grouped next to `Settings`): a
  per-project counterpart to the app-wide Settings dialog, same two-pane
  layout. Starts with **Git** (the auto-commit controls above), **Sync**
  (moved here from the Sync panel: "Also sync images, PDFs and other files"),
  and **Binder** (the `Color Binder By` mode, alongside its existing homes in
  the `View` menu and the binder's right-click menu).
- Added keyboard shortcuts for **Pull** (`Ctrl+Alt+L`) and **Commit and Push**
  (`Ctrl+Alt+Shift+C`) — both were Versions-menu-only before.
- Added a **User Manual** item to the **Help** menu, opening
  <https://ljantzen.github.io/smaragd/manual/> in your browser.

## v1.4.0 — 2026-09-30

- Added Notes: pin a short piece of text to the cursor's exact position — line *and*
  column, unlike a Bookmark's line-only granularity — with `Ctrl+Shift+J`, or by
  clicking a line's note icon slot in the gutter (left of the bookmark diamond; a noted
  line gets a dot there). The new **Notes** dock (`Tools > Notes`, `Ctrl+Shift+N`)
  lists every note in the project with a jump-to link and its text; `Ctrl+Alt+Down`/
  `Ctrl+Alt+Up` step between them, mirroring Bookmarks' own `Alt+Down`/`Alt+Up`.
- Added a **Dashboard** dock tab (#15, `Tools > Dashboard`, `Ctrl+Alt+G`):
  writing statistics as they evolve over time, as both numbers and graphs.
  A summary row (session count, total writing time, words written across
  sessions, documents created) plus bar charts for word count, sessions,
  and documents created/modified over the last 60 days, and two
  activity-pattern charts — by day of week and by hour of day — toggled
  between Words and Time. A session starts when a project opens and ends
  when it closes or you switch projects, tracked separately from the Word
  Count panel's daily Session Target.
- Added a manual ordering mode to the Story Grid (#79): an **Order** dropdown
  switches between **Manuscript** (the previous, read-only, binder-position
  order) and **Manual**, which shows the same freeform order as Corkboard and
  lets you reorder rows with ⬆/⬇ buttons in the `#` column — the same
  underlying order Corkboard's own Up/Down buttons edit, so reordering from
  either view stays in sync. Switching back to Manuscript always reproduces
  the same order regardless of anything reordered in Manual — ties (cards
  sharing a manuscript position, or with none at all) break by a fixed key,
  not whatever order Manual reordering happened to leave behind. Manuscript
  stays the default for existing projects; the chosen mode is remembered per
  project.
- Moved **Bookmarks** and **Notes** from the **View** menu to **Tools**, next to
  the other dock-tab toggles they work the same way as (Word Count, Streak,
  Dashboard) — `View` had grouped them with content panels (Binder, Preview,
  Backlinks) for no real reason. Same shortcuts, same docks; only the menu
  location changed.
- **Removed the experimental browser (WebAssembly) edition.** It was a useful
  experiment, but keeping the wasm32 build working alongside every native
  feature wasn't sustainable. The native desktop app (Linux, Windows, macOS)
  is unaffected; only the `wasm32-unknown-unknown` target, the
  `https://ljantzen.github.io/smaragd/app/` demo, and the manual's Browser
  Edition page are gone.

## v1.3.4 — 2026-09-29

- Fixed the Linux AppImage panicking on start (`Library libxkbcommon-x11.so could not
  be loaded`) on systems that don't already have it, or `libEGL`/`libGL`, installed.
  winit and glutin load these at runtime via `dlopen` rather than linking them, so the
  release pipeline's automatic dependency bundling missed them; they're now bundled
  explicitly.

## v1.3.3 — 2026-09-29

- Sync server: deleting a vault now takes the server's admin token; a device token
  alone can no longer wipe a vault (devices leave by revoking themselves, as the app
  already does).
- Sync server: the device list records which device paired each device, and the Sync
  panel shows it ("added by laptop"), so a device added with a stolen token can be
  traced. Revoking a device also voids the pairing codes it made. The database
  migrates on first start; unused pairing codes from before the upgrade are dropped.
- Sync server: request bodies are capped at 16 KiB except for pushing updates and
  snapshots, so anonymous requests can't make the server buffer megabytes; vault
  creation checks the admin token before parsing the request. At most 32 requests
  run at once and each must finish within 2 minutes.
- Sync: a server can no longer roll an image, PDF or other synced file back to an old
  version by replaying records it kept; a version the current one replaced is never
  adopted again.
- Sync: the settings file (which holds the sync passphrase) and this device's sync
  credentials and state are now readable only by your user account (`0600` on Linux
  and macOS). Existing files are narrowed the next time Smaragd reads them.
- Sync: creating a vault needs a passphrase of at least 12 characters that isn't one
  repeated pattern or just a number; Settings warns about a weak one. Existing vaults
  keep working with the passphrase they were created with.
- Sync: a device's token is only ever sent to the server that issued it. Previously,
  whoever could change a project's `.smaragd/sync.json` (a git collaborator, a
  restored backup) could point sync at their own server and receive the token and the
  encrypted project. Existing pairings are bound to the server they use on first
  launch; a `sync.json` that disagrees is ignored and the Sync panel explains.
- Sync: the app never follows HTTP redirects from the sync server, which could have
  sent the admin token elsewhere.
- Sync: a synced file larger than the server's size limit (or 1 GiB when the server
  sets none) is no longer downloaded or held in memory, and only the records of
  versions that can still win are buffered.
- Sync server: the example Docker commands and `docker-compose.yml` publish the port on
  localhost only, so the plain-HTTP port isn't exposed to the network by default.
- Sync: a pasted pairing ticket is no longer used straight away. The Sync panel shows
  which server it points to, warns if it uses plain HTTP, and joins only once you
  confirm. Tickets whose server address could be misread (like
  `trusted.example@other.example`) are rejected, and the Settings host and path fields
  follow the same rules.
- Sync server: refuses to start with an admin token shorter than 16 characters.
- Sync server: the vault quota now also counts 256 bytes for each stored update and
  document, so floods of tiny updates can't fill the disk far beyond it; every vault's
  usage is recomputed on first start. A new `SMARAGD_SYNC_MAX_VAULTS` setting (default
  100, `0` for no limit) caps how many vaults a server holds. Creating a vault on a full
  server now says so rather than asking for the admin token again.
- **Upgrading a sync server:** check its admin token is at least 16 characters first
  (a server started with a shorter one exits). Older apps keep working with the new
  server and vice versa; the Sync panel shows "added by" once both are upgraded.

## v1.3.2 — 2026-09-29

- Fixed the Linux packages (AppImage, .deb, .rpm, flatpak) missing from v1.3.1: a
  release check still expected an AppImage header patch that had been removed.
  The app itself is unchanged from v1.3.0.

## v1.3.1 — 2026-09-29

- Fixed the sync server's Docker image not being published for v1.3.0: the release
  didn't update the end-to-end tests' own lockfile, so the server workflow's checks
  failed before publishing. The image is published from this release on; the app
  itself is unchanged from v1.3.0.

## v1.3.0 — 2026-09-29

- Added **Search Everywhere**: tap `Shift` twice (or press `Ctrl+Shift+A`, or use `File > Search Everywhere…`) to search documents by name, text inside documents, actions by name, and settings from one box. `Tab` switches between the All, Documents, Text, Actions, and Settings tabs. You can turn the double-Shift gesture off under Settings > Shortcuts.
- "Reopen project on launch" now restores the whole working session, not just the project: the open document and cursor position, binder selection and collapsed folders, Back/Forward history, Focus Mode, and the window's size and position. This is saved to `session.json` next to `smaragd.toml` whenever the app closes.
- Fixed a cursor position being lost the first time a document was shown in a session, and jumps to a remembered position (Back/Forward, reopening a document) not scrolling the editor to it.
- Added a **Toggle Spell Check** shortcut (`F7` by default, also `Tools > Toggle Spell Check` and `:spell`). Toggling on restores the last language used in the open project, which each project now remembers separately.
- Added an **In Focus Mode** setting under Settings > Spell Check to force spell check on or off while Focus Mode lasts (default: keep current). Leaving Focus Mode restores the previous state.
- Added **Sync** (experimental — the data format and server protocol may still change, so keep backups on): keep a project identical across your own devices — laptop, desktop — in the background, through a small server you host yourself. Everything is **end-to-end encrypted** (the server only ever stores ciphertext, and never sees your passphrase, file names or text), and edits made on different devices, including while offline, are **merged automatically** — documents, folders (empty ones too) and project settings. A rename and an edit both survive, an edit beats a concurrent delete, frontmatter fields merge one by one, story cards merge field by field and their scene links follow a renamed scene, and per-device settings (git and plugin toggles, session and streak counters) never sync. A file you have open with unsaved edits is left alone until you save, then merged with whatever arrived. A document with a single change too large to upload (over 8 MB) is flagged in the Sync panel and skipped, without holding up anything else, and syncs again once the oversized content is removed. Set it up in **Settings > Sync** (server, masked passphrase, **Test Connection**) and the new **Sync** dock tab (**Tools > Sync Panel**, `Ctrl+Shift+Y`; **Tools > Sync Now**, `Ctrl+Alt+Y` — both remappable and in Search Everywhere): create a vault, add another device with a single-use pairing ticket, see and revoke devices, stop syncing. Joining with a copy of the project never overwrites anything — a differing file is kept as `name (conflict copy).md`. Sync is not a backup, and it is desktop-only for now; the passphrase can't be recovered and is stored in plain text in `smaragd.toml` like other settings. See the new **Sync** chapter in the user manual.
- Added the **sync server** (`crates/smaragd-sync-server`, experimental like Sync itself): a single small program with one SQLite file, shipped as a Docker image (non-root, health-checked; published to the GitHub Container Registry on release tags) with a compose file and a self-hosting guide. Docker is optional: the guide also covers building the binary and running it as a systemd service. Vaults are created with an admin token (or open registration, off by default), devices pair with single-use 10-minute codes and hold individually revocable tokens (stored only as hashes), and each vault has a size quota.
- The sync server looks after itself: every 6 hours it removes expired pairing codes, deletes vaults whose last device left 30 days ago, and vacuums its database when a meaningful amount of space can be reclaimed (all configurable, `0` turns each off). `smaragd-sync-server admin` adds `list`, `delete-vault`, `purge-empty`, `vacuum` and `maintenance` for hands-on cleanup — runnable with `docker exec` against a live server, and destructive commands only preview unless given `--yes`. Clients also replace a document's accumulated history with a snapshot after about 64 updates, so vaults stay small and new devices catch up quickly. Backups, upgrades and removing the encrypted history of deleted files remain the operator's job.
- Sync can now also carry a project's **images, PDFs and other files**: tick *Also sync images, PDFs and other files* in the Sync panel (a project setting, shared by every device syncing the project; off by default). Files travel whole, encrypted and in chunks, so large ones work; renaming or moving one doesn't re-upload it, and only a changed file is sent again. They can't be merged, so if two devices change the same file before syncing, one version wins everywhere and the other is kept as `name (conflict copy).ext`. The server advertises a per-file limit (`SMARAGD_SYNC_MAX_FILE_MB`, 100 MB by default); a bigger file stays on its device and the Sync panel says so. Old versions are removed from the server once a new one has arrived. The server now also sends large histories in pages, so upgrade the server and the app together.
- For contributors: the repository is now a Cargo workspace with three separate lockfiles (the app, the sync server, and the end-to-end tests), so the server's dependencies stay out of the flatpak-vendored lockfile. `just check` and CI cover all of them, new `just server-check`, `just e2e` and `just docker-build` recipes exist, and a new workflow tests the server, builds its Docker image and smoke-tests it as a running container (`just docker-smoke` locally) before publishing it on release tags. See ARCHITECTURE.md.
- Clarified the Collaboration chapter: its "no server" promise applies to live collaboration only, and now points to Sync.
- Added a `World` folder role (🌐), alongside Research/Trash/Templates/Manuscript, for a project's worldbuilding content. The World-Building project template now tags its `World` folder with it.
- Fixed the browser (WebAssembly) edition failing to build (and so failing
  to deploy to GitHub Pages) since v1.2.0: `getrandom` needs a
  `--cfg getrandom_backend="wasm_js"` RUSTFLAG on top of its own `wasm_js`
  Cargo feature to target `wasm32-unknown-unknown`, which nothing set, and
  a transitive copy pulled in via `ahash` (on the semver-incompatible 0.3
  line) needed that feature enabled separately from our direct 0.4
  dependency, since Cargo doesn't unify features across incompatible major
  versions of the same crate.

## v1.2.1 — 2026-09-22

- Fixed the release pipeline: `v1.2.0`'s release build failed because the
  flatpak build's vendored dependency snapshot
  (`packaging/flatpak/cargo-sources.json`) had gone stale relative to
  `Cargo.lock` (nothing regenerated it after a routine Dependabot bump), and
  because the Windows and macOS release jobs depended on the Linux job, that
  one packaging failure blocked the release entirely — no `v1.2.0` build was
  ever published for any platform. Regenerated the vendored snapshot, added
  a CI check that fails a PR if it drifts from `Cargo.lock` again, and
  decoupled the three platform release jobs so a single platform's failure
  can no longer block the others.
- Added install target to justfile
- Fixed the "New From Template" name prompt: the suggested name (the
  template's own name, e.g. "Location") now clears as soon as the field
  takes focus, instead of sitting there as text you had to select and
  delete before typing the document's real name.

## v1.2.0 — 2026-09-11

- Added an experimental browser (WebAssembly) edition — try it at
  https://ljantzen.github.io/smaragd/app/, linked from the landing page's
  new "Try it in your browser" button. No install: the binder, editor,
  dockable panels, story cards, and DOCX/EPUB/PDF export and import all run
  client-side, with the project stored in this browser's own IndexedDB
  storage rather than on disk — nothing is uploaded to a server, but there's
  also no cloud backup, so clearing site data or switching browsers/devices
  loses it (export regularly if you want a copy that survives outside the
  browser). It's a preview, not a replacement for the native app: git
  integration, peer-to-peer collaboration, Scrivener import, native OS
  notifications, and plugin scripts that shell out to external programs are
  all unavailable, and there's only one project per browser at a time (see
  the manual's new [Browser Edition](https://ljantzen.github.io/smaragd/manual/browser-edition.html) page).

## v1.1.1 — 2026-08-24

- Linux releases now also publish `.deb`, `.rpm`, and `.flatpak` packages
  alongside the AppImage.

## v1.1.0 — 2026-08-17

- Added a Recent Files switcher (`Ctrl+Shift+O`): opens a fuzzy quick-switcher
  like `Ctrl+P`'s Open Document, but scoped to this session's recent
  documents instead of the whole project. The first press shows recently
  *edited* documents; pressing the shortcut again while the popup is still
  open toggles to recently *opened* documents instead (and back again on
  further presses). `Up`/`Down` to navigate, `Enter` or a click to open,
  `Escape` to cancel — same as Open Document.
- Collaboration sessions now survive a dropped connection (#81): losing your
  collaborator (network loss, a laptop sleeping and waking, a phone switching
  networks) no longer ends the session outright. The panel shows "Lost
  connection to your collaborator — trying to reconnect…" and keeps trying
  to get them back for about a minute, reusing the same connection code, with
  edits typed on either side during the outage queued and sent once the
  connection returns. Only after that window (or an explicit **Cancel**)
  does the session end for good, same as before.
- Added a `verse` block type for poetry: a ` ```verse ` fenced block preserves
  its line breaks exactly as typed, instead of collapsing into a paragraph,
  and renders in its own font/size/italic (a style's new `[verse]` table —
  optional, older custom styles fall back to a sensible default) across
  Preview, PDF, DOCX, and EPUB export.
- Added right-click suggestions and "Add to Dictionary" for spell check:
  right-clicking a misspelled (underlined) word in the Editor now offers
  Hunspell's own correction candidates, plus an "Add to Dictionary" action
  for names and invented words that keep triggering false positives — added
  words are remembered across restarts and stop being flagged immediately,
  no need to change the buffer first.
- Added zooming to the Preview pane: Ctrl+scroll over the rendered document to
  scale its text up or down, or use `Ctrl++`/`Ctrl+-`/`Ctrl+0` (zoom
  in/out/reset), all remappable under **Settings > Shortcuts**. Only affects
  what Preview shows on screen — the underlying typesetting style, and thus
  Export, is unaffected — and the zoom level is remembered across restarts.
- Added an optional per-document stats readout in the Binder (**Settings >
  Appearance > Show document stats in binder**, off by default): each
  document row can show its line, word, and character count, right-aligned
  as `lines/words/chars`. Toggle it with `Ctrl+Alt+D`, remappable like any
  other shortcut under **Settings > Shortcuts**.
- Fixed bookmarks losing track of their document on a rename or move: a
  bookmark now follows its document through a rename, a drag-and-drop move,
  and a trash/restore round trip, instead of quietly going stale. A
  bookmark is only actually removed once its document is gone for good —
  permanently deleted, or via **Empty Trash** (#71).

## v1.0.2 — 2026-08-13

- Fixed the Linux AppImage still refusing to run (v1.0.1's fix wasn't
  enough) on distributions with a newer glibc (2.41+, e.g. Fedora 42 and
  other current rolling-release distros). Every current AppImage runtime,
  including upstream's newest builds, stamps its own identifying magic
  bytes over the ELF header's ABI-version field; glibc now rejects that on
  affected systems. The release pipeline zeroes that one byte after
  building the AppImage, and the CI smoke test now asserts it directly
  (the CI runner's own glibc is too old to have caught this by actually
  running the AppImage).

## v1.0.1 — 2026-08-13

- Fixed the Linux AppImage failing to run on some distributions with a
  misleading "No such file or directory" error (actually an `ELF file ABI
  version invalid` rejection from the loader, caused by an outdated AppImage
  runtime). The release pipeline now builds the AppImage with a current,
  version-pinned `appimagetool` instead of `linuxdeploy`'s bundled runtime,
  and a CI smoke test runs the generated AppImage before it's published.

## v1.0.0 — 2026-08-13

- The Editor can now show a line-number gutter (**Settings > Editor > Show
  line numbers**, off by default). Numbers count logical lines, not wrapped
  visual rows, so a long paragraph that wraps across several rows only gets
  numbered once.
- Added Bookmarks: mark a line in any document with `Ctrl+F2`, or by
  clicking its line's icon slot in the gutter above (a bookmarked line gets
  a diamond there) — the gutter needs to be turned on for the click/diamond,
  but the shortcut and the rest of the feature work either way.
  `Alt+Down`/`Alt+Up` step to the next/previous bookmark project-wide, in
  document-then-line order, wrapping at either end. A new **Bookmarks** dock
  (`View > Bookmarks`, `Ctrl+Alt+B`) lists every bookmark in the project as
  a clickable link (jumps straight there) with a Delete button per row.
  Stored per-project, alongside Story Cards; renaming, moving, or deleting a
  bookmarked file leaves its bookmark dangling — shown as "not found" in the
  dock, still deletable but no longer clickable — rather than kept in sync
  automatically, a deliberate v1 scope cut.
- The Preview tab now shows the open document's title at the top, next to
  the Style picker, matching the Editor pane, which already did.
- The Tags dock's "Rename…" button is now right-aligned in its row, like
  every other per-row action button (Bookmarks' "Delete", Backlinks'/Tags'
  own "Refresh") — it previously sat immediately after the tag link instead.

## v0.9.5 — 2026-08-13

- Settings gains its own **Spell Check** category, with a **Dictionaries**
  list (part of the new, still early spell-check groundwork — see below): a
  "Download" button per supported language fetches a real, individually
  license-reviewed Hunspell dictionary, SHA-256-verified against a tracked
  catalog, into your own data directory — never bundled into the app itself.
  Twenty languages available at launch: English (American), English
  (British), German, Dutch, Norwegian Bokmål, Norwegian Nynorsk, French,
  Spanish, Italian, Portuguese (Brazil), Portuguese (Portugal), Swedish,
  Danish, Polish, Russian, Georgian, Lithuanian, Persian, Turkmen, and
  Interlingue. Once downloaded, a dictionary is used immediately, no
  restart needed.
- The project's tag index (backing `all_tags`, `:tag` completion, and the
  `#tag`/tag-chip autocomplete popups) is now memoized in memory instead of
  rescanning every document on disk on every call — noticeable on large
  projects, since it was previously re-read from scratch every single frame
  the Editor or Metadata tab was open. Invalidated automatically whenever a
  document is created/renamed/moved/deleted/trashed, a tag is renamed, or
  the open document is saved.
- **File > Go Back / Go Forward** (`Alt+Left`/`Alt+Right`) navigate a
  browser-style history of every document opened this project session,
  restoring the cursor position last left in each one.
- Broken-link coloring: a `[[wikilink]]` whose target doesn't
  match any document in the project now renders in a distinct color, in both
  the Editor and the Preview, instead of looking like an ordinary link. Each
  built-in color theme has its own tuned color for this; a custom theme can
  set its own via the new (optional) `broken_wikilink` key.
- Fixed a Preview rendering bug (#66) where a `[[wikilink]]` sharing a line
  with plain text could render visibly smaller/misaligned relative to its
  neighbors, depending on the typesetting style's font. Every span in a line
  is now drawn as one combined block of text instead of gluing separate
  widgets together.
- A `` `[[not a link]]` `` or `` `#not-a-tag` `` written inside inline code is
  now left alone instead of being treated as a real wikilink/tag — matches
  the existing behavior for fenced code blocks (#37).
- The `:tag` command prompt now completes its argument against every tag
  already used somewhere in the project, the same way `:open`/`:theme`
  already complete against known titles/theme ids (#36).
- Typing `#` in the Editor now pops up an autocomplete list of the project's
  existing tags, the same way `[[` already suggests document titles (#35).
- The Tags dock gains a "Rename…" button next to each tag heading: renaming a
  tag now rewrites it everywhere in the project — every document's
  frontmatter `tags:` entry and every inline `#tag` mention — in one step
  (#32).
- Inline `#tag` mentions now render specially in the Preview tab — a subtle
  background pill in the link color, matching how `[[wikilink]]`s already
  stand out from plain text — and are clickable, opening the Tags dock
  pre-filtered to that tag (#34).
- The Tags dock now groups nested `#parent/child` tags hierarchically, the
  way Obsidian's tag pane does: `#projects` shows as a collapsible section
  with `#projects/tachylite` (and siblings) nested underneath, instead of
  every tag — nested or not — sitting in one flat list (#38).
- The Metadata panel's Tags field is now a chip editor instead of a bare
  comma-separated text box: existing tags show as removable pills, and
  typing a new one offers autocomplete against every tag already used
  elsewhere in the project — Enter, a comma, or clicking a suggestion
  commits it (#31).
- The Editor pane now shows the open document's title as a heading above
  the text, Obsidian-style — the dock tab itself just says "Editor", so
  previously nothing in the pane confirmed which document was open.

## v0.9.0 — 2026-08-11

- Nested submenus in the menu bar — **View > Theme**, **Window > Layouts**,
  **File > Export Manuscript…** (with 2+ Manuscript folders), **File >
  Import**, **File > Recent Projects**, and **View > Color Binder By** — are
  now fully keyboard-navigable: `Right`/`Enter` opens a focused submenu and
  focuses its first item, `Up`/`Down` move within it, `Left` backs out to the
  parent without closing it. Previously only their trigger row could be
  reached by keyboard. Closes #56.
- Settings gains a **History** category: an app-wide **"Enable Git
  integration"** switch (on for existing installs, off for a brand new one)
  that hides the Versions menu entirely, no-ops the Commit/Push shortcuts
  and every `:git` command, and skips the one-time "enable git support?"
  prompt when it's off — a stronger, global veto on top of the existing
  per-project opt-in.
- Automatic, Scrivener-style project backups: a zipped, timestamped snapshot
  of the whole project folder, written to a shared backup directory (one
  per project, disambiguated by filename, with the oldest pruned past a
  configurable count). Off by default; independent triggers for opening a
  project, closing one, and every explicit save, all in Settings > History.
- A file with uncommitted git changes gets a trailing "•" marker in the
  Binder (a folder shows the same marker if anything nested inside it is
  dirty) whenever git integration is on — a plain text suffix, not a color,
  so it shows alongside whichever `Color Binder By` mode is active instead
  of competing with it.
- Settings > Appearance gains a **UI font** picker — the same five bundled
  choices as the Editor font, but for the rest of the app's chrome (menus,
  the Binder, buttons, headings) instead of just the Editor/Preview.
- The user manual is now an [mdBook](https://ljantzen.github.io/smaragd/manual/)
  — real chapter navigation and full-text search instead of one long page —
  built and deployed automatically on every push to `main`.
- A new **File > Import** menu brings an existing manuscript into a project:
  **Word Document (.docx)** (split into one document per Heading 1, falling
  back to a single document if there's none), **EPUB** (one document per
  chapter, using the format's own well-defined spine order), **Scrivener
  Project** (its Draft/manuscript folder maps to smaragd's own Manuscript
  role; Trash is skipped rather than imported), and **PDF** (a single
  document — plain text only, no formatting or chapter structure, the
  fundamental limit of a format with no semantic markup to recover). Bold/
  italic/strikethrough formatting is preserved for DOCX/EPUB/Scrivener.
  Imported content lands under whichever binder folder is currently
  selected, or the project root.

## v0.8.0 — 2026-08-10

- The Preview tab now renders in the currently selected export typesetting
  style (fonts, sizes, justification, page proportions, drop cap) instead of
  a fixed Glow-CLI-style dev palette, with an inline Style picker that stays
  in sync with the Export dialog's own Style dropdown — switching one updates
  the other. As part of this, custom color themes' `[preview]` heading/
  wikilink/quote-bar color overrides are no longer supported (a leftover
  `[preview]` table in an existing theme file is simply ignored), and the
  Editor's Font/Size setting no longer affects Preview.
- Six more built-in typesetting styles: **Mass Market Paperback**, **Digest**,
  **Hardcover**, **Academic**, **Large Print**, and **Chapbook**, alongside
  the existing Manuscript and Trade Paperback — real trim sizes and type
  conventions per format, selectable from Export/Preview like any other
  style.
- A third bundled font, **Atkinson Hyperlegible** (a sans-serif designed by
  the Braille Institute for low-vision readers), joins Libertinus Serif and
  DejaVu Sans Mono as an Editor font choice and is now guaranteed to render
  identically in Preview, DOCX/EPUB, and print-PDF export. The new Large
  Print style uses it for body text; Hardcover uses it for headings over a
  serif body.
- Four more built-in typesetting styles following UK/European trim
  conventions rather than US/KDP ones: **UK B-Format Paperback** (129×198mm),
  **UK A-Format Paperback** (110×178mm), **A5 Paperback** (ISO 216, exactly
  148×210mm), and **Manuscript (A4)** (the existing Manuscript's submission
  conventions on A4 instead of US Letter) — twelve built-in styles in total.
- A custom typesetting style can now point a `font_file` at your own `.ttf`/
  `.otf` alongside `font` in `[body]`/`[headings]`/`[blockquote]`/`[code]`, so
  Preview (and print-PDF, without needing the font separately installed as a
  system font) render with your actual font instead of falling back to a
  generic face. A font file that's missing or invalid is skipped with an
  error message — that one slot falls back gracefully rather than crashing
  or blocking the rest of the style from loading.
- Story Grid columns can now be reordered and hidden: a **Columns** menu,
  right-aligned above the table, lists every column with a checkbox (hide/
  show) and Up/Down buttons (reorder), staying open across clicks so several
  columns can be adjusted in one go. Both the order and which columns are
  hidden persist across restarts, like the rest of Story Grid's view
  preferences.
- PDF export's drop cap is now a true *sunk* cap: the enlarged first letter
  with the next couple of lines of body text wrapped narrower beside it,
  computed with hand-rolled Typst layout math rather than a network-fetched
  package — no change to smaragd's fully offline export. Previously it was
  just an oversized inline letter on the first line. The wrapped lines are
  always ragged-right even in a justified style.

## v0.7.0 — 2026-08-06

- Story Cards now track a character's belief arc, not just plot mechanics.
  Each card gained a **POV Character**, **Prior Belief**, **New Belief**,
  **Value Shift** (e.g. "Trust -> Distrust"), and **Knowledge Gained**, and
  can link to more than one manuscript document (previously one at most —
  a card spanning several scenes, or several cards sharing one scene, is
  now representable). The card editor is restructured below its always-visible
  Scene #/Alpha Point/Subplots/POV/Linked-documents header into three tabs —
  **Plot** (Cause/Effect), **Belief and Knowledge** (the new fields), and
  **Third Rail** (Why It Matters/Realization/And So?). The "Linked documents"
  field now only suggests documents under a Manuscript-role folder (falling
  back to every non-Trash/Templates document if none is designated yet),
  and picking a suggestion auto-appends a comma so adding several is
  discoverable.
- Story Grid gained matching **Prior Belief**/**New Belief**/**Value Shift**
  columns and now shows every one of a card's linked documents (with a
  summed word count across them) instead of just one. Its POV column now
  prefers a card's own POV Character when set, falling back to the linked
  document's frontmatter POV as before.
- Added a **Belief Timeline** view (`View > Belief Timeline`,
  `Ctrl+Shift+E`): pick a POV character and see their story cards chained
  in manuscript order as Prior Belief → New Belief, skipping a belief that
  just repeats the previous card's, so the arc reads as a continuous chain.
- Moved Metadata from the Edit menu to View (`View > Metadata`, reordered
  alongside the other dock tabs: Editor, Preview, Corkboard, Story Grid,
  Binder, Metadata, Backlinks, Tags, Theme), and moved Focus Mode from View
  to Tools. Shortcuts (`Ctrl+Shift+M`, `F9`) are unchanged.
- Collaboration sessions no longer end unconditionally when either side
  opens a different document. Hosting and switching documents now keeps
  the session alive — the collaborator's view follows along to the new
  document automatically, with a status message noting the switch.
  Joining and opening one of your own documents now asks for confirmation
  first, since that still has to end the session; declining leaves the
  shared document open and the session running. Closing the current
  document, either side, is unchanged and still ends the session
  immediately.
- Added a **Point** field to the project-wide metadata (binder root row's
  Metadata dock), grouped with Title/Subtitle/Author above Logline — a
  single-line field, unlike the multiline Logline/What if/Synopsis boxes
  below it.
- Implemented `File > Close Project` (`Ctrl+Shift+W`, previously a disabled
  placeholder): saves the open document and any open Story Card draft if
  dirty, ends an active collaboration session, and returns every dock tab to
  its empty, no-project state. No save/discard/cancel prompt, matching Close
  Document's silent-autosave convention. Also clears `last_project_path`, so
  "Reopen project on launch" doesn't bring a deliberately closed project back.
- Added an optional desktop notification when a Pomodoro phase completes on
  its own (`File > Settings > Pomodoro`, off by default) — fixes #53. Never
  fires on a manual Skip, only an automatic completion. No audible chime yet.
- Folders now carry the same Type/Status/POV/Word Count Target/Tags metadata
  documents already had: click any non-root folder row in the Binder and the
  Metadata dock switches to a "Folder Metadata" form (the same fields and
  form documents use, minus a live word count of their own). The Status and
  POV rows, in both the document and folder forms, each gained an inline
  color-swatch button that assigns that status/POV value a project-wide
  binder background color.
- Binder rows (documents and folders alike) can now be background-colored by
  **Status**, **POV**, or a red→yellow→green **Word Count Progress**
  gradient toward each row's word count target — a folder's gradient uses
  the combined word count of everything nested inside it. Switch modes via
  `View > Color Binder By`, the remappable "Cycle Binder Color Mode"
  shortcut (default `Ctrl+Shift+C`), or by clicking the mode indicator that
  appears in the status bar once a mode other than the default, `Off`, is
  active.
- Story Grid's POV and Words columns now reuse that same coloring: a
  colored dot next to the POV name when that POV has an assigned color, and
  the word count itself tinted along the same red-to-green gradient toward
  the document's word count target.

## v0.6.2 — 2026-08-05

- New Project: picking an already-empty folder now creates the project
  directly in it instead of also prompting for a name to nest a subfolder
  under (a non-empty folder still prompts for a name, as before). Added a
  built-in "World-Building" template (Manuscript, Research, a World folder
  for characters/locations/items, and starter document Templates). The
  Binder panel's "no project open" placeholder now offers New Project /
  Open Project buttons, and — the first time the app has ever opened a
  project — New Project defaults to World-Building instead of Blank. The
  default dock layout also gained a Metadata/Backlinks column alongside
  Binder/Editor (affects a fresh install and "Restore Default Layout").
- Exiting with unsaved edits — the open document, or an open story card
  editor's draft — now prompts to Save, Discard, or Cancel instead of closing
  (or silently autosaving/losing them) right away.
- Added a Story Grid view (`View > Story Grid`, `Ctrl+Shift+G`): a read-only,
  manuscript-ordered table of the same Story Cards the Corkboard edits, with a
  computed manuscript position, POV and word count read live from each linked
  document, and every Story Genius field as its own column. Unplaced cards
  group into a Top/Bottom-configurable section.

## v0.6.1 — 2026-08-01

- Added a Writing Streak feature (`Tools > Streak`, `Ctrl+Alt+S`; off by
  default, configured per project — not the global Settings dialog). The
  dock tab has two inner tabs, switchable freely: Configure (enable flag, a
  word-count target per day of the week, how a week counts as "met," and
  how many consecutive missed weeks turn the light red) and Streak (a
  traffic-light badge for whether your most recently *completed* week met
  it — never the still-in-progress current week, so it can't turn red
  before you've had a chance to write — plus a live "Progress this week"
  readout). Opening a project defaults to whichever tab is more useful.
  A compact dot + percentage mirrors both in the status bar. Counts the
  same words as the Word Count panel's Track scope (Manuscript folders
  only, by default).

## v0.6.0 — 2026-07-31

- Added real-time peer-to-peer collaborative editing (Collaborate menu /
  panel, `Ctrl+Shift+L`): host a session on the currently open document and
  share the one-time connection code; a peer pastes it to join and both sides
  edit live with CRDT merging (Yjs/yrs) — no server ever holds the text.
  Traffic is end-to-end encrypted on top of iroh's transport security, keyed
  from a secret that lives only in the connection code itself: pairing
  requires each side to prove it holds that secret before the other reports a
  collaborator as connected, and a stranger who reaches the host's network
  endpoint without the code can neither read anything nor stop the genuine
  collaborator from joining.
- Added a UI Scale setting (`File > Settings > Appearance`, 50%–300%, default
  100%) — a manual multiplier on top of the OS/display server's own reported
  scaling, for cases where automatic HiDPI detection comes back wrong (e.g.
  some Wayland compositors) and the whole UI renders tiny with no way to fix
  it from inside the app until now.
- Added a "Recent Projects" submenu to `File`, listing the last 10 opened
  project folders (most recent first) for one-click reopening.

## v0.5.2 — 2026-07-31

- Added arrow-key navigation to the top menu bar: Up/Down moves the
  highlighted item within whichever dropdown is open, wrapping at the ends;
  Left/Right switches between the seven top-level menus, also wrapping.
- Added Alt+letter mnemonics to the top-level menu bar (Alt+F for File,
  Alt+E for Edit, etc.) to drop a menu down without the mouse.
- A folder assigned a role (Research/Trash/Templates/Manuscript) now shows a
  leading icon (🔍/🗑/📋/📖) in the binder instead of a trailing "(Role)" label.
- Added a Manuscript folder role: designate one or more folders as your
  manuscript's primary content (unlike Research/Trash/Templates, more than one
  folder can hold it at once), with a new "Export Manuscript…" File-menu
  shortcut that compiles straight from it — or the whole project if none is
  assigned yet.
- Added Word Count targets (`Tools > Word Count`), Scrivener-style: a Draft
  Target for the whole manuscript and a Session Target for today's writing,
  each with a progress bar, plus a target-less characters-typed activity
  counter (insertions and deletions both count). A per-project toggle picks
  whether the total tracks Manuscript folders only or the whole project minus
  Trash. Recomputes on a background thread on save/project-open/role- or
  scope-change/manual refresh (new "Refresh Word Count" shortcut, `F5`), never
  every frame, and mirrors the current count in the status bar.
- Fixed a bug where a keyboard shortcut given a default binding in code would
  load as unbound for anyone who already had a settings file predating that
  shortcut, rather than falling back to its default.
- Fixed markdown preview text (`[[wikilinks]]` and list-item bullets) not
  scaling with the configured Editor/Preview font size.

## v0.5.1 — 2026-07-29

- Added a GPL-3.0-or-later license and a contributor CLA.
- Added Windows and macOS release build workflows.
- Added a GitHub Pages landing page.
- Clarified how to report issues in CONTRIBUTING.md.
