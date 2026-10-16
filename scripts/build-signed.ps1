# Build the Windows installer with code signing (see scripts/sign-windows.ps1 for the credentials).
# Ordinary `npm run app:build` stays unsigned and needs no Windows SDK.
#
#   $env:MANGAAI_SIGN_PFX = "C:\path\cert.pfx"; $env:MANGAAI_SIGN_PFX_PASSWORD = "..."
#   .\scripts\build-signed.ps1
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

if (-not ($env:MANGAAI_SIGN_PFX -or $env:MANGAAI_SIGN_PFX_BASE64 -or ($env:AZURE_CLIENT_ID -and $env:MANGAAI_TS_ACCOUNT))) {
    throw "no signing credentials: set MANGAAI_SIGN_PFX (+ MANGAAI_SIGN_PFX_PASSWORD) or the Azure Trusted Signing variables"
}
# The Tauri CLI signs the NSIS uninstaller itself and finds signtool through this variable
# (or the Windows Kits registry key, which it does not always find).
if (-not $env:TAURI_WINDOWS_SIGNTOOL_PATH) {
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $tool = Get-ChildItem $kits -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\" } | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $tool) { throw "signtool.exe not found: install the Windows SDK" }
    $env:TAURI_WINDOWS_SIGNTOOL_PATH = $tool.FullName
}
Push-Location $Root
try {
    npm run tauri -- build --config src-tauri/tauri.signing.conf.json
    if ($LASTEXITCODE) { throw "build failed" }
} finally {
    Pop-Location
}
$installer = Get-ChildItem (Join-Path $Root "src-tauri\target\release\bundle\nsis") -Filter *-setup.exe | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Get-AuthenticodeSignature $installer.FullName | Format-List Status, StatusMessage, SignerCertificate, TimeStamperCertificate
