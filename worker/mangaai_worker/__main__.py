"""JSON-lines RPC loop: one request per stdin line, one response per stdout line.

Request:  {"id": 1, "method": "analyze", "params": {...}}
Response: {"id": 1, "ok": true, "result": {...}}
          {"id": 1, "ok": false, "error": {"code": "...", "message": "..."}}
"""

from __future__ import annotations

import json
import logging
import os
import sys
import traceback

# The app always speaks UTF-8; on Windows the default would be the ANSI code page,
# which mangles Hebrew/Japanese text and non-ASCII paths.
sys.stdin.reconfigure(encoding="utf-8")
# Keep the protocol channel clean: libraries that print go to stderr.
_proto = os.fdopen(os.dup(sys.stdout.fileno()), "w", encoding="utf-8", buffering=1, newline="\n")
sys.stdout = sys.stderr
sys.stderr.reconfigure(encoding="utf-8", errors="backslashreplace")

from . import native  # noqa: E402  (must run before Pillow is imported)

native.setup()

logging.basicConfig(level=logging.INFO, stream=sys.stderr, format="%(asctime)s %(name)s %(levelname)s %(message)s")
log = logging.getLogger("mangaai.worker")


def _health(_params: dict) -> dict:
    import platform

    import PIL
    from PIL import features

    from . import ANALYSIS_VERSION, INPAINT_VERSION, PROTOCOL_VERSION, RENDER_VERSION, models

    info = {
        "protocol": PROTOCOL_VERSION,
        "python": platform.python_version(),
        "pillow": PIL.__version__,
        "raqm": bool(features.check("raqm")),
        "versions": {"analysis": ANALYSIS_VERSION, "inpaint": INPAINT_VERSION, "render": RENDER_VERSION},
        "models": models.status(),
    }
    try:
        import torch

        info["torch"] = torch.__version__
        info["device"] = models.pick_device("auto")
        info["cuda"] = bool(torch.cuda.is_available())
        if info["cuda"]:
            info["gpu"] = torch.cuda.get_device_name(0)
    except Exception as e:  # torch missing => vision disabled, rendering still works
        info["torch"] = None
        info["torch_error"] = str(e)
    return info


def _analyze(p: dict) -> dict:
    from .analyze import analyze

    return analyze(p["image"], p.get("device", "auto"), float(p.get("threshold", 0.35)), p.get("rtl", True), p.get("ocr", True), p.get("lang", "ja"))


def _ocr_region(p: dict) -> dict:
    from .analyze import ocr_region

    return ocr_region(p["image"], p["box"], p.get("device", "auto"), p.get("lang", "ja"))


def _clean(p: dict) -> dict:
    from .clean import clean

    return clean(p["image"], p["out"], p["regions"], p.get("method", "lama"), p.get("device", "auto"))


def _render(p: dict) -> dict:
    from .render import render

    return render(p["clean"], p["out"], p["regions"], p["fonts_dir"], p.get("settings", {}))


def _prepare(_p: dict) -> dict:
    from . import models

    return models.prepare(lambda m: log.info(m))


METHODS = {
    "health": _health,
    "analyze": _analyze,
    "ocr_region": _ocr_region,
    "clean": _clean,
    "render": _render,
    "prepare_models": _prepare,
}


def _error_code(e: Exception) -> str:
    code = getattr(e, "code", None)
    if isinstance(code, str):
        return code
    if isinstance(e, FileNotFoundError):
        return "file_not_found"
    if isinstance(e, MemoryError):
        return "out_of_memory"
    name = type(e).__name__
    if "PIL" in type(e).__module__ or name == "UnidentifiedImageError":
        return "bad_image"
    return "worker_error"


def main() -> None:
    log.info("worker ready (pid %s)", os.getpid())
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        rid = None
        try:
            req = json.loads(line)
            rid = req.get("id")
            method = req.get("method")
            if method == "shutdown":
                _proto.write(json.dumps({"id": rid, "ok": True, "result": {}}) + "\n")
                break
            fn = METHODS.get(method)
            if fn is None:
                raise ValueError(f"unknown method {method}")
            result = fn(req.get("params") or {})
            resp = {"id": rid, "ok": True, "result": result}
        except Exception as e:  # report every failure to the caller, keep serving
            log.error("request %s failed: %s\n%s", rid, e, traceback.format_exc())
            resp = {"id": rid, "ok": False, "error": {"code": _error_code(e), "message": str(e)[:500]}}
        _proto.write(json.dumps(resp, ensure_ascii=False) + "\n")
        _proto.flush()


if __name__ == "__main__":
    main()
