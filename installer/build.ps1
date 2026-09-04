# Build the release binary and wrap it in the per-user installer.
#
#   powershell -ExecutionPolicy Bypass -File installer\build.ps1
#
# Needs NSIS on PATH (`makensis`). Everything else comes from the repository.

[CmdletBinding()]
param(
    # Skip the cargo build and package whatever is already in target\release.
    [switch]$NoBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}

# The one source of truth for the version. NSIS is told rather than asked, so
# the installer and the executable can never disagree.
$version = (Select-String -Path Cargo.toml -Pattern '^version\.workspace|^version = "([^"]+)"' |
            Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) {
    $version = (Select-String -Path Cargo.toml -Pattern '^\s*version = "([^"]+)"' |
                Select-Object -First 1).Matches.Groups[1].Value
}
if (-not $version) { throw "could not read the version out of Cargo.toml" }

Write-Host "== Zerem $version ==" -ForegroundColor Cyan

if (-not $NoBuild) {
    Write-Host '-- cargo build --release' -ForegroundColor Cyan
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'the release build failed' }
}

$exe = Join-Path $root 'target\release\zerem.exe'
if (-not (Test-Path $exe)) { throw "no binary at $exe" }
$mb = [math]::Round((Get-Item $exe).Length / 1MB, 2)
Write-Host "-- zerem.exe is $mb MB"

# The icon is generated into OUT_DIR during the build, at a path with a hash in
# it. NSIS wants a stable one, so the freshest copy is placed where the script
# expects it.
$ico = Get-ChildItem 'target\release\build' -Recurse -Filter 'zerem.ico' -ErrorAction SilentlyContinue |
       Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($ico) {
    Copy-Item $ico.FullName 'target\release\build\zerem.ico' -Force
} else {
    throw 'the generated icon is missing — did the release build run?'
}

if (-not (Get-Command makensis -ErrorAction SilentlyContinue)) {
    throw 'makensis is not on PATH. Install NSIS from https://nsis.sourceforge.io'
}

Write-Host '-- makensis' -ForegroundColor Cyan
makensis /DVERSION=$version (Join-Path $PSScriptRoot 'zerem.nsi')
if ($LASTEXITCODE -ne 0) { throw 'makensis failed' }

$setup = Join-Path $PSScriptRoot 'Zerem-Setup.exe'
$setupMb = [math]::Round((Get-Item $setup).Length / 1MB, 2)
Write-Host ''
Write-Host "Zerem-Setup.exe  $setupMb MB" -ForegroundColor Green
Write-Host "  installs to    %LOCALAPPDATA%\Programs\Zerem"
Write-Host '  no admin, no UAC'
Write-Host ''
Write-Host 'Not signed yet, so SmartScreen will warn on first run.' -ForegroundColor Yellow
