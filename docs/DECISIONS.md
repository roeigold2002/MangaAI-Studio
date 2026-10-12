# Technical decisions

| # | Decision | Reason |
|---|---|---|
| D1 | Tauri 2 + React/TypeScript desktop app | No existing desktop reader exposes a clean hook for a translation pipeline (see UPSTREAM_EVALUATION.md). Tauri is small and native on macOS, and leaves room for Windows/Linux builds. |
| D2 | Rust core owns the DB, files, secrets, providers and jobs | The reader works without Python. Keys stay out of the webview. Long jobs run on tokio tasks and never block the UI. |
| D3 | Python worker handles vision only (detect, OCR, inpaint, render) | The models only exist as PyTorch/transformers. A long-lived process speaks JSON lines over stdio, keeps models in memory, and has no network port. |
| D4 | SQLite (rusqlite, bundled) with `user_version` migrations | Single-user, transactional, one file. Images stay on disk. |
| D5 | Suwayomi-Server as the online source backend through GraphQL | It is a maintained connector system (Mihon extensions on JVM) with an MPL-2.0 license. We call its API and download pages into our own library, so content stays readable if Suwayomi goes away. Behind a `SourceAdapter` trait. |
| D6 | OCR with manga-ocr, detection with RT-DETR-v2 comic detector, inpainting with anime LaMa | Permissive licenses (Apache-2.0/MIT). All three were already cached on this machine. |
| D7 | Hebrew rendering with Pillow + libraqm (HarfBuzz + FriBidi) | Real shaping and the Unicode bidi algorithm. Strings are never reversed by hand. If raqm is missing, the worker reports it and stops rendering instead of producing wrong output. |
| D8 | Providers: Anthropic, OpenAI, Gemini, OpenAI-compatible, Ollama, Claude Code CLI | Each adapter follows its official API. The Claude Code CLI adapter (`claude -p`) uses the user's existing Claude login without an API key. |
| D9 | Structured JSON output with stable region ids (`p<page>r<n>`), validated in Rust | Malformed output is retried, then reported. Translations are never guessed. |
| D10 | Content-addressed derived files + deterministic cache keys | Unchanged pages are not re-translated or re-rendered, and translations are not paid for twice. |
| D11 | UI in Hebrew, `dir="rtl"` on the shell; the reader controls direction separately | Page-navigation direction, bubble text direction and UI direction are three independent settings. |
| D12 | Mock provider/OCR only in tests | No fake translations ship in the app. |
