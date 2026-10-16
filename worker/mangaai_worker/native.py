"""Native libraries loaded at run time.

Pillow's Windows wheels include libraqm and HarfBuzz but not FriBiDi. At import
time `PIL._imagingft` calls LoadLibraryA("fribidi"), then "fribidi-0", then
"libfribidi-0" (observed in the shipped .pyd); if none loads, Pillow silently
disables raqm and Hebrew cannot be shaped. LoadLibraryA searches PATH, so the
folder holding the app's fribidi-0.dll goes first on PATH before Pillow loads.
"""

from __future__ import annotations

import os
import sys


def setup() -> str | None:
    """Put MANGAAI_NATIVE_DIR on the DLL search path. Returns the folder used."""
    d = os.environ.get("MANGAAI_NATIVE_DIR")
    if not d or not os.path.isdir(d):
        return None
    if sys.platform == "win32":
        parts = os.environ.get("PATH", "").split(os.pathsep)
        if d not in parts:
            os.environ["PATH"] = os.pathsep.join([d, *parts])
    return d
