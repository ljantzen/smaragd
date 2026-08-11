# Document Metadata (Frontmatter)

Each document can carry a YAML frontmatter block (Longform/Scrivener-style manuscript metadata) at the very top of the file:

```yaml
---
type: Chapter
status: draft
pov: Alex
word_count_target: 2500
tags: [action, chapter-3]
---
```

Open **`View > Metadata`** (or **`Ctrl+Shift+M`**) to edit these fields through a dockable form (see [Dockable Tool Windows](dockable-tool-windows.md)) instead of hand-editing YAML. Unlike a typical dialog, there's no Save/Cancel step — edits apply as you type, the same way typing in the main editor does. Smaragd only ever reads/writes these five keys:

| Field | Meaning |
|---|---|
| `type` | Free-form section type — "Chapter", "Scene", "Part", or anything you want. Not tied to folder nesting. |
| `status` | Free-form drafting status — "draft", "revised", "final", or anything you want. |
| `pov` | Point-of-view character, free text. |
| `word_count_target` | A target word count for this document. |
| `tags` | A list of free-form tags — see [Tags](tags.md) for combining these with inline `#tag` mentions and searching by tag. |

Any other YAML key you've hand-added to the block (or that some other tool wrote) is left alone — Smaragd never round-trips the whole block through its own data model, so unrelated keys survive a save untouched. The frontmatter block is stripped from the Markdown preview so it doesn't render as a garbled paragraph.

The Metadata panel also shows a **Word count** — a live, read-only count of the open document's body (frontmatter excluded), recomputed continuously from whatever's currently in the editor, not just what was last saved.

The `Status:` and `POV:` rows each show a color-swatch button once that field isn't blank — click it to assign that status/POV value a project-wide binder background color. See [Binder Background Coloring](binder.md#binder-background-coloring) for what these colors are used for and how to switch which one (if any) the Binder actually displays.

## Dropdown Source Folders

By default, `type`/`status`/`pov` are free text — nothing stops "Scene" and "scene" and "seen" from all being typed for the same field across a project. To turn one of them into a closed dropdown instead, right-click any folder and check it under **Dropdown Source** for **Type**, **Status**, or **POV**. That folder's direct child documents' titles (not documents in a subfolder of it) become the dropdown's options for that field; the Metadata panel's `Type:`/`Status:`/`POV:` row switches from a text box to a dropdown automatically as soon as a field has at least one folder assigned and one document in it.

A few things worth knowing:

- **Independent per field, and independent of Folder Role.** Type, Status, and POV each have their own separate folder assignment — the same folder can drive more than one field, or each can point somewhere different. Checking a folder here doesn't touch whatever [Folder Role](folder-roles.md) it already has (or lack of one), and doesn't exclude it from [Export](export.md) — so an existing Research folder of character bios can double as the POV dropdown's source without anything else about it changing.
- **Never destroys an existing value.** If a document's `pov: Alice` was typed before you ever assigned a POV folder — or Alice's document has since been renamed or removed from that folder — the field still shows "Alice" as-is; it just isn't one of the clickable options until you pick something else from the dropdown.
- **"(none)"** is always the first dropdown entry, for clearing the field.
- Not recursive: only documents placed directly inside the assigned folder count, the same limitation [Templates](folder-roles.md) has.
