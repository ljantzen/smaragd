# Project Settings

**`File > Project Settings…`** (or **`F4`**) is a two-pane dialog, structured just like the app-wide `File > Settings` (see [Settings](settings.md)) — a category list on the left, that category's controls on the right — but scoped to the open project rather than app-wide, and disabled with no project open. It's grouped in the File menu right next to `Settings`.

- **Git**: **Automatically commit changes**, an interval in minutes, and **Push after each automatic commit** — see [Auto-commit](git-integration.md#auto-commit). Grayed out (with an explanatory note) unless git integration is on both app-wide and for this project.
- **Sync**: **Also sync images, PDFs and other files** — see [Images, PDFs and other files](sync.md#images-pdfs-and-other-files). Pairing, live status, and the device list stay in the Sync dock tab; this is the one actual *setting*, not a status or action.
- **Binder**: the same **Off / Status / POV / Word Count Progress** choice as `View > Color Binder By` — see [Binder Background Coloring](binder.md#binder-background-coloring). An additional, discoverable home for the same setting; the binder's own right-click menu and the status-bar indicator still work too.

More project-scoped settings will likely land here over time, the same way the app-wide `Settings` dialog grew its own categories.
