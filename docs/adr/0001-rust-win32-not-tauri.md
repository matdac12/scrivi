# Rust + raw Win32, not Tauri

scrivi is built in Rust on raw Win32, like tasca and musica, not on Tauri/WebView2 like parla and annota, and it is not a fork of MDHero (MIT, Tauri). A Notepad replacement is judged on start-up and idle weight: on this machine MDHero idled at ~224 MB across 7 processes and took 1.4–2.2 s to show a file, against ~4 MB and one process for tasca. We accept that the Rendered view and markdown styling cost more to build natively.

## Considered Options

- **Tauri + CodeMirror 6**: the easiest markdown styling, Rendered view and dark mode, but ~200 MB per window and a WebView process tree resident all day.
- **Fork MDHero**: the least code to write, but it inherits that weight plus Mermaid/KaTeX, and its design is a viewer first with a textarea editor.

## Consequences

- Light theme only in v1. Dark mode on Win32 relies on undocumented APIs, and it isn't needed.
