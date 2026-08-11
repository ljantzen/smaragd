# Git Integration

Modeled on the Obsidian Git plugin. **`Versions` menu**, or **`:git`** commands:

- Opt-in per project — you're offered "Enable Git Support" once when a project is opened, or you can trigger it manually (`Versions > Enable Git Support` or `:git enable`)
- Commit / Commit and Push / Push / Pull — shells out to the system `git` binary
- Push and pull run on a background thread, so a slow or hung network operation never freezes the UI
- A file with uncommitted changes gets a trailing "•" marker in the [Binder](binder.md#binder-background-coloring) — folders show the same marker if anything nested inside them is dirty

**`File > Settings > History`** has an app-wide **"Enable Git integration"** switch — a stronger, global kill switch on top of the per-project opt-in above. Off by default for a brand new install (on by default for anyone upgrading from a version before this setting existed, so it never silently turns git off under you). Turning it off hides the `Versions` menu entirely, makes the Commit/Push shortcuts and every `:git` command a no-op, drops the Git rows from the Shortcuts settings, removes `git` from the command-prompt's autocomplete, and skips the one-time "enable git support?" prompt when opening a project.
