# Changelog

All notable changes to scrivi are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
