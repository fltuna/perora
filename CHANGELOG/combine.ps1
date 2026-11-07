# Writes CHANGELOGS.md and CHANGELOGS.en.md: every version's file in this folder
# (v<version>_CHANGELOGS.md in Japanese, v<version>_CHANGELOGS.en.md in English), newest first.
# Run it after writing a version's files, and commit what it writes; the release workflow runs it
# with -Check, which fails when what is committed is not what it would write.
#
#   powershell -NoProfile -File CHANGELOG\combine.ps1 [-Check]
param([switch]$Check)
$ErrorActionPreference = 'Stop'

# The version a file is of, to put the files in order: 0.2.0-rc.1 before 0.2.0, as semver has it.
function Version-Of([string]$name) {
    if ($name -notmatch '^v(\d+)\.(\d+)\.(\d+)(-[^_]+)?_') { return $null }
    $pre = if ($Matches[4]) { 0 } else { 1 }
    return [pscustomobject]@{
        Key = '{0:D6}.{1:D6}.{2:D6}.{3}.{4}' -f [int]$Matches[1], [int]$Matches[2], [int]$Matches[3], $pre, $Matches[4]
    }
}

$utf8 = New-Object System.Text.UTF8Encoding($false)
$failed = $false
# ("変更履歴" by its code points: Windows PowerShell reads a script without a BOM in the system's
# code page, not as UTF-8.)
$japaneseTitle = '# ' + (-join ([char[]](0x5909, 0x66F4, 0x5C65, 0x6B74)))
foreach ($language in @(@{ Suffix = '_CHANGELOGS.md'; Out = 'CHANGELOGS.md'; Title = $japaneseTitle },
                        @{ Suffix = '_CHANGELOGS.en.md'; Out = 'CHANGELOGS.en.md'; Title = '# Changelog' })) {
    $files = Get-ChildItem -Path $PSScriptRoot -File |
        Where-Object { $_.Name.EndsWith($language.Suffix) -and ($language.Suffix -ne '_CHANGELOGS.md' -or -not $_.Name.EndsWith('_CHANGELOGS.en.md')) } |
        Where-Object { Version-Of $_.Name } |
        Sort-Object { (Version-Of $_.Name).Key } -Descending
    $parts = @($language.Title)
    foreach ($file in $files) {
        # (Line ends as the repository has them, whatever a checkout made of them.)
        $parts += ([IO.File]::ReadAllText($file.FullName, $utf8)).Replace("`r`n", "`n").Trim()
    }
    $text = ($parts -join "`n`n") + "`n"
    $out = Join-Path $PSScriptRoot $language.Out
    if ($Check) {
        $there = if (Test-Path $out) { [IO.File]::ReadAllText($out, $utf8).Replace("`r`n", "`n") } else { '' }
        if ($there -ne $text) {
            Write-Error "$($language.Out) is not what combine.ps1 writes: run it, and commit what it wrote" -ErrorAction Continue
            $failed = $true
        }
    } else {
        [IO.File]::WriteAllText($out, $text, $utf8)
        Write-Output "$($language.Out): $($files.Count) versions"
    }
}
# (Said either way: after a script that ends without `exit`, its caller's $LASTEXITCODE is unset.)
if ($failed) { exit 1 }
exit 0
