# Scrivi

A minimal text editor for Windows that replaces Notepad. It opens every
plain-text file, in Tabs in one Window, and gives markdown files light styling
plus a read-only rendered view on <kbd>Ctrl</kbd>+<kbd>E</kbd>.

Lean Rust on plain Win32: a few MB of memory when idle, no browser engine
running behind it (WebView2 exists only while a Tab is in the Rendered view).
Part of [mstack](https://github.com/matdac12/mstack).

<!-- SCREENSHOT: placeholder. Issue 06 or 09 adds the screenshot of the Window here. -->
*(Screenshot to come.)*

## Status

Building v1. This README describes what scrivi does as specified; the editor
itself is still being built.

## Install

### From Releases

1. Download `scrivi-<version>-windows-x64.zip` from the
   [Releases](https://github.com/matdac12/scrivi/releases) page. Check it
   against `SHA256SUMS.txt` if you like.
2. Unzip it and run `scripts\install.ps1` in Windows PowerShell, or do it by
   hand:
   - copy `scrivi.exe` to `%LOCALAPPDATA%\Programs\scrivi\`;
   - run `scrivi.exe --register`.
3. In Settings › Apps › Default apps, pick scrivi once for the file types you
   want. `--register` opens that page for you.

Releases are unsigned, so SmartScreen warns the first time. Close scrivi
before updating; the script refuses to overwrite a running copy.

`install.ps1` takes `-Destination <dir>` to install elsewhere and
`-NoRegister` to copy the exe without touching the registry.

`scrivi.exe --unregister` removes exactly what `--register` wrote. Both only
touch HKCU.

### From source

```
git clone https://github.com/matdac12/scrivi
cd scrivi
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

The script builds with `cargo build --release` (Rust 1.99.1, pinned by
`rust-toolchain.toml`), then installs as above.

## Usage

- `scrivi.exe` adds an Untitled tab. `scrivi.exe <path>` opens the file in a
  Tab, or switches to its Tab if it's already open. There is only ever one
  Window; a second launch hands over to the first.
- Files save explicitly. If a file changes on disk, a Notice offers Keep mine
  or Reload.
- Files over 50 MB open read-only. Markdown files over 1 MB get no styling and
  no Rendered view.

### Shortcuts

| Keys | Action |
|---|---|
| <kbd>Ctrl</kbd>+<kbd>T</kbd> (or <kbd>Ctrl</kbd>+<kbd>N</kbd>) | New tab |
| <kbd>Ctrl</kbd>+<kbd>O</kbd> | Open |
| <kbd>Ctrl</kbd>+<kbd>S</kbd> / <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd> | Save / Save as |
| <kbd>Ctrl</kbd>+<kbd>W</kbd> | Close tab (middle-click a tab also closes it) |
| <kbd>Ctrl</kbd>+<kbd>Tab</kbd> / <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Tab</kbd> | Next / previous tab |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> / <kbd>Ctrl</kbd>+<kbd>H</kbd> | Find / Replace |
| <kbd>Ctrl</kbd>+<kbd>G</kbd> | Go to line |
| <kbd>Ctrl</kbd>+<kbd>E</kbd> | Rendered view (markdown); <kbd>Esc</kbd> returns to the Source view |
| <kbd>Alt</kbd>+<kbd>Z</kbd> | Word wrap |
| <kbd>Ctrl</kbd>+<kbd>+</kbd> / <kbd>Ctrl</kbd>+<kbd>−</kbd> / <kbd>Ctrl</kbd>+<kbd>0</kbd> | Zoom in / out / reset (also <kbd>Ctrl</kbd>+wheel) |
| <kbd>Alt</kbd>, <kbd>F10</kbd>, <kbd>Alt</kbd>+<kbd>F</kbd>/<kbd>E</kbd>/<kbd>V</kbd> | Open the File / Edit / View menus |

### Caps+N hyperkey

With [hyperkey](https://github.com/matdac12/mstack), add this line by hand to
`%USERPROFILE%\Scripts\hyper.ahk`, outside hyperkey's managed region:

```
CapsLock & n::Run("<path>\scrivi.exe")
```

## Known limitations

<!-- Issue 16 fills this in from the registration checks in docs/qa.md. -->
*(To be filled in before the first release.)*

## Releasing

Releases are cut from `main` by pushing a tag; CI does the rest.

1. Bump `version` in `Cargo.toml`.
2. Move the `Unreleased` entries in `CHANGELOG.md` under the new version, with
   the date.
3. Commit, then tag and push:

   ```
   git tag v0.1.0
   git push origin v0.1.0
   ```

The tag must be `v` plus the exact `Cargo.toml` version.
`.github/workflows/release.yml` runs the same checks as CI, fails if the two
disagree, and publishes a GitHub Release with generated notes, `scrivi.exe`,
`scrivi-<version>-windows-x64.zip` and `SHA256SUMS.txt`. Releases are unsigned.

## Development

`AGENTS.md` has the rules and the three checks CI runs. Words follow
`GLOSSARY.md`; decisions are in `docs/adr/`.

## License

MIT — see [LICENSE](LICENSE). Third-party code is listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
