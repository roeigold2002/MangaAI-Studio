"""Box helpers and reading order. Boxes are [x1, y1, x2, y2] in pixels."""

from __future__ import annotations

Box = list[float]


def area(b: Box) -> float:
    return max(0.0, b[2] - b[0]) * max(0.0, b[3] - b[1])


def intersection(a: Box, b: Box) -> float:
    w = min(a[2], b[2]) - max(a[0], b[0])
    h = min(a[3], b[3]) - max(a[1], b[1])
    return max(0.0, w) * max(0.0, h)


def iou(a: Box, b: Box) -> float:
    inter = intersection(a, b)
    union = area(a) + area(b) - inter
    return inter / union if union > 0 else 0.0


def contained_fraction(inner: Box, outer: Box) -> float:
    """Fraction of `inner` that lies inside `outer`."""
    a = area(inner)
    return intersection(inner, outer) / a if a > 0 else 0.0


def union_box(boxes: list[Box]) -> Box:
    return [min(b[0] for b in boxes), min(b[1] for b in boxes), max(b[2] for b in boxes), max(b[3] for b in boxes)]


def clamp(b: Box, w: int, h: int) -> list[int]:
    return [max(0, int(b[0])), max(0, int(b[1])), min(w, int(round(b[2]))), min(h, int(round(b[3])))]


def expand(b: Box, px: float, w: int, h: int) -> list[int]:
    return clamp([b[0] - px, b[1] - px, b[2] + px, b[3] + px], w, h)


def nms(boxes: list[Box], scores: list[float], thresh: float = 0.5) -> list[int]:
    """Indices kept by greedy non-maximum suppression (also drops boxes mostly inside a kept box)."""
    order = sorted(range(len(boxes)), key=lambda i: -scores[i])
    keep: list[int] = []
    for i in order:
        if all(iou(boxes[i], boxes[k]) < thresh and contained_fraction(boxes[i], boxes[k]) < 0.85 for k in keep):
            keep.append(i)
    return keep


def merge_overlapping(boxes: list[Box], gap: float = 0.0) -> list[Box]:
    """Merge boxes that overlap (or are within `gap` px) into their union, until stable."""
    out = [list(b) for b in boxes]
    changed = True
    while changed:
        changed = False
        for i in range(len(out)):
            for j in range(i + 1, len(out)):
                a, b = out[i], out[j]
                if intersection([a[0] - gap, a[1] - gap, a[2] + gap, a[3] + gap], b) > 0:
                    out[i] = union_box([a, b])
                    out.pop(j)
                    changed = True
                    break
            if changed:
                break
    return out


def reading_order(boxes: list[Box], rtl: bool = True) -> list[int]:
    """Order boxes the way a reader scans a page.

    Boxes are grouped into horizontal bands (rows) by vertical overlap; bands go
    top to bottom, and inside a band boxes go right-to-left (manga) or
    left-to-right. Simple and predictable; the editor lets the user reorder.
    """
    idx = sorted(range(len(boxes)), key=lambda i: boxes[i][1])
    rows: list[list[int]] = []
    for i in idx:
        b = boxes[i]
        placed = False
        for row in rows:
            top = min(boxes[k][1] for k in row)
            bottom = max(boxes[k][3] for k in row)
            overlap = min(bottom, b[3]) - max(top, b[1])
            shortest = min(bottom - top, b[3] - b[1])
            if shortest > 0 and overlap / shortest >= 0.4:
                row.append(i)
                placed = True
                break
        if not placed:
            rows.append([i])
    rows.sort(key=lambda r: min(boxes[k][1] for k in r))
    out: list[int] = []
    for row in rows:
        row.sort(key=lambda k: (boxes[k][0] + boxes[k][2]) / 2, reverse=rtl)
        out.extend(row)
    return out
