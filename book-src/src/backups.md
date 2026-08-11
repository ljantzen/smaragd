# Backups

Modeled on Scrivener's own automatic backup scheme. **`File > Settings > History`**:

- Off by default — a master **"Enable automatic backups"** switch, plus independent triggers for backing up **when opening a project**, **when closing a project**, and **on every manual save** (`Ctrl+S`/`:w`/`:wq` — not the silent autosave that runs on losing focus or switching documents)
- Each backup is a zipped, timestamped snapshot of the whole project folder — `{project name}-{YYYY-MM-DD-HHMMSS}.zip` — written to one shared **backup folder** (defaulting to the platform's data directory, browsable to somewhere else, with a **Reset** button to go back to the default) rather than inside the project itself, so it survives the project folder being moved or deleted
- Multiple projects share the same backup folder; each project's own backups are told apart by their filename prefix, so keeping or pruning one project's backups never touches another's
- A backup includes everything under the project folder that isn't excluded by a `.gitignore`/`.ignore` file — including `.smaragd/` (manuscript ordering, folder roles, story cards, and the rest of the project's own metadata) — but always skips `.git`, since that's redundant with git's own history and often the single largest thing in the folder
- **Backups to keep** (1–100, default 10) caps how many of a project's own snapshots are kept; the oldest are deleted once a new backup pushes past that count
- A failed backup shows as an error toast and never blocks whatever you were doing (opening/closing a project, saving) — a backup is a safety net, not a precondition for those
- There's no in-app restore, and no manual "back up now" action — only the three triggers above. To restore a backup, unzip it somewhere and open the resulting folder as a project (**`File > Open Project`**) the normal way
