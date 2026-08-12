# Dictionaries

The files under each language subdirectory here (`en_US/`, `nb_NO/`, `ka_GE/`,
`lt_LT/`, `ru_RU/`, `fa_IR/`, `ie/`, `tk_TM/`, `fr_FR/`) are **independent,
separately licensed works** — third-party Hunspell dictionaries, redistributed
here on their own terms, not part of smaragd's own GPL-3.0-or-later source.
Nothing about being checked into this repository, or being downloaded
alongside smaragd's own binary, changes what license governs them or brings
them under smaragd's license: they're data, kept as genuinely separate files,
never compiled or linked into the application itself, only ever read at
runtime by the general-purpose `spellbook` Hunspell engine (`src/spellcheck.rs`).
This is the "mere aggregation" case GPLv2 itself describes — placing an
independent work alongside a GPL one on the same distribution medium doesn't
bring the independent work under the GPL, or vice versa.

Each language directory contains:

- the dictionary itself (`<code>.aff`/`<code>.dic`, Hunspell's affix/wordlist
  pair)
- `LICENSE` — that dictionary's own license, copied verbatim from its
  upstream source (not smaragd's `LICENSE` at the repo root, which is
  GPL-3.0-or-later and applies only to smaragd's own code)
- `SOURCE` — exactly where this file came from (upstream repository, path,
  and a pinned commit — never a moving branch), its license and copyright
  holder, confirmation of no modifications, and the date it was last reviewed

`catalog.json` in this directory is the machine-readable index of all of the
above (used by `src/spellcheck.rs`/the Settings > Editor > Dictionaries list)
— `SOURCE`/`LICENSE` are what to read for the human-readable version of the
same facts.

`placeholders/` holds the tiny (~20-word), hand-written, no-licensing-question
word lists `src/spellcheck.rs` falls back to before any real dictionary has
been downloaded/selected — see `NOTICE` for why those exist at all.

Redistribution here does not imply endorsement of, or by, any upstream
project or author — it's a straightforward "here is a useful independent
tool, properly attributed, on its own terms."
