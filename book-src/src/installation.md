# Installation

Prebuilt binaries for Linux, Windows, and macOS are on the [Releases page](https://github.com/ljantzen/smaragd/releases/latest). They're built by CI, not signed with a paid code-signing certificate, so Windows and macOS both show a first-run warning — this is expected, not a sign of a broken or tampered download.

## Windows

An unsigned `.exe` downloaded from a browser gets flagged by SmartScreen with a "Windows protected your PC" dialog. It's a soft block:

1. Click **More info**.
2. Click **Run anyway**.

## macOS

macOS tags anything downloaded via a browser with a quarantine attribute (`com.apple.quarantine`). Launching it then shows "cannot be opened because the developer cannot be verified" (or, on newer macOS, "is damaged and can't be opened"). This is also a soft block, not a hard one:

- **Right-click the app → Open → Open Anyway** — works on most versions, though Apple has tightened this on newer macOS: sometimes it takes a second step, going to **System Settings → Privacy & Security** and clicking **Open Anyway** there after the first failed attempt.
- Or, more reliably, clear the quarantine attribute directly from Terminal:

  ```bash
  xattr -cr /path/to/Smaragd.app
  ```

  This strips the quarantine flag and sidesteps Gatekeeper's warning entirely.

## Linux

Four package formats are published on the [Releases page](https://github.com/ljantzen/smaragd/releases/latest):

- **AppImage** — portable, no installation. Needs its executable bit set before it will run:

  ```bash
  chmod +x smaragd-*-x86_64.AppImage
  ```

- **.deb** — for Debian/Ubuntu and derivatives:

  ```bash
  sudo apt install ./smaragd-*-x86_64-linux.deb
  ```

- **.rpm** — for Fedora/openSUSE and derivatives:

  ```bash
  sudo dnf install ./smaragd-*-x86_64-linux.rpm
  ```

- **.flatpak** — sandboxed, works on any distro with Flatpak set up:

  ```bash
  flatpak install --user ./smaragd-*-x86_64.flatpak
  ```
