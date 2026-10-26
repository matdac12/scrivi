# Manual QA checklist

Run before each release. This file holds the sections written so far; the rest
of the checklist (§14) is added by the final QA pass.

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
