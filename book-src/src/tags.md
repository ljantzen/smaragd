# Tags

Beyond the `tags:` list in [Document Metadata](document-metadata.md), you can tag a document by writing `#tag` directly in its body — e.g. "Alice discovers the #mystery behind her mother's disappearance." A tag needs at least one letter (so `#42`, a footnote- or issue-style reference, is never mistaken for one), can include digits/`_`/`-`/`/` after the first letter, and ends at the first character outside that set — `/` lets you nest tags, e.g. `#projects/smaragd`.

**`View > Tags`** (or **`Ctrl+Shift+T`**) opens a dockable tool window (see [Dockable Tool Windows](dockable-tool-windows.md)) showing the *combined* tags — frontmatter `tags:` plus every inline `#tag` mention — of whichever document is currently open. Each tag is listed with every other document in the project that also carries it; click a document's title to jump to it. A tag with no other matching document yet is still shown, so you can confirm what's actually on the open document. A **Refresh** button re-scans on demand, the same as [Backlinks](backlinks.md)' own.

Click a tag heading itself, or type into the panel's own **Search** box, to switch from "tags on this document" to a project-wide list of every document carrying that tag, regardless of what's currently open — a **Clear** button empties the search box and returns to the current document's own tags. Tag matching is case-insensitive throughout (`#Mystery` and `#mystery` are the same tag), though whichever casing a given document actually used is what's displayed.

The **`:tag <name>`** [command prompt](command-prompt.md) command opens the Tags window pre-filled with a project-wide search for `<name>`, without needing to open the window or type into its search box first.
