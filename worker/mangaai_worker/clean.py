"""Text removal. Reads the original page, writes a separate cleaned image.

Strategy per region:
- plain bubble background (low variance around the text) -> fill with the
  background colour (exact, fast);
- anything else -> LaMa inpainting on a crop around the region, with OpenCV
  Telea as the fallback when the LaMa model is not available.
"""

from __future__ import annotations

import os
import tempfile
import time

import cv2
import numpy as np
from PIL import Image

from . import INPAINT_VERSION, models
from .geometry import clamp, expand


def _ink_mask(gray: np.ndarray, box: list[int], bubble: list[int] | None) -> tuple[np.ndarray, float, float]:
    """Mask of text strokes inside `box`. Returns (mask, background value, background std)."""
    h, w = gray.shape
    x1, y1, x2, y2 = expand(box, 3, w, h)
    crop = gray[y1:y2, x1:x2].astype(np.int16)
    border = np.concatenate([crop[0, :], crop[-1, :], crop[:, 0], crop[:, -1]])
    bg = float(np.median(border))
    bg_std = float(np.std(border))
    diff = np.abs(crop - bg)
    thr = max(40.0, bg_std * 2.5)
    ink = (diff > thr).astype(np.uint8) * 255
    size = max(3, int(min(x2 - x1, y2 - y1) * 0.03) | 1)
    ink = cv2.dilate(ink, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (size, size)), iterations=2)
    mask = np.zeros_like(gray, dtype=np.uint8)
    mask[y1:y2, x1:x2] = ink
    if bubble:
        # never paint outside the bubble (keeps the bubble outline intact)
        bx1, by1, bx2, by2 = bubble
        inner = np.zeros_like(mask)
        mx, my = int((bx2 - bx1) * 0.04), int((by2 - by1) * 0.04)
        inner[by1 + my : by2 - my, bx1 + mx : bx2 - mx] = 255
        mask = cv2.bitwise_and(mask, inner)
    return mask, bg, bg_std


def _lama(img: np.ndarray, mask: np.ndarray, box: list[int], device_pref: str) -> np.ndarray:
    import torch

    model, device = models.lama(models.pick_device(device_pref))
    h, w = mask.shape
    bw, bh = box[2] - box[0], box[3] - box[1]
    pad = int(max(bw, bh) * 0.6) + 32
    x1, y1, x2, y2 = expand(box, pad, w, h)
    crop = img[y1:y2, x1:x2]
    m = mask[y1:y2, x1:x2]
    ch, cw = m.shape
    # LaMa is happiest around 512-1024px; downscale big crops, keep multiples of 8.
    scale = min(1.0, 1024 / max(ch, cw))
    tw, th = max(8, int(cw * scale) // 8 * 8), max(8, int(ch * scale) // 8 * 8)
    ci = cv2.resize(crop, (tw, th), interpolation=cv2.INTER_AREA)
    mi = cv2.resize(m, (tw, th), interpolation=cv2.INTER_NEAREST)
    t_img = torch.from_numpy(ci).permute(2, 0, 1).unsqueeze(0).float().div(255).to(device)
    t_mask = torch.from_numpy((mi > 127).astype(np.float32)).unsqueeze(0).unsqueeze(0).to(device)
    with torch.inference_mode():
        out = model(t_img, t_mask)
    res = (out[0].permute(1, 2, 0).clamp(0, 1).mul(255).byte().cpu().numpy())
    res = cv2.resize(res, (cw, ch), interpolation=cv2.INTER_CUBIC)
    keep = (m > 127)[..., None]
    img = img.copy()
    img[y1:y2, x1:x2] = np.where(keep, res, crop)
    return img


def clean(image_path: str, out_path: str, regions: list[dict], method: str = "lama", device_pref: str = "auto") -> dict:
    """regions: [{rid, bbox, parts?, bubble?, kind}] — only regions to remove."""
    t0 = time.time()
    pil = Image.open(image_path).convert("RGB")
    img = np.array(pil)
    gray = cv2.cvtColor(img, cv2.COLOR_RGB2GRAY)
    h, w = gray.shape
    notes = []
    used = set()
    for r in regions:
        bubble = clamp(r["bubble"], w, h) if r.get("bubble") else None
        parts = r.get("parts") or [r["bbox"]]
        mask = np.zeros_like(gray)
        bg, bg_std = 255.0, 0.0
        for p in parts:
            m, bg, bg_std = _ink_mask(gray, clamp(p, w, h), bubble)
            mask = cv2.bitwise_or(mask, m)
        if not mask.any():
            notes.append({"rid": r.get("rid"), "kind": "no_text_pixels"})
            continue
        box = clamp(r["bbox"], w, h)
        plain = bg_std < 10 and r.get("kind") == "bubble"
        if method == "fill" or (plain and method != "opencv"):
            x1, y1, x2, y2 = expand(box, 3, w, h)
            ring = np.concatenate([img[y1, x1:x2], img[y2 - 1, x1:x2], img[y1:y2, x1], img[y1:y2, x2 - 1]])
            img[mask > 0] = np.median(ring, axis=0).astype(np.uint8)
            used.add("fill")
            continue
        done = False
        if method == "lama":
            try:
                img = _lama(img, mask, box, device_pref)
                used.add("lama")
                done = True
            except models.ModelUnavailable as e:
                notes.append({"rid": r.get("rid"), "kind": "lama_unavailable", "detail": str(e)})
            except Exception as e:  # e.g. MPS op missing -> retry on CPU
                try:
                    img = _lama(img, mask, box, "cpu")
                    used.add("lama")
                    done = True
                except Exception:
                    notes.append({"rid": r.get("rid"), "kind": "lama_failed", "detail": str(e)[:200]})
        if not done:
            img = cv2.inpaint(img, mask, 4, cv2.INPAINT_TELEA)
            used.add("opencv")
        if bg_std > 25:
            notes.append({"rid": r.get("rid"), "kind": "complex_background"})

    out_dir = os.path.dirname(out_path)
    os.makedirs(out_dir, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=out_dir, suffix=".png")
    os.close(fd)
    Image.fromarray(img).save(tmp, "PNG", optimize=False, compress_level=3)
    os.replace(tmp, out_path)
    return {"version": INPAINT_VERSION, "methods": sorted(used), "notes": notes, "seconds": round(time.time() - t0, 2)}
