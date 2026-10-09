# What Export Doesn't Do (Yet)

- No per-block styling for special block types beyond verse (see [Verse](markdown-preview.md#verse)) — the markdown parser has no other such concept today, only headings/paragraphs/quotes/lists/tables/code/images. Dialogue is deliberately not one of these: it's handled as plain paragraphs with [typewriter-quote curling](markdown-preview.md#typewriter-quotes), not a distinct block type, so this isn't an open gap.
- No footnotes, in any export format — the markdown parser has no footnote concept at all, so a numeric reference stays plain text everywhere rather than becoming a real footnote.
- EPUB output is one general-purpose file, not separately tuned per e-reader (Kindle/Apple Books/Kobo).
- Wikilinks resolve to a real in-book link in EPUB, when the target document is also part of the same export — otherwise (and always, in DOCX and LaTeX) they render as plain text.
- LaTeX export writes `.tex` source (plus an `images/` folder) for you to compile yourself with XeLaTeX or LuaLaTeX — see [Export](export.md#latex) — rather than a finished file like the other three formats. It also doesn't map a style's heading font/size (LaTeX's own standard sizing is used instead) or a custom style's `font_file` (only the plain font name is written, so a font not separately installed as a system font won't be found when compiling).
