# Themes and Appearance

Two independent settings:

- **Appearance** (`File > Settings`, or `:dmode <dark|light|system>`): plain Dark/Light/System styling. System follows your OS preference and updates immediately.
- **Color Theme** (`View > Theme`, or `:theme <id>`): a full Helix-inspired palette layered on top of the appearance base. `:theme` with no argument clears back to plain appearance styling.

Since the editor is a single plain-text field with no syntax-highlighting pipeline, each theme reproduces its palette's overall look (background, body text, one accent color for selection/links) rather than full per-token syntax highlighting.

## Built-in themes

| id | Label |
|---|---|
| `gruvbox` | Gruvbox |
| `gruvbox_light` | Gruvbox Light |
| `dracula` | Dracula |
| `nord` | Nord |
| `nord_light` | Nord Light |
| `solarized_dark` | Solarized Dark |
| `solarized_light` | Solarized Light |
| `catppuccin_mocha` | Catppuccin Mocha |
| `catppuccin_latte` | Catppuccin Latte |
| `onedark` | One Dark |
| `onelight` | One Light |
| `tokyonight` | Tokyo Night |
| `everforest_dark` | Everforest Dark |
| `everforest_light` | Everforest Light |
| `ayu_dark` | Ayu Dark |

## Custom themes

You can add your own themes as `.toml` files — no in-app editor, the same "drop a file in a folder" model as [Plugins](plugins.md). Custom themes live in `smaragd/themes/` inside smaragd's config directory (the same base path as the global plugins folder):

- Linux: `~/.config/smaragd/themes`
- macOS: `~/Library/Application Support/smaragd/themes`
- Windows: `%APPDATA%\smaragd\config\themes`

A minimal custom theme:

```toml
id = "my_theme"
label = "My Theme"
dark = true
background = "#1e1e2e"
foreground = "#cdd6f4"
accent = "#cba6f7"
```

`id`, `label`, `dark`, and the three colors are required; colors are `"#RRGGBB"` hex strings (the `#` is optional). `id` is what you'd type as `:theme my_theme` — it's lowercased automatically, so casing in the file doesn't matter.

An optional fourth color, `broken_wikilink`, sets the color a `[[wikilink]]` renders in (Editor and Preview alike) when its target doesn't match any document in the project. Omit it and it falls back to a plain default red:

```toml
broken_wikilink = "#f38ba8"
```

Use **`View > Theme > Reload Custom Themes`** to pick up a new or edited file without restarting the app. A theme file that fails to parse, has an invalid color, or whose `id` collides with an already-loaded theme (built-in or another custom one — whichever loaded first wins) is skipped with an error message rather than stopping other themes from loading. If the theme you currently have active stops resolving after a reload (for instance, you just introduced a mistake into the file you're editing), smaragd falls back to the default appearance rather than leaving a stale palette applied with nothing in the menu showing as selected.

## Editor Font

**`File > Settings`** has a **Font** section with a font and size for the Editor. (The Preview tab's fonts come from the selected [typesetting style](export-typesetting-styles.md) instead — see [Markdown Preview](markdown-preview.md) — so this setting no longer affects Preview; to resize Preview's text on the fly without changing that style, see [Zooming](markdown-preview.md#zooming).)

| Font | What it looks like |
|---|---|
| Proportional | egui's built-in sans-serif — the default |
| Monospace | egui's built-in fixed-width face |
| Libertinus Serif | A literary serif text font — the same one used by the [Trade Paperback export style](export-typesetting-styles.md) |
| DejaVu Sans Mono | A fixed-width face — the same one used by the [Manuscript export style](export-typesetting-styles.md) |
| Atkinson Hyperlegible | A sans-serif designed by the Braille Institute for readability, particularly for low-vision readers — the same one used by the [Large Print export style](export-typesetting-styles.md) |

These five are the only choices — not a live picker over every font installed on your system, so the app looks and behaves identically on every platform. A typesetting style naming one of these by name (`"Libertinus Serif"`/`"DejaVu Sans Mono"`/`"Atkinson Hyperlegible"`) renders in Preview with the real font; any other name falls back to a default face for on-screen preview purposes only — the exported DOCX/EPUB/PDF still use the real font name.

The **Appearance** settings category has a separate **UI font** picker — the same five choices as above, but for the rest of the app's chrome (menus, the Binder, buttons, headings) instead of the Editor/Preview specifically. It defaults to Proportional (egui's own default), so nothing changes until you pick something else.
