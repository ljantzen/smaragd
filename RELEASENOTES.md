# Release Notes

Notable changes to Smaragd, most recent first. Versions before 0.5.1 predate
this file.

## Unreleased

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
