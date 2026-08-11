# Pomodoro Timer

**`Tools > Pomodoro Timer`** (or the remappable shortcut, `Ctrl+Alt+T` by default) opens a Pomodoro dock tab — the classic interval-timer technique for focused writing sessions, alternating fixed blocks of work and rest.

- **Start** / **Pause** / **Skip** / **Reset** control the current phase. Skip jumps to the next phase immediately, whatever's left on the clock; Reset returns to a fresh Work phase (it keeps your completed-session count for the day, it just stops the clock and rewinds the current phase).
- When a phase's time runs out, smaragd automatically switches to the next one — Work leads to a Short Break, except every *n*th Work session (configurable, default every 4th) leads to a Long Break instead — **and pauses**, rather than continuing to run unattended. Starting the next phase is always a deliberate action, not something that happens silently while you're away from the keyboard.
- **"Show a desktop notification when a phase completes"** ([Settings](settings.md) > Pomodoro, off by default) fires an OS-level desktop notification the moment a phase ends on its own — useful if the app isn't in focus. It only fires on an automatic completion, never on a manual Skip, since you already know about that one. There's still no audible chime.

The timer keeps running whether or not its dock tab is open or visible (it's part of the app's state, not something tied to a window being shown), and a compact countdown — `⏱ Work 12:34` — shows in the status bar at the bottom of the window any time a session has been started, so it's visible at a glance without needing to switch tabs. It doesn't appear during [Focus Mode](focus-mode.md), which hides the whole status bar; the dock tab itself is unaffected and still works there.

Durations default to the traditional 25 minutes of work, a 5-minute short break, and a 15-minute long break every 4 sessions — all four are adjustable in [Settings](settings.md).
