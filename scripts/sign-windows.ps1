# Code-sign one Windows file (Tauri calls this for the app .exe, the installer and the uninstaller
# through bundle.windows.signCommand in tauri.windows.conf.json).
#
# Credentials come from the environment; with none set the file is left unsigned and the build
# continues, so ordinary local builds need nothing.
#
#   Certificate file (OV/EV .pfx from a certificate authority):
#     MANGAAI_SIGN_PFX           path to the .pfx, or
#     MANGAAI_SIGN_PFX_BASE64    the .pfx as base64 (how a GitHub secret holds it)
#     MANGAAI_SIGN_PFX_PASSWORD  its password
#   Azure Trusted Signing (about $10/month; needs `cargo install trusted-signing-cli`):
#     AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET
#     MANGAAI_TS_ENDPOINT (e.g. https://weu.codesigning.azure.net), MANGAAI_TS_ACCOUNT, MANGAAI_TS_PROFILE
#   MANGAAI_SIGN_TIMESTAMP       RFC 3161 server (default http://timestamp.digicert.com; "none" = no timestamp)
param([Parameter(Mandatory = $true)][string]$File)
$ErrorActionPreference = "Stop"

if (-not (Test-Path $File)) { throw "sign-windows: file not found: $File" }
$Description = "MangaAI Studio"
$Timestamp = if ($env:MANGAAI_SIGN_TIMESTAMP) { $env:MANGAAI_SIGN_TIMESTAMP } else { "http://timestamp.digicert.com" }

function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $found = Get-ChildItem $kits -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\" } | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $found) { throw "sign-windows: signtool.exe not found (install the Windows SDK)" }
    return $found.FullName
}

if ($env:AZURE_CLIENT_ID -and $env:MANGAAI_TS_ACCOUNT) {
    Write-Host "sign-windows: Azure Trusted Signing -> $File"
    & trusted-signing-cli -e $env:MANGAAI_TS_ENDPOINT -a $env:MANGAAI_TS_ACCOUNT -c $env:MANGAAI_TS_PROFILE -d $Description $File
    if ($LASTEXITCODE) { throw "sign-windows: trusted-signing-cli failed ($LASTEXITCODE)" }
    exit 0
}

$Pfx = $env:MANGAAI_SIGN_PFX
$Temp = $null
if (-not $Pfx -and $env:MANGAAI_SIGN_PFX_BASE64) {
    $Temp = Join-Path ([System.IO.Path]::GetTempPath()) ("mangaai-sign-" + [guid]::NewGuid() + ".pfx")
    [System.IO.File]::WriteAllBytes($Temp, [Convert]::FromBase64String($env:MANGAAI_SIGN_PFX_BASE64))
    $Pfx = $Temp
}
if (-not $Pfx) {
    Write-Host "sign-windows: no signing credentials set; $File stays unsigned"
    exit 0
}

try {
    $signtool = Find-SignTool
    $args = @("sign", "/fd", "SHA256", "/d", $Description, "/f", $Pfx)
    if ($env:MANGAAI_SIGN_PFX_PASSWORD) { $args += @("/p", $env:MANGAAI_SIGN_PFX_PASSWORD) }
    if ($Timestamp -ne "none") { $args += @("/tr", $Timestamp, "/td", "SHA256") }
    Write-Host "sign-windows: signtool -> $File"
    & $signtool @args $File
    if ($LASTEXITCODE) { throw "sign-windows: signtool failed ($LASTEXITCODE)" }
} finally {
    if ($Temp) { Remove-Item $Temp -Force -ErrorAction SilentlyContinue }
}
