# Project Metadata

Alongside per-document frontmatter, a project itself carries a handful of project-wide fields — a Title/Subtitle/Author, a one-line **Point**, a **Logline**, a **What if** premise question, and a longer **Synopsis** — for the book as a whole rather than any one document.

Rather than opening yet another dock tab, these reuse the same Metadata dock [Document Metadata](document-metadata.md) already uses: click the root row at the very top of the [Binder](binder.md) (the project itself) and the Metadata dock switches over to this form instead of a document's frontmatter. Clicking any document row switches it back.

- **Title** and **Author** are the same fields the [Export](export.md) dialog's Title/Author use — editing them here or there keeps both in sync.
- **Subtitle** feeds into export the same way — see [Export](export.md) for exactly where it shows up in each output format.
- **Point**, **Logline**, **What if**, and **Synopsis** are new fields with no other home yet; they don't currently appear anywhere in an exported DOCX/EPUB/PDF.

Like Document Metadata, there's no Save/Cancel step — edits apply as you type. **Point** is a single-line field, same as Title/Subtitle/Author; Logline/What if/Synopsis evenly split whatever vertical space is left in the tab below it. Synopsis (the field most likely to run long) keeps its scrollbar always visible, while Logline/What if only show theirs once there's actually more text than fits.
