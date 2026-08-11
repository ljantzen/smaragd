# What Export Doesn't Do (Yet)

- No per-block styling for verse, dialogue, or other special block types — the markdown parser has no such concept today, only headings/paragraphs/quotes/lists/tables/code/images.
- EPUB output is one general-purpose file, not separately tuned per e-reader (Kindle/Apple Books/Kobo).
- Wikilinks resolve to a real in-book link in EPUB, when the target document is also part of the same export — otherwise (and always, in DOCX) they render as plain text.
