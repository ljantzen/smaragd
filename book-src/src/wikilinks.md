# Wikilinks

Type `[[Topic]]` or `[[Topic|Alias]]` to link to another document by its filename (no path needed — resolution is by name, project-wide).

- In the preview, wikilinks render as clickable links.
- **Ctrl+Click** a link in the preview — or place your cursor on one in the editor and press **Ctrl+Enter** (the remappable "Activate Wikilink" shortcut — see [Keyboard Shortcuts](keyboard-shortcuts.md)) — to jump to it. If the target document doesn't exist yet, this creates it, in the same folder as the note you linked from.
- While typing `[[` in the editor, an autocomplete popup filters matching document titles as you type. Navigate it with arrow keys or Tab, and press Enter (or click) to accept.
- Broken-link styling: a `[[wikilink]]` whose target doesn't match any document in the project renders in a distinct color (both in the editor and the preview) instead of the normal link color, so a typo or a not-yet-created note stands out at a glance. Clicking (or Ctrl+Enter-ing) it still offers to create the missing document, same as always. This color comes from the active [color theme](themes.md) (`broken_wikilink`), so it changes along with the rest of the palette — a custom theme can set its own.
