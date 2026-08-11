# Folder Roles: Research, Trash, Templates, Manuscript

Right-click a folder and choose **Folder Role** to designate it as one of four special folders. A folder with a role assigned shows a leading icon in the binder (🔍/🗑/📋/📖) instead of a text label. Research, Trash, and Templates are exclusive — at most one folder per role, project-wide — but Manuscript isn't: several folders can hold it at once, e.g. one per book in a series.

- **Trash**: deleting a file or folder moves it here instead of removing it from disk. Right-click the Trash folder for **Empty Trash** (permanent, with confirmation), or right-click a trashed item for **Restore**.
- **Templates**: any document placed directly inside this folder (not in a subfolder of it) shows up in every other folder's right-click **"New From Template"** submenu. Picking one creates a new document from a copy of it — frontmatter included, with [template variables](#template-variables) substituted — after prompting you for a name. The template itself is never modified.
- **Research**: currently just a marker with no behavior yet attached — reserved for future features like word-count rollups. Unlike Trash and Templates, [Export](export.md) does *not* skip a Research-role folder — right-clicking one to export it exports it like any other folder.
- **Manuscript**: designates a folder as your project's primary manuscript content, mirroring Scrivener's Draft folder. **`File > Export Manuscript…`** compiles straight from it instead of you having to right-click and find the folder yourself: if no folder has the role yet, it falls back to exporting the whole project; if exactly one does, it exports that folder directly; if more than one does, it opens a submenu to pick which one. It's also what the [Word Count](word-count.md) panel's "Manuscript folders only" tracking scope sums.

## Template Variables

A template document can use two placeholders, substituted when a new document is created from it:

| Placeholder | Substituted with |
|---|---|
| `${{name}}` | The name you typed when creating the document (without the `.md` extension) |
| `${{date}}` | Today's date, formatted per the date format set in **`File > Settings`** |

For example, a template starting with:

```markdown
---
type: Scene
---
# ${{name}}

Started ${{date}}.
```

typed into the "New From Template" prompt as "Aria" would produce a document starting with `# Aria` and today's date in place of `${{date}}`.

The date format is a single format string, shared by every template — it's a [strftime](https://en.wikipedia.org/wiki/Strftime) pattern, the same mini-language used by `date`, `printf`, and most other tools that format dates from a code. It defaults to `%Y-%m-%d` (e.g. `2026-07-28`) when left blank. Some common formats:

| Format | Example output |
|---|---|
| `%Y-%m-%d` | `2026-07-28` |
| `%d/%m/%Y` | `28/07/2026` |
| `%m/%d/%Y` | `07/28/2026` |
| `%d %B %Y` | `28 July 2026` |
| `%B %-d, %Y` | `July 28, 2026` |
| `%A, %d %B %Y` | `Tuesday, 28 July 2026` |
| `%Y%m%d` | `20260728` |

A format that isn't a valid strftime pattern falls back to `%Y-%m-%d` automatically, both in a created document and in Settings' own live preview of the format — a typo here never blocks document creation.
