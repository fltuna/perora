# Builds Perora's installer, Perora_<version>_x64-setup.exe, from a release build of perora.exe
# (`cargo build --release -p perora-app`). The release workflow runs it, and so can anyone who has
# NSIS: by default the NSIS the Tauri CLI left in %LOCALAPPDATA%\tauri\NSIS, which has the plugin
# the script needs (nsis_tauri_utils.dll) in Plugins\x86-unicode\additional.
#
#   powershell -NoProfile -File installer\build.ps1 [-Makensis <makensis.exe>] [-Plugins <folder>]
#
# The installer is not signed here: the release workflow signs it for the updater afterwards.
param(
    [string]$Makensis = "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe",
    [string]$Plugins = "$env:LOCALAPPDATA\tauri\NSIS\Plugins\x86-unicode\additional",
    [string]$Exe = "",
    [string]$OutDir = ""
)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
if (-not $Exe) { $Exe = Join-Path $root 'target\release\perora.exe' }
if (-not $OutDir) { $OutDir = Join-Path $root 'target\release\installer' }

# The version is the workspace's (Cargo.toml, [workspace.package]).
$manifest = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($manifest -notmatch '(?ms)^\[workspace\.package\]\s*\r?\nversion = "([^"]+)"') {
    throw 'No version in Cargo.toml [workspace.package]'
}
$version = $Matches[1]
# Windows wants four numbers: 0.2.0-rc.1 is 0.2.0.0 to it, as Tauri made it.
$numbers = ($version -split '[-+]')[0]
$withBuild = "$numbers.0"

if (-not (Test-Path $Exe)) { throw "No ${Exe}: build it first (cargo build --release -p perora-app)" }
if (-not (Test-Path (Join-Path $Plugins 'nsis_tauri_utils.dll'))) { throw "No nsis_tauri_utils.dll in $Plugins" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$out = Join-Path $OutDir "Perora_${version}_x64-setup.exe"
# What the uninstall list shows as its size, in KiB (the program; the uninstaller is added to it).
$size = [math]::Ceiling((Get-Item $Exe).Length / 1024)

& $Makensis -V3 -INPUTCHARSET UTF8 `
    "-DVERSION=$version" `
    "-DVERSIONWITHBUILD=$withBuild" `
    "-DMAINBINARYSRCPATH=$((Resolve-Path $Exe).Path)" `
    "-DOUTFILE=$out" `
    "-DADDITIONALPLUGINSPATH=$((Resolve-Path $Plugins).Path)" `
    "-DESTIMATEDSIZE=$size" `
    (Join-Path $PSScriptRoot 'installer.nsi') | Out-Host
if ($LASTEXITCODE -ne 0) { throw "makensis failed ($LASTEXITCODE)" }
# (The one thing this writes out: where the installer is.)
Write-Output $out
