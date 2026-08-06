# Release Notes

Notable changes to Smaragd, most recent first. Versions before 0.5.1 predate
this file.

## Unreleased

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
