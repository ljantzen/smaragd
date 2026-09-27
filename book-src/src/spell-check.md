# Spell Check

Smaragd underlines words it doesn't recognize while you type, in the editor's warning color. Right-click an underlined word for a menu of Hunspell's own correction candidates — click one to replace the word on the spot — plus an **"Add to Dictionary"** entry for names and invented words that keep getting flagged: adding one stops it being underlined immediately, everywhere, and the word is remembered across restarts (**Settings > Spell Check** doesn't currently show or let you remove this list — for now, undoing an accidental add means editing `spell_check_custom_words` in `smaragd.toml` by hand).

It's off by default. Turn it on in **Settings > Spell Check** (`Ctrl+,`) with the **Language** dropdown.

After that, **`Tools > Toggle Spell Check`** (or **`F7`**, or `:spell` in the [command prompt](command-prompt.md)) turns it off and back on without going through Settings. Each project remembers the last language used in it, so toggling spell check back on in a Norwegian manuscript brings back Norwegian even if you were writing English in another project in between. In a project where spell check has never been used, toggling on asks you to pick a language in Settings first.

**In Focus Mode**, next to the language picker, controls spell check while [Focus Mode](focus-mode.md) is active: **Keep current** (the default) leaves it alone, **Off** hides the underlines for a distraction-free draft, and **On** turns it on in the current language, or the project's last language if it's off. `F7` still works inside Focus Mode, and leaving Focus Mode restores what you had before.

Smaragd ships with only tiny placeholder word lists (a few dozen words each), just enough to prove the feature works — not enough to be useful. Below the language picker, **Dictionaries** lists every supported language with a **Download** button: clicking it fetches a real, individually license-reviewed Hunspell dictionary into your own data directory (never bundled into the app itself), verified by SHA-256 against a tracked catalog before being kept. A downloaded dictionary is used immediately, no restart needed. Twenty languages are available at launch: English (American), English (British), German, Dutch, Norwegian Bokmål, Norwegian Nynorsk, French, Spanish, Italian, Portuguese (Brazil), Portuguese (Portugal), Swedish, Danish, Polish, Russian, Georgian, Lithuanian, Persian, Turkmen, and Interlingue.

See **Settings** below for where this fits among the other categories.
