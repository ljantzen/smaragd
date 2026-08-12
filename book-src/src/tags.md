# Tags

Beyond the `tags:` list in [Document Metadata](document-metadata.md), you can tag a document by writing `#tag` directly in its body — e.g. "Alice discovers the #mystery behind her mother's disappearance." A tag needs at least one letter (so `#42`, a footnote- or issue-style reference, is never mistaken for one), can include digits/`_`/`-`/`/` after the first letter, and ends at the first character outside that set — `/` lets you nest tags, e.g. `#projects/smaragd`. A `#tag` written inside inline code or a fenced code block is left alone, the same as a `[[wikilink]]` would be.

- While typing `#` in the editor, an autocomplete popup filters every tag already used anywhere in the project, the same way `[[` suggests document titles — see [Wikilinks](wikilinks.md). Navigate it with arrow keys or Tab, and press Enter (or click) to accept.
- In the preview, an inline `#tag` renders with a subtle background pill in the link color, distinguishing it from plain text, and is clickable — opening the Tags window pre-filtered to that tag, the same as clicking a tag heading in the window itself (below).

## The Tags window

**`View > Tags`** (or **`Ctrl+Shift+T`**) opens a dockable tool window (see [Dockable Tool Windows](dockable-tool-windows.md)) showing the *combined* tags — frontmatter `tags:` plus every inline `#tag` mention — of whichever document is currently open. Each tag is listed with every other document in the project that also carries it; click a document's title to jump to it. A tag with no other matching document yet is still shown, so you can confirm what's actually on the open document. A **Refresh** button re-scans on demand, the same as [Backlinks](backlinks.md)' own.

Nested tags (`#projects` and `#projects/tachylite`) are grouped hierarchically, Obsidian-style: `#projects` shows as a collapsible section with `#projects/tachylite` (and any siblings) nested underneath, rather than every tag sitting in one flat list regardless of nesting.

Click a tag heading itself, or type into the panel's own **Search** box, to switch from "tags on this document" to a project-wide list of every document carrying that tag, regardless of what's currently open — a **Clear** button empties the search box and returns to the current document's own tags. Tag matching is case-insensitive throughout (`#Mystery` and `#mystery` are the same tag), though whichever casing a given document actually used is what's displayed.

The **`:tag <name>`** [command prompt](command-prompt.md) command opens the Tags window pre-filled with a project-wide search for `<name>`, without needing to open the window or type into its search box first — its argument tab-completes against every known tag, the same as a note title or theme id does for other commands.

## Renaming a tag

Click **Rename…** next to any tag heading in the Tags window to rename it everywhere in the project in one step: every document's frontmatter `tags:` entry and every inline `#tag` mention, matching case-insensitively against the old name, are rewritten to the new one. There's no separate confirmation step or affected-document list — think of it the same way as [renaming a document](binder.md), which similarly updates every `[[wikilink]]` pointing at it without asking first.
