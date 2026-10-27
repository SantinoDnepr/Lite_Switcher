# Builds the release and collects a folder (and a zip) that can be unpacked anywhere:
# the programs, Language, LiteraryLanguages, ProgrammingLanguages, licences and docs.
#
# usage: pwsh -File tools\release.ps1 [-WithProgramming]
#
# The programming packs are private data (built from your own code): they are copied only with
# -WithProgramming, and the local lexicon (ProgrammingLanguages\*\local) is never copied.
param([switch]$WithProgramming)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

cargo build --release --workspace
if ($LASTEXITCODE -ne 0) { throw 'the build failed' }

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$name = "Lite_Switcher-$version"
$dist = Join-Path $root "dist\$name"
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Force $dist | Out-Null

foreach ($exe in 'lite-switcher.exe', 'lite-switcher-settings.exe', 'dictgen.exe') {
    Copy-Item "target\release\$exe" $dist
}
Copy-Item -Recurse Language, Help, LiteraryLanguages $dist
if ($WithProgramming) {
    Copy-Item -Recurse ProgrammingLanguages $dist
    Get-ChildItem (Join-Path $dist 'ProgrammingLanguages') -Directory | ForEach-Object {
        $local = Join-Path $_.FullName 'local'
        if (Test-Path $local) { Remove-Item -Recurse -Force $local }
    }
} else {
    New-Item -ItemType Directory -Force (Join-Path $dist 'ProgrammingLanguages') | Out-Null
}
Copy-Item README.md, LICENSE, LICENSE-DATA, NOTICE, history.md $dist
Copy-Item -Recurse docs $dist

$zip = Join-Path $root "dist\$name.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $dist -DestinationPath $zip
Write-Host "$zip"
