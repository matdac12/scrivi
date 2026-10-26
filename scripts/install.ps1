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

# Never overwrite a running scrivi. Match by exe path, not by image name.
$running = @(Get-Process -Name 'scrivi' -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $exe })
if ($running.Count -gt 0) {
    throw "scrivi is running from $exe (PID $($running.Id -join ', ')). Close it, then rerun this script."
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
