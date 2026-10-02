# Git Integration

Git works alongside [Sync](sync.md): git commits whatever is on disk at that moment, while sync moves the same files between your devices. Sync's own bookkeeping (your device's access token and state) is kept outside the project folder, so it never lands in a commit.

**`Versions` menu**, or **`:git`** commands:

- Opt-in per project — you're offered "Enable Git Support" once when a project is opened, or you can trigger it manually (`Versions > Enable Git Support` or `:git enable`)
- **Commit** (`Ctrl+Alt+C`) / **Commit and Push** (`Ctrl+Alt+Shift+C`) / **Push** (`Ctrl+Alt+P`) / **Pull** (`Ctrl+Alt+L`) — shells out to the system `git` binary
- Push and pull run on a background thread, so a slow or hung network operation never freezes the UI
- A file with uncommitted changes gets a trailing "•" marker in the [Binder](binder.md#binder-background-coloring) — folders show the same marker if anything nested inside them is dirty

## The Commit dialog

Confirming Commit or Commit and Push opens a multi-line message box, pre-filled from the [commit message template](#commit-message-template) below — edit it freely before confirming. `Ctrl+Enter` or the button commits (plain `Enter` inserts a newline instead, since the template's body commonly lists every changed file).

## Commit message template

**`File > Settings > History`** has a **Commit message template** — a multi-line text box used both for the Commit dialog's pre-filled message and for every automatic commit (see [Auto-commit](#auto-commit) below), with placeholders filled in each time:

| Placeholder | Value |
|---|---|
| `{{date}}` | Today, formatted per `Settings > Templates` |
| `{{time}}` | Current time, `HH:MM` (24-hour) |
| `{{numFiles}}` | Number of dirty files |
| `{{linesAdded}}` | Lines inserted |
| `{{linesDeleted}}` | Lines deleted |
| `{{linesChanged}}` | Lines inserted + deleted |
| `{{fileList}}` | One line per changed file, `A`/`M`/`D` followed by its path (added/changed/deleted) |

The default template is:

```
Smaragd backup

{{fileList}}
```

— so out of the box every commit's body lists what changed, without you having to discover `{{fileList}}` yourself. The Settings page shows a live preview and a quick-reference table of every placeholder.

## Auto-commit

**`File > Project Settings > Git`** (see [Project Settings](project-settings.md)) can commit automatically on an interval, per project:

- **Automatically commit changes** — off by default
- **Every N minutes** — how often (default 15)
- **Push after each automatic commit** — off by default; independent of the interval, so you can auto-commit locally without ever auto-pushing

An automatic commit uses the same template and rendering as a manual one, and reuses the manual Commit path exactly — "nothing to commit" is silently skipped, and a failure surfaces the same error toast a manual commit would. It never fires the moment a project opens (the timer starts counting from when you opened it, not from some earlier session), and it steps aside if a push/pull you triggered manually is already in flight.

## Version Activity

**`Versions > Version Activity`** (`Ctrl+Alt+V`) is a dockable tab with three sections:

- **Dirty files** — the same set that drives the Binder's "•" marker, listed out.
- **Recent commits** — `git log`'s last 20 commits: short hash, subject, author, and relative date.
- **Activity** — a running log of what smaragd itself did via git this session (commits, pushes, pulls, both manual and automatic), each with an outcome (success, "nothing to commit," or an error) and how long ago. A push or pull entry expands to list the files it actually transferred. Cleared when you close the project or open another one; not saved to disk.

A **Refresh** button re-reads dirty files and the commit log on demand, for changes made outside the app (e.g. a `git commit` from a terminal).

## Global on/off switch

**`File > Settings > History`** also has an app-wide **"Enable Git integration"** switch — a stronger, global kill switch on top of the per-project opt-in above. Off by default for a brand new install (on by default for anyone upgrading from a version before this setting existed, so it never silently turns git off under you). Turning it off hides the `Versions` menu entirely (Version Activity included), makes every git shortcut and `:git` command a no-op, drops the Git rows from the Shortcuts settings, removes `git` from the command-prompt's autocomplete, and skips the one-time "enable git support?" prompt when opening a project.
