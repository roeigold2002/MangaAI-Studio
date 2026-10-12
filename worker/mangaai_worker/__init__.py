"""MangaAI Studio vision worker.

Runs as a child process of the desktop app and speaks JSON lines over
stdin/stdout. It never opens a network port. Model weights are loaded from
the local Hugging Face cache; downloads only happen in `prepare_models`.
"""

PROTOCOL_VERSION = 1
# Bump when detection/OCR output changes so cached analyses are invalidated.
ANALYSIS_VERSION = "a1"
INPAINT_VERSION = "i1"
RENDER_VERSION = "r1"
