# Where Plugins Live

- **Global**, always loaded: `plugins/` inside smaragd's config directory
  - Linux: `~/.config/smaragd/plugins`
  - macOS: `~/Library/Application Support/smaragd/plugins`
  - Windows: `%APPDATA%\smaragd\config\plugins`
- **Per-project**: `.smaragd/plugins/` inside the project folder. This only loads once you explicitly turn it on for that project via **`Tools > Enable Project Plugins`** — a project folder shared or pulled from somewhere else could otherwise run unreviewed code the moment you open it.

Use **`Tools > Reload Plugins`** to pick up new or edited scripts without restarting the app. A script that fails to compile or run, or that tries to register a `:` command another plugin already owns, is skipped with an error message — it never stops other plugins from loading.
