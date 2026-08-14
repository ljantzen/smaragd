# Typesetting Styles

Twelve built-in styles ship with smaragd — the first eight follow US/Amazon-KDP trim conventions, the last four follow UK/European ones:

| id | Label | What it looks like |
|---|---|---|
| `manuscript` | Manuscript | Plain submission format: US Letter, 1in margins, double-spaced, ragged-right (not justified), no running header or drop cap |
| `trade_paperback` | Trade Paperback | 6×9in trim, justified body text, a running header (author's name / current chapter), and a drop cap on each chapter's first paragraph |
| `mass_market` | Mass Market Paperback | 4.25×6.87in trim (the standard mass-market size), small 9pt justified type and tight margins to match, running header, drop cap |
| `digest` | Digest | 5.5×8.5in trim — between Mass Market and Trade Paperback, common for novellas and shorter literary fiction — justified, running header, drop cap |
| `hardcover` | Hardcover | 6.14×9.21in trim (KDP's hardcover size), roomier margins and a larger drop cap for a more formal feel, sans-serif (Atkinson Hyperlegible) headings over a serif body, running header shows book title / current chapter |
| `academic` | Academic | A4, 12pt, ragged-right (not justified — matches APA-style manuscript guidance), no running header or drop cap; for a thesis chapter or paper draft, not a book |
| `large_print` | Large Print | 7×10in trim, 18pt Atkinson Hyperlegible (a sans-serif designed for low-vision readers) ragged-right body text, no drop cap or running header — accessibility features, not decoration |
| `chapbook` | Chapbook | 5×8in trim, generous margins, ragged-right (justification would fight a poem's own line breaks), no drop cap, running header shows author/title rather than a chapter |
| `uk_b_format` | UK B-Format Paperback | 129×198mm — the standard UK trade paperback trim, distinct from (not a rescale of) the US 6×9in Trade Paperback — 10pt justified, running header, drop cap |
| `uk_a_format` | UK A-Format Paperback | 110×178mm — the UK mass-market paperback trim, smaller and narrower than the US Mass Market Paperback — 9pt justified, running header, drop cap |
| `a5` | A5 Paperback | ISO 216 A5 (148×210mm exactly) — a common European trade/paperback trim, used as a book size in its own right rather than cut down from a larger sheet — 10pt justified, running header, drop cap |
| `manuscript_a4` | Manuscript (A4) | `manuscript`'s exact submission conventions (double-spaced, ragged-right, no header or drop cap) on ISO A4 instead of US Letter, for submitting outside North America |

Like [color themes](themes.md#custom-themes) and plugins, custom styles are `.toml` files you author or drop into `smaragd/styles/` inside smaragd's config directory (no in-app style editor):

- Linux: `~/.config/smaragd/styles`
- macOS: `~/Library/Application Support/smaragd/styles`
- Windows: `%APPDATA%\smaragd\config\styles`

A minimal custom style:

```toml
id = "novella"
label = "Novella"

[page]
width_mm = 139.7   # 5.5in
height_mm = 215.9  # 8.5in
margin_mm = 15.0

[body]
font = "Libertinus Serif"
size_pt = 11
line_height = 1.2
justify = true

[headings]
font = "Libertinus Serif"
sizes_pt = [22, 19, 16, 14, 13, 12]

[blockquote]
font = "Libertinus Serif"
size_pt = 11
italic = true

[code]
font = "DejaVu Sans Mono"
size_pt = 10
```

`id` and `label` are required, along with the `[page]`/`[body]`/`[headings]`/`[blockquote]`/`[code]` tables — `id` is what selects the style and is lowercased automatically. `sizes_pt` needs all six sizes (one per heading level, `h1`–`h6`). Three more tables are optional:

```toml
[verse]
font = "Libertinus Serif"
size_pt = 11
italic = false

[drop_cap]
scale = 3.0  # first letter renders at 3x body size

[running_header]
left = "{author}"
right = "{chapter}"
```

`[verse]` styles a ` ```verse ` fenced block (see [Verse](markdown-preview.md#verse)) — same shape as `[blockquote]`/`[code]`, but unlike either, it's optional: a style file predating verse support that omits it falls back to a built-in default rather than failing to load.

`{title}`/`{subtitle}`/`{author}` are substituted with whatever's typed into the export dialog; `{chapter}` (supported as a whole side's content, not mixed with other text) shows the current chapter on the print PDF specifically — DOCX and EPUB don't have a per-page "current chapter" concept, so a `{chapter}` token is just left blank there.

**"Libertinus Serif", "DejaVu Sans Mono", and "Atkinson Hyperlegible"** (the built-in styles' fonts) aren't arbitrary choices — they're guaranteed available to the PDF renderer specifically, bundled with smaragd itself rather than depending on what's installed on your system. A custom style naming some other font still works for DOCX/EPUB (which just reference a font by name, the same way any other document does — Word/an e-reader substitutes if it's not installed), and for PDF too if that font happens to be installed locally; if not, the PDF falls back to *some* available font rather than failing the export.

## Using your own font file

Naming a font that isn't one of those three works for DOCX/EPUB/PDF as above, but the **Preview** tab can't render it — it only knows the fonts actually installed with smaragd, so it falls back to a generic face on-screen even though the exported file uses the real font.

To make Preview (and PDF, without needing the font separately installed as a system font) use your own font file too, add a `font_file` key alongside `font` in any of `[body]`/`[headings]`/`[blockquote]`/`[code]`/`[verse]`:

```toml
[body]
font = "My Custom Font"
font_file = "MyCustomFont.ttf"
size_pt = 11
```

`font_file` accepts a `.ttf` or `.otf` file, either as an absolute path or (as above) relative to the style's own `.toml` file — so you can keep a style and its font side by side in `smaragd/styles/` and reference it by filename alone. `font` is still what DOCX/EPUB write into the document and what the file is registered under for Preview/PDF, so keep it a real, sensible font name — it's what a reader without the font installed will see substituted. Reload Custom Styles picks up a new or edited `font_file` the same as any other style change; a file that doesn't exist or isn't a valid font is skipped with an error message (that one font slot falls back to a generic face) rather than blocking the rest of the style from loading.

A relative `font_file` resolves against wherever *that particular `.toml` file* lives, which in turn lives in the OS-specific styles folder from the list above (Linux `~/.config/smaragd/styles`, macOS `~/Library/Application Support/smaragd/styles`, Windows `%APPDATA%\smaragd\config\styles`) — so `font_file = "MyFont.ttf"` always means "next to this style file" on every platform, no path syntax to adjust. An *absolute* path, though, is tied to the OS it was written for (`/home/you/Fonts/MyFont.ttf` vs. `C:\Fonts\MyFont.ttf`) and isn't portable if you copy the style file to a different machine running a different OS — a path that isn't recognized as absolute on the machine it's loaded on is silently treated as *relative* instead (almost certainly not the file you meant), rather than erroring. Keeping the font next to the style and using a relative path avoids this entirely, and is the recommended approach if you ever share or sync your `smaragd/styles/` folder across machines. Filesystem case-sensitivity is a related wrinkle: Linux is case-sensitive, macOS and Windows normally aren't, so a `font_file` whose case doesn't exactly match the real filename can still work on macOS/Windows but fail to be found on Linux.

Use **Reload Custom Styles** in the export dialog to pick up a new or edited `.toml` file without restarting. A style file that fails to parse, or whose `id` collides with an already-loaded style (built-in or another custom one — whichever loaded first wins), is skipped with an error message rather than stopping other styles from loading.
