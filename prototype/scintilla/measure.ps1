# Measures the prototype the same way as the earlier weight research:
# time until a visible window's title contains the file name (EnumWindows every
# 5 ms), then private working set and private bytes after 10 s idle, then a
# graceful WM_CLOSE. 3 runs per file, medians reported.
# For markdown files it also posts WM_APP+1 to the prototype's own window, which
# inserts one character through the normal edit path, so the log records an
# edit-triggered styling pass. Nothing is ever sent to any other window, and no
# keystrokes are synthesised.
# Finally it captures notes.md with PrintWindow (not a screen grab).
#
# Usage (from prototype\scintilla, after gen-test-files.ps1 and cargo build --release):
#   powershell -NoProfile -File measure.ps1
param(
    [string]$Exe = (Join-Path $PSScriptRoot 'target\release\scrivi-proto.exe'),
    [string]$Dir = (Join-Path $env:TEMP 'scrivi-proto-files'),
    [string[]]$Files = @('notes.md', 'big.md', 'log50.log', 'log200.log'),
    [int]$Runs = 3,
    [int]$IdleSeconds = 10
)
$ErrorActionPreference = 'Stop'

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;

public static class Win {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int L, T, R, B; }

    // Visible top-level window of process `pid` whose title contains `needle`, or zero.
    public static IntPtr Find(uint pid, string needle) {
        IntPtr found = IntPtr.Zero;
        var sb = new StringBuilder(512);
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p != pid || !IsWindowVisible(h)) return true;
            sb.Clear(); GetWindowText(h, sb, sb.Capacity);
            if (sb.ToString().Contains(needle)) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    // PrintWindow with PW_RENDERFULLCONTENT into a bitmap, at physical pixels.
    public static string Capture(IntPtr h, string path) {
        SetThreadDpiAwarenessContext(new IntPtr(-4)); // per-monitor v2
        RECT r; GetWindowRect(h, out r);
        int w = r.R - r.L, ht = r.B - r.T;
        using (var bmp = new Bitmap(w, ht, PixelFormat.Format32bppArgb))
        using (var g = Graphics.FromImage(bmp)) {
            IntPtr hdc = g.GetHdc();
            bool ok = PrintWindow(h, hdc, 2);
            g.ReleaseHdc(hdc);
            bmp.Save(path, ImageFormat.Png);
            return w + "x" + ht + (ok ? "" : " (PrintWindow failed)");
        }
    }
}
'@

$WM_CLOSE = 0x0010
$WM_APP_SIM_EDIT = 0x8001
$log = Join-Path (Split-Path $Exe) 'scrivi-proto.log'

function Median($xs) { $s = @($xs | Sort-Object); $s[[int][math]::Floor($s.Count / 2)] }

function Start-Proto([string]$file) {
    $path = Join-Path $Dir $file
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $Exe -ArgumentList "`"$path`"" -PassThru
    $h = [IntPtr]::Zero
    while ($h -eq [IntPtr]::Zero) {
        if ($sw.ElapsedMilliseconds -gt 120000) { throw "no window for $file" }
        Start-Sleep -Milliseconds 5
        $h = [Win]::Find([uint32]$p.Id, $file)
    }
    [pscustomobject]@{ Proc = $p; Hwnd = $h; ShownMs = $sw.ElapsedMilliseconds }
}

function Stop-Proto($r) {
    [void][Win]::PostMessage($r.Hwnd, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
    if (-not $r.Proc.WaitForExit(15000)) {
        Write-Warning "pid $($r.Proc.Id) ignored WM_CLOSE; killing"
        $r.Proc.Kill()
    }
}

if (Test-Path $log) { Remove-Item $log }
$exeSize = (Get-Item $Exe).Length
$rows = @()
foreach ($f in $Files) {
    $shown = @(); $ws = @(); $pb = @()
    for ($i = 1; $i -le $Runs; $i++) {
        $r = Start-Proto $f
        Start-Sleep -Seconds $IdleSeconds
        $perf = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($r.Proc.Id)"
        $shown += $r.ShownMs
        $ws += [math]::Round($perf.WorkingSetPrivate / 1MB, 1)
        $pb += [math]::Round($perf.PrivateBytes / 1MB, 1)
        if ($f -like '*.md') {
            [void][Win]::PostMessage($r.Hwnd, $WM_APP_SIM_EDIT, [IntPtr]::Zero, [IntPtr]::Zero)
            Start-Sleep -Milliseconds 1500
        }
        Stop-Proto $r
        Write-Host ("{0} run {1}: shown {2} ms, private WS {3} MB, private bytes {4} MB" -f $f, $i, $r.ShownMs, $ws[-1], $pb[-1])
        Start-Sleep -Seconds 2
    }
    $rows += [pscustomobject]@{
        File = $f
        SizeMB = [math]::Round((Get-Item (Join-Path $Dir $f)).Length / 1MB, 1)
        ShownMs = Median $shown
        ShownRuns = ($shown -join ' / ')
        PrivateWsMB = Median $ws
        PrivateBytesMB = Median $pb
    }
}

Write-Host "`nexe: $Exe ($exeSize bytes)"
$rows | Format-Table -AutoSize | Out-String | Write-Host

Write-Host 'Styling passes from the log (ms):'
Get-Content $log | Where-Object { $_ -match 'style-pass|painted-after-style' } | ForEach-Object { Write-Host "  $_" }

# Screenshot of notes.md via PrintWindow.
$png = Join-Path $Dir 'notes.png'
$r = Start-Proto 'notes.md'
Start-Sleep -Milliseconds 1500
$dim = [Win]::Capture($r.Hwnd, $png)
Stop-Proto $r
Write-Host "`nscreenshot: $png ($dim)"
