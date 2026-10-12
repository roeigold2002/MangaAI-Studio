# MangaAI Studio — Architecture

Personal desktop manga reader with AI translation into Hebrew. Single user, local-first.
Upstream evaluation: [docs/UPSTREAM_EVALUATION.md](docs/UPSTREAM_EVALUATION.md). Decisions log: [docs/DECISIONS.md](docs/DECISIONS.md).

## Overview

```
┌──────────────────────────── Tauri 2 desktop app ────────────────────────────┐
│  React + TypeScript UI (WKWebView)                                           │
│   Home · Library · Details · Reader · Editor · Browse · Downloads · Queue ·   │
│   History · Favorites · Translation settings · App settings                  │
│        │ invoke() commands / events          ▲ mangaimg:// image protocol    │
│  ──────┼─────────────────────────────────────┼─────────────────────────────  │
│  Rust core (src-tauri)                                                        │
│   db (SQLite, migrations) · library (import, natural sort, rescan)            │
│   assets (originals / clean / translated / thumbs, atomic writes)             │
│   secrets (macOS Keychain via keyring) · settings                             │
│   providers (Anthropic, OpenAI, Gemini, OpenAI-compatible, Ollama, Claude CLI)│
│   translate (prompt v-N, schema validation, translation memory, cost)         │
│   pipeline (per-page stages + cache keys) · jobs (persistent queue)           │
│   sources (adapter trait → Suwayomi GraphQL) · downloads (queue)              │
│        │ JSON lines over stdin/stdout                                         │
└────────┼─────────────────────────────────────────────────────────────────────┘
         ▼
  Python vision worker (worker/, uv-managed venv)
   detect (RT-DETR-v2) · ocr (manga-ocr) · order · inpaint (LaMa / OpenCV)
   render (Pillow + libraqm: HarfBuzz shaping + FriBidi bidi)

  Optional: Suwayomi-Server on 127.0.0.1:4567 (sources & extensions)
```

## Module boundaries

| Module | Location | Responsibility |
|---|---|---|
| Desktop shell | `src-tauri/src/lib.rs` | Window, command registration, startup recovery, `mangaimg://` protocol |
| Storage | `src-tauri/src/db/` | SQLite schema + migrations (`PRAGMA user_version`) |
| Library | `src-tauri/src/library/` | Import folders/ZIP/CBZ, natural page order, rescan without duplicates |
| Assets | `src-tauri/src/assets.rs` | File layout, hashing, thumbnails, atomic writes |
| Reader | `src/pages/Reader/` | Paged / spread / vertical modes, RTL/LTR navigation, preload, progress |
| Providers | `src-tauri/src/providers/` | `TranslationProvider` trait + one adapter per API |
| Translation | `src-tauri/src/translate/` | Prompt, context building, JSON validation, glossary & memory |
| Pipeline | `src-tauri/src/pipeline/` | Page stages, cache keys, status from real results |
| Jobs | `src-tauri/src/jobs/` | Persistent queue: pause/resume/cancel/retry/priority/recovery |
| Vision worker | `worker/mangaai_worker/` | Detection, OCR, inpainting, Hebrew typesetting |
| Sources | `src-tauri/src/sources/` | `SourceAdapter` trait; Suwayomi implementation |
| Downloads | `src-tauri/src/downloads/` | Download queue writing into the local library |

## Data layout

App data dir (`~/Library/Application Support/com.mangaai.studio/` on macOS):

```
library.db                     SQLite (metadata, progress, jobs, OCR, translations, glossary)
library/<manga-id>/<chapter-id>/   pages copied from downloads (imports reference the user's files in place)
derived/<sha256[0:2]>/<sha256>/    per source-image derived files
    clean-<key>.png                text-removed image
    tr-<key>.png                   rendered Hebrew page
thumbs/<key>.webp
logs/
```

Original files are never written. Imported folders/archives stay where they are; ZIP/CBZ pages are read from the archive.
Derived files are content-addressed by the source image hash, so rescans and re-imports reuse them.

## Translation pipeline (per page)

1. **Analyze** (worker): detect bubbles + text → OCR each text block → reading order (RTL by default).
   Cached by `sha256(image) + detector/ocr version`.
2. **Translate** (Rust): pages are batched (default 2) with the series glossary, confirmed translation
   memory, recent dialogue from previous pages, and optionally the page image. The provider must return
   JSON matching the schema in `translate/schema.rs`; invalid output is retried, then reported.
   Cached by `sha256(image) + sha256(ocr texts) + provider + model + prompt version + glossary version + settings`.
3. **Clean** (worker): inpaint the text mask (LaMa, OpenCV fallback). Cached by image + mask hash.
4. **Render** (worker): Hebrew typesetting into each region with libraqm (`direction=rtl`), font-size
   search, word wrap, outline. Cached by everything above + render settings + translation text hash.
5. Status is recorded per page; a chapter is "translated" only when every page succeeded.

Approved (user-edited) regions are never overwritten by background retranslation.

## Security & privacy

- API keys live in the macOS Keychain; never in SQLite, settings JSON, logs or the webview.
  Provider calls run in Rust; the webview only sees "key configured: yes/no".
- No telemetry. Network calls only to: the configured AI provider during translation,
  the configured Suwayomi address, and Hugging Face for model download during setup.
- Local-only mode: only `ollama` / local OpenAI-compatible endpoints are allowed.
- The image protocol only serves files registered in the DB (no arbitrary path reads).
