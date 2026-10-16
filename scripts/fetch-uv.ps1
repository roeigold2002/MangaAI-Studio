# Put a pinned uv.exe into src-tauri\windows\native\ so the installer ships it: installing the
# AI engine then needs nothing else on the computer. uv is MIT/Apache-2.0 (astral-sh/uv).
param([switch]$IfMissing)
$ErrorActionPreference = "Stop"

$Version = "0.12.10"
$Sha256 = "f65744f94072152b1f86ba2aace4d01f1124d9a8ecb235805039e3718c36cac2"
$Asset = "uv-x86_64-pc-windows-msvc.zip"
$Url = "https://github.com/astral-sh/uv/releases/download/$Version/$Asset"

$Root = Split-Path -Parent $PSScriptRoot
$Out = Join-Path $Root "src-tauri\windows\native"
if ($IfMissing -and (Test-Path (Join-Path $Out "uv.exe"))) { exit 0 }

$Work = Join-Path ([System.IO.Path]::GetTempPath()) "mangaai-uv-$Version"
New-Item -ItemType Directory -Force $Work, $Out | Out-Null
$Zip = Join-Path $Work $Asset
if (-not (Test-Path $Zip)) {
    Write-Host "-> Downloading uv $Version"
    Invoke-WebRequest -Uri $Url -OutFile $Zip
}
$Got = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLower()
if ($Got -ne $Sha256) {
    Remove-Item $Zip
    throw "checksum mismatch for $Zip (got $Got)"
}
Expand-Archive -Path $Zip -DestinationPath $Work -Force
Copy-Item (Join-Path $Work "uv.exe") $Out -Force
Write-Host "Done: $Out\uv.exe (uv $Version)"
