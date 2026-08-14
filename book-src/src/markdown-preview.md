# Markdown Preview

**`View > Preview`** (or the Toggle Preview shortcut) renders the current document the way it will actually look once exported — fonts, sizes, justification, page proportions, and drop cap all come from the currently selected [typesetting style](export-typesetting-styles.md), the same one the [Export](export.md) dialog uses. The open document's title sits at the left of the top bar, same as the [Editor pane](writing-and-editor.md) — the Preview tab itself is just labeled "Preview", so this is what actually confirms which document you're looking at. A **Style** picker at the right of that same bar lets you switch styles live and see the change immediately; picking a style there updates the project's export style too, so Preview and the Export dialog always agree on what you'll get switching one from the other.

Headings, quotes, code blocks, lists, and tables still render with sensible on-screen formatting, but no longer in the old color-coded "dev preview" palette — book export is effectively monochrome ink on a page, so Preview reads that way too.

Images work two ways:
- Standard Markdown: `![alt](path/to/image.png)`
- Obsidian-style embeds: `![[image.png]]`

Relative image paths resolve against the open document's own folder, and must stay inside the project (a path that tries to escape the project root — via `..` or a symlink — is refused). Remote `http(s)://` images are never fetched.

## Zooming

Hold Ctrl and scroll over the Preview pane to zoom the rendered text in and out — handy for proofreading at a larger size or fitting more of a page on screen. `Ctrl` + `+` and `Ctrl` + `-` do the same from the keyboard, and `Ctrl` + `0` resets back to 100%. This only scales what's shown in Preview; it doesn't change the [typesetting style](export-typesetting-styles.md)'s actual font sizes, so Export is unaffected, and your zoom level is remembered the next time you open the app.

## Typewriter Quotes

**`File > Settings > Editor`** has a **"Typewriter quotes in Preview and export"** checkbox, off by default. When it's on, straight typewriter punctuation is rewritten wherever markdown gets rendered *from* — the Preview pane here, and every [Export](export.md) format:

| Typed | Rendered |
|---|---|
| `"straight double quotes"` | "curly double quotes" |
| `'straight single quotes'` | 'curly single quotes' |
| `--` | — (em dash) |
| `...` | … (ellipsis) |

Your `.md` file on disk is never touched — the source text you type stays exactly as typed, straight quotes and all. Only the rendered *view* of it (Preview, or a compiled DOCX/EPUB/PDF) changes, so switching the setting off later shows your original punctuation again, nothing was lost. Quote direction (opening vs. closing) is inferred from context, the same simple heuristic most word processors use — it isn't guaranteed correct in every edge case (e.g. deeply nested quotes), but handles ordinary dialogue and contractions correctly.
