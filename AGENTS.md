# AGENTS.md

Rules for anyone (human or agent) changing this repo.

- **The spec is the source.** Traccia project SCRIVI. The planning map is the
  `wayfinder:map` issue there; its closed tickets hold the decisions. Don't
  re-decide what they settle.
- **Words:** use the terms in `GLOSSARY.md` (Tab, Source view, Rendered view,
  Disk change, ...), in code and in prose.
- **Decisions that are hard to reverse** get an ADR in `docs/adr/`.
- **Pure Rust on plain Win32** ([ADR 0001](docs/adr/0001-rust-win32-not-tauri.md)).
  WebView2 appears only while a tab is in the Rendered view.
- **Borrowed code** must be MIT, Apache-2.0, BSD, zlib, ISC or similarly
  permissive, and gets an entry in `THIRD_PARTY_NOTICES.md`. GPL/LGPL/MPL code
  (Notepad++, uchardet, ...) may be read for ideas, never copied.
