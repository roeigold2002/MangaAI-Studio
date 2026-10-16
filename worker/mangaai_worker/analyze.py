"""Text/bubble detection + OCR + reading order for one page."""

from __future__ import annotations

import re
import time

import numpy as np
from PIL import Image

from . import ANALYSIS_VERSION, models
from .geometry import Box, clamp, contained_fraction, expand, merge_overlapping, nms, reading_order, union_box

# Detector classes (ogkalu/comic-text-and-bubble-detector): 0 bubble, 1 text in bubble, 2 free text.
BUBBLE, TEXT_BUBBLE, TEXT_FREE = 0, 1, 2


def _detect_once(img: Image.Image, device: str, threshold: float):
    import torch

    proc, model = models.detector(device)
    inputs = proc(images=img, return_tensors="pt").to(device)
    with torch.inference_mode():
        out = model(**inputs)
    res = proc.post_process_object_detection(
        out, target_sizes=torch.tensor([img.size[::-1]], device=device), threshold=threshold
    )[0]
    return [
        (b.tolist(), float(s), int(lbl))
        for b, s, lbl in zip(res["boxes"].cpu(), res["scores"].cpu(), res["labels"].cpu())
    ]


def detect(img: Image.Image, device: str, threshold: float = 0.35):
    """Detections over the whole page. Tall pages (webtoons) are processed in overlapping slices."""
    w, h = img.size
    dets: list[tuple[Box, float, int]] = []
    if h / max(w, 1) <= 2.2:
        dets = _detect_once(img, device, threshold)
    else:
        step = int(w * 1.4)
        overlap = int(w * 0.3)
        y = 0
        while y < h:
            y2 = min(h, y + step)
            crop = img.crop((0, y, w, y2))
            for b, s, lbl in _detect_once(crop, device, threshold):
                dets.append(([b[0], b[1] + y, b[2], b[3] + y], s, lbl))
            if y2 >= h:
                break
            y = y2 - overlap
    # NMS per class
    out = []
    for cls in (BUBBLE, TEXT_BUBBLE, TEXT_FREE):
        boxes = [d[0] for d in dets if d[2] == cls]
        scores = [d[1] for d in dets if d[2] == cls]
        for i in nms(boxes, scores, 0.5):
            out.append((boxes[i], scores[i], cls))
    return out


def _ocr_crop(crop: Image.Image, device: str) -> tuple[str, float | None]:
    import torch
    from manga_ocr.ocr import post_process

    proc, tok, model = models.ocr(device)
    x = proc(crop.convert("L").convert("RGB"), return_tensors="pt").pixel_values.to(device)
    with torch.inference_mode():
        gen = model.generate(x, max_length=300, output_scores=True, return_dict_in_generate=True)
    text = post_process(tok.decode(gen.sequences[0].cpu(), skip_special_tokens=True))
    conf = None
    try:
        beams = getattr(gen, "beam_indices", None)
        if beams is not None:  # beam search: scores are per-beam, need beam_indices (already log-probs)
            scores = model.compute_transition_scores(gen.sequences, gen.scores, beams, normalize_logits=False)[0]
        else:
            scores = model.compute_transition_scores(gen.sequences, gen.scores, normalize_logits=True)[0]
        probs = torch.exp(scores.float()).cpu().numpy()
        if probs.size:
            conf = float(np.exp(np.mean(np.log(np.clip(probs, 1e-6, 1.0)))))  # geometric mean
    except Exception:
        pass
    return text, conf


def _en_ocr_lines(lines: list[Image.Image], device: str) -> list[tuple[str, float | None]]:
    """TrOCR over single text lines, batched. Returns (text, confidence) per line."""
    import torch

    proc, model = models.en_ocr(device)
    px = proc(images=[ln.convert("RGB") for ln in lines], return_tensors="pt").pixel_values.to(device, model.dtype)
    with torch.inference_mode():
        gen = model.generate(px, max_new_tokens=48, use_cache=True, output_scores=True, return_dict_in_generate=True)
    texts = proc.batch_decode(gen.sequences, skip_special_tokens=True)
    confs: list[float | None] = [None] * len(lines)
    try:
        scores = model.compute_transition_scores(gen.sequences, gen.scores, normalize_logits=True)
        for i in range(len(lines)):
            # only the tokens this line generated (later steps are padding after its end token)
            n = int((gen.sequences[i, 1:] != proc.tokenizer.pad_token_id).sum())
            lp = scores[i, : max(1, n)].float().cpu().numpy()
            confs[i] = float(np.exp(np.mean(np.clip(lp, np.log(1e-6), 0.0))))
    except Exception:
        pass
    return [(t.strip(), c) for t, c in zip(texts, confs)]


def join_en_lines(lines: list[str]) -> str:
    """Lettered English is broken into short centered lines; rejoin them into one sentence.
    A word split with a hyphen at the end of a line is put back together."""
    out = ""
    for ln in (x.strip() for x in lines):
        if not ln:
            continue
        if out.endswith("-") and not out.endswith("--") and ln[:1].isalpha() and out[-2:-1].isalpha():
            out = out[:-1] + ln
        else:
            out = f"{out} {ln}" if out else ln
    return out


# A line the model is this unsure about is art or an effect, not text.
EN_MIN_LINE_CONF = 0.25


def en_ocr_box(img: Image.Image, box: Box, device: str) -> tuple[str, float | None]:
    w, h = img.size
    # Tight crops: wider margins made TrOCR drop apostrophes ("IM GONNA") on real pages.
    pad = max(1, int(min(box[2] - box[0], box[3] - box[1]) * 0.02))
    c = expand(box, pad, w, h)
    if c[2] - c[0] < 4 or c[3] - c[1] < 4:
        return "", None
    crop = img.crop(tuple(c))
    bw, bh = crop.size
    bands = split_lines(np.array(crop.convert("L"))) or [(0, bh)]
    pieces = []
    for a, b in bands:
        m = max(2, int((b - a) * 0.08))
        pieces.append(crop.crop((0, max(0, a - m), bw, min(bh, b + m))))
    read = _en_ocr_lines(pieces, device)
    kept = [(t, cf) for t, cf in read if t and (cf is None or cf >= EN_MIN_LINE_CONF)]
    confs = [cf for _, cf in kept if cf is not None]
    return join_en_lines([t for t, _ in kept]), (min(confs) if confs else None)


_ONLY_PUNCT = re.compile(r"^[\s\.\,…・！？!?ー〜~―─「」『』（）()、。\-]*$")


def split_lines(gray: np.ndarray) -> list[tuple[int, int]]:
    """Horizontal text lines in a crop, from the row ink profile. [(y1, y2), ...]"""
    bg = np.median(np.concatenate([gray[0], gray[-1], gray[:, 0], gray[:, -1]]))
    ink = np.abs(gray.astype(np.int16) - bg) > 60
    rows = ink.sum(axis=1) > max(1, ink.shape[1] * 0.01)
    bands, start = [], None
    for y, on in enumerate(rows):
        if on and start is None:
            start = y
        elif not on and start is not None:
            bands.append((start, y))
            start = None
    if start is not None:
        bands.append((start, len(rows)))
    # merge bands separated by tiny gaps (accents, dakuten) and drop specks
    merged: list[list[int]] = []
    for a, b in bands:
        if merged and a - merged[-1][1] < max(2, (b - a) * 0.15):
            merged[-1][1] = b
        else:
            merged.append([a, b])
    tallest = max((b - a for a, b in merged), default=0)
    return [(a, b) for a, b in merged if b - a >= tallest * 0.35]


def ocr_box(img: Image.Image, box: Box, device: str) -> tuple[str, float | None]:
    w, h = img.size
    pad = max(2, int(min(box[2] - box[0], box[3] - box[1]) * 0.06))
    c = expand(box, pad, w, h)
    if c[2] - c[0] < 4 or c[3] - c[1] < 4:
        return "", None
    crop = img.crop(tuple(c))
    bw, bh = c[2] - c[0], c[3] - c[1]
    if bw > bh * 1.15:
        # manga-ocr reads single horizontal lines reliably; multi-line horizontal blocks are split
        lines = split_lines(np.array(crop.convert("L")))
        if len(lines) >= 2:
            texts, confs = [], []
            gray = np.array(crop.convert("L")).astype(np.int16)
            bg = np.median(np.concatenate([gray[0], gray[-1], gray[:, 0], gray[:, -1]]))
            for a, b in lines:
                m = max(2, int((b - a) * 0.2))
                t, cf = _ocr_crop(crop.crop((0, max(0, a - m), bw, min(bh, b + m))), device)
                # The OCR model can hallucinate long text for a few marks ("？！？"):
                # compare text length with the ink extent of the line.
                cols = np.where((np.abs(gray[a:b] - bg) > 60).any(axis=0))[0]
                est = (cols[-1] - cols[0] + 1) / max(1, b - a) if cols.size else 0
                if len(t) > est * 2 + 2:
                    confs.append(0.3)
                    continue
                if t:
                    texts.append(t)
                    if cf is not None:
                        confs.append(cf)
            return "".join(texts), (min(confs) if confs else None)
    return _ocr_crop(crop, device)


def analyze(image_path: str, device_pref: str = "auto", threshold: float = 0.35, rtl: bool = True, ocr: bool = True,
            lang: str = "ja") -> dict:
    """lang: "ja" (manga-ocr) or "en" (TrOCR). ocr=False: detection only (other languages);
    the translator then reads the text from the page image."""
    t0 = time.time()
    device = models.pick_device(device_pref)
    img = Image.open(image_path)
    img.load()
    img = img.convert("RGB")
    w, h = img.size
    dets = detect(img, device, threshold)
    bubbles = [d[0] for d in dets if d[2] == BUBBLE]
    texts = [(d[0], d[1], d[2]) for d in dets if d[2] in (TEXT_BUBBLE, TEXT_FREE)]

    # Group text boxes by the bubble that contains them; one region per bubble.
    groups: dict[int, list[tuple[Box, float, int]]] = {}
    free: list[tuple[Box, float, int]] = []
    for t in texts:
        best, best_f = -1, 0.0
        for bi, b in enumerate(bubbles):
            f = contained_fraction(t[0], b)
            if f > best_f:
                best, best_f = bi, f
        if best >= 0 and best_f >= 0.6:
            groups.setdefault(best, []).append(t)
        else:
            free.append(t)

    raw_regions = []
    for bi, items in groups.items():
        raw_regions.append({"kind": "bubble", "boxes": [it[0] for it in items], "bubble": bubbles[bi], "score": max(it[1] for it in items)})
    # Free text: merge boxes that touch (one caption split in pieces).
    free_boxes = merge_overlapping([f[0] for f in free], gap=4)
    for fb in free_boxes:
        score = max((f[1] for f in free if contained_fraction(f[0], fb) > 0.5), default=0.0)
        raw_regions.append({"kind": "free", "boxes": [fb], "bubble": None, "score": score})

    regions = []
    for r in raw_regions:
        boxes = r["boxes"]
        bbox = union_box(boxes)
        bw, bh = bbox[2] - bbox[0], bbox[3] - bbox[1]
        vertical = lang == "ja" and bh > bw * 1.15  # lettered English is always horizontal
        # OCR each piece in reading order (vertical Japanese columns read right-to-left).
        order = sorted(range(len(boxes)), key=lambda i: (-boxes[i][2], boxes[i][1])) if vertical else reading_order(boxes, rtl)
        parts, confs = [], []
        for i in (order if ocr else []):
            text, conf = (en_ocr_box if lang == "en" else ocr_box)(img, boxes[i], device)
            if text:
                parts.append(text)
                if conf is not None:
                    confs.append(conf)
        text = "".join(parts) if vertical else " ".join(parts)
        if ocr and not text.strip():
            continue
        conf = float(min(confs)) if confs else None
        regions.append({
            "kind": r["kind"],
            "bbox": clamp(bbox, w, h),
            "parts": [clamp(b, w, h) for b in boxes],
            "bubble": clamp(r["bubble"], w, h) if r["bubble"] else None,
            "vertical": vertical,
            "text": text,
            "confidence": conf,
            "det_score": round(r["score"], 3),
            "punct_only": bool(_ONLY_PUNCT.match(text)),
        })

    order = reading_order([r["bubble"] or r["bbox"] for r in regions], rtl)
    regions = [regions[i] for i in order]
    for n, r in enumerate(regions, 1):
        r["rid"] = f"r{n}"
    return {
        "version": ANALYSIS_VERSION,
        "width": w,
        "height": h,
        "device": device,
        "regions": regions,
        "seconds": round(time.time() - t0, 2),
    }


def ocr_region(image_path: str, box: Box, device_pref: str = "auto", lang: str = "ja") -> dict:
    device = models.pick_device(device_pref)
    img = Image.open(image_path).convert("RGB")
    text, conf = (en_ocr_box if lang == "en" else ocr_box)(img, box, device)
    return {"text": text, "confidence": conf}

