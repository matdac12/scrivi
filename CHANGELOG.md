# Changelog

All notable changes to scrivi are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `scrivi.exe --help` (and `-h`) prints the flags; `--` ends them, so a file
  named like a flag still opens. An unknown `--flag` prints the usage and
  exits 2.

### Changed

- A new app icon: a quill on an ivory tile, with a simpler drawing at 16 and
  20 px. `scripts/make-icon.ps1` rebuilds it from `assets/scrivi.svg`.
- Opening a file when the only Tab is an empty, untouched Untitled tab
  replaces that Tab, as Windows 11 Notepad does.
- Plain-ASCII files load faster: the 200 MB log takes about 0.35 s.
- CI also runs `cargo fmt --check` and the install script's tests.

### Fixed

- An elevated launch with UAC off no longer relaunches through Explorer in a
  loop; "Run as administrator" still ends up as a normal, unelevated Window.
- A launch retries the hand-over once if the running Window is busy.
- The install script refuses to overwrite a running scrivi even when Windows
  hides that process's path, instead of failing halfway.
- `--unregister` removes every value it can and reports all errors together;
  `--register` reports when it can't open Default apps.

## [0.1.0] - 2026-10-26

First release (milestone v1). Unsigned; SmartScreen warns once.

### Added

- A minimal text editor for Windows that opens every plain-text file in Tabs in
  one Window; a second launch hands over to the running one, also from an
  elevated console.
- One-row title bar with File / Edit / View menus (Alt, F10 and access keys),
  Tabs you can add, close, switch and scroll, and a status bar with line,
  column, line ending and encoding. DPI-aware across monitors.
- Source view on Scintilla with line numbers, word wrap (Alt+Z) and zoom
  (Ctrl+wheel); light markdown styling in markdown files and Untitled tabs.
- Rendered view on Ctrl+E for markdown, with relative images and safe links,
  shown with WebView2 only while a Tab is in it; Esc returns to the Source view.
- Open and save that never lose the file (atomic saves); UTF-8, UTF-8 with
  BOM, UTF-16 LE/BE and Windows-1252; LF, CRLF and CR; reopen with another
  encoding; drag and drop.
- Find, Replace and Go to line (Ctrl+F, Ctrl+H, Ctrl+G).
- Large files over 50 MB open read-only, with a Notice.
- Spell check in markdown and Untitled tabs, Italian and English, with a
  right-click menu and a personal dictionary.
- Remembered window, wrap and zoom; recent files in File and on the taskbar
  jump list.
- Disk change and deleted-on-disk Notices (Keep mine, Reload, Save).
- `--register` / `--unregister` (HKCU only, you pick scrivi in Default apps)
  and `scripts/install.ps1`.
- Release pipeline: a `vX.Y.Z` tag publishes `scrivi.exe`, a zip and
  `SHA256SUMS.txt`.

### Known limitations

- Opening a 200 MB log takes about 1 s (target about 337 ms); the exe is
  2.57 MB (goal 1–2 MB); a live log over 50 MB reloads after each change.
  See the README.
