# Backlinks

**`View > Backlinks`** (or **`Ctrl+Shift+B`**) opens a dockable tool window (see [Dockable Tool Windows](dockable-tool-windows.md)) listing every other document that `[[links]]` to whichever one is currently open — the reverse of a wikilink.

Each entry shows the linking document's title and a short snippet of the surrounding text, so you can tell *why* it links here without opening it. Click a title to jump to that document. A document that links more than once gets one entry per occurrence, grouped under its title. A **Refresh** button re-scans on demand, for the rare case where a file changed outside the app (e.g. a git pull) while your current document stayed open — otherwise the list updates automatically whenever you switch documents.

If no document is open, or nothing links to the current one yet, the panel says so instead of showing an empty list.

The same list can also be shown inline, in a resizable panel below the editor's own content, instead of (or alongside) this dock tab — toggle it from the Editor's ☰ menu or **`Ctrl+B`** (see [Writing and the Editor](writing-and-editor.md)).
