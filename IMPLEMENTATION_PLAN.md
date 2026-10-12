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
- ☑ Windows: cross-compiled from a Debian container (x86_64-pc-windows-gnu, MinGW): the app links into
  `mangaai-studio.exe` (PE32+ GUI x86-64) and the test binaries link. Running them needs a Windows host.
- Not possible on this machine: Apple notarization (requires a paid Developer ID) and Windows testing
  (no Windows host). The code paths are platform-neutral (keyring has Windows support, worker uses `Scripts/python.exe`).

## Phase 9: Beginner discovery flow (Discover → Download → Library → Translate → Read) ☑
- ☑ Navigation: בית · חיפוש · הספרייה שלי · הורדות · תור תרגום · הגדרות. Technical options live under Settings
  (tabs: כללי / תרגום / מקורות ותוספים (מתקדם) / נתונים).
- ☑ Home: search bar first, short first-run welcome (sources / library / provider status), continue reading.
- ☑ Search across every installed source at once (debounced, cancellable, per-source timeouts and failures,
  merge only on identical title and author). "No sources" is shown separately from "no results".
- ☑ Source bootstrap: detect server, managed one-click install of the pinned official Suwayomi release
  (SHA-256 verified, 127.0.0.1 only, auto start/stop), official store registered, no extension installed
  without an explicit click, extension stores and extensions managed under Settings.
- ☑ Result page: read now (first chapter jumps the queue and opens when downloaded), single chapter, all
  chapters, chapter range, disk estimate from earlier downloads, per-chapter status.
- ☑ Downloads resume from the last saved page; completion and failure toasts; library insertion is automatic.
- ☑ "תרגם לעברית": provider readiness check → short setup dialog (only when needed) → chapter choice → estimate.
- ☑ Reader: shown-version label, translate banner, in-reader job progress, partial results plus retry, page and
  chapter buttons.

### Verification: real vs mocked

| Step | How it was verified |
|---|---|
| First launch, no server | **Real**: fresh data folder, app UI (scripted clicks in the real webview) |
| One-click engine install and start | **Real**: downloaded the official v2.4.2366 macOS arm64 release, checksum verified, server started by the app |
| Official store + extension install, sources refresh | **Real**: store registered on setup; "Suwayomi" extension installed through the confirm dialog; online source count 0 → 1 |
| Search across sources | **Real** with 2 sources (Local source + Suwayomi extension, which fails honestly when not configured); **mock** for 3 sources with duplicates and failures |
| Result page → download all → library | **Real**: Local source chapter (CC-BY Pepper&Carrot), 4 pages, appeared in הספרייה שלי automatically |
| Translation setup dialog → translate | **Real**: Claude Code (opus), 4/4 pages, $0.21 reported |
| Read translated chapter | **Real**: reader showed Hebrew pages |
| Restart: downloads, translations, progress kept | **Real**: second launch on the same data folder; also a unit test |
| Managed server stops with the app (quit and SIGTERM) | **Real** |
| No sources / no results / duplicates / install failure / resume / provider not configured | **Mocks** (wiremock GraphQL, fake sources) |
| Online manga **websites** | **Not tested**: none are available without a third-party extension store, which the app does not add on its own |
