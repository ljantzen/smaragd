# Dashboard

**`Tools > Dashboard`** (or `Ctrl+Alt+G` by default) opens a Dashboard dock tab showing your writing statistics as they evolve over time, as both numbers and graphs — per project, like [Writing Streak](writing-streak.md).

At the top, a summary: total sessions, total writing time, total words written across those sessions, and how many documents you've created in the project so far.

Below that, four bar charts covering the last 60 days:

- **Word count** — the project's tracked total each day (the same history [Writing Streak](writing-streak.md) is judged against).
- **Sessions** — how many times you opened the project each day.
- **Documents created** and **Documents modified** — the latter read live from each file's own last-modified time, so it also reflects changes made outside smaragd (an external editor, a `git pull`), not just edits made through the app.

A **session** starts when you open the project and ends when you close it, switch to a different project, or the app quits — it's how many times you sat down to work on this project, and for how long, not a daily reset like the Word Count panel's Session Target.

Two more charts show *when* you tend to write, aggregated across every session on record: **Activity by day of week** and **Activity by hour of day**. A toggle switches both between **Words** (words written) and **Time** (minutes spent) as the measured quantity — a session spanning midnight counts entirely toward the day and hour it *started* in, not split across two.
