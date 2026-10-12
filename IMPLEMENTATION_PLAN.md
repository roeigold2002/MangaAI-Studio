# Implementation plan

Status: ☑ done and verified · ◐ partial · ☐ not done

## Phase 1: Inspection ☑
The workspace was empty. Host: macOS 27 arm64, 48 GB RAM, Node 26, Rust 1.98, uv, Ollama, Claude Code CLI.
Models were already cached locally (manga-ocr, comic detector, anime LaMa).

## Phase 2: Architecture ☑
ARCHITECTURE.md, docs/DECISIONS.md, docs/UPSTREAM_EVALUATION.md, THIRDPARTYNOTICES.md.

## Phase 3: Foundation ☑
- ☑ Tauri 2 + React shell, Hebrew RTL design system, navigation (Home, Library, Favorites, History,
  Browse, Downloads, Queue, Translation settings, Settings)
- ☑ SQLite schema + versioned migrations (tested: fresh, idempotent, refuses newer schema)
- ☑ Import of folders, ZIP and CBZ; nested folders inside archives; natural sort; rescan without duplicates;
  corrupt files reported (unit + integration tests; a real 1930-page folder imported in 2.8 s)
- ☑ `mangaimg://` protocol (DB-registered images only) + cached thumbnails
- ☑ Reader: single / spread / vertical, RTL/LTR, fit, zoom, full screen, thumbnails, preload,
  progress persistence, chapter-to-chapter navigation (verified with webview snapshots)

## Phase 4: Online sources ☑
- ☑ `SourceAdapter` trait + Suwayomi GraphQL adapter (queries checked against upstream source files;
  tested against a mock server)
- ☑ Browse/search/latest UI, chapter list, persistent download queue (progress, cancel, retry,
  dedupe, atomic chapter folders, disk-space check)
- ☑ Live run against Suwayomi-Server v2.4.2366 (official macOS arm64 release, checksum verified, bound to
  127.0.0.1, built-in Local source): list, browse, search, chapters, cancel, retry, 4-page download, dedupe,
  page served from the local library; Browse UI snapshot against the live server

## Phase 5: AI translation ☑
- ☑ Python worker: RT-DETR-v2 detection, manga-ocr with confidence, multi-line splitting, hallucination guard, reading order
- ☑ Providers: Anthropic (structured output + refusal fallback), OpenAI, OpenAI-compatible (schema fallback),
  Gemini, Ollama, Claude Code CLI; Keychain secrets; redaction
- ☑ Versioned prompt (he-v1), strict JSON schema, validation, retries (transport, malformed, missing ids)
- ☑ Glossary (confirmed vs suggested), translation memory, previous-line and page-summary context
- ☑ Persistent job queue: pause/resume/cancel, retry failed pages, priority, crash recovery,
  cost estimate, confirmation threshold, per-job cost cap, usage log
- ☑ Real end-to-end runs: Pepper&Carrot JA → Hebrew with Claude Code (opus), 3 pages for $0.12; repeated on
  the clean install after the concurrency change (2 pages, $0.08)

## Phase 6: Rendering & editing ☑
- ☑ LaMa inpainting with plain-bubble fill and OpenCV fallback; quality notes
- ☑ Hebrew typesetting with libraqm (RTL, bidi, mirrored brackets, numbers, Latin), fit search,
  page-level size consistency, outline on non-bubble text
- ☑ Editor: regions overlay, OCR/translation edit, approve → memory, layout overrides, add/delete/reorder,
  retry OCR/translation for one region, re-render, re-analyze, restore original, previous-version compare

## Phase 7: Hardening ☑
- ☑ 70 Rust tests, 17 Python tests, 9 TypeScript tests (see README → Tests)
- ☑ Privacy: no telemetry, offline model loading, cloud notice, local-only mode, keys in Keychain
- ☑ License review (THIRDPARTYNOTICES.md)
- ☑ Clean installation: fresh data dir, fresh HF cache, Python env created from the release bundle's
  `uv.lock`, all models downloaded (LaMa checksum verified), then the full e2e run on top of it
- ☑ Failure recovery with real components: invalid model → job failed, pages marked failed (not
  translated) → "retry failed pages" → done
- ☑ Offline reading: app run under `sandbox-exec (deny network*)`; library and reader (translated and
  original pages) render, Browse shows the offline message

## Phase 8: Packaging ☑
- ☑ `npm run app:build` → .app + .dmg for macOS arm64, ad-hoc signed with hardened runtime
  (`codesign --verify --deep --strict` passes); signed release launches
- ☑ Linux: Rust core (70 tests) and worker (17 tests, libraqm available) pass in a Debian bookworm
  aarch64 container with the Tauri/WebKitGTK build dependencies
- ☑ AI engine installer (in-app and `scripts/setup-worker.sh`)
- Not possible on this machine: Apple notarization (requires a paid Developer ID) and Windows testing
  (no Windows host). The code paths are platform-neutral (keyring has Windows support, worker uses `Scripts/python.exe`).
