# Installs or updates scrivi for the current user. Rerun it to update.
#   release -> copy scrivi.exe -> scrivi.exe --register -> Default apps step
#   source  -> build -> copy -> --register -> Default apps step
# A scrivi.exe next to this script (an unzipped release) is installed as is, with no build.
# -Destination  install folder (default %LOCALAPPDATA%\Programs\scrivi)
# -NoRegister   copy the exe only; leave the registry alone
[CmdletBinding()]
param(
    [string]$Destination = (Join-Path $env:LOCALAPPDATA 'Programs\scrivi'),
    [switch]$NoRegister
)
$ErrorActionPreference = 'Stop'

# The exe path of a process, or $null when Windows will not tell us.
# Get-Process .Path is empty for a process this user may not open (elevated, or
# another account), so fall back to CIM, which usually still knows.
function Get-ScriviProcessPath {
    param([Parameter(Mandatory)] $Process)
    if ($Process.Path) { return $Process.Path }
    try {
        $cim = Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $($Process.Id)" -ErrorAction Stop
        if ($cim.ExecutablePath) { return $cim.ExecutablePath }
    } catch {
        return $null
    }
    return $null
}

# Never overwrite a running scrivi. Match by exe path, not by image name.
# A scrivi whose path cannot be read counts as running: refusing to install beats
# failing at Copy-Item with a sharing violation.
# Compare canonical paths too: a process can report the 8.3 short form of the exe.
function Find-RunningScrivi {
    param([object[]] $Processes, [string] $Exe)
    $target = (Get-Item -LiteralPath $Exe -ErrorAction SilentlyContinue).FullName
    if (-not $target) { $target = [IO.Path]::GetFullPath($Exe) }
    foreach ($process in $Processes) {
        $path = Get-ScriviProcessPath -Process $process
        if (-not $path) { $process; continue }
        $full = (Get-Item -LiteralPath $path -ErrorAction SilentlyContinue).FullName
        if (-not $full) { $full = [IO.Path]::GetFullPath($path) }
        if ($full -eq $target) { $process }
    }
}

# install.tests.ps1 dot-sources this file with SCRIVI_INSTALL_TEST=1 to load the two
# functions above and nothing else: no build, no copy, no --register.
if ($env:SCRIVI_INSTALL_TEST -eq '1') { return }

$prebuilt = Join-Path $PSScriptRoot 'scrivi.exe'
$repo = Split-Path -Parent $PSScriptRoot
$built = Join-Path $repo 'target\release\scrivi.exe'
$dir = [IO.Path]::GetFullPath($Destination)
$exe = Join-Path $dir 'scrivi.exe'

if (Test-Path -LiteralPath $prebuilt) {
    Write-Host 'Using the release build next to this script (no build)...'
    $source = $prebuilt
} else {
    Write-Host 'Building (release)...'
    Push-Location $repo
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed (exit $LASTEXITCODE)" }
    } finally { Pop-Location }
    $source = $built
}

# Never overwrite a running scrivi, from this exe or from one we cannot inspect.
$running = @(Find-RunningScrivi -Processes @(Get-Process -Name 'scrivi' -ErrorAction SilentlyContinue) -Exe $exe)
if ($running.Count -gt 0) {
    throw "scrivi is running (PID $($running.Id -join ', ')). Close it, then rerun this script."
}

New-Item -ItemType Directory -Force -Path $dir | Out-Null
Copy-Item -LiteralPath $source -Destination $exe -Force
# A downloaded exe carries Mark-of-the-Web; drop it from the installed copy.
Unblock-File -LiteralPath $exe -ErrorAction SilentlyContinue
Write-Host "Installed $exe"

if ($NoRegister) {
    Write-Host 'Skipped --register (-NoRegister).'
    return
}

& $exe --register
if ($LASTEXITCODE -ne 0) { throw "scrivi.exe --register failed (exit $LASTEXITCODE)" }

Write-Host ''
Write-Host 'Last step: in Settings > Apps > Default apps, pick scrivi for the file types you want.'
Write-Host '(--register opens that page; the choice is yours and is made once.)'
