# Set up the AI engine (Python vision worker) for MangaAI Studio on Windows.
# Creates the environment in the app data folder and fetches models.
# Network: PyPI (packages), Hugging Face (detector + OCR weights), GitHub (LaMa weights).
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $PSScriptRoot
$Data = if ($env:MANGAAI_DATA_DIR) { $env:MANGAAI_DATA_DIR } else { Join-Path $env:APPDATA "com.mangaai.studio" }
New-Item -ItemType Directory -Force (Join-Path $Data "models") | Out-Null

if (-not (Get-Command uv -ErrorAction SilentlyContinue)) { throw "uv is required: winget install astral-sh.uv" }

$Native = Join-Path $Root "src-tauri\windows\native"
if (-not (Test-Path (Join-Path $Native "fribidi-0.dll"))) {
    Write-Warning "fribidi-0.dll not found; Hebrew shaping needs it. Run scripts\build-fribidi.ps1 (installed app builds bundle it)."
}

$Venv = Join-Path $Data "worker-venv"
Write-Host "-> Python environment: $Venv"
$env:UV_PROJECT_ENVIRONMENT = $Venv
uv sync --managed-python --python 3.12 --no-dev --frozen --project (Join-Path $Root "worker")
if ($LASTEXITCODE) { throw "uv sync failed" }
Remove-Item Env:UV_PROJECT_ENVIRONMENT

Write-Host "-> Models"
$env:MANGAAI_MODELS_DIR = Join-Path $Data "models"
$env:MANGAAI_NATIVE_DIR = $Native
Push-Location (Join-Path $Root "worker")
try {
    & (Join-Path $Venv "Scripts\python.exe") -c "from mangaai_worker import native; native.setup(); from mangaai_worker import models; print(models.prepare(print)); from PIL import features; print('raqm:', features.check('raqm'))"
    if ($LASTEXITCODE) { throw "model download failed" }
} finally {
    Pop-Location
}
Write-Host "Done. Start the app; Translation settings -> AI engine should show all green."
