# Build fribidi-0.dll for Windows (x64, static CRT) into src-tauri\windows\native\.
#
# Pillow's Windows wheels ship libraqm + HarfBuzz but load FriBiDi at run time
# (LoadLibraryA "fribidi" / "fribidi-0" / "libfribidi-0"); without it Hebrew is not
# shaped. The app bundles this DLL and puts its folder on the worker's PATH.
#
# Needs: Visual Studio (or Build Tools) with the C++ workload, and uv.
# FriBiDi is LGPL-2.1; it is shipped unmodified as a separate, dynamically loaded DLL.
param([switch]$IfMissing)
$ErrorActionPreference = "Stop"

$Version = "1.0.17"
$Sha256 = "6949dcde27d41cebad1fd741fcafc36d55a1020d2d872d4a6eb3914caabbada2"
$Url = "https://github.com/fribidi/fribidi/releases/download/v$Version/fribidi-$Version.tar.xz"

$Root = Split-Path -Parent $PSScriptRoot
$Out = Join-Path $Root "src-tauri\windows\native"
$Work = Join-Path ([System.IO.Path]::GetTempPath()) "mangaai-fribidi-$Version"
if ($IfMissing -and (Test-Path (Join-Path $Out "fribidi-0.dll"))) { exit 0 }

if (-not (Get-Command uv -ErrorAction SilentlyContinue)) { throw "uv is required: winget install astral-sh.uv" }

New-Item -ItemType Directory -Force $Work, $Out | Out-Null
$Tarball = Join-Path $Work "fribidi-$Version.tar.xz"
if (-not (Test-Path $Tarball)) {
    Write-Host "-> Downloading FriBiDi $Version"
    Invoke-WebRequest -Uri $Url -OutFile $Tarball
}
$Got = (Get-FileHash -Algorithm SHA256 $Tarball).Hash.ToLower()
if ($Got -ne $Sha256) {
    Remove-Item $Tarball
    throw "checksum mismatch for $Tarball (got $Got)"
}

$Src = Join-Path $Work "fribidi-$Version"
if (-not (Test-Path $Src)) { tar -xf $Tarball -C $Work }

Write-Host "-> Building with MSVC"
$Build = Join-Path $Src "build"
if (-not (Test-Path $Build)) {
    uvx --with ninja --from meson meson setup $Build $Src --vsenv --buildtype=release `
        -Ddocs=false -Dbin=false -Dtests=false --default-library=shared -Db_vscrt=mt
    if ($LASTEXITCODE) { throw "meson setup failed" }
}
uvx --with ninja --from meson meson compile -C $Build
if ($LASTEXITCODE) { throw "meson compile failed" }

Copy-Item (Join-Path $Build "lib\fribidi-0.dll") $Out -Force
Copy-Item (Join-Path $Src "COPYING") (Join-Path $Out "COPYING-fribidi.txt") -Force
Write-Host "Done: $Out\fribidi-0.dll"
