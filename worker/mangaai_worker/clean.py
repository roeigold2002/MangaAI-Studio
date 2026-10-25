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


def _ring(gray: np.ndarray, box: list[int], inner: int, outer: int) -> np.ndarray:
    """Pixels in a band around `box` (between `inner` and `outer` px outside it)."""
    h, w = gray.shape
    ox1, oy1, ox2, oy2 = expand(box, outer, w, h)
    ix1, iy1, ix2, iy2 = expand(box, inner, w, h)
    band = np.ones((oy2 - oy1, ox2 - ox1), dtype=bool)
    band[iy1 - oy1 : iy2 - oy1, ix1 - ox1 : ix2 - ox1] = False
    return gray[oy1:oy2, ox1:ox2][band]


def _ink_mask(gray: np.ndarray, box: list[int]) -> tuple[np.ndarray, float, float]:
    """Text strokes inside `box`. The background is measured on a ring *outside* the box,
    because detector boxes hug the text and their own border cuts through letters.
    Returns (mask, background value, background std)."""
    h, w = gray.shape
    ring = _ring(gray, box, 2, 8).astype(np.int16)
    bg = float(np.median(ring)) if ring.size else 255.0
    bg_std = float(np.std(ring)) if ring.size else 0.0
    x1, y1, x2, y2 = expand(box, 3, w, h)
    crop = gray[y1:y2, x1:x2].astype(np.int16)
    ink = (np.abs(crop - bg) > 60).astype(np.uint8) * 255
    size = max(3, int(min(x2 - x1, y2 - y1) * 0.03) | 1)
    ink = cv2.dilate(ink, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (size, size)), iterations=2)
    mask = np.zeros_like(gray, dtype=np.uint8)
    mask[y1:y2, x1:x2] = ink
    return mask, bg, bg_std


def _bubble_interior(gray: np.ndarray, bubble: list[int], text_box: list[int]) -> tuple[float, float]:
    """Median grey and the share of 'background-like' pixels in the bubble around the text."""
    bx1, by1, bx2, by2 = bubble
    mx, my = int((bx2 - bx1) * 0.12), int((by2 - by1) * 0.12)
    region = gray[by1 + my : by2 - my, bx1 + mx : bx2 - mx].astype(np.int16)
    if region.size == 0:
        return 255.0, 0.0
    keep = np.ones(region.shape, dtype=bool)
    tx1, ty1, tx2, ty2 = text_box
    keep[max(0, ty1 - 4 - by1 - my) : max(0, ty2 + 4 - by1 - my), max(0, tx1 - 4 - bx1 - mx) : max(0, tx2 + 4 - bx1 - mx)] = False
    vals = region[keep]
    if vals.size < 50:  # text fills the bubble: fall back to all interior pixels
        vals = region.ravel()
    med = float(np.median(vals))
    plain = float((np.abs(vals - med) < 30).mean())
    return med, plain


def _bubble_fill_mask(gray: np.ndarray, bubble: list[int], parts: list[list[int]], med: float, rect: np.ndarray) -> np.ndarray:
    """Exactly the bubble's inner area around the text: the connected background-coloured
    region that surrounds the text, plus the letters enclosed by it (holes). It stops at
    the bubble outline, so round bubbles and panel borders are left intact."""
    h, w = gray.shape
    bx1, by1, bx2, by2 = bubble
    sub = (np.abs(gray[by1:by2, bx1:bx2].astype(np.int16) - med) < 45).astype(np.uint8)
    n, labels = cv2.connectedComponents(sub, connectivity=4)
    # labels touching the area right around the text
    near = np.zeros(sub.shape, dtype=bool)
    for p in parts:
        x1, y1, x2, y2 = expand(p, 6, w, h)
        near[max(0, y1 - by1) : max(0, y2 - by1), max(0, x1 - bx1) : max(0, x2 - bx1)] = True
    ids, counts = np.unique(labels[near & (sub > 0)], return_counts=True)
    if ids.size == 0:
        return np.zeros_like(gray)
    comp = labels == ids[np.argmax(counts)]
    # Letters are found before any clipping: a glyph fully enclosed by the bubble background
    # is text even when the bubble is cut open by a panel border.
    raw_holes, lab, stats, hole_ids = _holes(comp)
    # leak check: an open outline lets the region run along the bubble box edge
    edge = np.concatenate([comp[0], comp[-1], comp[:, 0], comp[:, -1]])
    if edge.mean() > 0.35:
        ell = np.zeros(sub.shape, dtype=np.uint8)
        cy, cx = sub.shape[0] // 2, sub.shape[1] // 2
        cv2.ellipse(ell, (cx, cy), (max(1, int(cx * 0.9)), max(1, int(cy * 0.9))), 0, 0, 360, 1, -1)
        comp = comp & (ell > 0)
    # holes inside the region = the letters
    holes = _holes(comp)[0]
    area = rect[by1:by2, bx1:bx2] > 0
    filled = (comp | holes) & area
    letters = _whole_letters(lab, stats, hole_ids, area)
    if letters:
        grown = cv2.dilate(np.isin(lab, letters).astype(np.uint8), np.ones((5, 5), np.uint8)) > 0
        filled |= grown & (comp | holes | raw_holes)
    mask = np.zeros_like(gray)
    mask[by1:by2, bx1:bx2] = filled.astype(np.uint8) * 255
    return mask


def _holes(region: np.ndarray):
    """Areas fully enclosed by `region` (not reaching the crop border): (mask, labels, stats, ids)."""
    m, lab, stats, _ = cv2.connectedComponentsWithStats((~region).astype(np.uint8), connectivity=4)
    border_ids = set(np.unique(np.concatenate([lab[0], lab[-1], lab[:, 0], lab[:, -1]])).tolist())
    ids = [i for i in range(1, m) if i not in border_ids]
    return np.isin(lab, ids), lab, stats, ids


def _whole_letters(lab: np.ndarray, stats: np.ndarray, hole_ids: list[int], area: np.ndarray) -> list[int]:
    """Letters to erase completely: every hole the text area touches, then its neighbours
    along the same line. Detector boxes often clip the first or last glyph of a line; erasing
    only the box would leave that glyph (or half of it) next to the translation."""
    touched = set(np.unique(lab[area]).tolist())
    chosen = [i for i in hole_ids if i in touched]
    if not chosen:
        return []
    size = float(np.median([max(stats[i, cv2.CC_STAT_WIDTH], stats[i, cv2.CC_STAT_HEIGHT]) for i in chosen]))
    gap = max(4.0, 0.8 * size)
    box = lambda i: (stats[i, 0], stats[i, 1], stats[i, 0] + stats[i, 2], stats[i, 1] + stats[i, 3])  # noqa: E731
    pending = [i for i in hole_ids if i not in touched and max(stats[i, 2], stats[i, 3]) <= 2.5 * size]
    sel = [box(i) for i in chosen]
    changed = True
    while changed and pending:
        changed = False
        for i in list(pending):
            x1, y1, x2, y2 = box(i)
            for sx1, sy1, sx2, sy2 in sel:
                dx = max(sx1 - x2, x1 - sx2, 0)
                dy = max(sy1 - y2, y1 - sy2, 0)
                # same row (horizontal text) or same column (vertical text), within one glyph gap
                row = dy == 0 and min(y2, sy2) - max(y1, sy1) >= 0.3 * min(y2 - y1, sy2 - sy1)
                col = dx == 0 and min(x2, sx2) - max(x1, sx1) >= 0.3 * min(x2 - x1, sx2 - sx1)
                if (row and dx <= gap) or (col and dy <= gap):
                    chosen.append(i)
                    sel.append((x1, y1, x2, y2))
                    pending.remove(i)
                    changed = True
                    break
    return chosen


def _inside(mask: np.ndarray, bubble: list[int], shrink: float) -> np.ndarray:
    """Limit a mask to the bubble's inner rectangle (keeps the outline intact)."""
    bx1, by1, bx2, by2 = bubble
    inner = np.zeros_like(mask)
    mx, my = int((bx2 - bx1) * shrink), int((by2 - by1) * shrink)
    inner[by1 + my : by2 - my, bx1 + mx : bx2 - mx] = 255
    return cv2.bitwise_and(mask, inner)


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
        parts = [clamp(p, w, h) for p in (r.get("parts") or [r["bbox"]])]
        box = clamp(r["bbox"], w, h)

        # Speech bubble with a plain interior: erase the whole text area with the bubble colour.
        if bubble and method != "opencv":
            med, plain = _bubble_interior(gray, bubble, box)
            if plain >= 0.7:
                pad = max(4, int(min(box[2] - box[0], box[3] - box[1]) * 0.08))
                rect = np.zeros_like(gray)
                for p in parts:
                    x1, y1, x2, y2 = expand(p, pad, w, h)
                    rect[y1:y2, x1:x2] = 255
                rect = _inside(rect, bubble, 0.02)
                mask = _bubble_fill_mask(gray, bubble, parts, med, rect)
                if mask.any():
                    sel = (np.abs(gray.astype(np.int16) - med) < 30) & (mask == 0)
                    bx1, by1, bx2, by2 = bubble
                    sel_box = np.zeros_like(sel)
                    sel_box[by1:by2, bx1:bx2] = True
                    sel &= sel_box
                    color = np.median(img[sel], axis=0) if sel.any() else np.array([med, med, med])
                    img[mask > 0] = color.astype(np.uint8)
                    used.add("fill")
                    continue

        mask = np.zeros_like(gray)
        bg_std = 0.0
        for p in parts:
            m, _bg, sd = _ink_mask(gray, p)
            mask = cv2.bitwise_or(mask, m)
            bg_std = max(bg_std, sd)
        if bubble:
            mask = _inside(mask, bubble, 0.04)
        if not mask.any():
            # nothing stood out from the background: erase the detected box itself
            for p in parts:
                x1, y1, x2, y2 = expand(p, 2, w, h)
                mask[y1:y2, x1:x2] = 255
            notes.append({"rid": r.get("rid"), "kind": "no_text_pixels"})
        if method == "fill":
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
