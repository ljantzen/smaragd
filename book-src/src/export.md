# Export

Right-click any folder in the binder and choose **Export…** to compile it — and everything nested inside it, in the same top-to-bottom order shown in the binder — into a single DOCX, EPUB, print-ready PDF, or LaTeX source file. A nested folder whose role is **Trash** or **Templates** is skipped automatically, so deleted or template content never accidentally ends up in a compiled manuscript. If [Typewriter Quotes](markdown-preview.md#typewriter-quotes) is turned on, the exported file gets curly quotes/em dashes/ellipses too, same as the Preview pane.

**`File > Export Manuscript…`** is a shortcut to the same dialog for whichever folder(s) hold the [Manuscript role](folder-roles.md) — see there for how it behaves with zero, one, or several such folders.

The export dialog has:

- **Title** / **Subtitle** / **Author** — plain book metadata, remembered for next time (the same fields shown in [Project Metadata](project-metadata.md) — editing them in either place keeps both in sync). Subtitle is optional; leave it blank if your book doesn't have one.
- **Style** — a dropdown of typesetting styles (see [Typesetting Styles](export-typesetting-styles.md)). Fonts, page size, running headers, and drop caps all come from whichever style is selected, not from anything typed into this dialog. The same dropdown appears in the [Preview](markdown-preview.md) tab; switching it in either place updates the other.
- **Export as DOCX…** / **Export as EPUB…** / **Export as Print PDF…** / **Export as LaTeX…** — each opens a native "Save As" dialog (defaulting to a filename built from Title/Subtitle — `"Title - Subtitle.docx"`, falling back to whichever one is set, or `manuscript.docx` if neither is), then compiles.

Title/Subtitle/Author show up differently per format:

- **DOCX/PDF/LaTeX** get a centered title page before the manuscript — Title, then Subtitle (if set), then Author.
- **EPUB** has no title-page concept (it's reflowable text, not paginated), so Subtitle instead folds into the book's title metadata as `"Title: Subtitle"` — that's what shows up as the book's title in a reader/library view.
- A custom style's **running header** (see [Typesetting Styles](export-typesetting-styles.md)) can reference `{subtitle}` alongside `{title}`/`{author}`/`{chapter}`.

All four formats read from the *same* style, so switching styles changes DOCX, EPUB, PDF, and LaTeX output alike.

## LaTeX

Unlike the other three formats, LaTeX doesn't produce a finished, ready-to-read file — Smaragd has no LaTeX engine built in (PDF export uses an embedded [Typst](https://typst.app) engine instead; there's no equivalent pure-Rust LaTeX implementation to embed the same way). **Export as LaTeX…** instead writes a `.tex` source file, plus an `images/` folder next to it if the manuscript references any images, for you to compile yourself with **XeLaTeX or LuaLaTeX** — not plain `pdflatex`, which can't handle the `fontspec` package the generated source relies on for font selection and Unicode text (curly quotes, em dashes, etc., if [Typewriter Quotes](markdown-preview.md#typewriter-quotes) is on).

A few things the LaTeX output handles differently from the other formats:

- Heading fonts/sizes from the chosen [Typesetting Style](export-typesetting-styles.md) aren't applied — headings use LaTeX's own standard, well-tested sizing for chapters/sections instead. Body text, blockquotes, code blocks, and verse *do* use the style's chosen fonts and sizes.
- Wikilinks render as plain text, not clickable cross-references (same as PDF export — only EPUB resolves them to in-book links).
- A drop cap (if the style has one) uses the `lettrine` LaTeX package, which needs to be available in your TeX distribution (it's part of the common `texlive-latex-extra`-type bundles, not something exotic).
