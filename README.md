# MangaAI Studio

A personal desktop manga reader with AI translation into Hebrew. Local-first, single user,
no account, no telemetry. Runs natively on macOS (Apple Silicon) and Windows 10/11 x64; both are tested
end to end (import, OCR, translation, Hebrew typesetting, managed Suwayomi). The stack is Tauri 2 +
React, a Rust core and a Python worker; Linux builds and tests pass but the window is untested there.

- **Library**: import image folders, CBZ and ZIP (originals are never modified), natural page
  order (`2.jpg` < `10.jpg`), rescan for new chapters, categories, favorites, read state, history.
- **Reader**: single page, two-page spread, vertical scroll. RTL/LTR page direction per manga. Fit
  to screen or width, zoom, full screen, page thumbnails, preloading, remembered position, original/translated
  toggle, side-by-side compare, keyboard and mouse navigation. Works offline.
- **Translation**: bubble detection → OCR (Japanese: manga-ocr; English: TrOCR) → context-aware translation (glossary,
  translation memory, previous lines, optional page images) → text removal (LaMa) → Hebrew
  typesetting with HarfBuzz/FriBidi. Results are cached on disk; reopening a translated page costs nothing.
- **Providers**: Claude Code CLI (your Claude login, no API key), Anthropic API, OpenAI, Gemini,
  Ollama (local), any OpenAI-compatible server. API keys go in the OS credential store
  (macOS Keychain / Windows Credential Manager).
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

### Windows

| | Needed for | Install |
|---|---|---|
| Node 20+ and npm | building the UI | `winget install OpenJS.NodeJS` |
| Rust (stable, MSVC) | building the app | `winget install Rustlang.Rustup` |
| Visual Studio 2022+ or Build Tools with "Desktop development with C++" | Rust linker, FriBiDi | `winget install Microsoft.VisualStudio.2022.BuildTools` |
| uv | the AI engine's Python environment, building FriBiDi | `winget install astral-sh.uv` |
| WebView2 runtime | the window | preinstalled on Windows 11; the installer fetches it otherwise |

Hebrew shaping needs `fribidi-0.dll` (Pillow's Windows wheels leave it out). `npm run app:dev` and
`npm run app:build` build it once from the official release with `scripts/build-fribidi.ps1`
(checksum-verified, MSVC, static CRT) and the installer ships it. Nothing is installed system-wide.

**Code signing (Windows):** everything is ready except the certificate itself, which has to be bought.
- **Options:**
  - **Azure Trusted Signing** (about $10/month; the cheapest; a SmartScreen reputation builds quickly).
  - An **OV** certificate from a certificate authority (about $200–400/year; SmartScreen warnings fade as downloads accumulate).
  - An **EV** certificate (more expensive; trusted by SmartScreen right away).
- **Local signed build:**
  1. Set `MANGAAI_SIGN_PFX` and `MANGAAI_SIGN_PFX_PASSWORD` (or the Azure variables listed in `scripts/sign-windows.ps1`).
  2. Run `.\scriptsuild-signed.ps1`. It signs the app, the bundled DLLs, the NSIS plugins, the uninstaller and the
     installer, adds an RFC 3161 timestamp, and finally prints the installer's Authenticode status.
- **Releases:** add the repository secrets `WINDOWS_CERTIFICATE` (the .pfx as base64) and `WINDOWS_CERTIFICATE_PASSWORD`, or
  `AZURE_TENANT_ID`, `AZURE_CLIENT_ID` and `AZURE_CLIENT_SECRET` plus the variables `TRUSTED_SIGNING_ENDPOINT`,
  `TRUSTED_SIGNING_ACCOUNT` and `TRUSTED_SIGNING_PROFILE`. `release.yml` then signs automatically; without them it builds
  unsigned as before.
- **Ordinary builds:** `npm run app:build` never signs and needs no Windows SDK. The signing settings live in
  `src-tauri/tauri.signing.conf.json`, which is applied only for a signed build.
- **Verified:** with a self-signed test certificate (kept as a file; nothing was added to the Windows certificate store),
  the installed app, DLL and uninstaller all carried the signature and a DigiCert timestamp.

**NVIDIA GPU (optional, Windows):** PyPI's Windows PyTorch is CPU-only. With an NVIDIA card, Translation
settings → AI engine offers "use the NVIDIA GPU": the locked environment is installed as usual, then torch
is swapped for the same version built for CUDA 13.0 (hash-pinned in `worker/requirements-cuda-win.txt`;
Turing through Blackwell GPUs, driver 580+). Measured on an RTX 5080: detection + OCR 3.5× and LaMa text
removal about 9× faster than the CPU build. Unticking it and reinstalling returns to the exact CPU lock.

## Build and run

```bash
npm install
```

```bash
npm run app:dev
```

Release build (macOS: `src-tauri/target/release/bundle/macos/MangaAI Studio.app` and a `.dmg`;
Windows: `src-tauri/target/release/bundle/nsis/MangaAI Studio_<version>_x64-setup.exe`, a per-user installer):

```bash
npm run app:build
```

The bundle is ad-hoc signed (no Apple Developer ID, so not notarized). If macOS blocks the first
launch, right-click the app → Open.

## Releases and updates

Installed apps check `latest.json` on the newest published GitHub release once a day at startup, and
on request from Settings → Data → About → "check for updates". A newer version opens a prompt with its
version and release notes; nothing is downloaded until the user picks "install and restart". Queued
downloads and translations continue after the restart. Updates are signed with the updater key; the
public half is in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`), and the app rejects files
not signed with the matching private key.

To release a version:

1. Set the same version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
   (`npm install` and `cargo check` refresh the lock files), commit, and push.
2. Tag it with the release notes as the tag message. The notes are what the update prompt shows:

   ```bash
   git tag -a v0.2.0 -m "מה חדש בגרסה הזו"
   ```

   ```bash
   git push origin v0.2.0
   ```

3. The Release workflow (`.github/workflows/release.yml`) fails fast if the tag and the version
   differ, then builds the Windows installer and the macOS app and `.dmg`, signs the updater bundles, and
   attaches everything plus `latest.json` to a **draft** release (about 20 minutes).
4. Try the installer from the draft, then publish it. Apps see the update only once it is published.
   Editing the release text on GitHub does not change the notes in `latest.json`.

The workflow needs two repository secrets (Settings → Secrets and variables → Actions):
`TAURI_SIGNING_PRIVATE_KEY` (the contents of the private key file) and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Keep the key file and its password backed up outside the repo:
without them, installed copies can no longer be updated and need a manual reinstall. Local
`npm run app:build` builds do not create updater files and need no key.

## AI engine setup (once)

Either in the app (**Translation settings → AI engine → Install**, then **Download missing models**), or:

```bash
./scripts/setup-worker.sh
```

On Windows (PowerShell):

```powershell
.\scripts\setup-worker.ps1
```

This creates `~/Library/Application Support/com.mangaai.studio/worker-venv` (Windows:
`%APPDATA%\com.mangaai.studio\worker-venv`) from the locked
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
provider. Keys are stored in the macOS Keychain / Windows Credential Manager under the service
`com.mangaai.studio`.

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
- **Sources come from extension stores.** On setup the app registers two stores:
  - the official Suwayomi store. Its only extension, "Suwayomi", connects to another Suwayomi server and
    provides no manga websites.
  - the **Keiyoushi** community store (`keiyoushi/extensions`, registered through its `repo.json`). It had
    1,425 extensions in October 2026, 139 of them Japanese, including publisher sites such as BookWalker and Comic Action.
    Its old `index.min.json` is now only an "Outdated App" stub; Suwayomi follows `repo.json` to the current `index.pb`.

  Setup then installs the **default extensions**, MangaDex and MangaFire from Keiyoushi, and searches their
  **English** sources (`sources.languages` defaults to `["en"]`). A new install can search and download right
  away with nothing to configure. Both lists are settings: `sources.default_extensions` (package names) and
  `sources.languages`. No other extension is installed without a click. For a server set up earlier,
  **Settings → מקורות ותוספים (מתקדם) → השלמת ברירות המחדל** adds whatever is missing. Stores can also be added
  or removed there.
- Suwayomi 2.x replaced the old `extensionRepos` setting with **Extension Stores**
  (`addExtensionStore` / `removeExtensionStore`). The app uses the Extension Store API. Stores can be added or
  removed; Suwayomi has no "disable" operation.
- Some sources need settings (server address, login…): Settings → מקורות ותוספים → מקורות מותקנים → הגדרות המקור.
- **New chapters:** two minutes after launch and then every 12 hours (`sources.check_new_chapters_hours`, 0 = off), the
  app asks each online series' source for chapters the library has never seen in any version. New chapters are queued
  one version each, using the same rules as "download all" (`sources.auto_download_new`; off = notify only). A Windows
  notification and an in-app message name them. Chapters that failed before are not retried behind your back.
  **הספרייה שלי → בדיקת פרקים חדשים** checks right away. A live check against your own server, on a database copy:
  `cargo run --example check_new -- <copy of library.db> http://127.0.0.1:4567`.
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

`~/Library/Application Support/com.mangaai.studio/` on macOS, `%APPDATA%\com.mangaai.studio\` on Windows
(override with `MANGAAI_DATA_DIR`):

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

### Backup and sync between computers

**Settings → Data → גיבוי וסנכרון.**
- **Sync:** choose a folder both computers can reach (OneDrive, Dropbox, Google Drive or a network share). Each sync merges
  the folder's `mangaai-sync.json` into the library ("newer wins" per item), then writes the merged state back, so
  computers converge. Syncs run 30 s after launch, every 15 minutes, on exit and on demand.
  - **What syncs:** reading progress, read marks, favorites, categories, the glossary, translation memory, and the list
    of online series.
  - **How items are matched:** a series by its source and reference (local imports by title), a chapter by its number
    or title. Read marks, categories, glossary entries and memory are never deleted by a sync.
  - **What doesn't travel:** pages and translations. An online series that exists only on the other computer is added
    to the library, and the new-chapter check then downloads it. Local imports can't travel, since their files are
    on the other computer.
- **Backup:** a ZIP with a consistent copy of the database (`VACUUM INTO`) and the readable sync state, optionally with
  the downloaded chapters. API keys are never included. **Restore** checks the file and extracts missing chapter files;
  the database is swapped in on the next start, and the previous one is kept as `library.db.before-restore-<time>`.

## Tests

GitHub Actions (`.github/workflows/ci.yml`) runs these three suites and the TypeScript check on Windows and macOS (Apple Silicon)
for every pull request and every push to `main`.

```bash
cd src-tauri && cargo test
```

```bash
cd worker && DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib uv run --managed-python pytest -q
```

On Windows run it without the variable; the tests load `src-tauri\windows\native\fribidi-0.dll`.

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

The managed Suwayomi install itself (download, checksum, start, GraphQL, stop):

```bash
cd src-tauri && cargo run --example suwayomi_managed
```

(Create the CBZ first: `cd tests/fixtures/pepper_ep06_ja && zip -0 ../generated/pepper_ep06_ja.cbz *.jpg`.)

## Known limitations

- Local OCR covers Japanese (manga-ocr) and English (TrOCR `microsoft/trocr-base-printed`, about 1.3 GB, downloaded
  with "download missing models"). English OCR reads lettered dialogue well and gives low confidence on sound effects
  and art; small slips ("ABIT") are left for the translator and the editor. Korean and Chinese pages are read from the
  page image by the translator, which needs a provider with image input.
  Measured with Claude Haiku via the CLI, English OCR translates in about the same time and cost as reading from the
  image (22-32 s, about $0.027 per page). Its gains are text you can edit and re-OCR in the editor, and English
  translation with text-only providers such as Ollama, fully offline.
- RAR/CBR archives are not read. Extract them first; the import reports skipped RAR files.
- Reading order inside a page uses a row heuristic. Complex layouts may need reordering in the editor.
- Text removal on detailed artwork can leave traces. These pages are flagged "complex background" in the editor.
- Cost estimates are approximate. Actual reported usage is recorded per provider in the queue.
- No manga website source ships with the app. Website extensions come from the Keiyoushi store (or another
  store the user adds) and are installed only on request. The live tests used the official store's "Suwayomi"
  extension, the server's Local source, and registration of both default stores
  (`cargo run --example suwayomi_managed`, which also installs the default extensions and lists the English sources).
- Linux: the Rust core and worker test suites pass in a Debian container. The desktop window itself
  was not run there.
- Windows: tested on Windows 11 x64 (MSVC build, WebView2). Windows on ARM is not built. Until a code-signing
  certificate is configured (see "Code signing (Windows)"), the installer is unsigned and SmartScreen may ask for
  confirmation on first run ("More info" → "Run anyway").
