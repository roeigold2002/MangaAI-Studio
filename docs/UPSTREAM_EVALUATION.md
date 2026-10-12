# Upstream evaluation

Checked on 2026-10-12 against the live GitHub API and repository files (not just descriptions).

| Project | License (verified file) | Last push | Platform | Fit |
|---|---|---|---|---|
| [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server) | MPL-2.0 | 2026-10-12 | JVM server; release v2.4.2366 ships `macOS-arm64.tar.gz`, Windows, Linux | **Used as optional online-source backend** |
| [Mihon](https://github.com/mihonapp/mihon) | Apache-2.0 | 2026-10-12 | Android only | Not usable on desktop. Its extension ecosystem is reachable through Suwayomi. |
| [Kotatsu](https://github.com/KotatsuApp/Kotatsu) | GPL-3.0 | archived 2025-11 | Android only | Archived; not used. `kotatsu-parsers` is also archived (GPL-3.0). |
| [Libre Manga Translator](https://github.com/mrdhnto/libre-manga-translator) | AGPL-3.0-or-later (v1.0.0+), MIT for betas | 2026-10-09 | Browser extension (Svelte 5 + WXT, WebGPU/WASM) | Not integrated: runs inside a browser page, no reusable desktop API, AGPL would apply to the whole app. Pipeline ideas (detect → OCR → LLM → LaMa inpaint → render) match ours. |
| [comic-translate](https://github.com/ogkalu2/comic-translate) | Apache-2.0 | 2026-09-11 | Python/Qt desktop app | Not embedded (Qt app, not a library). It confirmed the model choices below. No code copied. |
| [manga-image-translator](https://github.com/zyddnys/manga-image-translator) | GPL-3.0 | 2026-10-11 | Python | Not used (GPL, monolithic). |
| [BallonsTranslator](https://github.com/dmMaze/BallonsTranslator) | GPL-3.0 | 2026-10-04 | Python/Qt | Not used (GPL, Qt app). |
| [manga-ocr](https://github.com/kha-white/manga-ocr) | Apache-2.0 | 2026-07-19 | Python library | **Used** as the Japanese OCR engine (pip dependency). |

## Models (Hugging Face API, `license:` tag)

| Model | License | Use |
|---|---|---|
| `ogkalu/comic-text-and-bubble-detector` (RT-DETR-v2) | Apache-2.0 | Text + bubble detection |
| `kha-white/manga-ocr-base` | Apache-2.0 | Japanese OCR (handles vertical text) |
| `anime-manga-big-lama.pt` (TorchScript of `dreMaz/AnimeMangaInpainting`) | MIT | Inpainting / text removal |

## Why not build on one of them

- No desktop reader in the list both runs on macOS and exposes an extension point for a translation pipeline.
  Suwayomi is a server; its official UI is a web app. Embedding a web UI we don't control would make
  the reader/translation integration a fork.
- Suwayomi *is* the mature connector system (it runs Mihon/Tachiyomi extensions on the JVM), so online
  discovery and downloading go through its GraphQL API instead of a custom scraper.
- The translation engines are either GPL/AGPL monoliths or Qt apps. We use the same permissively
  licensed models they use, through their libraries (`transformers`, `manga-ocr`, TorchScript LaMa),
  and write our own orchestration around them.
