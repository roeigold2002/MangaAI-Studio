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
# English OCR (printed text, one line at a time). Chosen over trocr-base-handwritten after a
# comparison on real English manga scanlations; pinned so cached analyses stay comparable.
EN_OCR_REPO = "microsoft/trocr-base-printed"
EN_OCR_REVISION = "93450be3f1ed40a930690d951ef3932687cc1892"
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

        files = ("config.json", "pytorch_model.bin") if repo == OCR_REPO else ("config.json", "model.safetensors")
        rev = EN_OCR_REVISION if repo == EN_OCR_REPO else None
        return all(isinstance(try_to_load_from_cache(repo, f, revision=rev), str) for f in files)
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


def en_ocr(device: str):
    """TrOCR for English lines: (processor, model). Half precision on GPUs."""
    key = f"ocr_en:{device}"
    with _lock:
        if key not in _cache:
            if not hf_cached(EN_OCR_REPO):
                raise ModelUnavailable(
                    "the English OCR model is not downloaded: Translation settings → AI engine → download missing models"
                )
            import torch
            from transformers import TrOCRProcessor, VisionEncoderDecoderModel

            proc = TrOCRProcessor.from_pretrained(EN_OCR_REPO, revision=EN_OCR_REVISION)
            model = VisionEncoderDecoderModel.from_pretrained(EN_OCR_REPO, revision=EN_OCR_REVISION)
            if device in ("cuda", "mps"):
                model = model.half()
            _cache[key] = (proc, model.to(device).eval())
            log.info("english ocr loaded on %s", device)
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

            # LaMa is TorchScript. The profiling executor re-optimizes the graph for every new
            # input shape (1-2 s each on an RTX 5080), and every text crop has its own shape;
            # the legacy executor optimizes once, after which new shapes cost ~0.05 s.
            try:
                torch._C._jit_set_profiling_executor(False)
                torch._C._jit_set_profiling_mode(False)
            except AttributeError:  # private API; keep working if a future torch drops it
                pass
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
        "ocr_en": hf_cached(EN_OCR_REPO),
        "lama": lama_path().exists(),
        "lama_path": str(lama_path()),
    }


def prepare(progress=None) -> dict:
    """Download missing models (explicit user action)."""
    import hashlib
    import urllib.request

    from huggingface_hub import snapshot_download

    for repo, rev in ((DETECTOR_REPO, None), (OCR_REPO, None), (EN_OCR_REPO, EN_OCR_REVISION)):
        if not hf_cached(repo):
            if progress:
                progress(f"downloading {repo}")
            snapshot_download(repo, revision=rev, local_files_only=False)
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
