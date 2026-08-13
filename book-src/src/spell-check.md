# Spell Check

Still early groundwork: smaragd underlines words it doesn't recognize while you type, in the editor's warning color — there's no right-click suggestions or "add to dictionary" yet, so expect false positives on names and invented words until a later update.

It's off by default. Turn it on in **Settings > Spell Check** (`Ctrl+,`) with the **Language** dropdown.

Smaragd ships with only tiny placeholder word lists (a few dozen words each), just enough to prove the feature works — not enough to be useful. Below the language picker, **Dictionaries** lists every supported language with a **Download** button: clicking it fetches a real, individually license-reviewed Hunspell dictionary into your own data directory (never bundled into the app itself), verified by SHA-256 against a tracked catalog before being kept. A downloaded dictionary is used immediately, no restart needed. Twenty languages are available at launch: English (American), English (British), German, Dutch, Norwegian Bokmål, Norwegian Nynorsk, French, Spanish, Italian, Portuguese (Brazil), Portuguese (Portugal), Swedish, Danish, Polish, Russian, Georgian, Lithuanian, Persian, Turkmen, and Interlingue.

See **Settings** below for where this fits among the other categories.
