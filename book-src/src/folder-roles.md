# Folder Roles: Research, Trash, Templates, Manuscript, World

Right-click a folder and choose **Folder Role** to designate it as one of five special folders. A folder with a role assigned shows a leading icon in the binder (🔍/🗑/📋/📖/🌐) instead of a text label. Research, Trash, Templates, and World are exclusive — at most one folder per role, project-wide — but Manuscript isn't: several folders can hold it at once, e.g. one per book in a series.

- **Trash**: deleting a file or folder moves it here instead of removing it from disk. Right-click the Trash folder for **Empty Trash** (permanent, with confirmation), or right-click a trashed item for **Restore**.
- **Templates**: any document placed directly inside this folder (not in a subfolder of it) shows up in every other folder's right-click **"New From Template"** submenu. Picking one creates a new document from a copy of it — frontmatter included, with [template variables](#template-variables) substituted — after prompting you for a name. The template itself is never modified.
- **Research**: currently just a marker with no behavior yet attached — reserved for future features like word-count rollups. Unlike Trash and Templates, [Export](export.md) does *not* skip a Research-role folder — right-clicking one to export it exports it like any other folder.
- **Manuscript**: designates a folder as your project's primary manuscript content, mirroring Scrivener's Draft folder. **`File > Export Manuscript…`** compiles straight from it instead of you having to right-click and find the folder yourself: if no folder has the role yet, it falls back to exporting the whole project; if exactly one does, it exports that folder directly; if more than one does, it opens a submenu to pick which one. It's also what the [Word Count](word-count.md) panel's "Manuscript folders only" tracking scope sums.
- **World**: like Research, currently just a marker with no behavior yet attached — for a project's worldbuilding content (characters, locations, items). The [World-Building project template](project-templates.md) tags its `World` folder with this role automatically.

## Template Variables

A template document can use these placeholders, substituted when a new document is created from it (also used by [Daily Notes](daily-notes.md)):

| Placeholder | Substituted with |
|---|---|
| `${{name}}` | The name you typed when creating the document (without the `.md` extension) |
| `${{date}}` | Today's date, formatted per the date format set in **`File > Settings`** |
| `${{date:FORMAT}}` | Today's date, formatted per `FORMAT` instead — a one-off override |
| `${{time}}` | The current time, formatted per the time format set in **`File > Settings`** |
| `${{time:FORMAT}}` | The current time, formatted per `FORMAT` instead |
| `${{previous_day}}` / `${{next_day}}` | Yesterday's/tomorrow's date (also take a `:FORMAT` override) |
| `${{cursor}}` | Where the text cursor lands once the document is created/opened |

For example, a template starting with:

```markdown
---
type: Scene
---
# ${{name}}

Started ${{date}} at ${{time}}.${{cursor}}
```

typed into the "New From Template" prompt as "Aria" would produce a document starting with `# Aria` and today's date and time in place of `${{date}}`/`${{time}}`, with the cursor left right after "Started ... at ...".

The date and time formats are each a single format string, shared by every template — they're [strftime](https://en.wikipedia.org/wiki/Strftime) patterns, the same mini-language used by `date`, `printf`, and most other tools that format dates from a code (if you know Obsidian's Periodic Notes plugin, this is the same idea as its moment.js format field, just a different token syntax). They default to `%Y-%m-%d` and `%H:%M` (e.g. `2026-07-28` and `14:05`) when left blank, unless a placeholder's own `:FORMAT` override says otherwise. **`File > Settings > Templates`** and **`File > Project Settings… > Daily Notes`** both show a live-previewed "Example formats" list of the ready-made formats below, right next to the fields that use them, in addition to this page.

Individual codes, for building your own format — the ones most likely to be useful:

| Code | Meaning | Example |
|---|---|---|
| `%Y` | 4-digit year | `2026` |
| `%y` | 2-digit year | `26` |
| `%m` | Month, zero-padded | `07` |
| `%-m` | Month, no padding | `7` |
| `%B` | Month name | `July` |
| `%b` | Month name, abbreviated | `Jul` |
| `%d` | Day of month, zero-padded | `28` |
| `%-d` | Day of month, no padding | `28` |
| `%A` | Weekday name | `Tuesday` |
| `%a` | Weekday name, abbreviated | `Tue` |
| `%j` | Day of year (1–366) | `209` |
| `%V` | ISO week number (1–53) | `31` |
| `%H` | Hour, 24-hour, zero-padded | `14` |
| `%I` | Hour, 12-hour, zero-padded | `02` |
| `%M` | Minute, zero-padded | `05` |
| `%S` | Second, zero-padded | `09` |
| `%p` | AM/PM | `PM` |
| `%Z` | Time zone name | `CEST` |
| `%%` | A literal `%` | `%` |

A code with no leading `-` (e.g. `%d`, `%m`) is zero-padded to a fixed width; adding one (`%-d`, `%-m`) drops the padding — useful for a human-readable date like `July 28, 2026` rather than `July 08, 2026`. Any character in the format that isn't a `%` code (spaces, `-`, `/`, `,`, letters) passes through literally, which is also how a Daily Notes folder format like `Journal/%Y/%m` produces a `/`-separated subfolder path rather than a single filename.

Some common complete formats:

| Format | Example output |
|---|---|
| `%Y-%m-%d` | `2026-07-28` |
| `%d/%m/%Y` | `28/07/2026` |
| `%m/%d/%Y` | `07/28/2026` |
| `%d %B %Y` | `28 July 2026` |
| `%B %-d, %Y` | `July 28, 2026` |
| `%A, %d %B %Y` | `Tuesday, 28 July 2026` |
| `%Y%m%d` | `20260728` |
| `%Y/%m` | `2026/07` — a Daily Notes folder nested by year then month |
| `%Y/%m/%d` | `2026/07/28` — nested by year, month, *and* day |
| `%H:%M` | `14:05` |
| `%H:%M:%S` | `14:05:09` |
| `%I:%M %p` | `02:05 PM` |

A format that isn't a valid strftime pattern falls back to `%Y-%m-%d` (or `%H:%M` for a time format) automatically, both in a created document and in every live preview of the format — a typo here never blocks document creation.
