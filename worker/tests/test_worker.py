"""Worker tests that need no ML models: geometry, line splitting, masks,
Hebrew layout/bidi rendering, and the JSON-lines protocol."""

import json
import os
import subprocess
import sys
from pathlib import Path

import numpy as np
import pytest
from PIL import Image, ImageDraw

from mangaai_worker import geometry, render
from mangaai_worker.analyze import split_lines
from mangaai_worker.clean import clean

ROOT = Path(__file__).resolve().parents[2]
FONTS = ROOT / "resources" / "fonts"
FONT = str(FONTS / "VarelaRound-Regular.ttf")

needs_raqm = pytest.mark.skipif(not render.raqm_available(), reason="libraqm not available")


def test_reading_order_rtl_and_ltr():
    boxes = [[600, 10, 700, 60], [100, 20, 200, 70], [300, 300, 400, 350], [50, 290, 150, 340]]
    assert geometry.reading_order(boxes, rtl=True) == [0, 1, 2, 3]
    assert geometry.reading_order(boxes, rtl=False) == [1, 0, 3, 2]


def test_nms_and_merge():
    boxes = [[0, 0, 10, 10], [1, 1, 10, 10], [50, 50, 60, 60]]
    keep = geometry.nms(boxes, [0.9, 0.8, 0.7])
    assert keep == [0, 2]
    merged = geometry.merge_overlapping([[0, 0, 10, 10], [12, 0, 20, 10]], gap=4)
    assert merged == [[0, 0, 20, 10]]


def _text_image(lines, size=(400, 200)):
    im = Image.new("L", size, 255)
    d = ImageDraw.Draw(im)
    for i, _ in enumerate(lines):
        d.rectangle((20, 20 + i * 60, 300, 50 + i * 60), fill=0)
    return np.array(im)


def test_split_lines_counts_rows():
    assert len(split_lines(_text_image(["a"]))) == 1
    assert len(split_lines(_text_image(["a", "b", "c"]))) == 3


def test_clean_fills_plain_bubble_and_keeps_original(tmp_path):
    src = tmp_path / "page.png"
    im = Image.new("RGB", (300, 200), "white")
    d = ImageDraw.Draw(im)
    d.ellipse((10, 10, 290, 190), outline="black", width=4)
    d.rectangle((100, 80, 200, 100), fill="black")  # "text"
    im.save(src)
    before = src.read_bytes()
    out = tmp_path / "clean.png"
    res = clean(str(src), str(out), [{"rid": "r1", "kind": "bubble", "bbox": [95, 75, 205, 105], "bubble": [10, 10, 290, 190]}], "fill")
    assert res["methods"] == ["fill"]
    arr = np.array(Image.open(out).convert("L"))
    assert arr[85:95, 110:190].min() > 200  # text gone
    assert arr[100, 12] < 80  # bubble outline kept
    assert src.read_bytes() == before  # original untouched


@needs_raqm
def test_hebrew_is_not_reversed_by_hand():
    f = render.font(FONT, 40)
    # Shaping through raqm with RTL direction: width is identical to the logical string,
    # and the bidi algorithm handles mixed runs.
    assert render.text_width(f, "שלום") > 0
    lines = render.wrap(f, "שלום עולם (Hello) 123!", 1000, False)
    assert lines == ["שלום עולם (Hello) 123!"]  # logical order preserved


@needs_raqm
@pytest.mark.parametrize(
    "text",
    [
        "שלום",
        "אני אוהב manga מאוד",
        "יש לי 3 חתולים, ו-12 כלבים!",
        "(בסוגריים) ומחוץ להם",
        "שורה ראשונה\nשורה שנייה",
        "זהו משפט ארוך מאוד שצריך להישבר לכמה שורות כדי להיכנס לתוך בועת הדיבור הקטנה הזאת בלי לחרוג ממנה",
        "אה?!",
        "",
    ],
)
def test_fit_stays_inside_box(text):
    lay = render.fit(text, FONT, 220, 160, 8, 60, 1.1) if text else None
    if lay is None:
        return
    assert not lay.overflow
    assert lay.width <= 220.5 and lay.height <= 160.5
    # every word survives wrapping, in order
    assert " ".join(lay.lines).split() == text.split()


@needs_raqm
def test_rtl_visual_order(tmp_path):
    """Render 'א' + '!' and check '!' ends up on the LEFT (RTL), not the right."""
    f = render.font(FONT, 80)
    im = Image.new("L", (400, 120), 255)
    d = ImageDraw.Draw(im)
    d.text((20, 10), "א!", font=f, fill=0, direction="rtl", language="he")
    arr = np.array(im) < 128
    cols = np.where(arr.any(axis=0))[0]
    # '!' is a thin glyph: the leftmost inked run should be narrow
    first_gap = next(i for i in range(cols[0], cols[-1]) if not arr[:, i].any())
    assert first_gap - cols[0] < 25


@needs_raqm
def test_render_end_to_end(tmp_path):
    clean_img = tmp_path / "clean.png"
    im = Image.new("RGB", (600, 400), "white")
    ImageDraw.Draw(im).ellipse((50, 50, 550, 350), outline="black", width=5)
    im.save(clean_img)
    out = tmp_path / "out.png"
    res = render.render(
        str(clean_img), str(out),
        [{"rid": "r1", "kind": "bubble", "bbox": [200, 150, 400, 250], "bubble": [50, 50, 550, 350],
          "text": "מה קורה? (Hi) 2024"}],
        str(FONTS), {},
    )
    assert out.exists()
    assert res["placed"][0]["rid"] == "r1"
    assert not res["notes"]
    assert np.array(Image.open(out).convert("L"))[150:250, 150:450].min() < 50


def test_missing_raqm_is_an_error(monkeypatch, tmp_path):
    monkeypatch.setattr(render, "raqm_available", lambda: False)
    with pytest.raises(render.RenderError):
        render.render("x", "y", [], str(FONTS), {})


def test_protocol_roundtrip():
    env = dict(os.environ, HF_HUB_OFFLINE="1")
    p = subprocess.run(
        [sys.executable, "-m", "mangaai_worker"],
        input='{"id":1,"method":"health"}\n{"id":2,"method":"nope"}\n{"id":3,"method":"shutdown"}\n',
        capture_output=True, text=True, cwd=ROOT / "worker", env=env, timeout=120,
    )
    lines = [json.loads(line) for line in p.stdout.splitlines()]
    assert lines[0]["id"] == 1 and lines[0]["ok"] and lines[0]["result"]["protocol"] == 1
    assert lines[1]["id"] == 2 and not lines[1]["ok"]
    assert lines[2]["id"] == 3 and lines[2]["ok"]


def test_protocol_is_utf8_on_every_platform(tmp_path):
    """The app writes raw UTF-8 (Hebrew text, Japanese/Hebrew file names). On Windows the
    worker's default stdin encoding is the ANSI code page, so this must not depend on it."""
    missing = tmp_path / "תמונה_画像.png"
    req = json.dumps({"id": 7, "method": "analyze", "params": {"image": str(missing), "ocr": False}}, ensure_ascii=False)
    env = {k: v for k, v in os.environ.items() if k not in ("PYTHONUTF8", "PYTHONIOENCODING")}
    p = subprocess.run(
        [sys.executable, "-m", "mangaai_worker"],
        input=(req + "\n" + '{"id":8,"method":"shutdown"}\n').encode("utf-8"),
        capture_output=True, cwd=ROOT / "worker", env=dict(env, HF_HUB_OFFLINE="1"), timeout=120,
    )
    out = p.stdout.decode("utf-8")
    assert "\r" not in out, "protocol lines end with \\n only"
    first = json.loads(out.splitlines()[0])
    assert first["id"] == 7 and not first["ok"]
    assert first["error"]["code"] == "file_not_found"
    assert "תמונה_画像.png" in first["error"]["message"], first["error"]["message"]


def test_glyph_clipped_by_the_detector_box_is_erased(tmp_path):
    """Regression (seen on a real page): the detector box clipped the first glyph of a line and
    the last glyph of another, and they stayed visible next to the Hebrew. Every letter in a
    plain bubble that touches the text area, or continues its line, must go."""
    src = tmp_path / "page.png"
    im = Image.new("RGB", (600, 360), (120, 90, 60))
    d = ImageDraw.Draw(im)
    d.ellipse((40, 30, 560, 330), fill="white", outline="black", width=5)
    glyphs = []
    for row, y in enumerate((120, 190)):
        for i in range(6):
            x = 150 + i * 50
            d.rectangle((x, y, x + 34, y + 40), outline="black", width=5)  # a "kanji": a ring of ink
            d.line((x + 6, y + 20, x + 28, y + 20), fill="black", width=4)
            glyphs.append((x, y))
    im.save(src)
    out = tmp_path / "clean.png"
    # box misses the first glyph of row 1 entirely and most of the last glyph of row 2
    box = [205, 118, 412, 232]
    res = clean(str(src), str(out), [{"rid": "r1", "kind": "bubble", "bbox": box, "bubble": [40, 30, 560, 330]}], "lama")
    assert res["methods"] == ["fill"]
    arr = np.array(Image.open(out).convert("L"))
    for x, y in glyphs:
        assert arr[y - 2 : y + 43, x - 2 : x + 37].min() > 200, f"glyph at {x},{y} erased"
    assert arr[180, 42:50].min() < 80, "bubble outline kept"
    assert arr[10, 10] < 150, "page outside the bubble untouched"


def test_letters_near_the_edge_of_a_bubble_cut_by_the_panel_border(tmp_path):
    """Regression: a bubble cut open by the panel border trips the leak check, which clips the
    background to an ellipse; letters near the box corners then stayed on the page."""
    src = tmp_path / "page.png"
    im = Image.new("RGB", (600, 400), (110, 80, 50))
    d = ImageDraw.Draw(im)
    d.rectangle((0, 0, 600, 40), fill="white")  # gutter above the panel
    d.rounded_rectangle((80, -60, 520, 230), radius=60, fill="white", outline="black", width=5)
    d.rectangle((80, 40, 520, 44), fill="white")  # the panel border cuts the bubble open
    glyphs = [(110, 60), (160, 60), (210, 60), (110, 150), (160, 150)]
    for x, y in glyphs:
        d.rectangle((x, y, x + 34, y + 40), outline="black", width=5)
    im.save(src)
    out = tmp_path / "clean.png"
    res = clean(str(src), str(out), [{"rid": "r1", "kind": "bubble", "bbox": [118, 58, 250, 192], "bubble": [80, 40, 520, 230]}], "lama")
    assert res["methods"] == ["fill"]
    arr = np.array(Image.open(out).convert("L"))
    for x, y in glyphs:
        assert arr[y - 2 : y + 43, x - 2 : x + 37].min() > 200, f"glyph at {x},{y} erased"
    assert arr[300, 300] < 150, "art below the bubble untouched"
    assert arr[120, 516:521].max() < 80, "bubble outline kept"


def test_tight_text_box_in_round_bubble_on_dark_page(tmp_path):
    """Regression: detector boxes hug the text, so letters touch the box border. The text must
    still be erased, and nothing outside the round bubble (dark art, outline) may be painted."""
    src = tmp_path / "page.png"
    im = Image.new("RGB", (400, 300), "black")
    d = ImageDraw.Draw(im)
    d.ellipse((60, 40, 340, 260), fill="white", outline="black", width=4)
    for i in range(3):  # three "lines of text" touching the text box edges
        d.rectangle((140, 110 + i * 30, 260, 124 + i * 30), fill="black")
    im.save(src)
    out = tmp_path / "clean.png"
    res = clean(str(src), str(out), [{"rid": "r1", "kind": "bubble", "bbox": [140, 110, 260, 184], "bubble": [60, 40, 340, 260]}], "lama")
    assert res["methods"] == ["fill"]
    arr = np.array(Image.open(out).convert("L"))
    assert arr[110:185, 140:261].min() > 200, "text erased"
    assert arr[45, 70] < 50 and arr[255, 330] < 50, "dark corners outside the bubble untouched"
    assert arr[150, 61:66].min() < 80, "bubble outline kept"
