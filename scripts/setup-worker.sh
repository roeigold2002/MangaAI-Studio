#!/usr/bin/env bash
# Set up the AI engine (Python vision worker) for MangaAI Studio.
# Creates the environment in the app data folder and fetches models.
# Network: PyPI (packages), Hugging Face (detector + OCR weights), GitHub (LaMa weights).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA="${MANGAAI_DATA_DIR:-$HOME/Library/Application Support/com.mangaai.studio}"
mkdir -p "$DATA/models"

command -v uv >/dev/null || { echo "uv is required: brew install uv" >&2; exit 1; }
if ! ls /opt/homebrew/lib/libfribidi* /usr/local/lib/libfribidi* >/dev/null 2>&1; then
  echo "FriBidi not found; Hebrew shaping needs it: brew install fribidi" >&2
fi

echo "→ Python environment: $DATA/worker-venv"
UV_PROJECT_ENVIRONMENT="$DATA/worker-venv" uv sync --managed-python --python 3.12 --no-dev --frozen --project "$ROOT/worker"

echo "→ Models"
cd "$ROOT/worker"
MANGAAI_MODELS_DIR="$DATA/models" "$DATA/worker-venv/bin/python" - <<'PY'
from mangaai_worker import models
print(models.prepare(print))
PY
echo "Done. Start the app; Translation settings → AI engine should show all green."
