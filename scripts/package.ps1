<#
Packages the built Seamless Music Player into the release zip the deps script installs from.
Reads the version from Cargo.toml unless -Version is given, copies target\release\smp.exe
across as 'Seamless Music Player.exe' (plus LICENSE when the repo has one), zips that to
dist\SeamlessMusicPlayer-<v>-win64.zip and writes dist\SHA256SUMS. It never builds.
#>
param([string]$Version)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent

if (-not $Version) {
    $cargo = Join-Path $repo 'Cargo.toml'
    if (-not (Test-Path $cargo)) { throw "No Cargo.toml beside this script's folder ($repo). Pass -Version instead." }
    foreach ($line in (Get-Content $cargo)) {
        if ($line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
    }
    if (-not $Version) { throw 'Cargo.toml has no version = "..." line.' }
}

$exe = Join-Path $repo 'target\release\smp.exe'
if (-not (Test-Path $exe)) { throw "No built exe at $exe. Run cargo build --release first; this script never builds." }

$dist = Join-Path $repo 'dist'
$zipName = 'SeamlessMusicPlayer-' + $Version + '-win64.zip'
$zipPath = Join-Path $dist $zipName
$stage = Join-Path ([IO.Path]::GetTempPath()) ('smp-package-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Force $stage, $dist
try {
    Copy-Item $exe (Join-Path $stage 'Seamless Music Player.exe')
    $lic = Join-Path $repo 'LICENSE'
    if (Test-Path $lic) { Copy-Item $lic $stage }
    if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zipPath -Force
    $hash = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLower()
    [IO.File]::WriteAllText((Join-Path $dist 'SHA256SUMS'), "$hash  $zipName`r`n", (New-Object Text.UTF8Encoding($false)))
    Write-Output "Packaged $zipName ($hash)"
} finally {
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
}
