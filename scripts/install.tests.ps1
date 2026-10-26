# Checks install.ps1's running-scrivi check without installing anything.
# Run it in either shell: powershell -File scripts\install.tests.ps1
#                       pwsh -File scripts\install.tests.ps1
$ErrorActionPreference = 'Stop'

# Load install.ps1's functions only: it must not build, copy or register here.
# Without the guard, dot-sourcing would run the whole installer, so check first.
$script = Join-Path $PSScriptRoot 'install.ps1'
if (-not (Select-String -LiteralPath $script -Pattern 'SCRIVI_INSTALL_TEST' -Quiet)) {
    throw "$script has no SCRIVI_INSTALL_TEST guard; refusing to dot-source it."
}
$env:SCRIVI_INSTALL_TEST = '1'
try {
    . $script
} finally {
    Remove-Item Env:SCRIVI_INSTALL_TEST -ErrorAction SilentlyContinue
}

$failures = 0
function Check([string] $Name, [bool] $Ok) {
    if ($Ok) { Write-Host "ok   $Name" } else { Write-Host "FAIL $Name"; $script:failures++ }
}

$exe = 'C:\Tools\scrivi\scrivi.exe'
$processes = @(
    [pscustomobject]@{ Id = 11; Path = $exe },
    [pscustomobject]@{ Id = 12; Path = 'C:\Other\scrivi.exe' },
    [pscustomobject]@{ Id = 13; Path = $null }
)

# The installed copy must not be overwritten, wherever it runs from.
$running = @(Find-RunningScrivi -Processes $processes -Exe $exe)
Check 'a scrivi running from this exe blocks' ($running.Id -contains 11)
Check 'a scrivi running from another path does not' (-not ($running.Id -contains 12))
Check 'a scrivi whose path cannot be read blocks' ($running.Id -contains 13)
Check 'no scrivi processes, nothing running' (@(Find-RunningScrivi -Processes @() -Exe $exe).Count -eq 0)

# Windows can report the same exe by its 8.3 short name; that must still match.
$temp = Join-Path $env:TEMP 'scrivi-path-alias-test'
New-Item -ItemType Directory -Force -Path $temp | Out-Null
try {
    $file = Join-Path $temp 'scrivi.exe'
    Set-Content -LiteralPath $file -Value ''
    $shortDir = (New-Object -ComObject Scripting.FileSystemObject).GetFolder($temp).ShortPath
    if ($shortDir -ne $temp) {
        $aliased = Join-Path $shortDir 'scrivi.exe'
        $found = @(Find-RunningScrivi -Processes @([pscustomobject]@{ Id = 14; Path = $aliased }) -Exe $file)
        Check 'a short-name path for this exe blocks' ($found.Id -contains 14)
    } else {
        Write-Host 'ok   (skipped: this volume has no 8.3 short names)'
    }
} finally {
    $left = @(Get-ChildItem -LiteralPath $temp -Force -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName)
    if ($left.Count -gt 0) { Remove-Item -LiteralPath $left -Force }
    Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue
}

# A real process Get-Process cannot open still has a path in CIM.
$me = Get-Process -Id $PID
Check 'CIM fills a path Get-Process could not read' (
    (Get-ScriviProcessPath -Process ([pscustomobject]@{ Id = $me.Id; Path = $null })) -eq $me.Path)
Check 'a process that has gone stays unknown' (
    $null -eq (Get-ScriviProcessPath -Process ([pscustomobject]@{ Id = 999999999 })))

if ($failures -gt 0) {
    Write-Host "$failures check(s) failed."
    exit 1
}
Write-Host 'All checks passed.'
