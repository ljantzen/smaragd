# Dictionaries

`catalog.json` here is the machine-readable index of every Hunspell
dictionary smaragd can download on request (Settings > Spell Check >
Dictionaries, `src/spellcheck.rs`) — language, license, upstream source and
pinned commit, and expected SHA-256 per file. It's `include_str!`-compiled
into the smaragd binary itself.

The dictionary files it indexes — the actual third-party, independently
licensed `.aff`/`.dic` word lists, each with its own `LICENSE`/`SOURCE` — are
**not** in this repository. They live in their own repo,
[smaragd-dictionaries](https://github.com/ljantzen/smaragd-dictionaries), so
smaragd's own git history doesn't have to carry several dozen megabytes of
data it never compiles or links against. `download_dictionary`
(`src/spellcheck.rs`) fetches them from there at runtime and verifies each
one's SHA-256 against this catalog before keeping it, so a tampered mirror
or a stale catalog entry can't silently swap in different content. See that
repo's own README.md/NOTICE for the full "why is a GPL-2.0-only dictionary
fine to redistribute here" story (GPLv2's own "mere aggregation" allowance —
these stay independent, unmodified, un-linked data files, never compiled or
linked into smaragd).

`placeholders/` holds the tiny (~20-word), hand-written, no-licensing-question
word lists `src/spellcheck.rs` falls back to before any real dictionary has
been downloaded/selected — see `NOTICE` for why those exist at all. Unlike
the real dictionaries, these *are* `include_bytes!`-bundled directly into
the smaragd binary, so they stay in this repo.
