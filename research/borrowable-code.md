# scrivi: borrowable code and stack research

Research date 2026-10-26. Research only: no app code, no commits. Stacks: **(N)** Rust + raw Win32 (`windows` 0.62, as in `tasca/` and `musica/`), **(W)** Tauri 2 / WebView2 (as in `parla/` and `annota/`). Terms follow [GLOSSARY.md](GLOSSARY.md).

Every licence below was read from the repo's own licence file (URL given per entry), and last-activity dates come from `gh api`, crates.io or npm. The detail sections are §1–§4. This page is the summary.

**Contents**
- [Summary: the facts that decide the stack](#the-facts-that-decide-the-stack)
- [Recommendation per stack](#recommendation-per-stack)
- [Licence quick reference](#licence-quick-reference)
- [Still unverified](#still-unverified)
- §1 Editing component · §2 Markdown, encoding, line endings, large files · §3 Windows integration · §4 Weight (measured)

## The facts that decide the stack

1. **Idle memory differs by ~100×.** Measured on this machine (§4): tasca idles at 4.1 MB private and musica at 1.9 MB, with no child processes. MDHero (Tauri 2, a visible window) idles at **224 MB private across 7 processes**. Windows Notepad idles at 87 MB. Microsoft itself says each WebView2 "launches multiple browser engine processes that add memory and startup overhead" ([MS Learn](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)).
2. **Time to content.** MDHero shows its empty frame in ~67 ms but **the file only after 1.4–2.2 s**. Notepad shows the file after ~1.5 s. Tauri's own CI hello-world needs 0.59 s to DOMContentLoaded ([benchmark JSON](https://github.com/tauri-apps/benchmark_results/blob/main/tauri-recent-windows.json)). Microsoft: "Don't use WebView2 for initial UI" ([MS Learn](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)). Explorer also starts **one process per selected file, up to 15** ([verb selection model](https://learn.microsoft.com/en-us/windows/win32/shell/how-to-employ-the-verb-selection-model)), so every second launch has to be cheap.
3. **"Headings bigger" is the hardest requirement for (N).** Scintilla, the best native engine for big plain text, draws every line at one height ("Currently all lines are the same height", [ScintillaDoc](https://www.scintilla.org/ScintillaDoc.html)), so its markdown headings can only be bold or coloured. RichEdit in rich mode can draw bigger headings via TOM; a self-built DirectWrite editor can too, at a large cost (caret, IME, undo, accessibility). In (W), CodeMirror 6 + `@lezer/markdown` (MIT) does it out of the box, with visible symbols, variable line heights and incremental parsing.
4. **The rendered view is free in (W) and a build job in (N).** There is no reusable Win32 markdown renderer. The (N) options are our own DirectWrite renderer (patterns from egui_commonmark, iced, inlyne), RTF streamed into a read-only RichEdit (no markdown→RTF crate exists), or a hybrid that creates WebView2 only when the user flips to the rendered view.
5. **Large files favour (N).** Scintilla plus Notepad4's large-file mode (`STYLES_NONE`, `TEXT_LARGE`, no undo while loading) is proven well past 50 MB. CM6 claims "a few million lines" but no hard limit, and pushing 50 MB through Tauri IPC into WebView2 is unmeasured. Monaco switches its own features off at 20 MB/50 MB, so it is out. RichEdit on the *system* msftedit.dll at 50 MB is unproven: Notepad's large-file speedups are in the Office RichEdit build, "not the Windows RichEdit in msftedit.dll" ([Murray Sargent](https://devblogs.microsoft.com/math-in-office/windows-11-notepad/)).
6. **Windows integration is Rust/Win32 code in both stacks.** File associations, jump lists, file watching and spell check are written in Rust either way. Tauri has no jump-list API ([#9778](https://github.com/tauri-apps/tauri/issues/9778)), and its installer `fileAssociations` is **harmful for `.txt`**: it overwrites the extension's default and writes no OpenWithProgids or Capabilities. **No app can make itself the default** (UserChoice hash, UCPD.sys, `UserChoiceLatest`); the supported route is to register in HKCU, then open `ms-settings:defaultapps?registeredAppUser=Scrivi` ([MS Learn](https://learn.microsoft.com/en-us/windows/apps/develop/windows-integration/default-apps-platform)). The only clear (W) win here is **dark mode** (tao/wry/muda do it). In (N) dark mode is ~200 lines ported from tao (Apache-2.0) and adzm (MIT), resting on undocumented uxtheme ordinals.
7. **Exe size is small either way; the frontend makes it big.** (N): ~0.5 MB, plus 0 B for RichEdit or ~1.1 MB (Scintilla.dll) to ~2.5 MB (with all of Lexilla). (W): a 2.8 MB floor (Tauri CI) and 3.4 MB for annota. MDHero's 16 MB comes from bundling Mermaid, KaTeX and highlight.js with no size-tuned release profile. MDHero's editor is a plain `<textarea>` with a highlight backdrop, so it cannot draw bigger headings either.

**Bottom line:** if "lightest possible" and "replace Notepad" lead, (N) wins on every measured number, at the cost of building the markdown rendered view and (with Scintilla) giving up bigger headings. If the markdown experience leads, (W) gets it almost for free, at ~200 MB and ~1–2 s per window. A **hybrid (N) that loads WebView2 only for the rendered view** keeps plain-text files at Win32 weight and pays the WebView cost only on demand.

## Recommendation per stack

### (N) Rust + raw Win32
- **Source view:** **Scintilla**, statically linked through `cc` (HPND-style permissive licence), driven with `SendMessageW`. Turn on DirectWrite technology, use Notepad4's large-file recipe, and add markdown styling from **pulldown-cmark** `into_offset_iter` (MIT): bold, italic, monospace code, dimmed markers, headings bold and coloured but *not* bigger. If bigger headings are non-negotiable, prototype **RichEdit (rich mode + TOM)** for markdown files only, and first time a 50 MB load on the system msftedit.dll.
- **Rendered view:** start with **RTF into a read-only RichEdit** (cheapest; 0 B; selection and copy for free). Move to an own DirectWrite renderer (tasca already has D2D/DWrite) if the "Word-ish" look is not acceptable. The other route is the lazy WebView2 hybrid via `webview2-com` 0.39 (MIT, matches `windows` 0.62) with comrak or pulldown-cmark + ammonia, scripts off, `NavigateToString` (2 MB cap).
- **Encoding / EOL / large files:** `encoding_rs` + `chardetng` + `codepage` (all permissive). Detection order: BOM → NUL test for BOM-less UTF-16 → strict UTF-8 → chardetng → ANSI code page. Write UTF-16 ourselves and save with `encode_from_utf8_without_replacement`. Port EOL detection and large-file logic from **Notepad4** (BSD-3). Above 50 MB use a read-only viewer with chunked `ReadFile` or `memmap2`.
- **Integration:** `notify` + `notify-debouncer-full` with a re-stat on each event; jump lists, `ISpellChecker` and HKCU registration all through `windows` 0.62; dark mode from **tao `dark_mode.rs`** (Apache-2.0) + **adzm** menu bar (MIT) + **win32-darkmode** ordinal table (MIT); single instance from tasca/musica's mutex pattern plus annota's `WM_COPYDATA` payload; drag & drop from `tasca/src/park/`.
- **Expected weight:** under 3 MB exe, single-digit-to-low-tens MB idle (extrapolated from tasca/musica; scrivi will hold file text and Scintilla's state on top).

### (W) Tauri 2 / WebView2
- **Source view:** **CodeMirror 6** (`@codemirror/view`, `state`, `commands`, `search`, `lang-markdown` with `@lezer/markdown` GFM), all MIT. The repos moved from GitHub (archived) to code.haverbeke.berlin in 2026, and npm releases continue. Not Monaco (~101 MB unpacked, self-limits at 20/50 MB). Reference apps: **SoloMD** (MIT) and **Paperling** (Apache-2.0), both Tauri + CM6.
- **Rendered view:** **markdown-it** (MIT, ~44 KB gzipped) + **DOMPurify** taken under its Apache-2.0 option, or **comrak** (BSD-2) in a Rust command. Render into a sandboxed iframe (no `allow-scripts`) or a second webview with scripts off, never into the page that holds Tauri IPC, and set a CSP. MDHero's `src/lib/renderer/pipeline.ts` (MIT) is a reading-view reference.
- **Rust side:** all file I/O (encoding_rs/chardetng, EOL, large-file viewer), `notify`, HKCU registration (not `bundle.fileAssociations`), jump lists and spell check (if language control is needed). Single instance with annota's `FindWindowW` fast path so a second launch does not boot the Tauri runtime.
- **Weight discipline:** Tauri's size profile (lto, codegen-units=1, opt-level "s", panic=abort, strip). No Mermaid or KaTeX. `MemoryUsageTargetLevel` Low when minimised (as annota does). Expect ~200 MB idle per window and 0.6–2 s to content.

## Licence quick reference

| Candidate | Licence (verified) | Use |
|---|---|---|
| Scintilla / Lexilla | HPND-style notice (no SPDX id) | link or vendor |
| scintilla-sys | MIT OR Apache-2.0 (its LICENSE-MIT carries an "imgui-rs" copyright line) | optional bindings |
| Notepad4, Notepad2, Notepad2-mod | BSD-3-Clause | port functions with attribution |
| Notepad3 | BSD-3-Clause, but bundles **uchardet (MPL-1.1/GPL/LGPL)** and **grepWin (GPL-3.0)** | its own code only |
| Notepads | MIT, but bundles **UTF-unknown (MPL/GPL/LGPL)** | patterns only (C#) |
| ropey, crop | MIT | dependency (only if self-built) |
| CodeMirror 6, @lezer/markdown, ProseMirror | MIT | dependency (W) |
| Monaco, VS Code | MIT | VS Code's `code.iss` registry pattern |
| Ace | BSD-3-Clause | not needed |
| pulldown-cmark, markdown-it, micromark, markdown-rs, md4c | MIT | dependency |
| comrak | BSD-2-Clause | dependency |
| encoding_rs | (Apache-2.0 OR MIT) AND BSD-3-Clause (WHATWG data) | dependency |
| chardetng, codepage, ammonia, memmap2, cosmic-text | Apache-2.0 OR MIT / MIT OR Apache-2.0 | dependency |
| charset-normalizer-rs | MIT | pass (dependency weight) |
| DOMPurify | MPL-2.0 OR Apache-2.0 | **take as Apache-2.0** |
| notify | **CC0-1.0** (no patent grant); debouncers MIT OR Apache-2.0 | dependency |
| tao | Apache-2.0 | port `dark_mode.rs`, keep NOTICE |
| muda | Apache-2.0 OR MIT, but its dark menu bar is **ported from PPSSPP (GPL-2.0+)** | cross-check only; use adzm |
| win32-darkmode, win32-custom-menubar-aero-theme | MIT | port |
| window-vibrancy, webview2-com | Apache-2.0 OR MIT / MIT | dependency |
| egui_commonmark / iced / inlyne | MIT OR Apache-2.0 / MIT / MIT | patterns only |
| MDHero, SoloMD, markamd, MarkText | MIT | patterns only |
| Paperling | Apache-2.0 | patterns only |
| **Not allowed (ideas only)** | uchardet, UTF-unknown (MPL-1.1 tri); win32-darkmodelib / darkmodelib (MPL-2.0); Zettlr (GPL-3.0); MarkFlowy (AGPL-3.0); Notepad++ (GPL); PPSSPP (GPL) | read only |
| **Do not ship** | SetUserFTA (proprietary); PS-SFTA (MIT claim in README, **no LICENSE file**); FTA-Manager (MIT, but it bypasses UCPD) | none |

**Likely `THIRD_PARTY_NOTICES.md` entries** if code is ported (not just linked): Notepad4/Notepad2 (BSD-3; encoding, EOL, file-watch, large-file functions), tao (Apache-2.0; dark mode), adzm (MIT; menu bar), win32-darkmode (MIT; ordinal table), VS Code (MIT; registry layout, if copied closely), Scintilla (if vendored).

## Still unverified

These need a prototype or a VM before committing:
- RichEdit (system msftedit.dll) loading and editing a 50 MB file; whether TOM temporary formatting can change font size.
- A 50 MB document through Tauri IPC into CM6: load time and memory.
- Cold start and idle memory of a bare `windows`-crate editor window (no build was made).
- Scintilla size statically linked with only the markdown lexer.
- Settings > Default apps: whether files with no extension (`.`) can be chosen; per-user override of the Explorer "Edit" verb; whether "New > Text Document" survives without Store Notepad; behaviour once `UserChoiceLatest` reaches this machine.
- How Windows 11 Notepad detects encodings and treats mixed line endings (undocumented).


---

## 1. Editing component

Research date 2026-10-26. Licences were read from the actual licence file at the URL given. Last-activity dates come from `gh api` (`pushed_at`, latest release) or npm publish times. Throughout, "symbols visible" means scrivi's model: the markdown markers stay on screen and only get light styling.

**The constraint that decides most of this:** scrivi wants headings *bigger*. Scintilla cannot do that without making every line taller ("Currently all lines are the same height", [ScintillaDoc SCI_TEXTHEIGHT](https://www.scintilla.org/ScintillaDoc.html)). RichEdit (rich mode), CodeMirror 6, Monaco (via decorations) and a self-built DirectWrite view can all do it.

### Native (Rust + Win32)

#### Scintilla (+ Lexilla)
- **Repo:** https://sourceforge.net/p/scintilla/code/ (Mercurial). Lexilla is mirrored at https://github.com/ScintillaOrg/lexilla
- **Licence:** HPND-style permissive notice (GitHub reports NOASSERTION), verified from https://www.scintilla.org/License.txt and https://github.com/ScintillaOrg/lexilla/blob/master/License.txt (same text, "Permission to use, copy, modify, and distribute ... without fee")
- **Last activity:** Scintilla 5.6.7 released 28 Sep 2026 ([history](https://www.scintilla.org/ScintillaHistory.html)). Lexilla 5.5.4 released 28 Sep 2026 ([history](https://www.scintilla.org/LexillaHistory.html)). Lexilla GitHub last push 2026-10-24.
- **Large files:**
  - The text lives in a gap buffer: `SplitVector::GapTo` in `src/SplitVector.h` ([copy in Notepad4](https://github.com/zufuliu/notepad4/blob/main/scintilla/src/SplitVector.h)).
  - `SC_DOCUMENTOPTION_STYLES_NONE` "saves significant memory, often 40%". `SC_DOCUMENTOPTION_TEXT_LARGE` allows documents over 2 GB in 64-bit builds. For multi-GB files the docs recommend the null lexer or `SCI_SETIDLESTYLING` ([ScintillaDoc](https://www.scintilla.org/ScintillaDoc.html)).
  - `SCI_SETLAYOUTTHREADS` runs wrap and measurement on several threads; the docs say a 4-core machine is "often able to reduce text layout time to just over one quarter" ([ScintillaDoc](https://www.scintilla.org/ScintillaDoc.html)).
  - The docs say all lines are the same height. Notepad4's Markdown headings are therefore only "bold; fore:#…", never bigger ([stlMarkdown.cpp](https://github.com/zufuliu/notepad4/blob/main/src/EditLexers/stlMarkdown.cpp)).
- **Rust bindings:** [scintilla-sys](https://github.com/snuk182/scintilla-sys-rs) 5.5.7 (crates.io, 2025-04-22).
  - Raw FFI with a `win32` feature that builds Scintilla through `cc`.
  - Licence: MIT OR Apache-2.0 per its [Cargo.toml](https://github.com/snuk182/scintilla-sys-rs/blob/master/Cargo.toml). Its [LICENSE-MIT](https://github.com/snuk182/scintilla-sys-rs/blob/master/LICENSE-MIT) still carries "imgui-rs Developers", so read it carefully.
  - Last push 2025-10-17. The other route is to build Scintilla yourself with `cc` and drive it with `SendMessageW` through the `windows` crate.
- **What we'd take:** the whole control, statically linked. Also these APIs:
  - `SCI_SETTECHNOLOGY(SC_TECHNOLOGY_DIRECTWRITE)`
  - undo grouping with `SCI_BEGINUNDOACTION`
  - the large-document options above
  - Lexilla's `LexMarkdown.cxx` for markdown styling
- **Verdict:** the best ready-made plain-text engine for 50 MB+. The one real gap is bigger headings: only bold or colour are possible.

#### Win32 RichEdit (msftedit.dll, `RICHEDIT50W`, TOM)
- **Repo:** none. It is an OS component; docs are at https://learn.microsoft.com/en-us/windows/win32/controls/rich-edit-controls
- **Licence:** proprietary system DLL that ships with Windows (no code to borrow). Use only.
- **Last activity:** ships with Windows. The newer Office RichEdit build is *not* the system DLL (see below).
- **Large files:**
  - Windows 11 Notepad runs on RichEdit. Murray Sargent writes that "Notepad is often used to view large files ... lines can be crazy long" and "We improved RichEdit's performance for large ASCII files". He also says "It's not the Windows RichEdit in msftedit.dll": Notepad uses the Microsoft 365 build ([Math in Office blog](https://devblogs.microsoft.com/math-in-office/windows-11-notepad/)).
  - The post does not say whether those gains reached the system msftedit.dll, so a 50 MB test on the system DLL is required.
  - The default typing limit is 32,767 characters. Raise it with `EM_EXLIMITTEXT`; `EM_STREAMIN` with `SF_TEXT` ignores the limit ([EM_EXLIMITTEXT](https://learn.microsoft.com/en-us/windows/win32/controls/em-exlimittext)).
  - Undo defaults to 100 actions ([EM_SETUNDOLIMIT](https://learn.microsoft.com/en-us/windows/win32/controls/em-setundolimit)).
- **Styling:**
  - TOM is reached through `EM_GETOLEINTERFACE`, then `ITextDocument`, `ITextRange` and `ITextFont` ([TOM](https://learn.microsoft.com/en-us/windows/win32/controls/text-object-model)).
  - Plain-text mode allows only one format: "The text in a plain text control can have only one format" ([EM_SETTEXTMODE](https://learn.microsoft.com/en-us/windows/win32/controls/em-settextmode)). Per-range styling (bigger headings) therefore needs rich mode.
  - Undo can be suspended while restyling (`ITextDocument::Undo(tomFalse)`, [docs](https://learn.microsoft.com/en-us/windows/win32/api/tom/nf-tom-itextdocument-undo)).
  - `ITextFont::Reset(tomApplyTmp)` applies temporary formatting ([docs](https://learn.microsoft.com/en-us/windows/win32/api/tom/nf-tom-itextfont-reset)). I could not verify which properties temporary formatting supports, in particular font size.
  - `EC_ENDOFLINE_DETECTFROMCONTENT` keeps the file's line endings ([blog](https://devblogs.microsoft.com/math-in-office/windows-11-notepad/)).
  - The `RichEditD2DPT` window class is what Notepad uses for DirectWrite (same blog).
- **What we'd take:** use only, no code. Patterns: TOM range styling with undo suspended, `EC_ENDOFLINE_DETECTFROMCONTENT`.
- **Verdict:** the only built-in control that can draw bigger headings. Large-file speed on the *system* DLL is unproven, and rich mode adds RTF clipboard and format quirks.

#### Win32 EDIT control
- **Repo:** none, OS component ([About Edit Controls](https://learn.microsoft.com/en-us/windows/win32/controls/about-edit-controls))
- **Licence:** OS component, use only
- **Last activity:** ships with Windows
- **Large files:**
  - The text is one contiguous `LocalAlloc` buffer (`EM_SETHANDLE`).
  - The typing limit defaults to 32,767 characters and can be raised with `EM_LIMITTEXT` ([EM_LIMITTEXT](https://learn.microsoft.com/en-us/windows/win32/controls/em-limittext)).
  - Undo is a single toggle: "An undo operation can also be undone ... as long as there is no intervening edit" ([EM_UNDO](https://learn.microsoft.com/en-us/windows/win32/controls/em-undo)). `WM_SETTEXT` empties it.
  - There is one font per control (`WM_SETFONT`) and no per-range styling.
  - `EM_SETENDOFLINE` supports LF and CR documents ([docs](https://learn.microsoft.com/en-us/windows/win32/controls/em-setendofline)).
- **What we'd take:** nothing for the editor. Fine for the Find and Go-to input boxes.
- **Verdict:** ruled out. It has no multi-level undo and no markdown styling.

#### Self-built: rope + DirectWrite (the tasca route)
Rendering would use DirectWrite `IDWriteTextLayout` per visible line through the `windows` crate (tasca already uses D2D/DWrite). Candidate buffers:

##### ropey
- **Repo:** https://github.com/cessen/ropey
- **Licence:** MIT, verified from https://github.com/cessen/ropey/blob/master/LICENSE.md
- **Last activity:** last push 2026-09-12. Stable release 1.6.1 (2023-10-18); 2.0.0-beta.1 (2025-08-02) on [crates.io](https://crates.io/crates/ropey).
- **Large files:** "Even on texts that are multiple gigabytes large, edits are measured in single-digit microseconds". It tracks all Unicode line endings including CRLF ([README](https://github.com/cessen/ropey)).
- **What we'd take:** the crate as the buffer: line↔char indexing for line:col and go-to-line.
- **Verdict:** safest rope choice.

##### crop
- **Repo:** https://github.com/noib3/crop (moved from `nomad/crop`)
- **Licence:** MIT, verified from https://github.com/noib3/crop/blob/main/LICENSE
- **Last activity:** last push 2026-08-23. Release 0.4.3 (2025-04-25, crates.io).
- **Large files:** B-tree rope with logarithmic edits. O(1) copy-on-write clones make it "cheap to snapshot a Rope and send it to a background thread". It uses byte offsets, while Ropey uses char offsets ([README](https://github.com/noib3/crop)).
- **What we'd take:** the crate. Cheap snapshots suit background save and search.
- **Verdict:** fine alternative to ropey.

##### xi-rope
- **Repo:** https://github.com/xi-editor/xi-editor (`rust/rope`)
- **Licence:** Apache-2.0, verified from https://github.com/xi-editor/xi-editor/blob/master/LICENSE
- **Last activity:** last push 2024-03-19. Crate 0.3.0 dates from 2019. The Lapce fork `lapce-xi-rope` 0.4.0 is from 2025-12-19.
- **Large files:** B-tree rope. The project itself says it "is currently discontinued" ([README](https://github.com/xi-editor/xi-editor)).
- **What we'd take:** ideas only (CRDT-style deltas). Not worth depending on.
- **Verdict:** skip.

##### cosmic-text
- **Repo:** https://github.com/pop-os/cosmic-text
- **Licence:** MIT OR Apache-2.0, verified from https://github.com/pop-os/cosmic-text/blob/main/LICENSE-MIT and `LICENSE-APACHE`
- **Last activity:** last push 2026-09-30. Release 0.19.0 (2026-04-22).
- **Large files:**
  - The `Buffer` holds per-line `BufferLine`s. A rope buffer "for large file support" ([PR #459](https://github.com/pop-os/cosmic-text/pull/459)) was closed without merging.
  - Open bug: "`shape_run` is quadratic and basically freezes on large runs" with 100–200 KiB of unrenderable text ([#523](https://github.com/pop-os/cosmic-text/issues/523)).
- **What we'd take:** probably nothing. It duplicates DirectWrite, which tasca already uses, and adds its own font stack.
- **Verdict:** wrong fit for a Windows-only, lightest-possible app.

**Self-built verdict:** it gives full control (bigger headings, exact behaviour) but the work is large: caret, selection, IME, wrap, undo and accessibility all have to be written. Only worth it if Scintilla's equal-height lines and RichEdit's unknowns are both unacceptable.

#### Notepad2 (Florian Balmer)
- **Repo:** no public VCS. Source zip from https://www.flos-freeware.ch/notepad2.html (4.2.25 src)
- **Licence:** BSD-3-Clause, verified from `License.txt` inside https://www.flos-freeware.ch/zip/notepad2_4.2.25_src.zip
- **Last activity:** 4.2.25; "Project last updated: October 28, 2012" ([site](https://www.flos-freeware.ch/notepad2.html))
- **Editing control:** Scintilla
- **What we'd take** (all BSD, can be copied with attribution):
  - `src/Edit.c`: `IsUnicode`, `IsUTF8`, `EditDetectEOLMode`, `EditLoadFile`, `EditFindReplaceDlg`
  - `src/Notepad2.c`: `InstallFileWatching` and `WatchTimerProc` (`FindFirstChangeNotification` plus a timer)
- **Verdict:** the original. Its descendants below are better maintained copies of the same code.

#### Notepad2-mod (XhmikosR)
- **Repo:** https://github.com/XhmikosR/notepad2-mod (archived)
- **Licence:** BSD-3-Clause, verified from https://github.com/XhmikosR/notepad2-mod/blob/master/License.txt
- **Last activity:** last push 2018-07-26. Release 4.2.25.998 (2017-08-06). Archived.
- **Editing control:** Scintilla
- **What we'd take:**
  - `src/Edit.c`: `IsUnicode` (L939), `IsUTF8` (L986), `EditDetectEOLMode` (L604), `EditFindReplaceDlg` (L5352), `EditLinenumDlg` (go to line, L6275)
  - `src/Notepad2.c`: `InstallFileWatching` (L7754)
- **Verdict:** frozen reference. Prefer Notepad4's newer versions of the same functions.

#### Notepad3 (rizonesoft)
- **Repo:** https://github.com/rizonesoft/Notepad3
- **Licence:** BSD-3-Clause, verified from https://github.com/rizonesoft/Notepad3/blob/master/Build/Docs/License.txt (there is no root LICENSE).
  - Bundled `src/uchardet` is **MPL-1.1** (tri-licensed with GPL/LGPL) per [COPYING](https://github.com/rizonesoft/Notepad3/blob/master/src/uchardet/uchardet/COPYING). Not in the allowed list: ideas only unless you decide otherwise.
- **Last activity:** last push 2026-10-26. Release RELEASE_7.26.602.1 (2026-06-02).
- **Editing control:** Scintilla + Lexilla (vendored, with its own `lexilla/np3_patches/`)
- **What we'd take:**
  - **Dark mode:** `src/DarkMode/DarkMode.cpp`, "Based on code from win32-darkmode by Richard Yu". That upstream is [ysc3839/win32-darkmode](https://github.com/ysc3839/win32-darkmode), MIT per its [LICENSE](https://raw.githubusercontent.com/ysc3839/win32-darkmode/master/LICENSE), last push 2023-03-28. Take it from upstream.
  - **Large-file prompt:** `src/Edit.c` `EditLoadFile` (L1241) warns above a configurable `FileLoadWarningMB` and switches to a plain "large file lexer" (`src/Styles.c` `GetLargeFileLexer`). Over 2 GB it forces UTF-8.
  - **File watching:** `src/Notepad3.c` `InstallFileWatching` (L13879) with `FindFirstChangeNotificationW` and a change dialog (`src/Dialogs.c` `ChangeNotifyDlg`).
  - **Encoding detection:** `src/uchardet/EncodingDetection.cpp` (uchardet or Google CED). In Rust the permissive equivalent is [chardetng](https://github.com/hsivonen/chardetng), Apache-2.0 OR MIT per its [COPYRIGHT](https://github.com/hsivonen/chardetng/blob/master/COPYRIGHT).
- **Verdict:** best source for the dark-mode and file-watch UX. Avoid its uchardet.
- Note: [ozone10/win32-darkmodelib](https://github.com/ozone10/win32-darkmodelib) (proposed for Notepad4 in [#1203](https://github.com/zufuliu/notepad4/issues/1203)) is **MPL-2.0** per its [LICENSE.md](https://raw.githubusercontent.com/ozone10/win32-darkmodelib/main/LICENSE.md). Not in the allowed list: ideas only unless you decide otherwise.

#### Notepad4 / matepath (zufuliu)
- **Repo:** https://github.com/zufuliu/notepad4
- **Licence:** BSD-3-Clause (Notepad4, matepath, Notepad2, Notepad2-mod), verified from https://github.com/zufuliu/notepad4/blob/main/License.txt. Vendored Scintilla keeps its HPND-style licence.
- **Last activity:** last push 2026-10-25. Release v26.10r6392 (2026-10-18). The whole editor plus matepath ships as a ~1.19 MB x64 zip ([release](https://github.com/zufuliu/notepad4/releases/tag/v26.10r6392)).
- **Editing control:** Scintilla (vendored and patched in `scintilla/`)
- **What we'd take:**
  - **Large-file mode:** `src/Edit.cpp` L154–163 recreates the document with `SC_DOCUMENTOPTION_TEXT_LARGE | SC_DOCUMENTOPTION_STYLES_NONE` and turns off change history and undo collection while loading. The max-size check at L1003–1030 caps files at the 2 GiB / free-memory limit.
  - **Encoding:** `src/EditEncoding.cpp` `EditDetermineEncoding` (L2332), `DetectUnicode`, `DetectUTF16Latin1`, `IsUTF8`.
  - **Line endings:** `src/Edit.cpp` `EditDetectEOLMode` (L429).
  - **File watching:** `src/Notepad4.cpp` `InstallFileWatching` (L7881) and `WatchTimerProc` (L7965).
  - **Find/replace:** `src/Edit.cpp` `EditFindReplaceDlg` (L5193).
  - **Markdown:** a BSD markdown lexer in `scintilla/lexers/LexMarkdown.cxx` with styles in `src/EditLexers/stlMarkdown.cpp`.
- **Dark mode:** not done. Issue [#105 "Dark Mode"](https://github.com/zufuliu/notepad4/issues/105) is open, and `src/DarkMode.cpp` is a 116-line dialog/treeview helper.
- **Verdict:** the best permissive code donor for file I/O, encoding, line endings and large-file handling.

#### Notepads (UWP, Jackie Liu, formerly JasonStein)
- **Repo:** https://github.com/0x7c13/Notepads
- **Licence:** MIT, verified from https://github.com/0x7c13/Notepads/blob/master/LICENSE.txt
  - Its encoding detector [UTF-unknown](https://github.com/CharsetDetector/UTF-unknown) is MPL-1.1 / GPL-2.0 / LGPL-2.1 (files in its `license/` folder). Not in the allowed list: ideas only.
- **Last activity:** last push 2026-10-26, but the last default-branch commit was 2026-03-29. Release v1.5.6.0 (2025-03-28).
- **Editing control:** UWP `RichEditBox`: `TextEditorCore : RichEditBox` in `src/Notepads/Controls/TextEditor/TextEditorCore.cs`.
- **Large files:** hard 1 MB cap: `fileProperties.Size > 1000 * 1024` in `src/Notepads/Utilities/FileSystemUtility.cs` L347. The README says "Notepads does not work well with large files; the file size limit is set to 1MB" ([README](https://github.com/0x7c13/Notepads#readme)).
- **What we'd take (C#, patterns only):**
  - `Utilities/LineEndingUtility.cs` for line-ending detection and apply
  - `Controls/TextEditor/TextEditor.xaml.cs` for the external-modification state (`FileModificationState`, date-modified compare)
  - `Controls/FindAndReplace/` for the find/replace UX
  - `Controls/Markdown/MarkdownExtensionView` for the markdown preview toggle
- **Verdict:** a UX reference, not an engine. It is a warning that RichEdit-family controls need care with big files.

### WebView (Tauri / WebView2)

#### CodeMirror 6 (+ @codemirror/lang-markdown, @lezer/markdown)
- **Repo:** https://code.haverbeke.berlin/codemirror/view (also `/state`, `/lang-markdown`, and https://code.haverbeke.berlin/lezer/markdown). The GitHub repos are **archived**: "This repository has moved to https://code.haverbeke.berlin/codemirror/view" ([GitHub README](https://github.com/codemirror/view)).
- **Licence:** MIT, verified from https://code.haverbeke.berlin/codemirror/view/raw/branch/main/LICENSE, `.../state/...LICENSE`, `.../lang-markdown/...LICENSE`, https://code.haverbeke.berlin/lezer/markdown/raw/branch/main/LICENSE
- **Last activity (npm publish):**
  - @codemirror/view 6.43.15: 2026-10-19
  - @lezer/markdown 1.8.1: 2026-10-19
  - @codemirror/state 6.7.6: 2026-09-22
  - @codemirror/lang-markdown 6.5.2: 2026-08-04
- **Large files:**
  - "CodeMirror doesn't render the entire document, when that document is big". The text is stored "split by line in a tree-shaped" structure, and line heights are tracked for the whole document, "initially estimated, measured accurately when content is drawn" ([guide](https://codemirror.net/docs/guide/)).
  - "Long lines (when not wrapped) ... can still make the viewport rather huge" ([guide](https://codemirror.net/docs/guide/)).
  - The [million-line demo](https://codemirror.net/examples/million/) handles "a few million lines". The parser limits its work, so highlighting can stop far down the file.
  - I found no documented hard size limit.
- **Markdown styling:** @lezer/markdown is an "incremental Markdown (CommonMark with support for extension) parser" ([README](https://code.haverbeke.berlin/lezer/markdown)). Highlighting keeps the source text, so the symbols stay visible.
  - `defaultLineHeight` "May not be accurate for all lines" ([ref](https://codemirror.net/docs/ref/)). Variable-height lines (bigger headings) are supported by the height tracking described above.
- **What we'd take:** `@codemirror/view`, `@codemirror/state`, `@codemirror/commands` (undo), `@codemirror/search`, `@codemirror/lang-markdown` + a `HighlightStyle`.
  - Unpacked npm sizes: view 1.26 MB, state 0.44 MB, lezer/markdown 0.46 MB, lang-markdown 0.07 MB (`npm view dist.unpackedSize`).
- **Verdict:** the clear WebView choice. It fits the "symbols visible, headings bigger" model exactly.

#### Monaco
- **Repo:** https://github.com/microsoft/monaco-editor (the core lives in https://github.com/microsoft/vscode)
- **Licence:** MIT, verified from https://raw.githubusercontent.com/microsoft/monaco-editor/main/LICENSE.txt
- **Last activity:** last push 2026-10-26. Release v0.57.0 (2026-09-24).
- **Large files:**
  - Piece-tree buffer whose memory is "very close to the original file size" ([VS Code blog](https://code.visualstudio.com/blogs/2018/03/23/text-buffer-reimplementation)).
  - `textModel.ts` turns off tokenization above `LARGE_FILE_SIZE_THRESHOLD = 20 MB` or 300K lines. Worker sync stops above `_MODEL_SYNC_LIMIT = 50 MB`. Heap-heavy operations stop above 256M chars ([source](https://github.com/microsoft/vscode/blob/main/src/vs/editor/common/model/textModel.ts)).
  - Decorations now accept `lineHeight` (a multiplier), `fontSize` and `fontWeight` ([model.ts](https://github.com/microsoft/vscode/blob/main/src/vs/editor/common/model.ts)).
- **What we'd take:** nothing. Its npm package unpacks to ~101 MB, and the IDE-scale features are unused.
- **Verdict:** too heavy for "lightest app possible". Its 20/50 MB thresholds fall exactly in scrivi's range.

#### Ace
- **Repo:** https://github.com/ajaxorg/ace
- **Licence:** BSD-3-Clause, verified from https://github.com/ajaxorg/ace/blob/master/LICENSE
- **Last activity:** last push 2026-10-21. Release v1.44.0 (2026-08-07).
- **Large files:** "Handles huge documents (at last check, 4,000,000 lines is the upper limit)" ([README](https://github.com/ajaxorg/ace)).
- **What we'd take:** nothing over CM6. I did not verify whether it supports variable line heights.
- **Verdict:** credible but older design. CM6 is the better fit.

#### ProseMirror / Milkdown / Tiptap (WYSIWYG)
- **Repo:** https://code.haverbeke.berlin/prosemirror/prosemirror-view (GitHub archived), https://github.com/Milkdown/milkdown, https://github.com/ueberdosis/tiptap
- **Licence:** all MIT, verified from https://code.haverbeke.berlin/prosemirror/prosemirror-view/raw/branch/main/LICENSE, https://github.com/Milkdown/milkdown/blob/main/LICENSE, https://github.com/ueberdosis/tiptap/blob/main/LICENSE.md
- **Last activity (npm):** prosemirror-view 1.42.8 (2026-10-24), @milkdown/core 7.22.3 (2026-10-10), @tiptap/core 3.32.0 (2026-10-13)
- **Large files:** not researched. They are schema-based rich-text trees, not source buffers.
- **What we'd take:** nothing. They parse markdown into a document model and hide the symbols, which breaks the "symbols stay visible" model and loses the exact source text on save.
- **Verdict:** don't fit.

#### MarkText (and its Muya engine)
- **Repo:** https://github.com/marktext/marktext (Muya is in `packages/muya`)
- **Licence:** MIT, verified from https://github.com/marktext/marktext/blob/develop/LICENSE. `@muyajs/core` declares MIT in `packages/muya/package.json`.
- **Last activity:** last push 2026-10-26. Release v0.23.0 (2026-10-21).
- **Stack:** Electron ~42. Muya is a custom WYSIWYG-style live-preview editor built on snabbdom. It parses markdown with `marked` and writes it back with `turndown` ([Muya README](https://github.com/marktext/marktext/tree/develop/packages/muya)).
- **Large files:** repeated slowness reports, e.g. [#4887](https://github.com/marktext/marktext/issues/4887) "App freezes indefinitely when opening large Markdown files (~300k words)" and [#5860](https://github.com/marktext/marktext/issues/5860).
- **What we'd take:** nothing for the editor (its markdown↔HTML round-trip contradicts source editing). Maybe the reading-view CSS.
- **Verdict:** wrong model.

#### Zettlr
- **Repo:** https://github.com/Zettlr/Zettlr
- **Licence:** **GPL-3.0**, verified from https://github.com/Zettlr/Zettlr/blob/develop/LICENSE. Ideas only.
- **Last activity:** last push 2026-10-25. Release v4.9.0 (2026-10-25).
- **Stack:** Electron ^44 + CodeMirror 6 (`@codemirror/view` ^6.43.14, `@codemirror/lang-markdown`, `@lezer/markdown` in [package.json](https://github.com/Zettlr/Zettlr/blob/develop/package.json)).
- **What we'd take:** ideas only. It proves CM6 + lezer-markdown can carry a full markdown source editor.
- **Verdict:** confirms the CM6 path; no code.

#### MDHero and other Tauri markdown editors
- **MDHero:** https://github.com/vaibhav-kakde-in/mdhero
  - MIT, verified from https://github.com/vaibhav-kakde-in/mdhero/blob/main/LICENSE
  - Last release v0.2.10 (2026-09-13). Tauri 2 + Svelte 5, rendered with `markdown-it`. "~8MB" ([README](https://github.com/vaibhav-kakde-in/mdhero)).
  - Its editor is a plain `<textarea>` with a mirrored highlight backdrop (`src/lib/components/Editor.svelte`), so headings cannot be bigger. It has file watching.
  - **Take:** the reading-view pipeline (`src/lib/renderer/pipeline.ts`) and the scroll-sync idea.
- **SoloMD:** https://github.com/zhitongblog/solomd
  - MIT, verified from https://github.com/zhitongblog/solomd/blob/main/LICENSE
  - Last push 2026-10-22. Tauri + Vue with CodeMirror 6 + `@codemirror/lang-markdown` + `markdown-it` ([app/package.json](https://github.com/zhitongblog/solomd/blob/main/app/package.json)). Self-described "~15 MB".
- **Paperling:** https://github.com/Razee4315/Paperling
  - Apache-2.0, verified from https://github.com/Razee4315/Paperling/blob/main/LICENSE
  - Last push 2026-10-23. Tauri + React with CM6 + lang-markdown + `react-markdown`.
- **markamd:** https://github.com/mattenarle10/markamd
  - MIT, verified from https://github.com/mattenarle10/markamd/blob/main/LICENSE
  - Last push 2026-10-13. Tauri + React with CM6 + `markdown-it`.
- **MarkFlowy:** https://github.com/drl990114/MarkFlowy
  - **AGPL-3.0**, verified from its [LICENSE](https://github.com/drl990114/MarkFlowy/blob/main/LICENSE). Ideas only.
- **Typora:** closed source, nothing to borrow.
- **Verdict:** the permissive Tauri editors all converge on **CM6 + lang-markdown for source, markdown-it/remark for the rendered view**. SoloMD and Paperling are the closest working references.

#### Takeaways for the stack choice
- **Bigger headings is the deciding requirement.** Scintilla, the best native engine for 50 MB plain text, draws every line at one height ([ScintillaDoc](https://www.scintilla.org/ScintillaDoc.html)). On (N), markdown either gets bold/colour-only headings, or needs RichEdit rich mode or a self-built DirectWrite view.
- **(W) has a clean answer:** CodeMirror 6 + @lezer/markdown (all MIT) gives visible-symbol styling with variable line heights and viewport rendering, and permissive Tauri apps (SoloMD, Paperling, markamd) already ship it. Two notes:
  - CM6 moved off GitHub to code.haverbeke.berlin in 2026; npm releases continue.
  - Monaco is out on weight, and it turns off its own features above 20 MB.
- **Large files favour (N).** Scintilla plus Notepad4's large-file mode (`STYLES_NONE`, `TEXT_LARGE`, no undo while loading) is proven well past 50 MB. CM6 only claims "a few million lines". Getting a 50 MB string into WebView2 still has to be measured (not covered here).
- **RichEdit is the wildcard:** native, zero added weight, and it can do bigger headings via TOM. But Microsoft's own large-file fixes are documented only for the Office build Notepad uses, not the system msftedit.dll. Prototype a 50 MB load before relying on it.
- **Code donors are permissive and plentiful on the native side:** Notepad4 and Notepad2/Notepad2-mod (BSD-3) cover encoding detection, EOL detection, file watching, find/replace and large-file mode. Use win32-darkmode (MIT) for dark mode. Avoid uchardet, UTF-unknown and win32-darkmodelib (MPL) unless you decide otherwise. chardetng (MIT/Apache) is the Rust-side encoding detector.
- **A hybrid is plausible:** Scintilla for all plain-text types (most files), with styled .md either bold/colour-only in Scintilla or in RichEdit. This keeps (N) light, but means two editor surfaces to maintain.

---

## 2. Markdown, encoding, line endings, large files

Researched 2026-10-26. Licences were checked against the LICENSE/COPYRIGHT text in each repo (fetched with `gh api .../contents/<file>` or the raw file). Activity dates come from the GitHub API (`pushed_at`, latest release or tag), crates.io or the npm registry. Stacks: **(N)** Rust + raw Win32 (`windows` 0.62, D2D/DirectWrite); **(W)** Tauri 2 / WebView2.

### Markdown

#### pulldown-cmark
- **Repo:** https://github.com/pulldown-cmark/pulldown-cmark
- **Licence:** MIT, verified from https://github.com/pulldown-cmark/pulldown-cmark/blob/main/LICENSE (crate manifest: `license = "MIT"`)
- **Last activity:** 2026-10-21 (release v0.13.5, also on crates.io); last push 2026-10-23
- **GFM:** has tables, task lists, strikethrough and footnotes as opt-in flags (`ENABLE_TABLES/TASKLISTS/STRIKETHROUGH/FOOTNOTES`). `ENABLE_GFM` only adds `[!NOTE]`-style alerts ([lib.rs options](https://github.com/pulldown-cmark/pulldown-cmark/blob/main/pulldown-cmark/src/lib.rs)). There are **no GFM extended autolinks** (bare `www.`/`https://` URLs). The maintainers' answer is to post-process `Text` events ([#494](https://github.com/pulldown-cmark/pulldown-cmark/issues/494); still listed as missing in [#518 "Full GFM Support"](https://github.com/pulldown-cmark/pulldown-cmark/issues/518), open).
- **Offsets:** yes. `into_offset_iter()` yields `(Event, Range<usize>)` byte ranges into the source ([README](https://github.com/pulldown-cmark/pulldown-cmark#readme)). A `Start(Strong)` range covers the `**` delimiters and the child `Text` ranges do not, so marker spans = parent range minus child ranges.
- **Incremental:** no. It is a pull parser over the whole `&str`. Text events borrow from the source, so allocation is minimal ("Fast; a bare minimum of allocation and copying", README).
- **Size/speed:** the repo publishes no numbers, only a `bench/` harness. The `html` feature is optional (`default = ["std","getopts","html"]`, so use `default-features = false`), which keeps the parser alone.
- **HTML/sanitising:** the HTML renderer writes `Html`/`InlineHtml` events **verbatim** ([html.rs](https://github.com/pulldown-cmark/pulldown-cmark/blob/main/pulldown-cmark/src/html.rs)). There is no safe mode, and the docs warn that heading attributes can carry `onclick`, so "sanitize the output instead" ([lib.rs](https://github.com/pulldown-cmark/pulldown-cmark/blob/main/pulldown-cmark/src/lib.rs)).
- **What we'd take:** the crate itself, used as the event/offset source for both styling and the rendered view.
- **Verdict:** best fit for (N): tiny, byte offsets, permissive. Bare-URL autolinks need our own pass.

#### comrak
- **Repo:** https://github.com/kivikakk/comrak
- **Licence:** BSD-2-Clause, verified from https://github.com/kivikakk/comrak/blob/main/COPYING. The file also carries the cmark-gfm BSD-2 notice and MIT notices for houdini and similar code. Cargo: `license = "BSD-2-Clause"`.
- **Last activity:** 2026-10-26 (release v0.59.0)
- **GFM:** all four GFM extensions: tables, task lists, strikethrough, autolinks. Tagfilter was deprecated in 0.55 and removed in 0.56. Footnotes and many more are extras ([README "Extensions"](https://github.com/kivikakk/comrak#extensions)).
- **Offsets:** `sourcepos` on every AST node, as 1-based line/column with byte columns (or char columns via `--sourcepos-chars`) ([README](https://github.com/kivikakk/comrak#readme), `Sourcepos` in [nodes.rs](https://github.com/kivikakk/comrak/blob/main/src/nodes.rs)). Inline sourcepos was buggy until late 2025: a batch of fixes closed 2025-10 to 2026-02 ([#541](https://github.com/kivikakk/comrak/issues/541), [#745](https://github.com/kivikakk/comrak/issues/745)). Line/col has to be converted to offsets.
- **Incremental:** no (arena AST).
- **Size/speed:** the README says plainly "Comrak is not and will not be the fastest" because it builds an AST ([Benchmarking](https://github.com/kivikakk/comrak#benchmarking)). Default features pull in `clap`, `syntect` and Oniguruma, so `default-features = false` is mandatory for us.
- **HTML/sanitising:** safe by default. It scrubs raw HTML and dangerous links unless `r#unsafe` is set, and recommends `ammonia` when it is ([README "Security"](https://github.com/kivikakk/comrak#security)).
- **What we'd take:** the crate if we want full GFM, including autolinks, plus a safe HTML renderer for the WebView path.
- **Verdict:** the most complete GFM in Rust and safe HTML out of the box. Heavier than pulldown-cmark, and its offsets are line/col.

#### markdown-rs (crate `markdown`)
- **Repo:** https://github.com/wooorm/markdown-rs
- **Licence:** MIT, verified from https://github.com/wooorm/markdown-rs/blob/main/license
- **Last activity:** 2025-04-23 (release 1.0.0, also the last push). Dormant for 18 months.
- **GFM:** "100% GFM": autolink literal, footnote, strikethrough, table, tasklist, tagfilter ([readme](https://github.com/wooorm/markdown-rs#readme)).
- **Offsets:** an mdast AST in which every node has `position` (line:col plus byte offset), e.g. `1:3-1:6 (2-5)` ([readme example](https://github.com/wooorm/markdown-rs#readme)). It is a state machine where "every byte is accounted for".
- **Incremental:** no.
- **Size/speed:** no numbers published. The readme warns that unclosed constructs can make it slow or crash it, and advises capping input ("500kb can hold a big book") and running it on another thread ([Security](https://github.com/wooorm/markdown-rs#security)).
- **HTML/sanitising:** safe by default. `allow_dangerous_html` and `allow_dangerous_protocol` are opt-in.
- **What we'd take:** ideas only (the mdast shape). We would use the crate only if its exact positions are needed.
- **Verdict:** the most spec-faithful choice, but stale and the slowest-risk parser. Pass.

#### md4c (C, for completeness)
- **Repo:** https://github.com/mity/md4c
- **Licence:** MIT, verified from https://github.com/mity/md4c/blob/master/LICENSE.md
- **Last activity:** 2026-10-19 (last push; no GitHub releases)
- **Offsets:** **none**. "Provide source information to caller" has been open since [#91](https://github.com/mity/md4c/issues/91) (also [#273](https://github.com/mity/md4c/issues/273)).
- **What we'd take:** ideas only.
- **Verdict:** fast SAX-style C parser, but without offsets it cannot drive source styling, and it would need FFI. Pass.

#### markdown-it (JS)
- **Repo:** https://github.com/markdown-it/markdown-it
- **Licence:** MIT, verified from https://github.com/markdown-it/markdown-it/blob/master/LICENSE
- **Last activity:** 2026-10-16 (npm 15.0.3); last push 2026-10-23
- **GFM:** tables and strikethrough are built-in rules (`rules_block/table.ts`, `rules_inline/strikethrough.ts`). Autolinks need `linkify: true` (linkify-it). Task lists and footnotes are plugins: [markdown-it-task-lists](https://www.npmjs.com/package/markdown-it-task-lists) (ISC, last published 2018-03-06) and markdown-it-footnote (MIT, 2023-12-06) ([docs/usage.md](https://github.com/markdown-it/markdown-it/blob/master/docs/usage.md)).
- **Offsets:** **line-level only**. `Token.map` is `[line_begin, line_end]` on block tokens ([token.ts](https://github.com/markdown-it/markdown-it/blob/master/src/token.ts)). Inline tokens carry no positions.
- **Size/speed:** the browser ESM build `markdown-it.esm.min.mjs` 15.0.3 is 124,681 bytes, about 44 KB gzipped (measured from [jsDelivr](https://cdn.jsdelivr.net/npm/markdown-it@15.0.3/dist/browser/markdown-it.esm.min.mjs)). Their own benchmark (2013 MBP): 743 ops/s on a 7.7 KB README with the full preset, 1,568 ops/s with the CommonMark preset ([docs/benchmark.md](https://github.com/markdown-it/markdown-it/blob/master/docs/benchmark.md)).
- **HTML/sanitising:** `html: false` by default, which escapes raw HTML. It blocks `javascript:`, `vbscript:`, `file:` and most `data:` links. Enabling HTML needs an external sanitiser ([docs/safety.md](https://github.com/markdown-it/markdown-it/blob/master/docs/safety.md)).
- **What we'd take:** in (W), the renderer for the read-only view, if we render in JS.
- **Verdict:** a good JS renderer, but useless for source styling.

#### micromark (JS)
- **Repo:** https://github.com/micromark/micromark
- **Licence:** MIT, verified from https://github.com/micromark/micromark/blob/main/license
- **Last activity:** 2026-09-26 (release 4.0.3). `micromark-extension-gfm` was last published 2023-06-26 (3.0.0).
- **GFM:** 100% via `micromark-extension-gfm` (autolink literal, footnote, strikethrough, table, tagfilter, task list) ([readme](https://github.com/micromark/micromark#readme)).
- **Offsets:** concrete tokens with positions. Use it with `mdast-util-from-markdown` to get an AST with positions.
- **Size/speed:** "smallest CM parser at ±14kb" (readme). No speed numbers.
- **HTML/sanitising:** safe by default.
- **What we'd take:** ideas only. It is markdown-rs's JS twin.
- **Verdict:** fine, but in (W) lezer covers styling and markdown-it covers rendering better.

#### @lezer/markdown (CodeMirror 6)
- **Repo:** moved to https://code.haverbeke.berlin/lezer/markdown. The GitHub mirror https://github.com/lezer-parser/markdown is **archived** and its last commit is "Add forwarding link".
- **Licence:** MIT, verified from https://code.haverbeke.berlin/lezer/markdown/raw/branch/main/LICENSE
- **Last activity:** 2026-10-21 (last commit on the new forge); npm 1.8.1 published 2026-10-19
- **GFM:** `GFM = [Table, TaskList, Strikethrough, Autolink]` ([extension.ts](https://code.haverbeke.berlin/lezer/markdown/src/branch/main/src/extension.ts)). **No footnotes.**
- **Offsets / incremental:** yes. It produces Lezer trees with absolute positions and "consumes fragments of such trees for its incremental parsing". It does not validate link references (`[a][b]` always parses as a link), and it "doesn't help with outputting HTML" ([README](https://github.com/lezer-parser/markdown#readme)).
- **What we'd take:** in (W) with CodeMirror 6, `@codemirror/lang-markdown` + lezer GFM directly drive the light styling. Mark-node types such as `EmphasisMark` and `HeaderMark` make "symbols stay visible but dimmed" trivial.
- **Verdict:** the only truly incremental, offset-exact parser in the list. JS only.

#### tree-sitter-markdown
- **Repo:** https://github.com/tree-sitter-grammars/tree-sitter-markdown
- **Licence:** MIT, verified from https://github.com/tree-sitter-grammars/tree-sitter-markdown/blob/split_parser/LICENSE
- **Last activity:** 2026-10-24 (release v0.5.4)
- **GFM:** task lists, strikethrough and pipe tables are on by default ([README](https://github.com/tree-sitter-grammars/tree-sitter-markdown#extensions)).
- **Offsets / incremental:** yes, byte offsets with tree-sitter incremental reparse. However, its own README says it has "lots of inaccuracies" and is "not recommended … where correctness is important"; it is meant for highlighting.
- **What we'd take:** ideas only, or a fallback if we ever need incremental styling in (N).
- **Verdict:** right job (highlighting), but it pulls in the tree-sitter runtime plus two grammars (block and inline). Overkill for .md files that are typically small.

#### Sanitisers (for any HTML path)
- **ammonia** https://github.com/rust-ammonia/ammonia. MIT OR Apache-2.0, verified from [LICENSE-MIT](https://github.com/rust-ammonia/ammonia/blob/master/LICENSE-MIT) / [LICENSE-APACHE](https://github.com/rust-ammonia/ammonia/blob/master/LICENSE-APACHE). v4.2.4, 2026-10-17. html5ever-based whitelist sanitiser. comrak recommends it.
- **DOMPurify** https://github.com/cure53/DOMPurify. MPL-2.0 OR Apache-2.0: [LICENSE](https://github.com/cure53/DOMPurify/blob/main/LICENSE) is the Apache-2.0 text, [LICENSE-MPL](https://github.com/cure53/DOMPurify/blob/main/LICENSE-MPL) is the alternative, and package.json reads `(MPL-2.0 OR Apache-2.0)`. **Take it under Apache-2.0.** v3.4.17, 2026-10-12.

#### How the parser drives (1) light source styling and (2) the rendered view

**(N) Rust + Win32**
1. *Styling.* Debounce edits, re-parse the whole document with `pulldown_cmark::Parser::new_ext(src, opts).into_offset_iter()`, map events to style runs, and apply them to the editing control:
   - `Heading(level)`: bigger font.
   - `Strong`/`Emphasis`/`Strikethrough`: weight, style or strike.
   - `Code`/`CodeBlock`: monospace.
   - The delimiter remainder: dim colour.

   A full re-parse is the honest choice: none of the Rust parsers is incremental, and link reference definitions are document-global. If needed, cap styling to `.md` files under a size limit. Byte ranges map 1:1 onto a UTF-8 control (Scintilla). A UTF-16 control (RichEdit) needs a byte→UTF-16 index conversion. Add a small regex-free pass for bare URLs, because pulldown-cmark lacks GFM autolinks.
2. *Rendered view.* There are three real options:
   - **Own DirectWrite layout from the event stream.** Build one `IDWriteTextLayout` per block, apply `SetFontWeight/SetFontStyle/SetStrikethrough/SetUnderline/SetFontSize` on ranges, and draw quote bars, code backgrounds, rules, bullets and table grids with D2D. Use `HitTestPoint` for links. It has the smallest footprint and is fully themeable. The cost is tables, images and nested lists, which we write ourselves. Prior art to borrow patterns from (all permissive):
     - egui_commonmark's event→element mapping;
     - iced's `markdown::Item` model;
     - inlyne's own-layout renderer.
   - **RTF into a read-only RichEdit** (`MSFTEDIT_CLASS`, `EM_STREAMIN` with `SF_RTF`). Walk the events and emit RTF:
     - `\b`, `\i`, `\strike` and `\fs` for inline styles and heading sizes;
     - `\field{\*\fldinst HYPERLINK ...}` for links;
     - `\trowd…\cell…\row` for tables. Rich Edit 4.1+ (Msftedit.dll) added "wrapping within cells, improved handling via RTF" ([About Rich Edit](https://learn.microsoft.com/en-us/windows/win32/controls/about-rich-edit-controls)).

     It is cheap to build, uses a system DLL, and gives selection and copy for free. It looks "Word-ish": code blocks can only get paragraph shading, and images need `\pict`. A crates.io search found **no maintained markdown→RTF crate**, so we would write the emitter ourselves.
   - **Hybrid: create WebView2 only when the user flips to the rendered view.** Use comrak (safe HTML) or pulldown-cmark plus ammonia, then `NavigateToString`, which has a 2 MB limit and an `about:blank` origin ([ICoreWebView2::NavigateToString](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#navigatetostring)). Set `IsScriptEnabled = false`, as the docs recommend "when showing static HTML content" ([security](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security)). The runtime "will be included as part of the Windows 11 operating system" ([distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)). Costs from Microsoft's docs:
     - "each control launches multiple browser engine processes that add memory and startup overhead";
     - "on a cold launch, WebView2 must spin up its processes and disk caches";
     - "Don't use WebView2 for initial UI" ([performance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance));
     - the process group is one browser process, renderers, GPU and utility processes ([process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)).

     Microsoft publishes no MB figures. Rust binding: `webview2-com` 0.39.1 (MIT, [LICENSE](https://github.com/wravery/webview2-rs/blob/main/LICENSE), 2026-03-11), built on `windows-core` 0.62, which matches our `windows` version.

**(W) Tauri 2 / WebView2**
1. *Styling.* With CodeMirror 6, lezer markdown + GFM gives incremental, exact styling for free. With Monaco there is only a Monarch tokenizer (that is the other agent's area). Either way, no Rust parser is involved.
2. *Rendered view.* Either use markdown-it in JS + DOMPurify, or use comrak in a Rust command and return sanitised HTML. **Never inject untrusted HTML into the app's own document:** that page holds Tauri IPC. Render into a `sandbox` iframe without `allow-scripts`, or into a second webview with scripts disabled, and keep a strict CSP. Tauri's CSP "is only enabled if set on the Tauri configuration file" ([Tauri CSP](https://v2.tauri.app/security/csp/)).

#### Native markdown renderers in Rust (borrowable?)
- **egui_commonmark** https://github.com/lampsitter/egui_commonmark. MIT OR Apache-2.0, verified from [LICENSE-MIT](https://github.com/lampsitter/egui_commonmark/blob/master/LICENSE-MIT) and [LICENSE-APACHE](https://github.com/lampsitter/egui_commonmark/blob/master/LICENSE-APACHE). 0.26.0, 2026-10-15. It renders pulldown-cmark events into egui widgets and supports tables, strikethrough, task lists and footnotes. **Take:** the event→block/inline state machine in `egui_commonmark_backend/src/pulldown.rs` as a pattern. The egui types themselves are not usable without egui.
- **iced `markdown` widget** https://github.com/iced-rs/iced/blob/master/widget/src/markdown.rs. MIT, verified from [LICENSE](https://github.com/iced-rs/iced/blob/master/LICENSE). iced 0.14.0, 2025-12-07; last push 2026-10-21. Built on pulldown-cmark. `Content::push_str` re-parses only the last item (or the last bullet of a list), which is streaming-incremental for appends. **Take:** the `Item` model and the "re-parse only the tail block" idea.
- **inlyne** https://github.com/Inlyne-Project/inlyne. MIT, verified from [LICENSE](https://github.com/Inlyne-Project/inlyne/blob/main/LICENSE). Last push 2026-08-06. It is a "GPU powered yet browserless" viewer: comrak → HTML → html5ever → its own layout on wgpu/glyphon. **Take:** ideas for block layout. The wgpu stack is too heavy to reuse.
- **Verdict:** there is no drop-in Win32/DirectWrite renderer. Writing our own DirectWrite renderer means a couple of permissive projects to crib structure from, not code to vendor.

### Encoding detection

#### encoding_rs
- **Repo:** https://github.com/hsivonen/encoding_rs
- **Licence:** (Apache-2.0 OR MIT) AND BSD-3-Clause, verified from [COPYRIGHT](https://github.com/hsivonen/encoding_rs/blob/main/COPYRIGHT), [LICENSE-MIT](https://github.com/hsivonen/encoding_rs/blob/main/LICENSE-MIT), [LICENSE-APACHE](https://github.com/hsivonen/encoding_rs/blob/main/LICENSE-APACHE) and [LICENSE-WHATWG](https://github.com/hsivonen/encoding_rs/blob/main/LICENSE-WHATWG). The BSD-3 part covers the WHATWG-derived data tables.
- **Last activity:** 2026-09-24 (crates.io 0.8.42); last push 2026-10-12
- **What it does:** decodes and **encodes** all WHATWG encodings. `Encoding::for_bom` does BOM sniffing and `decode_with_bom_removal` decodes. `decode_without_bom_handling_and_without_replacement` returns `None` on malformed input, which is the strict check. UTF-8 validation delegates to `simdutf8` ([README](https://github.com/hsivonen/encoding_rs#readme)). Legacy encoders are "optimized for binary size rather than speed".
- **Round-trip gotchas:**
  - "you can't use this crate to produce output in UTF-16LE, UTF-16BE", because their output encoding is UTF-8 ([README "API surprises"](https://github.com/hsivonen/encoding_rs#api-surprises-to-watch-out-for)). **Write UTF-16 ourselves** with `str::encode_utf16()` + `to_le_bytes/to_be_bytes`.
  - `Encoding::encode` replaces unmappable characters "with HTML numeric character references" ([lib.rs](https://github.com/hsivonen/encoding_rs/blob/main/src/lib.rs)), which is wrong for a text editor. Use `Encoder::encode_from_utf8_without_replacement`. On `Unmappable`, offer "save as UTF-8" (classic Notepad behaviour).
  - If the strict decode of the detected encoding fails, open the file with a warning (or read-only). Otherwise saving silently turns bad bytes into U+FFFD.
- **Companion:** `codepage` crate (Apache-2.0 OR MIT, verified from [COPYRIGHT](https://github.com/hsivonen/codepage/blob/master/COPYRIGHT); 0.1.3, 2026-09-12) maps Windows code-page numbers (e.g. `GetACP()`) to `&'static Encoding`.
- **What we'd take:** the crate (both stacks; in (W) do I/O in Rust).
- **Verdict:** the foundation, with no alternative worth considering.

#### chardetng
- **Repo:** https://github.com/hsivonen/chardetng
- **Licence:** Apache-2.0 OR MIT, verified from [COPYRIGHT](https://github.com/hsivonen/chardetng/blob/main/COPYRIGHT), [LICENSE-MIT](https://github.com/hsivonen/chardetng/blob/main/LICENSE-MIT) and [LICENSE-APACHE](https://github.com/hsivonen/chardetng/blob/main/LICENSE-APACHE)
- **Last activity:** 2026-03-30 (crates.io 1.0.0, the last push)
- **What it does:** Firefox's legacy-encoding detector. It works mostly by negative matching: one encoding error or one C1 control disqualifies a candidate, and pair frequencies from Wikipedia score the rest. It does **not** detect UTF-16 ("belongs on the BOM layer"). UTF-8 is returned only if allowed via `Utf8Detection` ([README](https://github.com/hsivonen/chardetng#readme)). API: `EncodingDetector::new()`, `feed(bytes, last)`, `guess(tld, allow_utf8)`, `find_score(enc)`. The `tld` hint shifts the fallback, so we can derive it from the user's region.
- **Size/speed:** its binary contribution is "28% of what ced would have contributed", at the cost of being "42% longer to process" than ced ([write-up](https://hsivonen.fi/chardetng/)). It reuses encoding_rs decoders, so the marginal size is small. Firefox honours its UTF-8 result **only for `file:` URLs**, and guesses after the first 1024 bytes and again at end of stream.
- **What we'd take:** the crate, used only after the UTF-8 check fails.
- **Verdict:** use it.

#### charset-normalizer-rs
- **Repo:** https://github.com/nickspring/charset-normalizer-rs
- **Licence:** MIT, verified from https://github.com/nickspring/charset-normalizer-rs/blob/main/LICENSE. Cargo uses `license-file`, which is why crates.io shows "non-standard".
- **Last activity:** 2025-09-21 (release 1.1.0)
- **Size/speed:** its own table on 400+ files gives 97.1% accuracy at 1.5 ms/file, against chardetng's 90.7% at 1.6 ms ([README](https://github.com/nickspring/charset-normalizer-rs#-performance)). It depends on the old `encoding` crate, `regex`, `icu_normalizer/properties`, `serde_json`, `cached` and others ([Cargo.toml](https://github.com/nickspring/charset-normalizer-rs/blob/main/Cargo.toml)).
- **What we'd take:** ideas only.
- **Verdict:** more accurate on its own benchmark, but the dependency weight conflicts with "lightest possible". Pass.

#### uchardet
- **Repo:** https://gitlab.freedesktop.org/uchardet/uchardet
- **Licence:** MPL-1.1 / GPL-2.0+ / LGPL-2.1+ tri-licence, verified from [COPYING](https://gitlab.freedesktop.org/uchardet/uchardet/-/raw/master/COPYING) (the MPL 1.1 text) and the `MPL 1.1/GPL 2.0/LGPL 2.1` licence block in [src/uchardet.cpp](https://gitlab.freedesktop.org/uchardet/uchardet/-/raw/master/src/uchardet.cpp). **Not in the allowed list: ideas only unless the user decides otherwise.**
- **Last activity:** 2026-07-09 (GitLab `last_activity_at`); last tag v0.0.8, 2022-12-08
- **What we'd take:** ideas only.
- **Verdict:** this is the detector behind Notepad3 and (via its C# port UTF.Unknown) Notepads. chardetng does the same job permissively.

#### How other editors decide
- **Firefox (file:)**: BOM, then chardetng with UTF-8 allowed, because for file: URLs "the entire content can be assumed to be present up front" ([chardetng write-up](https://hsivonen.fi/chardetng/)).
- **Notepad3** ([EncodingDetection.cpp](https://github.com/rizonesoft/Notepad3/blob/master/src/uchardet/EncodingDetection.cpp); Notepad3 itself is BSD-3-Clause per [Build/Docs/License.txt](https://github.com/rizonesoft/Notepad3/blob/master/Build/Docs/License.txt), last push 2026-10-26). The order is:
  1. Forced encoding or `.nfo`→OEM.
  2. Encoding tag in file variables.
  3. UTF-32 BOM.
  4. One scan giving UTF-8 validity, pure-ASCII and NUL presence.
  5. BOM-less UTF-32, then UTF-16 by NUL-byte distribution, cross-checked with `IsTextUnicode`.
  6. uchardet on at most 200,000 bytes (a sample for larger files).
  7. Prefer **valid UTF-8** over an unreliable guess. The confidence bar is `AnalyzeReliableConfidenceLevel` = 60, then the ANSI default.
- **Notepads** ([FileSystemUtility.cs](https://github.com/0x7c13/Notepads/blob/master/src/Notepads/Utilities/FileSystemUtility.cs); MIT per [LICENSE.txt](https://github.com/0x7c13/Notepads/blob/master/LICENSE.txt); last commit on master 2026-03-29). The order is:
  1. BOM.
  2. UTF.Unknown, where ASCII is treated as UTF-8 and confidence >0.8 is accepted outright.
  3. Otherwise prefer a candidate that is UTF-8 or the system ANSI code page.
  4. Below 0.5 confidence, try strict UTF-8.
  5. On a `DecoderFallbackException`, fall back to the system ANSI code page.
- **Windows Notepad**: since Windows 10 build 18298 / 1903 it saves "UTF-8 without a Byte Order Mark" by default for new files and shows the encoding in the status bar ([Insider blog 2018-12-10](https://blogs.windows.com/windows-insider/2018/12/10/announcing-windows-10-insider-preview-build-18298/)). Classic Notepad guessed BOM-less UTF-16 with `IsTextUnicode`, whose statistical tests "are not foolproof" ([API docs](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-istextunicode); [Old New Thing](https://devblogs.microsoft.com/oldnewthing/20070417-00/?p=27223)). The current Windows 11 Notepad detection order is **not publicly documented**.

#### Recommended detection order (scrivi)
1. **BOM**: `Encoding::for_bom` (UTF-8, UTF-16LE/BE). Remember the BOM flag and write it back.
2. **NUL heuristic for BOM-less UTF-16**: NULs concentrated on even or odd positions. Do not rely on `IsTextUnicode` alone. Note that UTF-8 with stray NULs must stay UTF-8, as Notepad3 does.
3. **Strict UTF-8 validation over the whole file** (SIMD, cheap at 50 MB). Valid means UTF-8; pure ASCII also means UTF-8, since it is a subset and saves byte-identical.
4. **chardetng** over the whole buffer (`Utf8Detection::Deny`, TLD hint from the user's region).
5. **Fallback**: system ANSI code page (`GetACP()` → `codepage`). Accept chardetng's answer only if its strict decode succeeds; otherwise use ANSI.
6. Always provide a "Reopen with encoding…" menu. Show the encoding (and "with BOM") in the status bar, like Notepad.

### Line endings

- **Detect:** one byte scan counting CRLF, lone CR and lone LF, done after decoding for UTF-16. Use the **majority** as the document's mode and flag "mixed" if more than one count is non-zero. That is exactly Notepad3's `EditDetectEOLMode`, which notes that `'\r'`/`'\n'` are never trail bytes in any ASCII-compatible encoding ([Edit.c](https://github.com/rizonesoft/Notepad3/blob/master/src/Edit.c)). Notepad3 warns on inconsistent EOLs by default (`WarnInconsistEOLs = true`, `FixLineEndings = false`, [Config.cpp](https://github.com/rizonesoft/Notepad3/blob/master/src/Config/Config.cpp)).
- **Windows Notepad:** since 2018 it supports LF, CR and CRLF. New files are CRLF. Existing files keep "the file's current line ending format", and "the status bar indicates the detected EOL format". Paste/Enter behaviour can be set in `HKCU\Software\Microsoft\Notepad` ([devblog](https://devblogs.microsoft.com/commandline/extended-eol-in-notepad/)). How it handles *mixed* files is not documented.
- **Notepads:** picks one mode (any CRLF → CRLF, else CR, else LF), shows "Windows (CRLF) / Macintosh (CR) / Unix (LF)", and **normalises the whole text** to that mode ([LineEndingUtility.cs](https://github.com/0x7c13/Notepads/blob/master/src/Notepads/Utilities/LineEndingUtility.cs)). That is lossy for mixed files.
- **Preserving:** the cleanest approach is to keep the buffer's original bytes, as Scintilla does: mixed files stay mixed, Enter inserts the majority EOL, and paste converts to it. Offer an explicit "Convert line endings" command. If the control normalises internally (RichEdit uses `\r`), store a per-line EOL side-table or accept normalisation and say so in the status bar ("Mixed → CRLF on save").
- **Status bar:** use Notepad's wording ("Windows (CRLF)", "Unix (LF)", "Macintosh (CR)") plus "Mixed" when flagged. Clicking it converts.
- **Ropes vs CR:** `crop` recognises only LF and CRLF as line terminators ([README](https://github.com/nomad/crop#line-breaks)). Ropey's set is configurable at build time ([README](https://github.com/cessen/ropey#readme)). This matters for CR-only files.

### Large files

- **What numbers editors use:**
  - **VS Code:** tokenization off above **20 MB or 300K lines**; no syncing to extension host/workers above **50 MB**; heap-heavy operations refused above **256M chars** ([textModel.ts](https://github.com/microsoft/vscode/blob/main/src/vs/editor/common/model/textModel.ts)).
  - **Notepad3:** "load big file" warning above `FileLoadWarningMB` = **4 MB** default (0–2048; [Config.cpp](https://github.com/rizonesoft/Notepad3/blob/master/src/Config/Config.cpp), [Edit.c](https://github.com/rizonesoft/Notepad3/blob/master/src/Edit.c)). It switches to a "large file lexer", caches layout for the whole document only at ≤2 MB, refuses ≥2 GB, and reads the whole file into one buffer.
  - **Notepad++** (GPL, ideas only): no styling at **200 MB+** by default (`NPP_STYLING_FILESIZE_LIMIT_DEFAULT`, [NppConstants.h](https://github.com/notepad-plus-plus/notepad-plus-plus/blob/master/PowerEditor/src/MISC/Common/NppConstants.h); options in the [manual](https://npp-user-manual.org/docs/preferences/)).
  - **Notepads:** hard **1 MB** limit ("does not support file greater than 1MB", [FileSystemUtility.cs](https://github.com/0x7c13/Notepads/blob/master/src/Notepads/Utilities/FileSystemUtility.cs)).
  - **Windows Notepad:** no published limit. It got "performance improvements, especially when scrolling very large files" in 11.2205 ([Insider blog 2022-06-09](https://blogs.windows.com/windows-insider/2022/06/09/new-updates-for-notepad-and-media-player-for-windows-insiders/)).
  - **Sublime:** no official numbers found.
- **Line index, not line objects:** VS Code's old per-line objects turned a 35 MB / 13.7M-line file into about 600 MB ("roughly 20 times"), which is why they moved to a piece tree with line-start arrays per buffer ([VS Code blog](https://code.visualstudio.com/blogs/2018/03/23/text-buffer-reimplementation)). For scrivi, keep a `Vec<u32>` of line starts built with `memchr`, at 4 bytes/line. Build it on a background thread and paint the first screen immediately.
- **Buffer structures:**
  - **Gap buffer:** Scintilla's `SplitVector` is a gap buffer ([SplitVector.h](https://github.com/rizonesoft/Notepad3/blob/master/scintilla/src/SplitVector.h)). It is simple and great for localised edits.
  - **Piece table / piece tree:** VS Code uses one. It needs near-zero memory beyond the original and is ideal for a read-only original plus edits.
  - **Rope:** Ropey (MIT, [LICENSE.md](https://github.com/cessen/ropey/blob/master/LICENSE.md); 1.6.1, last push 2026-09-12) claims about 10% load overhead (100 MB → 110 MB) and 1.8M random inserts/s. `crop` (MIT, [LICENSE](https://github.com/nomad/crop/blob/main/LICENSE); 0.4.3) is 3–4× faster than Ropey on crdt traces and byte-indexed ([README](https://github.com/nomad/crop#speed)).

  Which one we use is decided by the edit control (another agent's section). If we own the buffer in (N), a byte-indexed rope or piece table fits a UTF-8 world.
- **Memory-mapped files:** good for the **read-only >50 MB** mode: no copy, and the OS pages data in ([File mapping](https://learn.microsoft.com/en-us/windows/win32/memory/file-mapping)). There are two caveats:
  - While a mapping is open, other processes **cannot truncate** the file; `SetEndOfFile` requires unmapping first ([SetEndOfFile](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setendoffile)). That bites log writers that rotate the file.
  - In Rust, `memmap2` (MIT OR Apache-2.0, [LICENSE-MIT](https://github.com/RazrFalcon/memmap2-rs/blob/master/LICENSE-MIT); 0.9.12, 2026-10-21) marks maps `unsafe` because of "Undefined Behavior … if the underlying file is subsequently modified, in or out of process".

  The alternative is **chunked `ReadFile` with a sparse index**: read 1–4 MB windows on demand, index every Nth line start, and never hold the whole file. It is safer and fine for viewing.
- **Encoding for large files:** UTF-8 files can be viewed in place. Legacy and UTF-16 files need a streaming decode (`Decoder::decode_to_utf8`) into a temp buffer or on demand per chunk. Simplest is to make >50 MB non-UTF-8 read-only and decode only the visible window.
- **Suggested policy:**
  - ≤50 MB: read fully and edit normally. For `.md`, turn off markdown styling above a smaller cap, e.g. matching Notepad3's 4 MB warning; we would tune this number ourselves.
  - >50 MB: read-only viewer with chunked or mmap reading, a background line index, and no styling or wrap.
  - Show the reason in the status bar.

#### Takeaways for the stack choice
- **Source styling is easier in (W):** CodeMirror 6 + `@lezer/markdown` is incremental and offset-exact out of the box. In (N), pulldown-cmark's `into_offset_iter` is good enough, but it means a full re-parse per debounce plus our own bare-URL autolink pass.
- **The rendered view is the real cost of (N).** We can write our own DirectWrite renderer (most work, lightest result, with patterns from egui_commonmark, iced and inlyne), emit RTF into a read-only RichEdit (least work, "Word-like" look, no crate exists), or lazy-load WebView2 only for preview (Microsoft documents a multi-process, cold-start cost, and `webview2-com` matches `windows` 0.62).
- **Encoding, EOL and large-file logic is stack-neutral Rust:** encoding_rs + chardetng + codepage, all permissive. In (W) it still belongs in the Rust side. Two traps: encoding_rs cannot write UTF-16, and `encode()` emits HTML entities for unmappables.
- **Any HTML path needs sanitising and isolation:** pulldown-cmark passes raw HTML through, while comrak and markdown-it are safe by default. Add ammonia or DOMPurify (Apache-2.0 side), and in (W) keep preview HTML out of the IPC-bearing document.
- **50 MB is generous by peer standards:** Notepad3 warns at 4 MB, VS Code drops tokenization at 20 MB, Notepads stops at 1 MB. Plan a read-only, chunked or mmap viewer above the threshold, and a background line index below it.

---

## 3. Windows integration

Research date 2026-10-26. Stacks: **(N)** Rust + raw Win32 (`windows` 0.62), **(W)** Tauri 2 / WebView2. Terms follow `notes/scrivi/GLOSSARY.md` (one **window**, many **tabs**, **disk change**).

What the local repos already solve:

- **Single instance.** In (N), `tasca/src/resident.rs` + `tasca/src/launch.rs` and `musica/src/ui.rs` + `musica/src/launch.rs` take a named mutex (`CreateMutexW`), then `FindWindowW` + `AllowSetForegroundWindow` + `PostMessageW` hand off to the window that is already running. Commands only, no payload. In (W), `annota/src-tauri/src/single_instance.rs` forwards the full argv with `WM_COPYDATA` in the exact format of `tauri-plugin-single-instance` 2.5.4 (pinned, covered by tests). `parla/src-tauri/src/single_instance.rs` uses a mutex plus an event.
- **HKCU registry writes.** `parla/src-tauri/src/autostart.rs` (`RegCreateKeyExW`/`RegSetValueExW`) and `annota/src-tauri/src/autostart.rs` (`RegSetKeyValueW`/`RegDeleteKeyValueW`). `annota/src-tauri/nsis-hooks.nsh` cleans up HKCU on uninstall. `annota/src-tauri/tauri.conf.json` already uses `"installMode": "currentUser"`.
- **Shell links.** `tasca/src/startup.rs` writes a `.lnk` with `IShellLinkW`. It sets no AppUserModelID yet.
- **Drag & drop of files.** `tasca/src/park/` (`OleInitialize`, `RegisterDragDrop`, `IDropTarget`, `CF_HDROP` read via `DragQueryFileW` in `park/hdrop.rs`).
- **DWM attributes.** `musica/src/ui.rs` (`DWMWA_BORDER_COLOR`, `DWMWA_WINDOW_CORNER_PREFERENCE`), `tasca/src/resident.rs` and `annota/src-tauri/src/overlay.rs` (`DWMWA_TRANSITIONS_FORCEDISABLED`).
- **Not solved anywhere locally:** dark title bar (`DWMWA_USE_IMMERSIVE_DARK_MODE`), dark menus, theme-change handling, AppUserModelID, jump lists, file associations, file watching, spell check. tasca and musica draw their own UI with Direct2D and never needed system theming.

Registry on this machine (Windows 11 25H2, build 26200, read with `reg query`):

- `HKCR\.txt` defaults to `txtfilelegacy`, which only has a `printto` verb. `PerceivedType=text`.
- `HKCU\...\Explorer\FileExts\.txt\UserChoice` (and `.log`) point to the Store Notepad ProgID `AppX4ztfk9wxr86nxmzzq47px0nh0e58b8fw`. That ProgID has `Shell\open` and `Shell\Edit` verbs, with `AppUserModelID=Microsoft.WindowsNotepad_8wekyb3d8bbwe!App`.
- `HKCR\SystemFileAssociations\text\shell\{open,edit}\command` is `%SystemRoot%\system32\NOTEPAD.EXE %1`.
- `batfile\shell\edit` and `regfile\shell\edit` hard-code `notepad.exe`.
- There is no `UserChoiceLatest` key yet, no `HKCR\.` key, and no `HKCR\.txt\ShellNew`.
- VS Code, Cursor and Zed appear only as `OpenWithProgids` entries.

---

### 1. File associations and default-app registration

**How it works.** Since Windows 8 the per-user default for an extension lives in `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.ext\UserChoice` as a `ProgId` plus a `Hash`. Windows ignores the entry if the hash is wrong. Microsoft now says this plainly: "Windows does not allow programmatic changes to default apps without user interaction in system UI", "User setting data for app defaults is obfuscated in registry data stores. Registry-based changes are not supported for apps", and the data is "protected by a Windows filter driver (UCPD.sys)" ([Windows app defaults platform, updated 2026-09](https://learn.microsoft.com/en-us/windows/apps/develop/windows-integration/default-apps-platform)). `IApplicationAssociationRegistration::SetAppAsDefault*` is deprecated from Windows 8 ([Default Programs](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs)).

UCPD.sys (March 2024) blocks registry writes from "untrusted" processes (`reg.exe`, `powershell.exe`, `regedit.exe`, `rundll32.exe`…) to protected keys. Those keys are http/https and the PDF/htm defaults ([Binary Ninja, 2025-03](https://binary.ninja/2025/03/25/default-browser-upcd.html); [FTA-Manager README](https://github.com/feiglein74/FTA-Manager)). **`.txt` is not in any published UCPD list**, but the UserChoice hash still applies to it.

**`UserChoiceLatest` (2025)** is a new `FileExts\.ext\UserChoiceLatest` key with a new hash that includes a machine ID ([kolbi.cz, 2025-04-20](https://kolbi.cz/blog/2025/04/20/userchoicelatest-microsofts-new-protection-for-file-type-associations/)):

- It is controlled by `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\SystemProtectedUserData\<SID>\AnyoneRead\AppDefaults\HashVersion`. At `1`, Windows ignores the old `UserChoice` entirely.
- It is A/B-rolled to Windows 11 Home/Pro (feature IDs 43229420 and 27623730). The author expects Windows 10 never to get it.
- This machine does not have it yet.
- The same blog reports (2026-07-23) that KB5121767 turns on extra UCPD features on Windows 11 Enterprise ([kolbi.cz/blog](https://kolbi.cz/blog/)). I found no newer hash change.

**Reverse-engineered tools: do not ship any of them.**

- **SetUserFTA** is commercial and closed. The free 1.8.3 "Personal Edition" is "for personal, non-commercial use" and nags. 2.x is quote-only ([setuserfta.com](https://setuserfta.com/)). Its author says it supports the new hash.
- **PS-SFTA** and **FTA-Manager** are open MIT PowerShell re-implementations of the *old* hash. FTA-Manager also documents disabling UCPD and a `regini.exe` bypass.
- Writing UserChoice from scrivi would break Microsoft's stated policy and Store policy 10.2.8. It would break again silently on every `UserChoiceLatest` machine, and it means circumventing a security driver. **Ideas only, and the idea is "don't".**

**The supported path is to register, then ask the user.**

1. **ProgID per type:** `HKCU\Software\Classes\Scrivi.txt` (and `Scrivi.log`, `Scrivi.md`…). Give each one `(Default)` = friendly type name, `DefaultIcon`, `AppUserModelID`, `shell\open\command` = `"…\scrivi.exe" "%1"`, **and `shell\edit\command`** (see "Edit" below) ([File Types](https://learn.microsoft.com/en-us/windows/win32/shell/fa-file-types); [AppUserModelIDs](https://learn.microsoft.com/en-us/windows/win32/shell/appids)).
2. **OpenWithProgids:** `HKCU\Software\Classes\.txt\OpenWithProgids\Scrivi.txt` (empty value). This puts scrivi in "Open with" and makes it a candidate default. **Never overwrite `.txt`'s `(Default)`**. "Whenever an application takes over this file type by changing the default value, it should also add an entry to this list" ([File Types](https://learn.microsoft.com/en-us/windows/win32/shell/fa-file-types)).
3. **Application key:** `HKCU\Software\Classes\Applications\scrivi.exe` with `shell\open\command`, `FriendlyAppName` and `SupportedTypes` (lists the extensions for the Open-with cascade) ([Application Registration](https://learn.microsoft.com/en-us/windows/win32/shell/app-registration)).
4. **Capabilities + RegisteredApplications:**
   - `HKCU\Software\Scrivi\Capabilities` holds `ApplicationName`, `ApplicationDescription` (required, or the app is hidden from the Default apps UI) and `FileAssociations\.txt = Scrivi.txt`, and so on.
   - `HKCU\Software\RegisteredApplications\Scrivi = Software\Scrivi\Capabilities`. `ApplicationName` must match this value name.
   - Source: [Default Programs](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs).
5. **Notify the shell:** `SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_DWORD|SHCNF_FLUSH, …)` ([Default Programs](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs)). Windows then "will automatically prompt the user when they open a file … when a new app is installed that registered for that file or link type". That prompt is the "Open with / Always" flyout, and it is the one-click route the OS gives us ([app defaults platform](https://learn.microsoft.com/en-us/windows/apps/develop/windows-integration/default-apps-platform)).
6. **In-app "Make scrivi the default" button** opens `ms-settings:defaultapps?registeredAppUser=Scrivi` (per-user install, value name from `HKCU\Software\RegisteredApplications`) or `registeredAppMachine=` (HKLM). It works on Windows 11 21H2/22H2 with the 2023-04 CU and on 23H2+ ([Launch the Default Apps settings page](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-default-apps-settings)). On that page the user still picks the app **per extension**. Microsoft documents the one-click "Set default" button only for browsers ([support article](https://support.microsoft.com/en-us/windows/change-default-programs-in-windows-e5d82cad-17d1-c53b-3505-f10a32e1894d)). I found **no** one-click "set as default for all text types" for non-browser apps.
7. **Check whether we are the default** with `IApplicationAssociationRegistration::QueryCurrentDefault` (still supported, per the platform page).

**Per-user (HKCU) vs per-machine.** Everything above works under HKCU, with no elevation. HKCR is a merged view and "the user-specific settings have priority" ([HKEY_CLASSES_ROOT](https://learn.microsoft.com/en-us/windows/win32/sysinfo/hkey-classes-root-key)). Per-user matches annota's NSIS `currentUser` mode and the `registeredAppUser` deep link. Admins can push defaults via the [ApplicationDefaults policy CSP](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-applicationdefaults) (DISM XML). That is not for a personal tool.

**Files with no extension.** There is no Microsoft doc for this. Community sources describe a `HKCR\.` key whose `(Default)` is a ProgID, plus `PerceivedType=text` for preview ([ElevenForum](https://www.elevenforum.com/t/explorer-options-for-files-without-extensions.47606/), [ITPro Today](https://itprotoday.com/windows-78/how-do-i-create-default-association-files-no-extension)). This machine has `HKCU\...\FileExts\.\OpenWithList`, so Explorer tracks extensionless files under the `.` pseudo-extension, and an "Always" choice should land in `FileExts\.\UserChoice`.

- Plan: `HKCU\Software\Classes\.\OpenWithProgids\Scrivi.txt`, plus listing `.` in `Capabilities\FileAssociations`.
- **Unverified:** whether Settings shows `.` as a choosable type. Test on a VM.

**Explorer "Edit" verb.** For `.txt`, "Edit" comes from the default ProgID's `shell\edit`, here Store Notepad's `Shell\Edit`. Types without their own edit verb fall back to `SystemFileAssociations\text\shell\edit` (`PerceivedType=text`, [Perceived Types](https://learn.microsoft.com/en-us/windows/win32/shell/fa-perceivedtypes)), which runs `system32\NOTEPAD.EXE`. `batfile`/`regfile` hard-code `notepad.exe`. On Windows 11 `system32\notepad.exe` is redirected to Store Notepad through an App execution alias ([winhelponline](https://www.winhelponline.com/blog/restore-old-classic-notepad-windows/), [Microsoft Q&A](https://learn.microsoft.com/en-us/answers/a/12540324)).

Options for scrivi, in order of preference:

- (a) Ship `shell\edit` on every scrivi ProgID. This covers types where scrivi is the default.
- (b) Write per-user overrides `HKCU\Software\Classes\SystemFileAssociations\text\shell\edit\command`, plus `batfile`/`cmdfile`/`regfile` `shell\edit\command`. These are ordinary keys, not UserChoice and not UCPD-protected, but they take over a system verb, so make them an explicit opt-in and remove them on uninstall.
- (c) Notepad3's "replace Notepad" trick: the `Image File Execution Options\notepad.exe` `Debugger` value, with a `/z` switch to swallow Windows' leading flag ([Notepad3 FAQ](https://github.com/rizonesoft/Notepad3/blob/master/readme/faq/FAQ.md)). It needs HKLM/admin and is MITRE ATT&CK [T1546.012](https://attack.mitre.org/techniques/T1546/012/), so EDR may flag it. Ideas only, not recommended.

**What Store Notepad does.** It is a packaged app. Its manifest-declared `uap:FileTypeAssociation`s become `AppX…` ProgIDs with `open` and `Edit` verbs and `ContractId=Windows.File`. The deep-link form for packaged apps is `registeredAUMID=`. Notepads (MSIX) shows the same pattern: `uap:FileTypeAssociation` groups, `uap4:ShellNewFileName` for "New > Text Document", `uap5:AppExecutionAlias` ([Package.appxmanifest](https://github.com/0x7c13/Notepads/blob/master/src/Notepads/Package.appxmanifest)). An MSIX or sparse package would give scrivi the same, but that adds signing and packaging to either stack.

**What Tauri's installers do for (W).** Both bundlers fall short of the steps above:

- **NSIS:** `bundle.fileAssociations` expands to `APP_ASSOCIATE` in [`FileAssociation.nsh`](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/FileAssociation.nsh), called from [`installer.nsi`](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi).
  - It **overwrites `Software\Classes\.ext` `(Default)`** with the ProgID (keeping a `_backup` value) and writes `shell\open` only.
  - No `OpenWithProgids`, no `Capabilities`, no `RegisteredApplications`, no `edit` verb.
  - The ProgID defaults to the bare extension name unless `name` is set.
  - It ends with `SHChangeNotify(SHCNE_ASSOCCHANGED)`.
  - For `.txt` that hijacks `txtfilelegacy` in HKCU, which is the behaviour Microsoft's doc tells installers to avoid.
- **WiX/MSI:** [`main.wxs`](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/msi/main.wxs) emits advertised `<ProgId>`/`<Extension>` with an `open` verb only.
- **Fix:** leave `fileAssociations` empty and write the registry in `installerHooks` (annota already uses `nsis-hooks.nsh`), or let the app register itself on first run (same code as (N)).

**How VS Code does it (MIT).** [`build/win32/code.iss`](https://github.com/microsoft/vscode/blob/main/build/win32/code.iss), `[Registry]` section:

- Each extension gets a `VSCode.<ext>` ProgID with `AppUserModelID`, `DefaultIcon`, `shell\open\command`.
- It adds an `OpenWithProgids` value and deletes its own old bare value.
- It writes `Applications\Code.exe`.
- Every key is `uninsdeletekey`/`uninsdeletevalue`.
- The root is `HKCU` when `PrivilegesRequired=lowest` (user setup), else `HKLM`.
- It writes **no** `Capabilities` or `RegisteredApplications`, so VS Code never appears as an app in Settings > Default apps. Scrivi wants that page, so it needs one step more than VS Code.

**Per stack:**
- **(N):** about 150 lines of HKCU `RegSetKeyValueW` (pattern in `annota/src-tauri/src/autostart.rs`) run by `scrivi.exe --register` / `--unregister`. Then `SHChangeNotify`, `ShellExecuteW("ms-settings:defaultapps?registeredAppUser=Scrivi")` and `QueryCurrentDefault`. No installer needed; a zip plus first-run registration works.
- **(W):** the same registry work, but either in NSIS hooks (NSIS script) or in Rust in the Tauri backend. Do **not** use `bundle.fileAssociations` for `.txt`. Neither stack can set the default itself; that limit applies equally to both.

#### vscode (build/win32/code.iss)
- **Repo:** https://github.com/microsoft/vscode
- **Licence:** MIT, verified from https://github.com/microsoft/vscode/blob/main/LICENSE.txt
- **Last activity:** 2026-10-26 (push to main); release 1.143.1 on 2026-10-22
- **What we'd take:** the `[Registry]` pattern in `build/win32/code.iss`: per-extension ProgID + `AppUserModelID` value + `OpenWithProgids` + `Applications\<exe>`, HKCU/HKLM switch, uninstall flags
- **Verdict:** best template for the ProgID half. Add `Capabilities` + `RegisteredApplications` ourselves.

#### Notepads (Package.appxmanifest)
- **Repo:** https://github.com/0x7c13/Notepads
- **Licence:** MIT, verified from https://github.com/0x7c13/Notepads/blob/master/LICENSE.txt
- **Last activity:** 2026-10-26 (push to master); release v1.5.6.0 on 2025-03-28
- **What we'd take:** the extension list and grouping in `src/Notepads/Package.appxmanifest` (about 215 `FileType` entries); the `uap4:ShellNew*` pattern, only if we ever package
- **Verdict:** a useful list of text extensions; the mechanism only applies to MSIX.

#### Notepad3 (replace-Notepad method)
- **Repo:** https://github.com/rizonesoft/Notepad3
- **Licence:** BSD-3-Clause (Notepad3/MiniPath part), verified from https://github.com/rizonesoft/Notepad3/blob/master/Build/Docs/License.txt. The same file lists bundled grepWin as GPL-3.0; do not touch that part.
- **Last activity:** 2026-10-26 (push to master); release 7.26.602.1 on 2026-06-02
- **What we'd take:** ideas only for IFEO (`readme/faq/FAQ.md`, `/z` switch)
- **Verdict:** IFEO is possible but admin-only and looks like malware persistence. Skip it.

#### PS-SFTA
- **Repo:** https://github.com/DanysysTeam/PS-SFTA
- **Licence:** MIT per README only; the repo has **no LICENSE file** (https://github.com/DanysysTeam/PS-SFTA/blob/master/README.md, "Usage is provided under the MIT License")
- **Last activity:** 2022-10-10 (push)
- **What we'd take:** nothing (old hash only, unsupported by Microsoft)
- **Verdict:** do not ship. It breaks policy and fails on `UserChoiceLatest`.

#### FTA-Manager
- **Repo:** https://github.com/feiglein74/FTA-Manager
- **Licence:** MIT, verified from https://github.com/feiglein74/FTA-Manager/blob/master/LICENSE
- **Last activity:** 2026-10-10 (push)
- **What we'd take:** nothing; reading only, to understand UCPD
- **Verdict:** do not ship; it includes UCPD-disabling and bypass code.

#### SetUserFTA
- **Repo:** none. Closed-source binaries at https://setuserfta.com/
- **Licence:** proprietary. The free edition is "personal, non-commercial use" (https://setuserfta.com/)
- **Last activity:** "© 2026", maintained 2.x line (no dates published)
- **What we'd take:** nothing
- **Verdict:** licence and policy risk. Excluded.

---

### 2. Jump lists (recent files)

**How it works.** The taskbar and Start jump list builds its **Recent** category from `SHAddToRecentDocs` calls. Explorer and the common file dialog call it for you; files opened another way (drag & drop, our own forwarding from a second launch, session restore) need an explicit `SHAddToRecentDocs(SHARD_PATHW | SHARD_APPIDINFO…)` ([Taskbar Extensions](https://learn.microsoft.com/en-us/windows/win32/shell/taskbar-extensions)). "Only file types for which an application is a registered handler appear in that application's Jump List" ([IObjectWithAppUserModelID](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-iobjectwithappusermodelid)). **So jump lists depend on the §1 ProgIDs; without them Recent stays empty.**

If scrivi sets an explicit AppUserModelID, the **same** ID must be on the process (`SetCurrentProcessExplicitAppUserModelID`), the Start-menu shortcut (`System.AppUserModel.ID`), every ProgID (`AppUserModelID` value) and any `ICustomDestinationList` ([AppUserModelIDs](https://learn.microsoft.com/en-us/windows/win32/shell/appids)). `ICustomDestinationList` is only needed for custom categories or Tasks. To keep Recent next to them, call `AppendKnownCategory(KDC_RECENT)` ([AppendKnownCategory](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-icustomdestinationlist-appendknowncategory)).

Notepad3 does the minimum: `SHAddToRecentDocs(SHARD_PATHW, path)` in `AddFilePathToRecentDocs` (`src/Config/Config.cpp`) and a configurable AppUserModelID (`src/Helpers.c`). Notepads has its jump list code commented out (`src/Notepads/App.xaml.cs`).

**Rust:** there is no maintained crate for *writing* jump lists (crates.io search 2026-10-26 found only forensic readers). Everything is in `windows` 0.62: `SHAddToRecentDocs`, `SetCurrentProcessExplicitAppUserModelID`, `ICustomDestinationList`/`DestinationList`, `IApplicationDestinations` (features `Win32_UI_Shell`, `Win32_System_Com`), checked in the local registry copy of `windows-0.62.2`.

**Tauri:** no jump list or recent-documents API. Issue [#9778 "Recent documents"](https://github.com/tauri-apps/tauri/issues/9778) has been open since 2024-05-14. The NSIS bundler does stamp an AppUserModelID on its shortcuts (`SetLnkAppUserModelId` in `installer.nsi`).

**Per stack:**
- **(N):** about 40 lines of `windows` crate COM on the UI thread, plus the AUMID on the shortcut (extend `tasca/src/startup.rs`'s `IShellLinkW` code with `IPropertyStore`).
- **(W):** the same Rust code in the Tauri backend. It is not blocked, just not provided, and WebView2 adds nothing.

#### Notepad3 (recent docs)
- **Repo:** https://github.com/rizonesoft/Notepad3
- **Licence:** BSD-3-Clause, verified from https://github.com/rizonesoft/Notepad3/blob/master/Build/Docs/License.txt
- **Last activity:** 2026-10-26 (push to master)
- **What we'd take:** the `AddFilePathToRecentDocs` / `SetWindowAppUserModelID` pattern (`src/Config/Config.cpp`, `src/Helpers.c`)
- **Verdict:** a small, clear reference; port it with attribution.

---

### 3. File-change watching (disk change)

**How it works.** There are three Win32 options:

- **`FindFirstChangeNotificationW` on the parent directory.** It "does not indicate the change that satisfied the wait condition", and "notifications may not be returned … for a remote file system" ([docs](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstchangenotificationw)).
- **`ReadDirectoryChangesW` on the parent directory.** It returns per-file actions (added, removed, modified, renamed old/new). On overflow it returns success with 0 bytes, meaning "you should compute the changes by enumerating". It fails with `ERROR_INVALID_PARAMETER` if the buffer is over 64 KB on a network share, and with `ERROR_INVALID_FUNCTION` if the redirector does not support it. Size and last-write changes are seen only once the cache flushes ([docs](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw)).
- **Polling** `GetFileAttributesExW`, comparing size and mtime.

**Pitfalls:**

- Agents and editors often save atomically: write a temp file, then rename over the target, or delete and create. A watcher sees remove + create/rename, and a naive editor closes the tab.
- One save often fires 2–3 modify events.
- Our own saves echo back to us.
- Watching a single file always means watching its directory.

**What the editors do:**
- **Notepad3** (`src/Notepad3.c`):
  - `FileChangeObserver` runs `FindFirstChangeNotificationW` on the directory in a worker thread.
  - `RaiseFlagIfCurrentFileChanged` then compares size + last-write time.
  - A poll timer runs alongside (`FileCheckInterval`, push/poll/both methods).
  - A 100 ms `AtomicSaveTimerProc` re-checks after a delete before declaring the file gone.
- **Notepads:** polls only, every 6 s, comparing `DateModified` (`TextEditor.xaml.cs`, `_fileStatusCheckerPollingRateInSec = 6`).
- **VS Code** (non-recursive watcher, `src/vs/platform/files/node/watcher/nodejs/nodejsWatcherLib.ts`):
  - It uses `fs.watch` (libuv → `ReadDirectoryChangesW`).
  - "A delay in reacting to file deletes to support atomic save operations" (`FILE_DELETE_HANDLER_DELAY = 100`).
  - It aggregates changes over 75 ms and throttles bursts.

**Recommended design (either stack):**

- Watch the **directory** with `ReadDirectoryChangesW` and filter to the open files.
- Debounce about 100 ms.
- On any event or rescan, **re-stat** the file (size + mtime, optionally a content hash) and compare with what scrivi last loaded or saved. That one comparison drops duplicates and our own echoes, and makes delete-then-recreate look like a modify.
- Add a slow poll (2–5 s) as a backstop for SMB/WebDAV/OneDrive paths.

**`notify` crate:**

- [`notify`](https://crates.io/crates/notify) 8.2.1 (2026-10-25) is **CC0-1.0**; `notify-debouncer-full`/`-mini`, `notify-types` and `file-id` are **MIT OR Apache-2.0**.
- Its Windows backend (`notify/src/windows.rs`) is overlapped `ReadDirectoryChangesW` with a 16 KB buffer, so it stays under the 64 KB network limit.
- It emulates single-file watches through the parent directory and reports overflow as `Flag::Rescan`.
- `notify-debouncer-full` pairs rename events through `file-id`.
- CC0 is permissive (public-domain dedication) but grants no patent licence. It is acceptable under our rule.

**Per stack:**
- **(N):** use `notify` + `notify-debouncer-full`, or hand-roll about 150 lines of `ReadDirectoryChangesW` modelled on notify. Post to the UI thread with `PostMessageW`.
- **(W):** the same Rust crate in the backend, emitting a Tauri event to the frontend. Tauri's `fs` plugin `watch` (feature `watch`) wraps `notify` too, but backend Rust gives control over the debounce and re-stat. No real difference between stacks.

#### notify
- **Repo:** https://github.com/notify-rs/notify
- **Licence:** CC0-1.0 (`notify`), verified from https://github.com/notify-rs/notify/blob/main/notify/LICENSE-CC0. MIT OR Apache-2.0 for the debouncers and `file-id`, verified from https://github.com/notify-rs/notify/blob/main/notify-debouncer-full/LICENSE-MIT
- **Last activity:** 2026-10-26 (push); release notify 8.2.1 on 2026-10-25 (9.0.0-rc.6 on 2026-10-17)
- **What we'd take:** the crates as dependencies (`notify`, `notify-debouncer-full`)
- **Verdict:** use it; it already handles buffer size, rescan and rename pairing.

#### Notepad3 (file watching)
- **Repo:** https://github.com/rizonesoft/Notepad3
- **Licence:** BSD-3-Clause, verified from https://github.com/rizonesoft/Notepad3/blob/master/Build/Docs/License.txt
- **Last activity:** 2026-10-26 (push)
- **What we'd take:** the push + poll + atomic-save-timer state machine (`FileChangeObserver`, `RaiseFlagIfCurrentFileChanged`, `AtomicSaveTimerProc` in `src/Notepad3.c`)
- **Verdict:** a good behaviour reference for disk-change UX, including a log-tail mode.

#### vscode (nodejs watcher)
- **Repo:** https://github.com/microsoft/vscode
- **Licence:** MIT, verified from https://github.com/microsoft/vscode/blob/main/LICENSE.txt
- **Last activity:** 2026-10-26 (push)
- **What we'd take:** the timing constants and the delete-delay idea (`nodejsWatcherLib.ts`)
- **Verdict:** confirms the 75–100 ms debounce plus delayed delete.

---

### 4. Spell check

**How it works (native).**

- **API:** `ISpellCheckerFactory` → `CreateSpellChecker(lang)` → `ISpellChecker::Check`/`ComprehensiveCheck`. It returns `IEnumSpellingError` with start index, length and corrective action, plus `Suggest`, `Add`, `Ignore` and a `SpellCheckerChanged` event. It is Windows 8+, uses the user's installed Windows languages and runs in-process ([ISpellChecker](https://learn.microsoft.com/en-us/windows/win32/api/spellcheck/nn-spellcheck-ispellchecker), [ISpellCheckerFactory](https://learn.microsoft.com/en-us/windows/win32/api/spellcheck/nn-spellcheck-ispellcheckerfactory)).
- **Rust bindings:** all present in `windows` 0.62 under `Win32::Globalization` (`ISpellCheckerFactory`, `ISpellChecker2`, `SpellCheckerFactory` CLSID).
- **Underlining in a custom control:** run `Check` per visible line (debounced), keep ranges, and draw a squiggle under each range at paint time. With DirectWrite (as tasca/musica draw), that means a path from `IDWriteTextLayout::HitTestTextRange` rectangles. The right-click menu shows `Suggest()` results.
- **Shortcut if the edit surface is a RichEdit control:** `EM_SETLANGOPTIONS` with `IMF_SPELLCHECKING` makes RichEdit (Windows 8+) underline and suggest by itself ([EM_GETLANGOPTIONS flags](https://learn.microsoft.com/en-us/windows/win32/controls/em-getlangoptions)).

**How it works (WebView2).** Chromium spellcheck runs on `<textarea>`/`contenteditable` with `spellcheck="true"` and shows suggestions in the default context menu. Control from the host is weak:

- The spellcheck language follows the WebView2 environment `Language`, not the HTML `lang` attribute ([WebView2Feedback #5294](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5294), open).
- There is no API to choose languages or toggle the engine. "Spellchecking languages other than English" ([#2040](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2040)) has been backlog since 2022. [#3758](https://github.com/MicrosoftEdge/WebView2Feedback/issues/3758) was closed as a duplicate. "Add option to change spellchecking method" ([#4840](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4840)) is still open.
- Spellcheck is skipped after programmatic focus ([#5728](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5728), open 2026-10).
- A custom context menu loses the suggestions ([#2340](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2340)).

**CodeMirror 6:** native spellcheck is opted into with `EditorView.contentAttributes.of({spellcheck: "true"})`. The maintainer says it works ("spell-checks content inside CodeMirror as expected"). A September 2026 report that it never runs under Electron 44 / Chrome 152 is unresolved ([discuss.codemirror.net](https://discuss.codemirror.net/t/native-spellcheck-is-never-applied-to-a-codemirror-6-editing-host-on-chromium-152-electron-44/9886)). Large documents virtualise lines, so only visible text is checked, which is fine for an editor.

**Per stack:**
- **(N):** about 300 lines: `ISpellChecker` + squiggle painting + suggestion menu. Full control (per-file language, markdown-aware skipping of code spans) and the same dictionaries as Windows.
- **(W):** zero code for basic English underlines in a `textarea` or CM6. Per-file or multi-language control effectively needs a Rust command calling `ISpellChecker` and CM6 decorations. That costs the same as (N), plus IPC.

(No borrowable repo: the API is small, and the Microsoft sample is the doc.)

---

### 5. Dark mode, menus, Mica

**How it works.**

- **Reading the theme:** `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme` (DWORD 0 = dark; 0 on this machine). The documented alternative is `UISettings.GetColorValue(Foreground)` + `ColorValuesChanged`. On change, top-level windows get `WM_SETTINGCHANGE` with `lParam = "ImmersiveColorSet"` ([Support Dark and Light themes in Win32 apps](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-windows-themes)).
- **Title bar:** `DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE = 20, &TRUE)`, documented "starting with Windows 11 Build 22000" ([DWMWINDOWATTRIBUTE source](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/dwmapi/ne-dwmapi-dwmwindowattribute.md)). Windows 10 builds before 18985 need the undocumented value **19**. tao switches at `build > 18985` and forces a caption repaint with a `WM_NCACTIVATE` pair (`src/platform_impl/windows/dark_mode.rs`).
- **Menus and common controls** have no documented dark mode. The known path uses uxtheme **ordinals** ([ysc3839 DarkMode.h](https://github.com/ysc3839/win32-darkmode/blob/master/win32-darkmode/DarkMode.h)):
  - 135 = `SetPreferredAppMode` (1903+), `AllowDarkModeForApp` in 1809
  - 133 = `AllowDarkModeForWindow`
  - 136 = `FlushMenuThemes`
  - 104 = `RefreshImmersiveColorPolicyState`
  - 132 = `ShouldAppsUseDarkMode`
  - plus `SetWindowTheme(hwnd, L"DarkMode_Explorer", …)` on controls.
  - **Popup and context menus** go dark after `SetPreferredAppMode(AllowDark)` + `FlushMenuThemes`.
  - **The menu *bar* stays light.** You owner-draw it by handling the undocumented `WM_UAHDRAWMENU` (0x91) / `WM_UAHDRAWMENUITEM` (0x92) and painting over the 1-px line below it ([adzm](https://github.com/adzm/win32-custom-menubar-aero-theme)).
  - **Pitfall:** the original win32-darkmode only enables itself on a hard-coded build list (17763/18362/18363/19041), so it is off on Windows 11 as shipped. Ports relax that check. These are ordinals, so any Windows update could break them; always fall back to light.
- **Mica:** `DWMWA_SYSTEMBACKDROP_TYPE` (38) = `DWMSBT_MAINWINDOW` (Mica) / `DWMSBT_TABBEDWINDOW` (Mica Alt), from Windows 11 build 22621 ([DWM_SYSTEMBACKDROP_TYPE](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/dwmapi/ne-dwmapi-dwm_systembackdrop_type.md)). The client area must be transparent or extended into the frame for it to show, which fights a GDI or opaque-D2D editor surface. The user also "hates AI animations" and prefers static art, so Mica is optional at best.

**Tauri/WebView2.**

- tao (Tauri's window layer) already implements the ordinal + DWM dance and follows the system theme for the title bar: `try_theme`, `ShouldAppsUseDarkMode`, the `AppsUseLightTheme` fallback, attribute 19/20.
- wry sets WebView2 `PreferredColorScheme` from the window theme (`set_theme` in `src/webview2/mod.rs`), so CSS `prefers-color-scheme` follows Windows and Tauri emits theme-changed events.
- muda (Tauri's menu crate) owner-draws a dark menu bar with the UAH messages (`src/platform_impl/windows/dark_menu_bar.rs`).
- Mica is `windowEffects` / [window-vibrancy](https://github.com/tauri-apps/window-vibrancy) `apply_mica`.
- So in (W) dark mode is close to free, and most of the editor UI is HTML anyway.

**Per stack:**
- **(N):** port about 200 lines: tao's `dark_mode.rs` logic, the uxtheme ordinals, the UAH menu-bar painting from adzm/muda, and `WM_SETTINGCHANGE` handling. Popup menus come free once `SetPreferredAppMode` runs. If scrivi skips a classic `HMENU` menu bar entirely (custom-drawn chrome, like musica), only the title bar and context menus matter, which cuts this roughly in half.
- **(W):** built in. Set `theme: null` (follow the system) and style the page with `prefers-color-scheme`.

#### tao (dark_mode.rs)
- **Repo:** https://github.com/tauri-apps/tao
- **Licence:** Apache-2.0, verified from https://github.com/tauri-apps/tao/blob/dev/LICENSE (file header: "Copyright 2014-2021 The winit contributors / 2021-2023 Tauri Programme")
- **Last activity:** 2026-10-23 (push); crate 0.37.2 on 2026-10-23
- **What we'd take:** `src/platform_impl/windows/dark_mode.rs`: build gates, ordinal loading via `GetProcAddress(MAKEINTRESOURCE)`, the 19/20 switch, the `WM_NCACTIVATE` refresh, high-contrast check. It is already Rust on the `windows` crate.
- **Verdict:** best source for (N). Keep the Apache-2.0 NOTICE and attribution.

#### muda (dark_menu_bar.rs)
- **Repo:** https://github.com/tauri-apps/muda
- **Licence:** Apache-2.0 OR MIT, verified from https://github.com/tauri-apps/muda/blob/dev/LICENSE-MIT and https://github.com/tauri-apps/muda/blob/dev/LICENSE.spdx
- **Last activity:** 2026-10-25 (push); crate 0.21.2 on 2026-10-08
- **What we'd take:** the UAH message constants and drawing approach in `src/platform_impl/windows/dark_menu_bar.rs` (Rust, `windows-sys`)
- **Verdict:** usable, but **provenance flag**: its header says it ports "hrydgard/ppsspp … UAHMenuBar.cpp" and PPSSPP is GPL-2.0+ (its README: "Originally released under the GPL 2.0 (and later)"). Re-derive the menu-bar code from adzm (MIT) instead and use muda as a cross-check only.

#### win32-darkmode
- **Repo:** https://github.com/ysc3839/win32-darkmode
- **Licence:** MIT, verified from https://github.com/ysc3839/win32-darkmode/blob/master/LICENSE (the repo also carries a PolyHook_2_0 licence for the IAT-hook helper)
- **Last activity:** 2023-03-28 (push; no releases)
- **What we'd take:** the ordinal table in `win32-darkmode/DarkMode.h`, the `ListViewUtil.h` header colours, the `SetWindowTheme("DarkMode_Explorer")` usage
- **Verdict:** the canonical reference. Drop the build whitelist and the IAT hook (only needed for scrollbars).

#### win32-custom-menubar-aero-theme
- **Repo:** https://github.com/adzm/win32-custom-menubar-aero-theme
- **Licence:** MIT, verified from https://github.com/adzm/win32-custom-menubar-aero-theme/blob/main/LICENSE
- **Last activity:** 2025-01-15 (push; no releases)
- **What we'd take:** `UAHMenuBar.h` structs and `UAHMenuBar.cpp` handling of `WM_UAHDRAWMENU` / `WM_UAHDRAWMENUITEM` / the bottom-line overpaint
- **Verdict:** the clean MIT source for a dark menu bar in (N).

#### darkmodelib
- **Repo:** https://github.com/ozone10/darkmodelib
- **Licence:** MPL-2.0 (main), verified from https://github.com/ozone10/darkmodelib/blob/main/LICENSE.md. Some files are MIT (https://github.com/ozone10/darkmodelib/blob/main/LICENSE-MIT.md); check each header.
- **Last activity:** 2026-10-10 (push)
- **What we'd take:** ideas only (control-by-control dark theming; it is the Notepad++ dark-mode lineage). MPL-2.0 is file-level copyleft, outside our allowed list.
- **Verdict:** read to find out which controls need what; do not copy.

#### window-vibrancy
- **Repo:** https://github.com/tauri-apps/window-vibrancy
- **Licence:** Apache-2.0 OR MIT, verified from https://github.com/tauri-apps/window-vibrancy/blob/dev/LICENSE.spdx (LICENSE-MIT / LICENSE-APACHE present)
- **Last activity:** 2026-10-15 (push); release v0.8.1 on 2026-09-22
- **What we'd take:** `apply_mica` in `src/windows.rs` (attribute 38 on 22621+, undocumented 1029 `DWMWA_MICA_EFFECT` on 22000)
- **Verdict:** a one-call dependency in either stack if Mica is ever wanted.

---

### 6. Other integration: single instance, CLI, drop, Open with, ShellNew

**Single instance + tabs.** Explorer launches legacy command verbs with the `Document` multi-select model: **one process per selected file, up to 15** ([verb selection model](https://learn.microsoft.com/en-us/windows/win32/shell/how-to-employ-the-verb-selection-model)). A one-window/many-tabs design therefore *must* forward each launch to the running window.

- **(N):** extend the tasca/musica pattern (mutex + `FindWindowW`) with annota's `WM_COPYDATA` payload (cwd + argv; `SendMessageTimeoutW`, then `AllowSetForegroundWindow(pid)`). Startup cost is a bare Win32 exe, so a second launch can find the window and exit in a few ms.
- **(W):** `tauri-plugin-single-instance` (2.5.4, MIT/Apache, [plugins-workspace](https://github.com/tauri-apps/plugins-workspace)) delivers `argv` + `cwd` to a callback. annota shows the trap: the second launch still initialises the Tauri runtime before forwarding, unless you add a hand-written `FindWindowW` fast path as `annota/src-tauri/src/single_instance.rs` does.

**Command line `scrivi.exe file.txt`.** Both stacks read `std::env::args_os()`. Quote `"%1"` in every verb. Handle Notepad-style flags (`/p`, `/pt`), because `txtfilelegacy\shell\printto` and IFEO-style launches pass them (Notepad3's `/z` exists for that reason).

**Drag & drop onto the window.**
- **(N):** reuse `tasca/src/park/` (`IDropTarget` + `CF_HDROP`). `DragAcceptFiles` + `WM_DROPFILES` is the 10-line minimum.
- **(W):** wry turns WebView2's own external drop off (`SetAllowExternalDrop(false)`) and registers a native drop target (`src/webview2/drag_drop.rs`). Tauri emits drag-drop events with full paths, so this works. HTML5 `drop` events in the page do not get paths.

**"Open with" list.** `OpenWithProgids` per extension plus `Applications\scrivi.exe\SupportedTypes` (§1). The two stacks are the same here.

**Explorer "New > Text Document" (ShellNew).** It comes from `HKCR\.txt\ShellNew` (`NullFile`). Tauri's NSIS `APP_ASSOCIATE_EX` writes exactly that ([FileAssociation.nsh](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/FileAssociation.nsh)). On this machine `.txt` has **no** `ShellNew` key, and `.txt` is absent from the `Discardable\PostSetup\ShellNew\Classes` cache, so the entry presumably comes from the Store Notepad package (as `uap4:ShellNewFileName` does for Notepads). **Unverified:** whether "Text Document" vanishes if Store Notepad is removed. Scrivi could add `HKCU\Software\Classes\.txt\ShellNew\NullFile` as an opt-in, and the new file then opens in whatever the `.txt` default is.

**Per stack:** every item here is plain Win32 or registry work that (W) also has to do in Rust. The two differences:
- **Second-launch latency:** (N) is faster unless (W) copies annota's fast path.
- **Drag & drop:** comes free in (W).

---

#### Takeaways for the stack choice

- **Default-app status is the same in both stacks:** nobody can set it programmatically (hash + UCPD + `UserChoiceLatest`). Scrivi registers ProgIDs/Capabilities in HKCU, then sends the user to `ms-settings:defaultapps?registeredAppUser=Scrivi` or relies on the OS "new app" prompt. Don't ship SetUserFTA-style code.
- **Tauri's installer associations are a liability for `.txt`:** they overwrite the extension's default ProgID and skip `OpenWithProgids`/`Capabilities`. (W) must hand-write the registry anyway, so the "installer does it for you" advantage is gone.
- **Jump lists, file watching (`notify`), spell check (`ISpellChecker`) and single instance are Rust/Win32 code in both stacks.** Tauri provides none of them except single instance, and WebView2's spellcheck is uncontrollable beyond defaults.
- **Dark mode is where (W) wins clearly:** tao/wry/muda already do title bar, menus and `prefers-color-scheme`. In (N) it is about 200 lines of undocumented-ordinal code ported from tao/adzm (Apache/MIT), with breakage risk on Windows updates. Skipping a classic menu bar shrinks it a lot.
- **(N) wins on "replace Notepad" feel:** a tiny exe that forwards to the running window in milliseconds when Explorer spawns up to 15 processes at once. (W) needs annota's fast-path hack to match.
- **Gaps to verify on a VM before committing:** extensionless files via `.` in Settings, the Edit-verb override, ShellNew without Store Notepad, and behaviour once `UserChoiceLatest` reaches this machine.

---

## 4. Weight per stack: exe size, start time, idle memory

### MDHero

- **Who makes it:** Vaibhav Kakde, an individual developer ("Made by Vaibhav Kakde", [mdhero.app](https://mdhero.app)). It is open source under MIT: [github.com/vaibhav-kakde-in/mdhero](https://github.com/vaibhav-kakde-in/mdhero), repo created 2026-04-08, about 300 stars. The latest release is v0.2.10 (2026-09-13); the installed copy here is 0.2.7.
- **Stack:** Tauri 2 (Rust) with a SvelteKit / Svelte 5 frontend, built with Vite and Tailwind 4 ([package.json](https://github.com/vaibhav-kakde-in/mdhero/blob/main/package.json), [README "Stack"](https://github.com/vaibhav-kakde-in/mdhero#stack)). The Rust side uses tauri plus the opener, dialog, cli, process, window-state, updater and single-instance plugins, and `notify` for file watching ([Cargo.toml](https://github.com/vaibhav-kakde-in/mdhero/blob/main/src-tauri/Cargo.toml)).
- **Rendering bundle:** markdown-it (with anchor, task-lists and texmath), highlight.js, KaTeX, Mermaid 11, DOMPurify and mark.js. Mermaid and KaTeX are large, and they are most of why the frontend is heavy.
- **The editor is not CodeMirror or Monaco.** It is a plain `<textarea>`. Line numbers and markdown colouring are painted in a mirrored backdrop behind transparent textarea text ([Editor.svelte](https://github.com/vaibhav-kakde-in/mdhero/blob/main/src/lib/components/Editor.svelte)). The project describes itself as a viewer first, with editing added in v0.2.0.
- **Why the exe is 16 MB when the site says "~8MB":** the "~8MB" is the approximate install footprint for macOS and the installers. The x64 NSIS setup for 0.2.7 is 5.3 MB ([releases](https://github.com/vaibhav-kakde-in/mdhero/releases)). The Windows exe is 16,200,192 B. That exe embeds the brotli-compressed frontend, including Mermaid and KaTeX. `Cargo.toml` also has **no `[profile.release]` size settings** (no lto, opt-level "s", strip or panic=abort), so the Rust side is not size-tuned either.
- **What it feels like at startup (measured below):** the native Tauri frame appears in about 50–130 ms, empty. The file's content and title arrive 1.4–2.2 s later.

### Measured locally

| App | Stack | Exe size | First visible window (median of 3) | File shown in title (median of 3) | Idle private working set, process tree | Idle private bytes (commit), process tree | Child processes |
|---|---|---|---|---|---|---|---|
| **MDHero 0.2.7** | Tauri 2 / WebView2, Svelte | 16,200,192 B | 67 ms (126 / 67 / 50) | **2,060 ms** (2060 / 2214 / 1440) | **224 MB** (host 4.6 MB) | 288 MB | **6** × msedgewebview2 |
| **Windows Notepad 11.2607.14.0** | WinUI/XAML + Office RichEdit (packaged) | Notepad.exe 3.3 MB (package 18.9 MB) | 733 ms (1875 / 577 / 733) | **1,520 ms** (2485 / 1447 / 1520) | **87 MB** | 109 MB | 0 |
| annota (already running, window hidden) | Tauri 2 / WebView2, vanilla TS | 3,414,016 B installed (3,416,576 B in target/) | n/a (tray) | n/a | 40 MB (host 4.1 MB) | 140 MB | 6 × msedgewebview2 |
| tasca (already running, overlay hidden) | Rust raw Win32 + Direct2D/DirectWrite | 532,992 B installed (607,744 B in target/) | n/a (tray) | n/a | **4.1 MB** | 8.3 MB | 0 |
| musica (already running) | Rust raw Win32 (opt-level "s") | 461,312 B | n/a (tray) | n/a | **1.9 MB** host; 21 MB with mpv | 2.5 MB host; 47 MB with mpv | 1 × mpv.exe (audio engine) |
| *(not measured)* parla | Tauri 2 plus ONNX runtime and models | 41 MB | | | | | |

**Method.** The script lives in `%TEMP%\scrivi-weight\`. It launched the exe with a small `test.md` (headings, list, code block, table) as its argument. Two timers ran from just before `Start-Process`: one until the app's first visible top-level window of at least 200×200 px, and one until a visible window's title contained `test.md`. Both were polled every 5 ms via `EnumWindows`. The title marker is the fairer "ready" signal for MDHero, because Tauri shows an empty native frame long before the webview has rendered. After 10 s of idle, the script summed `WorkingSetPrivate` and `PrivateBytes` from `Win32_PerfRawData_PerfProc_Process` over the process and all its descendants. It then closed the app gracefully with WM_CLOSE (no force-kills were needed in the final series) and waited 3 s between runs. Run 1 of each app is the cold one. annota, tasca and musica were already running, so the script sampled them in place 3 times, 5 s apart; the values did not change. Nothing was started for these three and nothing was killed, so they have no start time. Earlier test series of MDHero gave the same memory, 212–233 MB private working set.

**Machine.** Intel Core i7-1370P (14 cores / 20 threads), 64 GB RAM, Windows 11 Pro 25H2, build 26200.9711. WebView2 Runtime 155.0.4283.62 (Evergreen). Other WebView2 apps were running at the same time; their processes are not in the sums.

**Caveats.** Plain working set includes DLL pages shared with other processes, which overcounts WebView2 (MDHero's tree shows 490 MB plain working set), so the table uses private working set. annota's 40 MB is a hidden webview with `MemoryUsageTargetLevel = Low` set by its own code (`annota/src-tauri/src/editor.rs`), so it is not a visible-window figure. Its commit (140 MB) is closer to the real floor. The timings are wall-clock on a fast laptop with warm disk caches after run 1.

### Published numbers

- **Tauri's own claim:** "a minimal Tauri app can be less than 600KB in size" ([Tauri docs, What is Tauri](https://v2.tauri.app/start/)). The page gives no method. The size page recommends `codegen-units = 1`, `lto = true`, `opt-level = "s"`, `panic = "abort"`, `strip = true` ([App size](https://v2.tauri.app/concept/size/)).
- **Tauri's own CI benchmark on a Windows runner, 2026-10-26** ([tauri-recent-windows.json](https://github.com/tauri-apps/benchmark_results/blob/main/tauri-recent-windows.json), [wry-recent-windows.json](https://github.com/tauri-apps/benchmark_results/blob/main/wry-recent-windows.json); primary, raw JSON):
  - Size: Tauri hello-world binary is **2,822,656 B**; a bare wry (webview, no Tauri) hello-world is **344,576 B**.
  - Start time: mean time to DOMContentLoaded is **0.588 s** for Tauri hello-world and **0.525 s** for wry, on a GitHub Actions VM.
  - Memory: not reported for Windows (the `max_memory` field is empty). The README warns that "results from different OS' are not directly comparable".
- **WebView2 process model:** every WebView2 app gets "a single browser process", "one or more renderer processes" and helpers such as GPU and audio. The process group belongs to one user data folder, and separate apps only share processes if they deliberately share a user data folder ([MS Learn, Process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)). This matches the 6 children measured for both MDHero and annota.
- **Microsoft on WebView2 cost:**
  - "each control launches multiple browser engine processes that add memory and startup overhead". Microsoft advises "Don't use WebView2 for initial UI … use lightweight XAML or Win32 screens" ([MS Learn, Performance best practices](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)).
  - The same page says startup is faster when Edge is running at the same version, because "the required WebView2 binaries are already in memory".
- **Memory trimming:**
  - Setting `MemoryUsageTargetLevel` to Low is "best effort". Memory "could potentially … be swapped out to disk", and the app must set it back to Normal itself ([ICoreWebView2_19](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_19)).
  - `TrySuspend` goes further and pauses scripts ([Performance best practices](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance)).
- **Runtime on disk:** the Evergreen runtime is shared by all apps and ships with Windows 11. Edge and WebView2 binaries are hard-linked when their versions match, so they cost a scrivi user 0 extra bytes. The Fixed Version alternative is "over 250 MB" ([MS Learn, Distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)).
- **Scintilla:** in the official SciTE 5.6.7 x64 zip, measured from the archive listing ([SciTE download](https://www.scintilla.org/SciTEDownload.html)):
  - `Scintilla.dll` is **1,131,880 B**.
  - `Lexilla.dll` (all lexers) is **1,400,168 B**.
  - Static linking with only a markdown lexer would be smaller. I have not verified by how much.
- **RichEdit:** `msftedit.dll` ships with Windows (3,293,184 B on this machine), so it adds **0 B** to the exe.
  - Windows 11 Notepad is built on RichEdit, but Murray Sargent (Microsoft) notes it uses the Office build of RichEdit, "not the Windows RichEdit in msftedit.dll", in a `RichEditD2DPT` window that draws text with DirectWrite ([Math in Office blog, Windows 11 Notepad, 2022-02-22](https://devblogs.microsoft.com/math-in-office/windows-11-notepad/)).
  - So Notepad's 87 MB comes mostly from WinUI/XAML, not from RichEdit.
- **Minimal Rust `windows`-crate window:** I found no primary measured figure. Local evidence: musica (raw Win32 with a tray, a menu and IPC) is 461 KB, and tasca (with Direct2D and DirectWrite) is 533–608 KB. A bare window with a message loop should be smaller (unverified).
- **Tauri on WebView2 vs Electron memory:** in a [Tauri issue #5889](https://github.com/tauri-apps/tauri/issues/5889) (2022, Windows 10, read from Resource Monitor), Tauri loading Postman used 399 MB against 318 MB for Electron. Tauri maintainers called the method unscientific. Treat this as secondary.
- **Blog figures** such as "Tauri idle 30–40 MB" have no stated method (secondary, not relied on here). I found no controlled, published Win32-vs-WebView2 idle-memory comparison.

#### Takeaways for the stack choice

- **Memory differs by about two orders of magnitude.** A raw Win32 Rust app idles at **2–4 MB private** with no child processes (musica, tasca). A visible WebView2 markdown app idles at **~220 MB private across 7 processes** (MDHero). Even a hidden WebView2 window with the Low memory target keeps 6 processes and ~140 MB committed (annota).
- **On exe size both stacks are small; the frontend is what makes it big.** The Win32 path is ~0.5 MB, plus 0 B for RichEdit or ~1.1–2.5 MB for Scintilla. Tauri's floor is ~2.8 MB (Tauri CI) or 3.4 MB (annota). MDHero reaches 16 MB through Mermaid, KaTeX and highlight.js plus an untuned release profile, not because of Tauri.
- **Startup:** a WebView2 app shows its frame quickly but its content late. MDHero showed the file after 1.4–2.2 s, and Tauri's own hello world needs ~0.59 s to DOMContentLoaded. A raw Win32 editor's window time was not measured here; it would need a throwaway build. It does not pay for spawning a browser process, which Microsoft itself names as the startup cost.
- **Notepad is not the bar for "light":** WinUI Notepad takes 87 MB and ~1.5 s to show a file. Raw Win32 can beat it easily; a WebView2 app will be about 2.5× heavier than it.
- **If (W) is chosen anyway:**
  - Keep the frontend tiny: no Mermaid or KaTeX in the main bundle, and a textarea or a small editor.
  - Apply Tauri's size profile.
  - Expect ~200 MB per open window.
  - Use `MemoryUsageTargetLevel` Low when minimised, as annota does.
