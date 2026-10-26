# Changelog

All notable changes to scrivi are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - Unreleased

First release (milestone v1). Unsigned; SmartScreen warns once.

### Added

- A minimal text editor for Windows that opens every plain-text file in Tabs in
  one Window.
- Source view on Scintilla with line numbers, word wrap and zoom; light
  markdown styling in markdown files and Untitled tabs.
- Rendered view on Ctrl+E, shown with WebView2 only while a tab is in it.
- Explicit saving, Disk change and deleted-on-disk Notices, Large file handling
  (read-only over 50 MB).
- Find and replace, go to line, encoding and line-ending control, spell check
  in markdown (Italian and English).
- Recent files and the taskbar jump list.
- `--register` / `--unregister` (HKCU only) and `scripts/install.ps1`.
- Release pipeline: a `vX.Y.Z` tag publishes `scrivi.exe`, a zip and
  `SHA256SUMS.txt`.
