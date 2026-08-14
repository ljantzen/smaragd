# Story Cards (Corkboard)

**`View > Corkboard`** opens a wrapping grid of scene cards. A card isn't just a Lisa Cron *Story Genius*-style cause-and-effect breakdown — it also tracks the psychological change a scene represents: a character's belief going in, and what it becomes coming out.

At the top of the Corkboard, two project-wide fields capture what Cron calls the "Third Rail" — the protagonist's driving force, not tied to any one scene:

- **Desire** — the external/internal want the protagonist is pursuing
- **Misbelief** — the flawed, usually childhood-formed belief standing in its way

Every scene card below is meant to test or advance this pair. Each card has a header, always visible, and three tabs underneath it for everything else.

The header:

- **Scene #** — a free-text label, independent of manuscript order
- **Alpha Point** — the scene's core moment
- **Subplots** — optional, comma-separated tags
- **POV Character** — becomes a dropdown once you've designated a Dropdown Source folder for POV (see [Dropdown Source Folders](document-metadata.md#dropdown-source-folders)), otherwise free text
- **Linked documents** — comma-separated, with autocomplete as you type. A card can link to more than one manuscript document (spanning several scenes), and more than one card can link to the same document. Only documents under a Manuscript-role folder are suggested (see [Folder Roles](folder-roles.md)) — falling back to every non-Trash/Templates document if the project has no Manuscript folder designated yet. Picking a suggestion appends a comma automatically, so it's clear you can keep typing to add another

The three tabs:

- **Plot** — **Cause** (the external event that occurs) and **Effect** (its external and internal consequence)
- **Belief and Knowledge** — **Prior Belief** (what the POV Character believes going into this card), **New Belief** (what they believe as a result of it), **Value Shift** (a short label for the value at stake, e.g. "Trust -> Distrust"), and **Knowledge Gained** (comma-separated facts the character learns)
- **Third Rail** — **Why It Matters** (the scene's link back to the protagonist's Desire/Misbelief — why these events matter to them personally), **Realization** (what the protagonist comes to understand), and **And So?** (what they do next, as a result of that realization)

Cards are independent of the binder tree: you can reorder them freely, create a card with no linked document yet (pure plotting, before you've drafted the scene), or link a card to a document that later gets renamed or deleted — the link just resolves to "not found" rather than breaking anything, the same way a dangling `[[wikilink]]` behaves.

## Story Grid

**`View > Story Grid`** opens a second view of the same cards as a table — one row per card. An **Order** dropdown at the top picks what that row order means: **Manuscript** (the default) sorts by whatever order each card's earliest linked document sits in the binder today, read-only, same as before; **Manual** instead shows — and lets you reorder — the same freeform order you set on the Corkboard, with ⬆/⬇ buttons in the `#` column moving a row exactly the way Corkboard's own Up/Down buttons do (it's the same underlying order, just editable from either view). The chosen mode is remembered per project, like the rest of the Story Grid's ordering.

Each row shows a computed manuscript position (`#`), the card's own `Scene #` label (unchanged, shown alongside rather than replaced), every one of its linked documents' titles, POV, and a word count summed across all of them (read live from disk, the same way the Metadata and Word Count panels do), and every field from the card — Cause, Effect, Why It Matters, Realization, And So, Prior Belief, New Belief, Value Shift, and subplot tags. The POV column prefers the card's own POV Character when it's set, falling back to the linked document's frontmatter POV otherwise.

The columns shown, and their order, are configurable via the panel's own **Columns** menu — reorder or hide any of them to fit what you're working on.

In **Manuscript** order, cards with no linked document, or where every link is stale, group into an **Unplaced** section — a toggle at the top of the panel puts that section above or below the placed rows. Unlike everything else on this page, that toggle is an app-wide preference, not a per-project one: it's remembered across every project you open, the same way UI Scale or your theme choice is. It has no effect in **Manual** order, since every card shows in its own place in the freeform order rather than splitting out. Clicking a linked document's title opens it in the Editor, same as Corkboard's own 🔗 link; clicking a row's Scene # opens the card editor.

Switching back to **Manuscript** always reproduces the same order, regardless of anything reordered while in **Manual**: two cards sharing a manuscript position, or two Unplaced cards with no position at all, fall back to a fixed tie-break rather than whatever order Manual reordering happened to leave them in.

The **POV** and **Words** columns are colored the same way the [Binder](binder.md#binder-background-coloring) colors its own rows: a colored dot next to the POV name whenever that POV has an assigned color, and the word count itself tinted along the same red→yellow→green gradient toward the (first resolved) document's word count target. Unlike the Binder, this coloring isn't mode-switched — it's always on, independent of whatever `Color Binder By` mode is currently active.

The Story Grid never reorders the manuscript itself, in either order mode — its `#` column always shows a card's computed manuscript position where one resolves, and its Up/Down buttons (in Manual order) only move the card within the freeform Corkboard order, not the documents themselves. To reorder scenes in the manuscript, reorder the documents in the Binder.

## Belief Timeline

**`View > Belief Timeline`** (`Ctrl+Shift+E`) shows one character's arc across the whole manuscript: pick a POV Character from the dropdown (populated from whatever names story cards have set in their own POV Character field — not the Metadata panel's POV dropdown source, since a card can describe a belief shift before any scene exists for it) and see their cards, in manuscript order, chained as Prior Belief → New Belief. A repeated belief that just restates the previous card's New Belief is skipped, so the chain reads as one continuous arc rather than restating itself. Cards with no resolvable linked document trail at the end. Clicking a card's linked scene opens it in the Editor, same as Story Grid.

If no story card has a POV Character set yet, the panel just says so — set one from the Corkboard card editor's header to start populating this view.
