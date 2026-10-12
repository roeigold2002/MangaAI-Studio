"""Lazy model loading. Weights come from the local cache; nothing downloads
unless `allow_download=True` (the explicit "prepare models" action)."""

from __future__ import annotations

import logging
import os
import threading
from pathlib import Path

log = logging.getLogger("mangaai.models")

DETECTOR_REPO = "ogkalu/comic-text-and-bubble-detector"
OCR_REPO = "kha-white/manga-ocr-base"
LAMA_URL = "https://github.com/Sanster/models/releases/download/AnimeMangaInpainting/anime-manga-big-lama.pt"
LAMA_SHA256 = "479d3afdcb7ed2fd944ed4ebcc39ca45b33491f0f2e43eb1000bd623cfb41823"
LAMA_FILE = "anime-manga-big-lama.pt"

_lock = threading.Lock()
_cache: dict[str, object] = {}


class ModelUnavailable(Exception):
    code = "model_missing"


def models_dir() -> Path:
    d = Path(os.environ.get("MANGAAI_MODELS_DIR", Path.home() / ".cache" / "mangaai"))
    d.mkdir(parents=True, exist_ok=True)
    return d


def pick_device(pref: str = "auto") -> str:
    import torch

    if pref in ("cpu", "mps", "cuda"):
        return pref
    if torch.cuda.is_available():
        return "cuda"
    if torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def hf_cached(repo: str) -> bool:
    """True if the weights are in the local HF cache (no network)."""
    try:
        from huggingface_hub import try_to_load_from_cache

        files = ("config.json", "model.safetensors") if repo == DETECTOR_REPO else ("config.json", "pytorch_model.bin")
        return all(isinstance(try_to_load_from_cache(repo, f), str) for f in files)
    except Exception:
        return False


def detector(device: str):
    key = f"det:{device}"
    with _lock:
        if key not in _cache:
            if not hf_cached(DETECTOR_REPO):
                raise ModelUnavailable(f"detector model {DETECTOR_REPO} is not downloaded")
            from transformers import RTDetrImageProcessor, RTDetrV2ForObjectDetection

            proc = RTDetrImageProcessor.from_pretrained(DETECTOR_REPO, size={"width": 640, "height": 640})
            model = RTDetrV2ForObjectDetection.from_pretrained(DETECTOR_REPO).to(device).eval()
            _cache[key] = (proc, model)
            log.info("detector loaded on %s", device)
        return _cache[key]


def ocr(device: str):
    key = f"ocr:{device}"
    with _lock:
        if key not in _cache:
            if not hf_cached(OCR_REPO):
                raise ModelUnavailable(f"OCR model {OCR_REPO} is not downloaded")
            from manga_ocr.ocr import MangaOcrModel
            from transformers import AutoTokenizer, ViTImageProcessor

            proc = ViTImageProcessor.from_pretrained(OCR_REPO)
            tok = AutoTokenizer.from_pretrained(OCR_REPO, tokenizer_type="bert-japanese")
            model = MangaOcrModel.from_pretrained(OCR_REPO).to(device).eval()
            _cache[key] = (proc, tok, model)
            log.info("ocr loaded on %s", device)
        return _cache[key]


def lama_path() -> Path:
    return models_dir() / LAMA_FILE


def lama(device: str):
    key = f"lama:{device}"
    with _lock:
        if key not in _cache:
            p = lama_path()
            if not p.exists():
                raise ModelUnavailable(f"inpainting model not found at {p}")
            import torch

            m = torch.jit.load(str(p), map_location="cpu")
            m.eval()
            try:
                m = m.to(device)
            except Exception:  # some TorchScript ops are CPU-only
                device = "cpu"
            _cache[key] = (m, device)
            log.info("lama loaded on %s", device)
        return _cache[key]


def status() -> dict:
    return {
        "detector": hf_cached(DETECTOR_REPO),
        "ocr": hf_cached(OCR_REPO),
        "lama": lama_path().exists(),
        "lama_path": str(lama_path()),
    }


def prepare(progress=None) -> dict:
    """Download missing models (explicit user action)."""
    import hashlib
    import urllib.request

    from huggingface_hub import snapshot_download

    for repo in (DETECTOR_REPO, OCR_REPO):
        if not hf_cached(repo):
            if progress:
                progress(f"downloading {repo}")
            snapshot_download(repo, local_files_only=False)
    p = lama_path()
    if not p.exists():
        if progress:
            progress("downloading inpainting model")
        tmp = p.with_suffix(".part")
        urllib.request.urlretrieve(LAMA_URL, tmp)
        h = hashlib.sha256(tmp.read_bytes()).hexdigest()
        if h != LAMA_SHA256:
            tmp.unlink(missing_ok=True)
            raise RuntimeError("inpainting model checksum mismatch")
        tmp.replace(p)
    return status()
