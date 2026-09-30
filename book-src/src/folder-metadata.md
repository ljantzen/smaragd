# Folder Metadata

Folders can carry the same five metadata fields a document's own frontmatter does — but since a folder has no file of its own to hold YAML, they're stored in the project's `.smaragd/project.json` instead, one entry per folder.

**Click any folder row in the Binder** (other than the project root, which opens [Project Metadata](project-metadata.md) instead) to switch the Metadata dock to that folder's own fields, headed **"Folder Metadata"** so it's always clear you're editing a folder and not a document:

| Field | Meaning |
|---|---|
| `Type` | Free-form section type, same as a document's. |
| `Status` | Free-form drafting status, same as a document's. |
| `POV` | Point-of-view character, free text. |
| `Word Count Target` | A target word count for everything nested inside this folder. |
| `Tags` | Free-form tags, edited the same removable pill-style chips a document's are. |

See [Document Metadata](document-metadata.md) for the fuller description of what each field means and how the chip editor works — it's the identical form, just without a live **Word count** readout, since a folder has no body of its own to count.

## What actually uses it

- **Status** and **POV** feed [Binder Background Coloring](binder.md#binder-background-coloring) the same way a document's own `status`/`pov` do — a folder's row gets colored by whichever value it carries, once that coloring mode is active. The color-swatch button next to `Status:`/`POV:` assigns the same project-wide color a document's swatch would; coloring "draft" or "Alice" once colors every row carrying that value, document or folder alike.
- **Word Count Target** feeds the **Word Count Progress** coloring mode specifically: a folder's row is colored by the *combined* word count of every document nested inside it (computed off the UI thread, alongside the [Word Count](word-count.md) panel's own total) against the folder's own target.
- **Type** and **Tags** aren't read anywhere else yet — they're carried along purely for parity with the document form, in case you want the same fields available at a glance without opening every document inside. In particular, a folder's `Tags` don't feed the [Tags](tags.md) dock or `#tag` search; only a document's own tags do.

Folder metadata is entirely independent of [Folder Role](folder-roles.md) (Research/Trash/Templates/Manuscript/World) — the same folder can have both, neither, or either on its own; setting one never touches the other.
