# Project Templates

**`File > New Project`** shows a template picker before the usual folder picker (and name prompt, for a non-empty folder) — pick a starting scaffold, then locate the new project as before. Five templates ship built-in:

| Template | What it scaffolds |
|---|---|
| **Blank** (default*) | Nothing — an empty project, exactly like `File > New Project` behaved before templates existed |
| **Novel** | A `Manuscript` folder with two starter chapters, a `Characters` folder with a Protagonist document (Desire/Misbelief/Arc headings), plus Research and Trash folders (roles already assigned) |
| **Nonfiction** | A `Manuscript` folder with an Introduction and a "Part One" subfolder containing a first chapter, plus Research and Trash |
| **Screenplay** | A `Screenplay` folder with Act One/Two/Three starter documents, plus Research and Trash. Smaragd's editor is plain Markdown, not Fountain — this reproduces a screenplay draft's *look* with headings, not a real screenplay-format pipeline |
| **World-Building** | A `Manuscript` folder with a starter chapter, Research, a `World` folder (`Characters` with Main/Supporting subfolders, `Locations`, `Items`), and a Templates folder with Character/Location stationery documents (`${{name}}` placeholder, "New From Template" — see [Template Variables](folder-roles.md#template-variables)), plus Trash — all roles already assigned |

\* The very first time you've ever opened a project in smaragd, the picker instead starts on **World-Building** — see [Projects](projects.md).

**`File > Save Project as Template…`** turns your *current* project's own folder/document structure into a reusable custom template, prompting for a name. It excludes:
- Whatever's currently inside the project's Trash folder (if one's configured)
- Narrative state that belongs to one specific project, not a reusable shape: story cards, the protagonist Desire/Misbelief pair, and book/export/git metadata

Custom templates are stored in `smaragd/project_templates/` in the platform config directory (the same base path as custom themes/styles/plugins — see [Plugins](plugins.md)):

- Linux: `~/.config/smaragd/project_templates`
- macOS: `~/Library/Application Support/smaragd/project_templates`
- Windows: `%APPDATA%\smaragd\config\project_templates`

Each is a subfolder containing a `template.toml` (label, description) and a `content/` folder mirroring the structure to stamp out. Unlike custom themes/styles, there's no "Reload Custom Templates" button — a hand-dropped or hand-edited template only shows up in the picker after restarting the app (saving one via **Save Project as Template…** refreshes the list immediately, since the app already knows it just wrote it).
