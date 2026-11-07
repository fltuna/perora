# Writes the latest.json of a release: what the Perora already installed reads to learn that there
# is a newer version (crates/core/src/update.rs, and the Tauri version's updater before it), beside
# the installer and its signature on the release.
#
#   pwsh -File installer\latest.ps1 -Version 0.2.0 -Signature <installer>.sig -Out latest.json
#
# What changed is taken from CHANGELOG/v<version>_CHANGELOGS.md (Japanese) and
# CHANGELOG/v<version>_CHANGELOGS.en.md (English), the files whose names begin with the version
# (`changes`, all 0.2.0 reads); and what changed in the last versions up to it from
# CHANGELOG/CHANGELOGS.md and .en.md, the newest -HistoryVersions of them (`history`: from it,
# Perora 0.2.1 on shows what changed since the version installed, when more than one was passed).
param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Signature,
    [Parameter(Mandatory)][string]$Out,
    [int]$HistoryVersions = 10
)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$utf8 = New-Object System.Text.UTF8Encoding($false)

function Changes-In([string]$name) {
    $path = Join-Path $root "CHANGELOG\$name"
    if (-not (Test-Path $path)) { return $null }
    # (Line ends as the repository has them, whatever a checkout made of them.)
    return ([IO.File]::ReadAllText($path, $utf8)).Replace("`r`n", "`n").Trim()
}

# The newest versions of a changelog of them all, cut at their `## <version>` headings (the file's
# own `#` title left out).
function History-In([string]$name) {
    $text = Changes-In $name
    if (-not $text) { return $null }
    $versions = @([regex]::Split("`n$text", "`n(?=## )") | Where-Object { $_.StartsWith('## ') })
    if ($versions.Count -eq 0) { return $null }
    return (@($versions | Select-Object -First $HistoryVersions | ForEach-Object { $_.Trim() })) -join "`n`n"
}

$tag = "v$Version"
$installer = "Perora_${Version}_x64-setup.exe"
$url = "https://github.com/fltuna/perora/releases/download/$tag/$installer"
# The signature as `tauri signer sign` wrote it: minisign's, in base64, on one line.
$signed = ([IO.File]::ReadAllText((Resolve-Path $Signature).Path, $utf8)).Trim()
$platform = [ordered]@{ signature = $signed; url = $url }

$changes = [ordered]@{}
$ja = Changes-In "${tag}_CHANGELOGS.md"
$en = Changes-In "${tag}_CHANGELOGS.en.md"
if ($ja) { $changes.ja = $ja }
if ($en) { $changes.en = $en }

$history = [ordered]@{}
$jaHistory = History-In 'CHANGELOGS.md'
$enHistory = History-In 'CHANGELOGS.en.md'
if ($jaHistory) { $history.ja = $jaHistory }
if ($enHistory) { $history.en = $enHistory }

$latest = [ordered]@{
    version = $Version
    # A text: what the Tauri version's updater reads as `notes`. Anything but a text there, and it
    # could read none of this, and would update no more. (What changed is in `changes`.)
    notes = 'Download and run Perora_*_x64-setup.exe. Perora already installed updates itself.'
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'")
    changes = $changes
    history = $history
    platforms = [ordered]@{
        'windows-x86_64' = $platform
        'windows-x86_64-nsis' = $platform
    }
}
[IO.File]::WriteAllText($Out, ($latest | ConvertTo-Json -Depth 5), $utf8)
Write-Output "$Out ($(if ($ja) { 'ja' } else { 'no ja' }), $(if ($en) { 'en' } else { 'no en' }))"
