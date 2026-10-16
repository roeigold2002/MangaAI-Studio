# Third-party notices

MangaAI Studio's own code is under the MIT license. It uses the components below
unmodified. No third-party source code was copied into this repository.

Generated from `cargo metadata`, `node_modules/*/package.json` and the worker's
installed Python packages on 2026-10-12. Re-check after changing dependencies.

## Bundled with the app

### Fonts (`resources/fonts/`, `src/assets/fonts/`)
| Font | License | Source |
|---|---|---|
| Varela Round (Hebrew) | SIL OFL 1.1 (`OFL-varelaround.txt`) | github.com/alefalefalef/Varela-Round-Hebrew via google/fonts |
| Secular One | SIL OFL 1.1 (`OFL-secularone.txt`) | github.com/googlefonts/secular |
| Karantina | SIL OFL 1.1 (`OFL-karantina.txt`) | github.com/ronykoch/Karantina |
| Amatic SC | SIL OFL 1.1 (`OFL-amaticsc.txt`) | github.com/googlefonts/AmaticSC |
| Rubik (UI, via `@fontsource-variable/rubik`) | SIL OFL 1.1 | fontsource.org |

The OFL allows bundling. The fonts are not sold separately and are not renamed.

### FriBiDi (Windows builds only: `native/fribidi-0.dll`)
GNU FriBiDi 1.0.17, LGPL-2.1-or-later (`native/COPYING-fribidi.txt`), github.com/fribidi/fribidi.
Built unmodified from the release tarball by `scripts/build-fribidi.ps1` (SHA-256 checked) and shipped
as a separate DLL that Pillow loads at run time, so it can be replaced with any compatible build.
Pillow's Windows wheels include HarfBuzz and libraqm but leave FriBiDi to the application.

### uv (Windows builds only: `native/uv.exe`)
uv 0.12.10 by Astral, MIT OR Apache-2.0, github.com/astral-sh/uv. The official release binary, unmodified
(`scripts/fetch-uv.ps1` checks its SHA-256); it creates the AI engine's Python environment from `worker/uv.lock`.

### PyTorch NVIDIA build (Windows, optional)
`worker/requirements-cuda-win.txt` pins torch 2.14.1+cu130 (BSD-3-Clause; bundles NVIDIA CUDA runtime
libraries under the NVIDIA CUDA EULA). Downloaded from download.pytorch.org only when the user enables
"NVIDIA GPU" while installing the AI engine; it is never bundled with the app.

### Rust crates (552 in the dependency tree)
Most are MIT and/or Apache-2.0. Others:
- **MPL-2.0** (file-level copyleft; used unmodified; source at crates.io): `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short` (via Tauri/wry), `option-ext`.
- **BSD-3-Clause**: `alloc-no-stdlib`, `alloc-stdlib`, `brotli`, `brotli-decompressor`, `subtle`.
- **Unicode-3.0**: ICU data crates. **Zlib / ISC / Unlicense** dual-licensed crates.
- `r-efi` (MIT OR Apache-2.0 OR LGPL-2.1+): used under MIT/Apache.
- SQLite (via `rusqlite` bundled): public domain.

Key direct dependencies: tauri, tauri-plugin-dialog, tauri-plugin-opener, tauri-plugin-single-instance, windows-sys (MIT/Apache-2.0), rusqlite (MIT),
tokio (MIT), reqwest (MIT/Apache-2.0), serde (MIT/Apache-2.0), zip (MIT), image (MIT/Apache-2.0),
keyring (MIT/Apache-2.0), sha2 (MIT/Apache-2.0), tracing (MIT).

Full list with license texts: `cargo about` or `cargo license` in `src-tauri/`.

### JavaScript packages
| Package | License |
|---|---|
| react, react-dom, react-router-dom, zustand | MIT |
| @tauri-apps/api, plugin-dialog, plugin-opener | MIT OR Apache-2.0 |
| lucide-react | ISC |
| @fontsource-variable/rubik | OFL-1.1 |

## Installed on demand (AI engine, not inside the .app)

The Python worker environment is created by `uv` from `worker/uv.lock` the first time
the user installs the AI engine.

| Package | License |
|---|---|
| torch | BSD-3-Clause (PyTorch) plus bundled parts; wheel metadata: Apache-2.0 AND Apache-2.0 WITH LLVM-exception AND BSD-2-Clause AND BSD-3-Clause AND BSL-1.0 AND MIT |
| transformers, huggingface_hub, tokenizers, safetensors, hf-xet | Apache-2.0 |
| manga-ocr (kha-white) | Apache-2.0 |
| pillow | MIT-CMU (HPND) |
| numpy | BSD-3-Clause (+ bundled 0BSD/MIT/Zlib parts) |
| opencv-python-headless | Apache-2.0 |
| fugashi | MIT AND BSD-3-Clause |
| unidic-lite | MIT (package); UniDic data: BSD-3-Clause |
| jaconv, loguru, rich, typer, PyYAML | MIT |
| regex | Apache-2.0 AND CNRI-Python |
| tqdm | MPL-2.0 AND MIT |
| others (click, idna, Jinja2, MarkupSafe, networkx, sympy, mpmath, fsspec, filelock, …) | BSD / MIT / Apache-2.0 / PSF |

System libraries loaded at runtime by Pillow for Hebrew shaping: **libraqm** (MIT),
**HarfBuzz** (MIT), **FriBidi** (LGPL-2.1-or-later). FriBidi is loaded dynamically
from the system (`brew install fribidi`). It is not bundled or modified.

### Model weights (downloaded to the user's cache)
| Model | License | Use |
|---|---|---|
| `ogkalu/comic-text-and-bubble-detector` (RT-DETR-v2) | Apache-2.0 | text and bubble detection |
| `kha-white/manga-ocr-base` | Apache-2.0 | Japanese OCR |
| `anime-manga-big-lama.pt`: TorchScript export (Sanster/models) of `dreMaz/AnimeMangaInpainting`, based on LaMa (Apache-2.0) | MIT | text removal |

## External services and programs (not bundled)
- **Suwayomi-Server**, MPL-2.0: an optional separate program. MangaAI Studio talks to it over its GraphQL API and does not include or modify its code.
- **Claude Code CLI**, **Ollama**, and the Anthropic / OpenAI / Google APIs are used only if configured, under their own terms.

## Test material
- `tests/fixtures/pepper_ep06_ja/`: *Pepper&Carrot* episode 6, Japanese edition. Art and story by David Revoy (www.peppercarrot.com), CC-BY 4.0; Japanese translation by the Pepper&Carrot community. See `ATTRIBUTION.txt`.

## Projects evaluated, not used
Libre Manga Translator (AGPL-3.0), manga-image-translator (GPL-3.0), BallonsTranslator (GPL-3.0),
comic-translate (Apache-2.0), Mihon (Apache-2.0), Kotatsu (GPL-3.0). No code from these projects is
included. See `docs/UPSTREAM_EVALUATION.md`.
