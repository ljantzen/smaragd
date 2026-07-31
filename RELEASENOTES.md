# Release Notes

Notable changes to Smaragd, most recent first. Versions before 0.5.1 predate
this file.

## Unreleased

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
  each with a progress bar, plus a target-less "characters typed this session"
  activity counter (insertions and deletions both count). A per-project toggle
  picks whether the total tracks Manuscript folders only or the whole project
  minus Trash. Recomputes on a background thread on save/project-open/role- or
  scope-change/manual refresh (new "Refresh Word Count" shortcut, `F5`), never
  every frame, and mirrors the Draft Target's progress in the status bar.

## v0.5.1 — 2026-07-29

- Added a GPL-3.0-or-later license and a contributor CLA.
- Added Windows and macOS release build workflows.
- Added a GitHub Pages landing page.
- Clarified how to report issues in CONTRIBUTING.md.
