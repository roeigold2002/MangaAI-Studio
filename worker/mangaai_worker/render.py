"""Hebrew typesetting into speech bubbles.

Text shaping and the Unicode bidi algorithm come from libraqm (HarfBuzz +
FriBidi) through Pillow. Strings are kept in logical order and wrapped by
words; each line is drawn with base direction RTL, so embedded English,
numbers and punctuation are placed by the bidi algorithm, not by us.
"""

from __future__ import annotations

import os
import sys
import tempfile
import time
from dataclasses import dataclass

import numpy as np
from PIL import Image, ImageDraw, ImageFont, features

from . import RENDER_VERSION
from .geometry import clamp


class RenderError(Exception):
    code = "render_failed"


def raqm_available() -> bool:
    return bool(features.check("raqm"))


def require_raqm() -> None:
    if not raqm_available():
        if sys.platform == "win32":
            hint = ("fribidi-0.dll was not found. Reinstall the app, or for a development build run "
                    "scripts\\build-fribidi.ps1, then restart the AI engine.")
        else:
            hint = "Install fribidi (e.g. `brew install fribidi`) and restart the AI engine."
        raise RenderError(
            f"Pillow was loaded without libraqm (HarfBuzz/FriBidi). Hebrew cannot be shaped correctly. {hint}"
        )


_font_cache: dict[tuple[str, int], ImageFont.FreeTypeFont] = {}


def font(path: str, size: int) -> ImageFont.FreeTypeFont:
    key = (path, size)
    f = _font_cache.get(key)
    if f is None:
        f = ImageFont.truetype(path, size, layout_engine=ImageFont.Layout.RAQM)
        _font_cache[key] = f
    return f


def text_width(f: ImageFont.FreeTypeFont, s: str) -> float:
    return f.getlength(s, direction="rtl", language="he")


@dataclass
class Layout:
    size: int
    lines: list[str]
    line_height: float
    width: float
    height: float
    overflow: bool


def wrap(f: ImageFont.FreeTypeFont, text: str, max_w: float, allow_break: bool) -> list[str] | None:
    """Greedy word wrap in logical order. Returns None if a word cannot fit."""
    lines: list[str] = []
    for para in text.split("\n"):
        words = para.split()
        if not words:
            continue
        cur = ""
        for word in words:
            cand = word if not cur else f"{cur} {word}"
            if text_width(f, cand) <= max_w:
                cur = cand
                continue
            if cur:
                lines.append(cur)
            if text_width(f, word) <= max_w:
                cur = word
            elif allow_break:
                # split an over-long word by characters (last resort)
                piece = ""
                for ch in word:
                    if text_width(f, piece + ch) > max_w and piece:
                        lines.append(piece)
                        piece = ch
                    else:
                        piece += ch
                cur = piece
            else:
                return None
        if cur:
            lines.append(cur)
    return lines


def fit(text: str, font_path: str, box_w: float, box_h: float, min_size: int, max_size: int, spacing: float,
        fixed_size: int | None = None) -> Layout:
    def try_size(s: int, allow_break: bool) -> Layout | None:
        f = font(font_path, s)
        lines = wrap(f, text, box_w, allow_break)
        if lines is None:
            return None
        lh = s * spacing
        width = max((text_width(f, ln) for ln in lines), default=0)
        height = lh * len(lines)
        return Layout(s, lines, lh, width, height, width > box_w + 0.5 or height > box_h + 0.5)

    if fixed_size:
        lay = try_size(fixed_size, True)
        assert lay is not None
        return lay
    lo, hi, best = min_size, max(min_size, max_size), None
    while lo <= hi:  # largest size that fits without breaking words
        mid = (lo + hi) // 2
        lay = try_size(mid, False)
        if lay is not None and not lay.overflow:
            best = lay
            lo = mid + 1
        else:
            hi = mid - 1
    if best is not None:
        return best
    lay = try_size(min_size, True)
    assert lay is not None
    return lay


def render_box(region: dict, w: int, h: int, padding: float) -> list[int]:
    """Where the translation goes: manual box, else the bubble interior, else the text box."""
    if region.get("box"):
        return clamp(region["box"], w, h)
    tb = region["bbox"]
    if region.get("bubble"):
        bx1, by1, bx2, by2 = region["bubble"]
        bw, bh = bx2 - bx1, by2 - by1
        # rectangle inscribed in an ellipse is ~70% of each side; keep a little more
        ix, iy = bw * max(padding, 0.13), bh * max(padding, 0.15)
        inner = [bx1 + ix, by1 + iy, bx2 - ix, by2 - iy]
        # never smaller than the original text area
        box = [min(inner[0], tb[0]), min(inner[1], tb[1]), max(inner[2], tb[2]), max(inner[3], tb[3])]
        box = [max(box[0], bx1 + bw * 0.06), max(box[1], by1 + bh * 0.06), min(box[2], bx2 - bw * 0.06), min(box[3], by2 - bh * 0.06)]
        return clamp(box, w, h)
    tw, th = tb[2] - tb[0], tb[3] - tb[1]
    # Hebrew is horizontal: widen narrow vertical-Japanese boxes
    if th > tw * 1.5:
        extra = (min(th, tw * 3) - tw) / 2
        tb = [tb[0] - extra, tb[1], tb[2] + extra, tb[3]]
    return clamp(tb, w, h)


def _luminance(img: np.ndarray, box: list[int]) -> float:
    x1, y1, x2, y2 = box
    crop = img[y1:y2, x1:x2]
    if crop.size == 0:
        return 255.0
    return float(np.median(crop[..., :3] @ np.array([0.299, 0.587, 0.114])))


def render(clean_path: str, out_path: str, regions: list[dict], fonts_dir: str, settings: dict) -> dict:
    """regions: [{rid, bbox, bubble?, box?, text, category?, font?, font_size?, align?, dx?, dy?}]"""
    require_raqm()
    t0 = time.time()
    base = Image.open(clean_path).convert("RGB")
    arr = np.array(base)
    w, h = base.size
    draw = ImageDraw.Draw(base)
    min_size = int(settings.get("min_font_px", 12))
    max_size = int(settings.get("max_font_px", 64))
    spacing = float(settings.get("line_spacing", 1.08))
    padding = float(settings.get("padding", 0.10))
    stroke = int(settings.get("stroke_px", 3))
    scale = max(w, h) / 1600.0  # settings are for a ~1600px page
    notes, placed = [], []
    jobs = []
    for r in regions:
        text = (r.get("text") or "").strip()
        if not text:
            continue
        loud = r.get("category") in ("sfx", "shout") or text.endswith("!!")
        fname = r.get("font") or (settings.get("font_loud") if loud else settings.get("font")) or "VarelaRound-Regular.ttf"
        fpath = os.path.join(fonts_dir, os.path.basename(fname))
        if not os.path.exists(fpath):
            raise RenderError(f"font not found: {fname}")
        box = render_box(r, w, h, padding)
        bw, bh = box[2] - box[0], box[3] - box[1]
        if bw < 8 or bh < 8:
            notes.append({"rid": r.get("rid"), "kind": "box_too_small"})
            continue
        lay = fit(text, fpath, bw, bh, max(6, int(min_size * scale)), max(8, int(max_size * scale)), spacing,
                  r.get("font_size"))
        jobs.append([r, text, fpath, box, lay, loud])

    # Keep sizes consistent on a page: big bubbles with short lines shouldn't
    # get huge text. Cap at 1.25x the median size (manual sizes and loud text exempt).
    sizes = sorted(j[4].size for j in jobs if not j[5] and not j[0].get("font_size"))
    if len(sizes) >= 3:
        cap = int(sizes[len(sizes) // 2] * 1.25)
        for j in jobs:
            r, text, fpath, box, lay, loud = j
            if not loud and not r.get("font_size") and lay.size > cap:
                j[4] = fit(text, fpath, box[2] - box[0], box[3] - box[1], max(6, int(min_size * scale)), cap, spacing)

    for r, text, fpath, box, lay, _loud in jobs:
        bw, bh = box[2] - box[0], box[3] - box[1]
        if lay.overflow:
            notes.append({"rid": r.get("rid"), "kind": "overflow", "size": lay.size})
        f = font(fpath, lay.size)
        dark_bg = _luminance(arr, box) < 110
        fill = (255, 255, 255) if dark_bg else (16, 16, 16)
        outline = (0, 0, 0) if dark_bg else (255, 255, 255)
        sw = max(1, int(stroke * scale * lay.size / 40)) if r.get("kind") != "bubble" or dark_bg else 0
        align = r.get("align", "center")
        cy = box[1] + (bh - lay.height) / 2 + float(r.get("dy", 0))
        for i, line in enumerate(lay.lines):
            lw = text_width(f, line)
            if align == "right":
                x = box[2] - lw
            elif align == "left":
                x = box[0]
            else:
                x = box[0] + (bw - lw) / 2
            x += float(r.get("dx", 0))
            y = cy + i * lay.line_height + (lay.line_height - lay.size) / 2
            draw.text((x, y), line, font=f, fill=fill, direction="rtl", language="he",
                      stroke_width=sw, stroke_fill=outline)
        placed.append({"rid": r.get("rid"), "box": box, "size": lay.size, "lines": lay.lines})

    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(out_path), suffix=".png")
    os.close(fd)
    base.save(tmp, "PNG", compress_level=4)
    os.replace(tmp, out_path)
    return {"version": RENDER_VERSION, "placed": placed, "notes": notes, "seconds": round(time.time() - t0, 2)}
