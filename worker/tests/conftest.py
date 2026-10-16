import os
from pathlib import Path

# On Windows, Pillow needs fribidi-0.dll on the DLL search path before it is imported
# (see mangaai_worker/native.py). Use the copy built by scripts/build-fribidi.ps1.
_native = Path(__file__).resolve().parents[2] / "src-tauri" / "windows" / "native"
if _native.is_dir():
    os.environ.setdefault("MANGAAI_NATIVE_DIR", str(_native))

from mangaai_worker import native  # noqa: E402

native.setup()
