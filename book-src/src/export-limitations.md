# What Export Doesn't Do (Yet)

- No per-block styling for special block types beyond verse (see [Verse](markdown-preview.md#verse)) — the markdown parser has no other such concept today, only headings/paragraphs/quotes/lists/tables/code/images. Dialogue is deliberately not one of these: it's handled as plain paragraphs with [typewriter-quote curling](markdown-preview.md#typewriter-quotes), not a distinct block type, so this isn't an open gap.
- EPUB output is one general-purpose file, not separately tuned per e-reader (Kindle/Apple Books/Kobo).
- Wikilinks resolve to a real in-book link in EPUB, when the target document is also part of the same export — otherwise (and always, in DOCX) they render as plain text.
