# AGENTS.md

Rules for anyone (human or agent) changing this repo.

- **The spec is the source.** Traccia project SCRIVI, document
  `scrivi-v1-spec.md`. The build is milestone **v1** (issues numbered 01–18,
  blocking set; its description holds the working rules). Decisions live in the
  closed SCRIVI-2..7 tickets of the planning map SCRIVI-1. Don't re-decide what
  they settle.
- **Words:** use the terms in `GLOSSARY.md` (Tab, Source view, Rendered view,
  Disk change, ...), in code and in prose.
- **Decisions that are hard to reverse** get an ADR in `docs/adr/`.
- **Pure Rust on plain Win32** ([ADR 0001](docs/adr/0001-rust-win32-not-tauri.md)).
  WebView2 appears only while a tab is in the Rendered view.
- **Borrowed code** must be MIT, Apache-2.0, BSD, zlib, ISC or similarly
  permissive, and gets an entry in `THIRD_PARTY_NOTICES.md`. GPL/LGPL/MPL code
  (Notepad++, uchardet, ...) may be read for ideas, never copied.
- **Reference branches** (never merged): `research/borrowable-code`,
  `research/webview2-on-demand`, `prototype/scintilla-source-view`,
  `prototype/window-look`.

- **Releases:** push a `vX.Y.Z` tag equal to the `Cargo.toml` version, with
  `CHANGELOG.md` updated in the same commit; `release.yml` builds and
  publishes. Steps in the README's *Releasing* section. Releases are unsigned.
  Never move or re-push a published tag; cut a new patch version instead.

## Checks (CI runs the same three on `windows-latest`)

```
cargo build --release
cargo clippy --all-targets -- -D warnings
cargo test
```
