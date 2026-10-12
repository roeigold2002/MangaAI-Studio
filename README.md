# MangaAI Studio

A personal desktop manga reader with AI translation into Hebrew. Local-first, single user,
no account, no telemetry. macOS (Apple Silicon) is the tested platform. The stack (Tauri 2 +
React, Rust core, Python worker) also targets Windows and Linux, but those builds are untested.

- **Library**: import image folders, CBZ and ZIP (originals are never modified), natural page
  order (`2.jpg` < `10.jpg`), rescan for new chapters, categories, favorites, read state, history.
- **Reader**: single page, two-page spread, vertical scroll. RTL/LTR page direction per manga. Fit
  to screen or width, zoom, full screen, page thumbnails, preloading, remembered position, original/translated
  toggle, side-by-side compare, keyboard and mouse navigation. Works offline.
- **Translation**: bubble detection → Japanese OCR → context-aware translation (glossary,
  translation memory, previous lines, optional page images) → text removal (LaMa) → Hebrew
  typesetting with HarfBuzz/FriBidi. Results are cached on disk; reopening a translated page costs nothing.
- **Providers**: Claude Code CLI (your Claude login, no API key), Anthropic API, OpenAI, Gemini,
  Ollama (local), any OpenAI-compatible server. API keys go in the macOS Keychain.
- **Editor**: fix OCR text, edit and approve translations (stored in translation memory and never
  overwritten), font size/alignment/position, add or delete regions, change reading order,
  re-run OCR or translation for one region, restore the original, compare with the previous render.
- **Queue**: persistent, survives restarts. Pause, resume, cancel, retry failed pages, priority
  for the chapter you're reading, cost estimate and confirmation threshold, per-job cost cap.
- **Online sources**: through a Suwayomi-Server you run yourself (Mihon/Tachiyomi
  extensions). Search, browse, chapter lists, download queue with progress, cancel and retry.
  Downloads land in the local library.

Architecture: [ARCHITECTURE.md](ARCHITECTURE.md) · decisions: [docs/DECISIONS.md](docs/DECISIONS.md) ·
upstream review: [docs/UPSTREAM_EVALUATION.md](docs/UPSTREAM_EVALUATION.md) · licenses: [THIRDPARTYNOTICES.md](THIRDPARTYNOTICES.md)

## Requirements

| | Needed for | Install |
|---|---|---|
| Node 20+ and npm | building the UI | `brew install node` |
| Rust (stable) | building the app | `brew install rust` or rustup |
| uv | the AI engine's Python environment | `brew install uv` |
| FriBidi | Hebrew text shaping (loaded by Pillow/libraqm) | `brew install fribidi` |
| ~3 GB disk | PyTorch + models | |

Reading and importing need none of the AI parts.

## Build and run

```bash
npm install
```

```bash
npm run app:dev
```

Release build (creates `src-tauri/target/release/bundle/macos/MangaAI Studio.app` and a `.dmg`):

```bash
npm run app:build
```

The bundle is ad-hoc signed (no Apple Developer ID, so not notarized). If macOS blocks the first
launch, right-click the app → Open.

## AI engine setup (once)

Either in the app (**Translation settings → AI engine → Install**, then **Download missing models**), or:

```bash
./scripts/setup-worker.sh
```

This creates `~/Library/Application Support/com.mangaai.studio/worker-venv` from the locked
`worker/uv.lock` and downloads three models (detector and OCR from Hugging Face, LaMa from
GitHub). After that the engine runs offline: the worker is started with `HF_HUB_OFFLINE=1`.

In development the app uses `worker/.venv` if it exists (`cd worker && uv sync --managed-python`).

## Configuring a translation provider

Open **Translation settings**, pick a provider, and press **Test connection**.

- **Claude Code (default)**: needs the `claude` CLI logged in (`claude` → `/login`). Uses your
  Claude plan. Set the model to `opus`, `sonnet` or `haiku`.
- **Anthropic API**: paste a key from console.anthropic.com. Default model `claude-opus-5-5`
  ($4 / $20 per million input/output tokens, editable for estimates). Uses structured JSON output.
- **OpenAI / Gemini**: paste a key and enter a model id that supports image input if
  "send page images" is on.
- **Ollama**: `ollama serve`, pull a model, pick it from the list. Nothing leaves the computer.
  Hebrew quality depends heavily on the model.
- **OpenAI-compatible**: base URL such as `http://127.0.0.1:1234/v1` (LM Studio).

Before the first cloud translation you must accept the privacy notice. It explains that page text,
and page images if enabled, are sent to the provider. **Local-only mode** blocks every cloud
provider. Keys are stored in the macOS Keychain under the service `com.mangaai.studio`.

## Discover → download → library → translate → read

1. **Home / חיפוש**: type a title. The search runs across every installed source at once (no source picker).
   Results from different sources are merged only when both title and author match exactly.
2. **Result page**: "קריאה עכשיו" downloads the first chapter at the front of the queue and opens it;
   "הורדת כל הפרקים", a single chapter, or a chapter range. Large batches show the chapter count and a disk-space
   estimate based on chapters already downloaded.
3. Downloads run in the background (cancel / retry / resume from the last saved page) and appear in
   **הספרייה שלי** automatically. A toast confirms each completed chapter.
4. **תרגם לעברית** (manga page or inside the reader): if no translation provider is ready, a short dialog
   asks for one (Claude Code needs no key; API providers ask for a key, stored in the Keychain). Then pick
   chapters, see the estimate, start. Progress shows inside the reader; partial results stay readable and the
   rest can be retried from the reader.
5. The reader labels which version is shown (עברית / מקור). Translations are cached, so reopening costs nothing.

## Online sources: how a fresh install gets them

Searching and downloading go through **Suwayomi-Server**, which runs Mihon/Tachiyomi source extensions.

- **No server yet** → the app offers **התקנה והפעלה**: it downloads the pinned official release
  (`v2.4.2366`, about 330 MB, macOS/Linux/Windows x64 builds), verifies its SHA-256 against the release's
  `Checksums.sha256`, unpacks it to `…/com.mangaai.studio/suwayomi/`, and starts it on `127.0.0.1:4567`
  with its own data folder. The app starts it with every launch and stops it on quit.
- **You already run Suwayomi** → Settings → מקורות ותוספים: enter its address (a *server* URL, not a store URL).
  A server the app did not install is never started, stopped or modified.
- **Sources come from extension stores.** On setup the app registers the official Suwayomi store. It was
  verified in October 2026 to contain one extension, "Suwayomi", which connects to another Suwayomi server
  and provides no manga websites. Other stores are published by third parties. The app does not add them on
  its own and installs no extension without an explicit click. Add a store and install extensions under
  **Settings → מקורות ותוספים (מתקדם)**.
- Suwayomi 2.x replaced the old `extensionRepos` setting with **Extension Stores**
  (`addExtensionStore` / `removeExtensionStore`). The app uses the Extension Store API. Stores can be added or
  removed; Suwayomi has no "disable" operation.
- Some sources need settings (server address, login…): Settings → מקורות ותוספים → מקורות מותקנים → הגדרות המקור.
- The managed server's **Local source** reads `…/suwayomi/data/local/<Title>/<Chapter>.cbz`. Files placed there
  show up in search too.

### Troubleshooting

| What you see | What to do |
|---|---|
| "צריך להתקין את מנוע המקורות" | Press **התקנה והפעלה** (needs internet once), or point Settings at your own server. |
| "מנוע המקורות כבוי" | Press **הפעלה**. If it stops again, see `logs/suwayomi.log`. Port 4567 may be taken; change `managed_port`, or stop the other program. |
| "אין עדיין מקורות אונליין" | The server runs but has no source extensions. Add a store and install an extension under Settings → מקורות ותוספים. |
| Search shows "N מקורות לא ענו" | Those sources failed (site down, Cloudflare, …). Results from the others are still shown; **נסה שוב** retries. |
| Store rejected ("HTTP error 404" …) | The address must be the store's index `.json` over https; the server fetches it and refuses non-stores. |
| Translation dialog keeps asking | The listed item is missing (key, model, Claude Code not found, or the privacy confirmation). |

## Where things are stored

`~/Library/Application Support/com.mangaai.studio/` (override with `MANGAAI_DATA_DIR`):

```
library.db        SQLite: library, progress, OCR, translations, glossary, memory, jobs
library/          downloaded chapters
derived/          cleaned + translated page images (content addressed; originals untouched)
thumbs/           thumbnails
models/           LaMa weights
worker-venv/      AI engine Python environment
logs/             app.log, worker.log (no API keys)
```

**Settings → Data** shows the sizes, opens the folder, clears thumbnails and exports everything
(minus images and secrets) to JSON.

## Tests

```bash
cd src-tauri && cargo test
```

```bash
cd worker && DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib uv run --managed-python pytest -q
```

```bash
npm test
```

Routine tests use mocks: wiremock GraphQL servers for multi-source search, no sources, no results,
duplicates, store and extension failures and the post-install source refresh; fake sources for download
resume and failure. The real end-to-end journeys are listed in IMPLEMENTATION_PLAN.md (Phase 9).

The automated tests never call paid APIs; translation uses a scripted mock provider and a fake
vision worker. The end-to-end check below uses the real worker and a real provider, and costs about
$0.04–0.07 per page with Claude Opus:

```bash
cd src-tauri && cargo run --example e2e -- ../tests/fixtures/generated/pepper_ep06_ja.cbz claude_cli opus 1
```

It also forces a real failure (invalid model), checks the pages are marked failed, and recovers
with "retry failed pages". The live Suwayomi check (needs a running server):

```bash
cd src-tauri && cargo run --example suwayomi_live -- http://127.0.0.1:4567 local
```

(Create the CBZ first: `cd tests/fixtures/pepper_ep06_ja && zip -0 ../generated/pepper_ep06_ja.cbz *.jpg`.)

## Known limitations

- OCR is Japanese only (manga-ocr). Korean, Chinese and English need another OCR engine behind
  the same worker interface. The language selector marks them unavailable.
- RAR/CBR archives are not read. Extract them first; the import reports skipped RAR files.
- Reading order inside a page uses a row heuristic. Complex layouts may need reordering in the editor.
- Text removal on detailed artwork can leave traces. These pages are flagged "complex background" in the editor.
- Cost estimates are approximate. Actual reported usage is recorded per provider in the queue.
- No manga website source ships with the app or the official store. Online manga sites need a third-party
  extension store chosen by the user. The live tests used the official store's "Suwayomi" extension and
  the server's Local source.
- Linux: the Rust core and worker test suites pass in a Debian container. The desktop window itself
  was not run there. Windows: the app and its test binaries cross-compile and link for x86_64 Windows (MinGW), but
  were not run (no Windows machine available).
