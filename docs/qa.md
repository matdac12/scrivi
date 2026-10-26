# Manual QA checklist

Run before each release, top to bottom, on a fresh install (the order is the
order a person meets the app). Each box says **how** to check and **what to
expect**. Tick it, or write the result next to it. Nothing here runs in CI:
it needs a real desktop, a mouse and, for some items, a second monitor.

Words follow `GLOSSARY.md`. Sample files: `tests/fixtures/` (the Rendered view
one is `tests/fixtures/rendered/notes/notes.md`). Items marked *(record)* have
no fixed expectation in the spec: write down what you see.

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
  - Expect: scrivi opens with the file in a Tab; no console window.

## Launch and one Window (§2)

- [ ] **One Window.** `scrivi.exe a.txt`, then `scrivi.exe b.txt`, from a console and from Explorer.
  - Expect: a second Tab in the same Window; the second process exits at once; one `scrivi.exe` in Task Manager.
- [ ] **Elevated launch (SCRIVI-13).** From an *Administrator* console run `scrivi.exe C:\some\file.md`.
  - Expect: the file opens in a normal, non-elevated resident (check the Elevated column in Task Manager),
    in one Tab: the empty Untitled tab it starts with is taken over (SCRIVI-28).
- [ ] **Command line.** `scrivi.exe --notes.md` (expect the usage on stderr, exit code 2, nothing opens);
  `scrivi.exe --help` (expect the usage, exit code 0); `scrivi.exe -- --notes.md` (expect the file to open).
- [ ] **An empty Untitled tab is taken over.** Ctrl+N, then open a file (File › Open, drag-drop, Recent or a
  second launch). Expect: one Tab, the file's. Type in an Untitled tab first and open a file: expect two Tabs.
- [ ] **Exit.** Close the Window. Expect: no `scrivi.exe` left in Task Manager.

## Window and title row (§4)

- [ ] **Move and maximize.** Drag the Window by the title row; double-click the title row to maximize and restore.
  - Expect: the row is the drag area; no gaps or flicker.
- [ ] **Resize.** Drag all 8 edges and corners.
  - Expect: each resizes; nothing is painted outside the Window.
- [ ] **Snap Layouts.** Hover the maximize button (Windows 11).
  - Expect: the Snap Layouts flyout appears.
- [ ] **Auto-hide taskbar.** Turn on auto-hide, maximize the Window, move the mouse to the taskbar edge.
  - Expect: the taskbar still reveals.
- [ ] **Caption buttons.** Press a caption button and release over the empty title row; then press minimize,
  drag onto maximize and release.
  - Expect: no button stays drawn pressed; the second case fires nothing.
- [ ] **Look.** Compare with variant E (`prototype/window-look/index.html?variant=E`, branch `prototype/window-look`)
  and `docs/screenshots/source-view.png`.
  - Expect: it matches: one row with icon, File / Edit / View, Tabs, `+`, then the caption buttons.
- [ ] **Title and status bar.** Open a file and edit it.
  - Expect: title `<tab name> — scrivi`; the status bar shows live `Ln, Col`, the line ending and the encoding.

## Menus, Alt and access keys (§14)

- [ ] **Click navigation.** Click File, then move onto Edit and View.
  - Expect: the popup follows the mouse. Click File again while it is open: it closes.
- [ ] **Alt alone.** Press and release Alt.
  - Expect: File is highlighted and F, E and V are underlined; ← and → move between menus, Enter opens one,
    Esc leaves. The same with F10.
- [ ] **Alt+F.** Expect: File opens with the first item selected. Press ↓ to Encoding, → opens the submenu,
  ← closes it.
- [ ] **Alt+E / Alt+V** open Edit / View; an access key in an open menu (for example `N`, `S`) runs that item.
- [ ] **Shortcut labels.** Open each menu.
  - Expect: every item shows its keys and they agree with the README shortcut table.
- [ ] **Greyed items.** Look at Edit and View on an empty Untitled tab and on a `.txt` / read-only Tab.
  - Expect: Undo / Redo grey when there is nothing to do; Cut and Paste grey in a read-only Tab;
    Rendered view grey on a `.txt` (see *Rendered view*); Recent files grey when the list is empty.

## Tabs (§5, §14)

- [ ] **Open several.** Select 5 files in Explorer and open them; then open one of them again.
  - Expect: 5 Tabs; the repeat switches to that file's Tab, no duplicate.
- [ ] **Keeps its view.** Scroll a long wrapped `.md`, select some text, Ctrl+Tab away and back.
  - Expect: the same scroll position and selection.
- [ ] **Overflow and scrolling.** Open 12 or more files.
  - Expect: Tabs stay at least 120 px wide, then ‹ › scroll whole Tabs. Hovering a Tab shows a tooltip with
    the full path.
- [ ] **Close with unsaved edits.** Type in a Tab, then Ctrl+W.
  - Expect: Save / Don't save / Cancel. Alt+F4 with 2 edited Tabs: one dialog listing both, with
    Save all / Discard all / Cancel.
- [ ] **Add and close.** Middle-click a Tab, its ×, the `+`, Ctrl+T and Ctrl+N.
  - Expect: the Tab closes (middle-click and ×) or a new Untitled tab appears (`+`, Ctrl+T, Ctrl+N).
    Ctrl+Tab / Ctrl+Shift+Tab switch.
- [ ] **Save As onto a file open in another Tab.** Expect: refused with a message.

## Open, save, encodings, line endings (§6, §7)

- [ ] **Unedited Ctrl+S keeps the bytes.** Open a UTF-16 LE, a Windows-1252 and a CRLF file, Ctrl+S each
  without editing, then `fc /b original saved`.
  - Expect: the bytes are unchanged.
- [ ] **Line endings.** On a CRLF file choose File > Line endings > LF.
  - Expect: the unsaved dot appears, the status bar says LF, and saving writes LF.
- [ ] **Undo clears the dot.** Type a letter, then Ctrl+Z.
  - Expect: the dot appears, then goes away.
- [ ] **Save an Untitled tab.** Ctrl+N, type, Ctrl+S.
  - Expect: Save As opens in Documents with "Untitled 1.md".
- [ ] **Unencodable text.** File > Encoding > Windows-1252 on a Tab containing an emoji, then Ctrl+S.
  - Expect: the "Save as UTF-8" dialog appears.
- [ ] **Drag and drop.** Drag 2 files from Explorer onto the editor, then 2 onto the title row.
  - Expect: each opens in a Tab.
- [ ] **Save all on close.** Alt+F4 with 2 edited Tabs, choose Save all.
  - Expect: both files are written.
- [ ] **Invalid bytes.** Open a file with invalid bytes after a UTF-8 BOM.
  - Expect: it is read-only with a note. File > Reopen with encoding… > Windows-1252 makes it editable.
- [ ] **Save-path safety.** Save a file that has a hard link (`fsutil hardlink create`), and a file with a
  very long name.
  - Expect: both names of the hard link show the new text; the long name saves.
- [ ] **Non-text file** (a `.png`). *(record)*: no crash.

## Source view and markdown styling (§3)

- [ ] **Markdown look.** Open a `.md` notes file; compare with `docs/screenshots/source-view.png`.
  - Expect: bold blue headings, grey markers, tinted code, a green `[x]`.
- [ ] **Typing restyles.** Type in a 900 KB `.md`.
  - Expect: the styling updates after a short pause, with no stutter.
- [ ] **Untitled tabs are styled.** Ctrl+N and type markdown. Expect: styled.
- [ ] **Over 1 MB.** Open a `.md` over 1 MB.
  - Expect: unstyled; the status bar says "Markdown styling off (over 1 MB)".
- [ ] **Changing extension.** Save As `.md` → `.txt`, then `.txt` → `.md`.
  - Expect: the styling clears, then comes back.

## Large file mode (§8)

- [ ] **Over 50 MB** (make one: see *Performance*).
  - Expect: the amber Notice "Large file (N MB): read-only, without wrap or styling." with ×, and
    "Read-only" in the status bar.
- [ ] **Editing is refused.** Type in it. Expect: nothing changes; × dismisses the Notice.
- [ ] **Just under the limit.** A 49 MB file. Expect: editable and it wraps.

## Find, go to line, wrap, zoom

- [ ] **Find.** Select a word, press Ctrl+F.
  - Expect: the bar opens pre-filled; matches are highlighted, the current one more strongly, with "n of m".
    Enter, Shift+Enter and F3 step and wrap around. Esc closes the bar when it has focus.
- [ ] **Match case and Replace.** Toggle `Aa`; press Ctrl+H.
  - Expect: `Aa` changes the matches; Ctrl+H adds the replace row; Replace replaces and moves on;
    Replace all, then one Ctrl+Z restores everything.
- [ ] **Go to line.** Ctrl+G, a number, Enter.
  - Expect: jumps to the line, centres it, and the bar closes.
- [ ] **Word wrap.** Alt+Z.
  - Expect: toggles wrap, checked in View, greyed on a Large file. Alt alone after Alt+Z does not open
    the menu bar.
- [ ] **Zoom.** Ctrl++, Ctrl+−, Ctrl+0 and Ctrl+wheel.
  - Expect: it zooms; the status bar shows the level, for example 118%.
- [ ] **Italian keyboard.** Press the Ctrl+plus and Ctrl+minus keys. Expect: they zoom in and out.
- [ ] **Minimum width.** Narrow the Window to its 480 px minimum with the find bar open.
  - Expect: the bar still fits. Repeat across a DPI change (see *DPI*).
- [ ] **Combinations.** Open the find bar with a Notice showing and with spell squiggles on screen;
  press Ctrl+E with the bar open.
  - Expect: the layout holds; Ctrl+E closes the bar.

## Spell check (§3)

- [ ] **Flags the typo.** In a `.md` type "Prenotare il tagliandoo per il preventivo".
  - Expect: only "tagliandoo" gets a muted-red squiggle.
- [ ] **Right-click it.**
  - Expect: suggestions (tagliando…), Add to dictionary, then Undo / Cut / Copy / Paste / Select all.
- [ ] **Add to dictionary.** Expect: the squiggle goes at once and still holds after a restart.
- [ ] **No squiggle** under a `` `code` `` span or a bare URL.
- [ ] **A `.txt` Tab** gets no squiggles at all.
- [ ] **English.** Misspell an English word in the same `.md`. Expect: it is flagged too.

## Rendered view (§3, §14)

- [ ] **Compare with the mockup.** Open `tests/fixtures/rendered/notes/notes.md`, press Ctrl+E
  (see `docs/screenshots/rendered-view.png`).
  - Expect: it matches the mockup; the images are lower down the page.
- [ ] **Links.** Click "WebView2", "write to me", "the table".
  - Expect: the browser opens; the mail client opens; the page scrolls to Tools.
- [ ] **Leave it.** Press Esc; then Ctrl+E again and press Ctrl+E inside the page; check Ctrl+Tab and Ctrl+W
  with the page focused.
  - Expect: each of Esc and Ctrl+E returns to the Source view; Ctrl+Tab and Ctrl+W work.
- [ ] **Image from an Untitled tab.** In an Untitled tab type `![x](some.png)`, with `some.png` in Documents; Ctrl+E.
  - Expect: the image shows.
- [ ] **Minimize and restore** while rendered.
  - Expect: the page is back, and the arrow keys scroll it.
- [ ] **View > Rendered view** enabled on `.md`, greyed on `.txt`.
- [ ] **Page is inert.** Right-click in the page; drop a file on the page.
  - Expect: no context menu; no navigation.
- [ ] **Menu keys from the page.** With the page focused press Alt, F10 and Alt+F; then Alt+Z.
  - Expect: they reach the menu bar; Alt+Z still toggles wrap.
- [ ] **Edit menu while rendered.** Expect: Cut / Copy / Paste / Select all are greyed; Ctrl+C inside the
  page still copies. Find, Replace and Go to line are greyed.
- [ ] **Big page.** Make a `.md` of 10k lines of `# a`; Ctrl+E.
  - Expect: the Window does not freeze.
- [ ] **Too large.** A 1.2 MB `.md`, Ctrl+E. Expect: "Too large to render".
- [ ] **Teardown.** About 1 s after leaving the Rendered view, look in Task Manager.
  - Expect: no `msedgewebview2.exe` under scrivi (also checked under *Performance*).

## Remembered settings and the jump list (§10)

Release build.

- [ ] **Jump list.** `scrivi.exe --register`, open 2–3 files, right-click the taskbar icon.
  - Expect: they are listed under Recent; clicking one opens it in the running Window.
- [ ] **Recent files.** File > Recent files.
  - Expect: newest first, at most 10; reopening or saving a file moves it to the top. Delete a file and pick
    it: a status flash, and it leaves the list.
- [ ] **Window.** Move and resize, close, reopen; maximize, close, reopen.
  - Expect: same place; still maximized. A position on a monitor that is gone falls back to a visible one.
- [ ] **Wrap and zoom.** Ctrl+wheel zoom and Alt+Z, restart. Expect: both survive.

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

Needs two monitors with different scaling (for example 100% and 150%).

- [ ] **Drag the Window** from one monitor to the other, in the Source view.
  - Expect: the title row, margins, Tabs and line numbers rescale; no clipped Tabs or overlap.
- [ ] **Same in the Rendered view.** Expect: the page rescales.
- [ ] **Find bar across the move.** Open the find bar at the 480 px minimum width and drag between monitors.
  - Expect: the bar still fits.

## Disk change (SCRIVI-19)

Spec §8 and §9. Rewrite files from PowerShell, for example `Set-Content .\a.txt "new text"`.

- [ ] **Changed with unsaved edits.** Open a file and type something, then rewrite the file from PowerShell.
  - Expect: within about 1 s the amber Notice "*name* changed on disk while you had unsaved edits." shows,
    with [Keep mine] and [Reload].
  - Each button lightens on hover.
  - Pressing a button and dragging off it does not fire it; pressing and releasing on it does.
- [ ] **Keep mine and Reload.** Repeat the above for each button.
  - Expect: Keep mine dismisses the Notice and the next Ctrl+S overwrites the file. Reload discards your
    edits and shows the file's new text.
- [ ] **Unedited file.** Rewrite an unedited open file from PowerShell.
  - Expect: it reloads silently within about 1 s and keeps the caret line.
- [ ] **Deleted or renamed.** Delete or rename an open file.
  - Expect: the rose Notice "…was deleted or renamed on disk…" shows with [Save], the text is kept and the
    unsaved dot appears. Click Save: the file is recreated at its old path.
- [ ] **Own save.** Ctrl+S on an edited Tab.
  - Expect: no Notice and no reload.
- [ ] **Per-Tab Notices.** With a Notice showing on one Tab, switch to another Tab and back.
  - Expect: each Tab keeps its own Notice.
- [ ] **Known and accepted:** a live log over 50 MB (a Large file being appended to) reloads after each change.

## Performance (idle machine)

Run on an idle machine with the release build, after a reboot or at least with scrivi closed and the file
cache warm (open the file once, close scrivi, then time). Write the numbers next to each item. Use Task
Manager (Details tab) for memory and size, and Process Explorer or `Get-CimInstance Win32_Process` for children.

- [ ] **Notes file.** Time from double-click to text on screen for a small `.md` / `.txt` file, scrivi not running.
  - Expect: about as fast as Notepad; no splash, no flash of an empty Window.
- [ ] **50 MB log.** Make one:
  `$s = 'x' * 99 + "`n"; [IO.File]::WriteAllText("$env:TEMP\50mb.log", $s * 540000)` (about 54 MB, just over the 50 MiB limit), then open it.
  - Expect: read-only Notice, usable at once; record the time.
- [ ] **200 MB log.** The same with `* 2000000`.
  - Expect: record the time. SCRIVI-18 measured about 1 s on a busy machine; the spec target is about 337 ms
    (see SCRIVI-30, which tracks the gap).
- [ ] **Exe size.** `(Get-Item .\scrivi.exe).Length`.
  - Expect: record it. Now 2.57 MB, against the spec's 1–2 MB; a known limitation (README).
- [ ] **Idle memory.** One empty Untitled tab, wait a minute; Task Manager > Details > *Memory (private working set)*.
  - Expect: a few MB. Record it, and again with a 1 MB text file open.
- [ ] **No child processes at idle.** With a `.txt` and a markdown file in Source view, list scrivi's children.
  - Expect: none. In particular no `msedgewebview2.exe`.
- [ ] **WebView2 exits after the Rendered view.** Ctrl+E on a markdown Tab: `msedgewebview2.exe` children appear.
  Press Esc (and, separately, close the markdown Tab while rendered).
  - Expect: within a few seconds every `msedgewebview2.exe` child of scrivi is gone.

## Release

Steps from SCRIVI-26 (issue 18). Only after every section above passes, and the results are recorded here
and in README "Known limitations".

- [ ] `Cargo.toml` version is `0.1.0` and `CHANGELOG.md` has the 0.1.0 entry with the date (README *Releasing*).
- [ ] The README shortcut table matches spec §4 (SCRIVI-25).
- [ ] The three checks pass on `main`: `cargo build --release`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- [ ] Tag and push: `git tag v0.1.0`, then `git push origin v0.1.0`. Never move or re-push a published tag;
  cut a new patch version instead.
- [ ] `release.yml` finishes green (SCRIVI-25: after a real `v0.1.0` push the three assets appear on GitHub Releases).
- [ ] The GitHub Release has the 3 assets: `scrivi.exe`, `scrivi-0.1.0-windows-x64.zip`, `SHA256SUMS.txt`.
- [ ] Download the zip, check it against `SHA256SUMS.txt` and run `scripts\install.ps1` once: the
  *Setup* section works from the published zip.
- [ ] Add the Caps+N line by hand to `%USERPROFILE%\Scripts\hyper.ahk`, outside hyperkey's managed region:
  `CapsLock & n::Run("<path>\scrivi.exe")`. Reload it and press Caps+N: scrivi opens.
- [ ] Update mstack's README row for scrivi.
