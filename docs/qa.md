# Manual QA checklist

Run before each release, top to bottom, on a fresh install (the order is the
order a person meets the app). Each box says **how** to check and **what to
expect**. Tick it, or write the result next to it. Nothing here runs in CI:
it needs a real desktop, a mouse and, for some items, a second monitor.

Words follow `GLOSSARY.md`. Sample files: `tests/fixtures/` (and any `.md` file
with an image, a heading and a link to it).

## Setup

- [ ] **Get a release build.** Either download `scrivi-<version>-windows-x64.zip`
  from Releases and check it against `SHA256SUMS.txt`, or run
  `cargo build --release` and use `target\release\scrivi.exe`.
  - Expect: `scrivi.exe` exists; SmartScreen warns once if it came from the zip.
- [ ] **Install.** In Windows PowerShell run `scripts\install.ps1` from the
  unzipped folder (or the repo).
  - Expect: `scrivi.exe` in `%LOCALAPPDATA%\Programs\scrivi\`, then `--register`
    runs. Run it again with scrivi open: it refuses to overwrite a running copy.
- [ ] **Register.** `scrivi.exe --register`.
  - Expect: Settings opens at Default apps for Scrivi. Nothing is written to `UserChoice`.
- [ ] **Pick scrivi** in Settings > Default apps for `.txt`, `.md` and `.log`
  (the per-extension results are recorded under *Registration*).
- [ ] **First launch.** Double-click a `.txt` file.
  - Expect: scrivi opens at once with the file in a Tab; no console window.

## Window and title row (§4)

- [ ] **One row.** Look at the top of the Window.
  - Expect: one row: icon, File / Edit / View, the Tabs, the `+`, then minimize / maximize / close.
    The row is the drag area; double-click it to maximize and restore.
- [ ] **Caption buttons.** Hover maximize (Windows 11).
  - Expect: Snap Layouts flyout. Min / max / close work; close hover is red.
- [ ] **Title and status bar.** Open a file, edit it.
  - Expect: the title reads `<tab name> — scrivi`; the status bar shows live `Ln, Col`,
    the line ending (LF / CRLF / CR) and the encoding.
- [ ] **Resize and edges.** Drag every edge and corner; snap to a half; maximize.
  - Expect: no flicker, no gaps, nothing painted outside the Window.
- [ ] **Minimize, restore, Alt+Tab.** Expect: the Window returns as it was.

## Menus, Alt and access keys (§14)

- [ ] **Alt opens the menu bar.** Press and release Alt, then F10.
  - Expect: File gets focus (underlined access keys appear); Left / Right moves between
    File, Edit, View; Esc closes.
- [ ] **Alt+F / Alt+E / Alt+V** open File / Edit / View. Then press an access key
  in the open menu (for example `N` in File for New tab, `S` for Save, `x` for Exit).
  - Expect: the underlined letter runs that item.
- [ ] **Hover and keys in menus.** Open File, hover each item, use Up / Down / Home / End, Enter.
  - Expect: one highlight at a time; a submenu (Recent files, Encoding, Line endings)
    opens on Right or hover and closes on Left or Esc (Esc closes only the submenu).
- [ ] **Shortcut labels.** Open each menu.
  - Expect: every item shows its keys: Ctrl+T, Ctrl+O, Ctrl+S, Ctrl+Shift+S, Ctrl+W, Ctrl+Z, Ctrl+Y,
    Ctrl+X, Ctrl+C, Ctrl+V, Ctrl+A, Ctrl+F, Ctrl+H, Ctrl+G, Ctrl+E, Alt+Z, Ctrl++, Ctrl+−, Ctrl+0.
- [ ] **Greyed items.** Empty Untitled tab; then a read-only Tab.
  - Expect: Undo / Redo grey when there is nothing to do; Cut and Paste grey in a read-only Tab;
    Recent files grey when the list is empty.
- [ ] **Clicking away** (on the editor, on another Window) closes an open menu.
- [ ] **Menu keys over the Rendered view** (see below): they still work.

## Tabs (§5, §14)

- [ ] **Add and close.** Ctrl+T (and Ctrl+N), then Ctrl+W; click `+`; click a Tab's ×;
  middle-click a Tab.
  - Expect: new Untitled tabs; a neighbour becomes active after a close; closing the last Tab closes the Window.
- [ ] **Switch.** Click a Tab; Ctrl+Tab and Ctrl+Shift+Tab.
  - Expect: each Tab keeps its own caret, scroll, undo history and Notice.
- [ ] **Unsaved dot.** Edit a saved Tab.
  - Expect: a dot replaces the × until you save; Save clears it.
- [ ] **Close with unsaved edits.** Ctrl+W on an edited Tab.
  - Expect: Save / Don't save / Cancel. Closing the Window with several unsaved Tabs shows one
    dialog listing them: Save all / Discard all / Cancel. Cancel keeps everything.
- [ ] **Overflow.** Open about 30 files (a folder of `.txt`, Ctrl+O with multiple selection or
  `scrivi.exe a.txt b.txt ...`), in a narrow Window.
  - Expect: Tabs shrink to a minimum width then the strip scrolls; the active Tab is scrolled
    into view; the mouse wheel over the strip scrolls it; `+` stays reachable.
- [ ] **Tooltips.** Hover a Tab with a long name.
  - Expect: the full path.
- [ ] **Open the same file twice** (Ctrl+O, double-click in Explorer, `scrivi.exe <path>` again).
  - Expect: switches to the existing Tab, no duplicate; the Window comes to front (or the taskbar flashes).
- [ ] **Save As onto a file open in another Tab.** Expect: refused, with a message.

## Open, save, encodings, line endings (§6, §7)

- [ ] **Open dialog.** Ctrl+O, pick several files.
  - Expect: one Tab each; the file type filter lets you pick any file.
- [ ] **Save an Untitled tab** (Ctrl+S): Save As dialog, default name, `.txt` added if none.
  - Expect: the Tab takes the file's name; the file appears in Explorer.
- [ ] **Save is atomic.** Edit a file, Save; check the folder.
  - Expect: no stray temp files; the file keeps its times / attributes as far as Windows allows;
    a read-only file or a locked file gives a clear error and keeps your text.
- [ ] **Encodings.** Open UTF-8, UTF-8 with BOM, UTF-16 LE, UTF-16 BE and Windows-1252 files
  (accents, euro sign, Italian text).
  - Expect: right text, and the status bar names the encoding. File > Encoding changes how it
    will be saved; *Reopen with encoding…* reloads with another one. Saving keeps the BOM state.
- [ ] **Line endings.** Open an LF, a CRLF and a CR file; File > Line endings.
  - Expect: the status bar shows each; choosing another converts on save; a new line keeps the file's style.
- [ ] **Recent files.** Open and save a few files; File > Recent files.
  - Expect: newest first, at most 10; a click opens one; a missing file is dropped with a message.
- [ ] **Drag and drop** a file onto the Window. Expect: opens in a Tab.
- [ ] **Non-text file** (a `.png` or `.exe`). Expect: no crash; write down what it shows.

## Source view and markdown styling (§3)

- [ ] **Source view basics.** Type, paste, undo / redo, select all.
  - Expect: line numbers; a calm font; the caret line is subtly highlighted.
- [ ] **Markdown styling.** Open `notes.md` with headings, bold, italic, strike, inline code,
  a link, a bare URL, a task list, a numbered list, a quote and a fenced code block;
  see `docs/screenshots/source-view.png`.
  - Expect: each is styled as in the screenshot; Untitled tabs are styled too, and lose the styling when saved as `.txt`.
- [ ] **A `.txt` or `.log` gets no markdown styling.**
- [ ] **A markdown file over 1 MB** gets no styling and no Rendered view (Ctrl+E greyed).
- [ ] **Word wrap.** Alt+Z on a long line.
  - Expect: wraps / unwraps; the choice is remembered (see *Remembered settings*).
- [ ] **Zoom.** Ctrl++ / Ctrl+− / Ctrl+0 and Ctrl+wheel.
  - Expect: the text and line numbers scale together; reset returns to 100%; the level is remembered.

## Find, replace, go to line

- [ ] **Find.** Ctrl+F, type a word.
  - Expect: the find bar appears under the Tabs; matches are highlighted as you type; Enter / Shift+Enter
    go next / previous; Esc closes and keeps the selection; a search with no match says so.
- [ ] **Replace.** Ctrl+H; Replace and Replace all.
  - Expect: counts are right; one Ctrl+Z undoes a Replace all.
- [ ] **Match case** (the `Aa` toggle in the find bar). Expect: it changes the matches at once; the bar shows "n of m".
- [ ] **Go to line.** Ctrl+G, enter 1, a middle line, and a number past the end.
  - Expect: the caret lands there; an out-of-range number goes to the last line or says so.
- [ ] **In a Large file** (read-only): Find works, Replace is off.
- [ ] **In the Rendered view:** Find, Replace and Go to line are greyed (they only act on the Source view).

## Rendered view (§3, §14)

- [ ] **Toggle.** In a markdown Tab press Ctrl+E (or View > Rendered view); see
  `docs/screenshots/rendered-view.png`.
  - Expect: formatted output in the same place; the status bar reads *Rendered view · Esc to edit*;
    the first show feels instant on notes-sized files. Text is read-only.
- [ ] **Esc returns** to the Source view with the caret and scroll where you left them.
  Ctrl+E also toggles back.
- [ ] **Content.** Headings, task list checkboxes, nested lists, tables, quotes, code blocks, strikethrough,
  footnotes (if in your sample).
  - Expect: formatted as in the screenshot; scripts and raw HTML are not run (try a `<script>` and an `<iframe>`).
- [ ] **Images.** A relative image (`![x](img/a.png)`), one in a parent folder (`../pics/b.png`),
  one with a space in its name, and an SVG.
  - Expect: they show. A remote image (`https://...`), a `file:///` image and a drive-letter path
    do **not** load (privacy: no network from the page).
- [ ] **Links.** Click an `https://` link and a `mailto:` link.
  - Expect: the default browser / mail client opens; the Window page does not navigate.
- [ ] **Anchor links.** Click `[the table](#the-table)` (heading slug).
  - Expect: scrolls to that heading in the page.
- [ ] **Other links** (`file:///...`, `other.md`, `javascript:`). Expect: nothing happens.
- [ ] **No new windows.** Ctrl+click or middle-click a link. Expect: no extra WebView2 window.
- [ ] **Keys inside the page.** Ctrl+E, Esc, Ctrl+W, Ctrl+T, Ctrl+Tab, Ctrl+O, Alt+F and Alt+Z.
  - Expect: handled by scrivi, not swallowed by the page; Ctrl+C copies the selected text.
- [ ] **Menu while rendered.** Edit: Cut and Paste are grey, Copy works on a selection; no spell
  check menu appears on right-click.
- [ ] **Two markdown Tabs.** Render one, switch to another.
  - Expect: each Tab remembers whether it is rendered; switching is quick and shows no stale page.
- [ ] **Edit then render.** Change text, Ctrl+E.
  - Expect: it shows the current (unsaved) text.
- [ ] **Wide and tall content.** A wide table and a very long page.
  - Expect: scrolls; the page resizes with the Window; DPI looks sharp.
- [ ] **Teardown.** See *Performance*: after Esc, the `msedgewebview2.exe` children of scrivi exit.
- [ ] **WebView2 runtime missing** (only if you can test it, for example in a VM without it).
  - Expect: a clear message, and the Source view keeps working.

## Large file mode (§8)

- [ ] **Open a file over 50 MB** (see *Performance* for how to make one).
  - Expect: opens read-only; a Notice says so; no word wrap, no styling, no undo history;
    scrolling and Ctrl+End are smooth; Find works.
- [ ] **Edit attempts.** Type, paste, Ctrl+Z in it. Expect: nothing changes; no crash.
- [ ] **A big markdown file** (over 1 MB, under 50 MB). Expect: no styling, no Rendered view.
- [ ] **Close it** and check memory. Expect: it drops back to near idle.
- [ ] **Live log (accepted limitation).** Append to a Large file from PowerShell
  (`Add-Content`) while it is open.
  - Expect: it reloads after each change (the debounce means about once a second); this is the
    accepted v1 limitation, listed under README *Known limitations*.

## Spell check (§3)

- [ ] **Markdown and Untitled only.** Open `.md` with a misspelling in Italian and in English, then a `.txt`.
  - Expect: red squiggles under unknown words in the first; none in `.txt` or in code spans / blocks / URLs.
- [ ] **Right-click a squiggle.**
  - Expect: suggestions at the top ("(No suggestions)" if none); choosing one fixes the word (one undo step);
    *Add to dictionary* removes the squiggle at once and it stays away after a restart. The usual
    Undo / Cut / Copy / Paste items follow.
- [ ] **Both languages** accept their own words. A word right in either Italian or English is not flagged.
- [ ] **Typing.** Type a wrong word, then a space. Expect: squiggle appears shortly; the UI stays smooth on a long file.
- [ ] **No Windows spell checker for a language** (turn one off in Settings > Time & language > Typing).
  - Expect: no crash; that language just is not checked.

## Remembered settings and the jump list (§10)

- [ ] **Window.** Move, resize, maximize; quit; relaunch.
  - Expect: same place and size; maximized comes back maximized; a window left on a disconnected monitor
    comes back on screen.
- [ ] **Wrap and zoom** come back as you left them.
- [ ] **Settings file.** Look at `%LOCALAPPDATA%\scrivi\scrivi.ini`.
  - Expect: plain text with window, wrap, zoom and `[recent]`. Delete it: scrivi starts with defaults.
- [ ] **Jump list.** Right-click scrivi on the taskbar (and pin it).
  - Expect: your recent files (the same ten as File > Recent files); clicking one opens it in the running
    Window (no second Window). Also check Explorer's own Recent list gets the file.
- [ ] **Tabs are not restored** after a restart: expect a fresh Untitled tab (the closing prompt asked about
  unsaved text first).

## Launch and one Window (§2)

- [ ] **One resident.** `scrivi.exe a.txt`, then `scrivi.exe b.txt` from a console and from Explorer.
  - Expect: a second Tab in the same Window; the second process exits at once; one `scrivi.exe` in Task Manager.
- [ ] **Launch speed.** Double-click a `.txt` with scrivi closed.
  - Expect: the text is visible about as fast as Notepad does (numbers in *Performance*).
- [ ] **Elevated launch (SCRIVI-13).** Start scrivi from an *Administrator* console or "Run as administrator".
  - Expect: scrivi relaunches itself through Explorer as the normal user (so drag-and-drop and the
    one-Window rule keep working); no second Window, no UAC loop; the file opens.
  - Also: with a normal scrivi open, run `scrivi.exe b.txt` from the admin console. Expect: opens in the existing Window.
- [ ] **Paths.** Open a file with spaces, accents, a long path (over 260 characters), a UNC path and a
  network drive. Expect: opens; Save works.
- [ ] **Bad arguments.** `scrivi.exe C:\nope.txt`.
  - Expect: no crash. Write down what happens (an empty Tab named `nope.txt`, or a message): the spec does not
    fix it.
- [ ] **Exit.** Close the Window. Expect: the process ends (no `scrivi.exe` left in Task Manager).

## Registration (§12)

These need a real Windows machine and clicks in Settings. They are the hand
check for SCRIVI-26. Record every result below in this file and in README
"Known limitations".

**Setup.** Put `scrivi.exe` where it will live, run `scrivi.exe --register`.
Settings opens at Default apps for Scrivi. Nothing is written to `UserChoice`.

### Basics
1. In Settings, set scrivi as the default for `.txt`, `.md` and `.log`.
2. Double-click one file of each type in Explorer.
   - Expected: each opens in scrivi (a new tab if scrivi is already running).
   - Record: pass or fail per extension.
3. Run `scrivi.exe --unregister`.
   - Expected: no `Scrivi` keys remain: `HKCU\Software\Scrivi`, `HKCU\Software\Classes\Scrivi.TextFile`,
     the `Scrivi` value in `HKCU\Software\RegisteredApplications`, and `Scrivi.TextFile` under every
     `HKCU\Software\Classes\<ext>\OpenWithProgids`.
   - Record: anything left behind.

### Edge case 1: files with no extension
1. After `--register`, in Settings > Default apps look for a way to choose scrivi for files with no
   extension (the `.` type). Also try right-click > Open with > Choose another app > Always on a file
   named `LICENSE`.
2. Double-click `LICENSE`.
   - Expected (unverified): opens in scrivi if Settings or "Always" accepts the `.` type; otherwise
     only Open with works.
   - Record: whether Settings lists `.`, whether "Always" sticks, and the result of the double-click.

### Edge case 2: Explorer's Edit verb
1. With scrivi the default for `.txt` and `.ini`, right-click a `.txt` and a `.ini` > Edit (Windows 11:
   Show more options if Edit is hidden).
   - Expected: scrivi opens the file (scrivi ships `shell\edit` on its ProgID).
   - Record: which program opens, per extension. Also check a type scrivi is not default for
     (`.bat`, `.reg`): expected to stay on Notepad, which is a known limitation.

### Edge case 3: right-click > New > Text Document
1. In an Explorer folder, right-click > New > Text Document, name it, then double-click it.
   - Expected (unverified): the empty `.txt` opens in scrivi, because scrivi is the `.txt` default.
     New-file creation itself stays with Windows.
   - Record: whether it opens in scrivi, and whether the New menu entry still works.

### Edge case 4: UserChoiceLatest
1. Check `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\SystemProtectedUserData\<your SID>\AnyoneRead\AppDefaults`
   for `HashVersion` (read only; do not edit). Also look for
   `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.txt\UserChoiceLatest`.
2. Repeat Basics 1 and 2 on this machine.
   - Expected: scrivi stays the default after picking it in Settings. Research says this machine did not
     have `UserChoiceLatest` yet, so this may only be testable after Windows rolls it out.
   - Record: `HashVersion` value (or absent), whether `UserChoiceLatest` exists, and whether the
     default sticks.

## DPI change between monitors (§4, §14)

Needs two monitors with different scaling (for example 100% and 150%), or one monitor whose scale you
change while scrivi runs.

- [ ] **Drag the Window** from one monitor to the other, in the Source view.
  - Expect: the title row, Tabs, menus, status bar, line numbers and text all rescale at once; no
    blurry edges, no clipped Tabs, no overlap; the caption buttons stay hit-able.
- [ ] **Same in the Rendered view.** Expect: the page rescales sharply; the Window keeps its on-screen size.
- [ ] **Menus and dialogs** opened after the move (File menu, Find bar, Save As, the close prompt).
  - Expect: sized for the new monitor.
- [ ] **Change the Windows scale** (Settings > Display) with scrivi open. Expect: the same as dragging.
- [ ] **Relaunch on the second monitor.** Expect: the remembered Window position and size still fit.

## Disk change (SCRIVI-19)

**Pending.** SCRIVI-19 (issue 11, Disk changes) is still being built. The exact hand-check list is added
when it lands. Spec §8 and §9 acceptance, to be turned into steps:

- [ ] An agent or PowerShell rewriting an open, unedited file reloads it within about a second, keeping the
  caret line where possible.
- [ ] With unsaved edits, the Tab shows the dot and the amber Disk change Notice when it is shown.
  *Reload* discards the edits; *Keep mine* dismisses the Notice and the next Save overwrites the file.
- [ ] Deleting or renaming the file keeps the text, marks the Tab unsaved and shows the rose "deleted on disk"
  Notice; *Save* writes the file again at its old path.
- [ ] Ctrl+S never triggers a Disk change or a "deleted on disk" Notice for its own Tab (saves are atomic:
  they look like rename, delete and create to a watcher).
- [ ] Each Tab keeps its own Notice; it shows when its Tab is active.
- [ ] **Accepted limitation:** a Large file that is being appended to (a live log over 50 MB) reloads after
  each debounced change.

## Performance (idle machine)

Run on an idle machine with the release build, after a reboot or at least with scrivi closed and the file
cache warm (open the file once, close scrivi, then time). Write the numbers next to each item. Use Task
Manager (Details tab) for memory and size, and Process Explorer or `Get-CimInstance Win32_Process` for children.

- [ ] **Notes file.** Time from double-click to text on screen for a small `.md` / `.txt` file, scrivi not running.
  - Expect: about as fast as Notepad; no splash, no flash of an empty Window.
- [ ] **Hand-over speed.** With scrivi running, open a second file from Explorer.
  - Expect: the new Tab appears at once.
- [ ] **50 MB log.** Make one:
  `$s = 'x' * 99 + "`n"; [IO.File]::WriteAllText("$env:TEMP\50mb.log", $s * 540000)` (about 54 MB, just over the 50 MiB limit), then open it.
  - Expect: read-only Notice, usable at once; record the time.
- [ ] **200 MB log.** The same with `* 2000000`.
  - Expect: record the time. SCRIVI-18 measured about 1 s on a busy machine; the spec target is about 337 ms
    (see SCRIVI-30, which tracks the gap). Slower than 1 s on an idle machine is worth a note. Memory should
    not be several times the file size.
- [ ] **Exe size.** `(Get-Item .\scrivi.exe).Length`.
  - Expect: record it. Now 2.57 MB, against the spec's 1–2 MB; a known limitation (README).
- [ ] **Idle memory.** One empty Untitled tab, wait a minute; Task Manager > Details > *Memory (private working set)*.
  - Expect: a few MB. Record it, and again with a 1 MB text file open.
- [ ] **No child processes at idle.** With a `.txt` and a markdown file in Source view, list scrivi's children.
  - Expect: none. In particular no `msedgewebview2.exe`.
- [ ] **WebView2 exits after the Rendered view.** Ctrl+E on a markdown Tab: `msedgewebview2.exe` children appear.
  Press Esc (and, separately, close the markdown Tab while rendered).
  - Expect: within a few seconds every `msedgewebview2.exe` child of scrivi is gone and memory falls back.
- [ ] **Repeated toggles.** Ctrl+E / Esc about 20 times. Expect: no growth in memory, handles or child count.
- [ ] **Idle CPU.** Leave it ten minutes. Expect: 0% CPU; with a file open, the watcher stays quiet.

## Release

Steps from SCRIVI-26 (issue 18). Only after every section above passes, and the results are recorded here
and in README "Known limitations".

- [ ] `Cargo.toml` version is `0.1.0` and `CHANGELOG.md` has the 0.1.0 entry with the date (README *Releasing*).
- [ ] The three checks pass on `main`: `cargo build --release`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- [ ] Tag and push: `git tag v0.1.0`, then `git push origin v0.1.0`. Never move or re-push a published tag;
  cut a new patch version instead.
- [ ] `release.yml` finishes green.
- [ ] The GitHub Release has the 3 assets: `scrivi.exe`, `scrivi-0.1.0-windows-x64.zip`, `SHA256SUMS.txt`.
- [ ] Download the zip, check it against `SHA256SUMS.txt` and run `scripts\install.ps1` once: the
  *Setup* section works from the published zip.
- [ ] Add the Caps+N line by hand to `%USERPROFILE%\Scripts\hyper.ahk`, outside hyperkey's managed region:
  `CapsLock & n::Run("<path>\scrivi.exe")`. Reload it and press Caps+N: scrivi opens.
- [ ] Update mstack's README row for scrivi.
