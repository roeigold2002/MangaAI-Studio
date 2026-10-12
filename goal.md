MEGA PROMPT — Personal Manga Reader & AI Hebrew Translation Studio
1. Your role

Act as a senior software architect, desktop application engineer, AI translation engineer, OCR/computer-vision specialist, UX designer, and test engineer.

Your mission is to build a polished, production-quality, open-source-based desktop manga reading application for my personal use.

You are operating inside Claude Code with access to the project's development environment. Do not merely explain what should be done. Inspect the repository, make decisions based on evidence, implement the application, run tests, fix problems, and document the result.

Work autonomously whenever reasonable. Ask me questions only when a genuinely blocking decision cannot be resolved safely through repository inspection or a documented default.

Do not stop after producing a plan or a prototype. Aim for a fully working application, implementing the project in verifiable stages.

2. Product vision

Build a personal desktop application that lets me:

Discover manga through configurable, supported online sources.
Download manga chapters and volumes.
Import manga already stored on my computer.
Organize everything in a beautiful local library.
Read manga comfortably, including right-to-left reading.
Translate manga pages into natural, high-quality Hebrew using AI.
Choose my preferred AI provider and model.
Save translated pages and chapters locally.
Reopen translated manga without paying for translation again.
Edit and correct translations.
Preserve original pages and switch between original and translated versions.
Read downloaded manga without an internet connection.
Keep the application private, with no mandatory account, subscription to this application, or public server.

The application is for one person, on a personal computer. Do not build unnecessary multi-user infrastructure, social features, public hosting, or user-management systems.

Do not assume a specific operating system. Inspect the current environment and design for the platform that is actually available. Prefer a cross-platform architecture when it does not add unreasonable complexity.

3. First task: inspect existing open-source projects

Before implementing a new manga reader, investigate the following projects and their current state:

Suwayomi-Server

https://github.com/Suwayomi/Suwayomi-Server

Libre Manga Translator

https://github.com/mrdhnto/libre-manga-translator

Mihon

https://github.com/mihonapp/mihon

Kotatsu

https://github.com/KotatsuApp/Kotatsu

Other actively maintained desktop manga readers or translation projects that offer a better fit.

Use the available web and Git tools when appropriate. Inspect actual repository files, documentation, dependency compatibility, recent maintenance activity, issue history when accessible, and license terms.

For each candidate, determine:

Whether it is genuinely open source.
Its current license and obligations.
Whether it runs on the target desktop platform.
Whether it supports local manga imports and offline reading.
Whether it supports downloads and configurable content sources.
Whether its reader can be extended or integrated cleanly.
Whether its translation components can be reused legally and technically.
Whether it has a stable API, plugin system, or reusable modules.
Whether integration would be easier and more maintainable than building the relevant component ourselves.

Do not assume that a project supports a feature just because its description suggests it does. Verify the implementation.

Architecture decision

After investigating, choose the simplest robust architecture.

Preferred strategy:

Reuse an existing manga library/downloader/reader wherever practical.
Reuse a compatible translation engine where licensing and architecture permit.
Develop only the missing components needed for the desired user experience.
Prefer modular integration over invasive modifications to upstream projects.
Pin dependencies and document upstream changes.
Keep the original project license notices and comply with all applicable license obligations.

If no existing project can reasonably provide the desired desktop experience, create a desktop application using a suitable stack such as Tauri with React and TypeScript, or another well-justified alternative.

Do not spend the entire project building a custom downloader if a mature component already solves that problem.

Produce a concise architecture decision document before making major implementation decisions, then continue with implementation.

4. Core application experience

Create a cohesive desktop application with a polished, modern dark theme, excellent typography, smooth navigation, and a layout suitable for long reading sessions.

Avoid an unfinished-looking developer dashboard.

Main navigation

Include:

Home
Manga Library
Browse / Sources
Downloads
Translation Queue
Reading History
Favorites
Translation Settings
Application Settings

Use Hebrew-aware layouts where appropriate, while retaining familiar navigation patterns for an international manga application.

Home screen

Display:

Recently read manga.
Recently added manga.
Downloads in progress.
Recently translated chapters.
Translation queue status.
Quick access to continue reading.
Search and import actions.
Manga library

Support:

Grid and list views.
Cover thumbnails.
Search by title.
Sorting and filtering.
Favorites.
Custom categories.
Read/unread chapter states.
Reading progress.
Last-read position.
Downloaded chapter indicators.
Translation status indicators.

Store library metadata and reading progress persistently.

Manga details

Display:

Cover.
Title and alternate titles.
Description and available metadata.
Chapter list.
Read status.
Download status.
Translation status.
Download all / selected chapters.
Translate selected chapters.
Continue reading.

Handle missing metadata gracefully.

5. Importing and downloading manga

Implement two distinct workflows.

A. Local imports

Support importing manga from:

Image folders.
ZIP archives containing page images.
CBZ archives.
Other formats only when support is practical and reliable.

Support common image formats, including JPEG, PNG and WebP.

Preserve original files whenever possible. Do not modify user-owned archives destructively.

Allow the user to select a root manga folder and scan it for supported files.

Detect page ordering carefully. Handle filenames such as:

1.jpg
02.jpg
10.jpg
page_001.png
chapter-01/page-001.webp

Do not sort pages lexicographically when numeric sorting is required.

Allow users to rescan imported folders and detect new chapters without creating unnecessary duplicates.

B. Online sources

Investigate and integrate a suitable existing source-management/downloader component.

Requirements:

Use configurable and supported sources.
Provide source search when the source supports it.
Show chapter lists and download progress.
Support cancellation and retry.
Avoid duplicate downloads.
Store downloaded chapters locally.
Respect source terms, access controls, copyright restrictions and applicable law.
Never bypass authentication, paywalls, DRM or other access controls.
Do not hardcode a single website as the only source.
Do not build fragile scraping infrastructure when a maintained connector system is available.

If online source integration is not viable within the chosen architecture, make local imports fully functional first and implement a documented adapter interface for future sources.

Downloaded files must be accessible to the local library even if the original source becomes unavailable.

6. Desktop manga reader

Build a reading interface designed for manga rather than a generic image viewer.

Essential reader features
Single-page reading.
Two-page spread.
Vertical scrolling.
Horizontal page navigation.
Keyboard navigation.
Mouse and trackpad navigation.
Fit-to-width and fit-to-height.
Zoom controls.
Full-screen mode.
Page counter.
Chapter navigation.
Configurable reading direction.
Remembered reading position.
Original / translated toggle.
Optional side-by-side original and translated comparison.
Page thumbnails or a page navigator.
Smooth loading of large images.
Preloading of nearby pages.
No unnecessary loading of an entire large chapter into memory.

Support right-to-left manga reading and left-to-right reading.

For Hebrew, use appropriate bidirectional text handling. Do not assume that reversing the order of characters is a valid way to render Hebrew.

The reader must distinguish between:

The direction in which the reader navigates pages.
The direction of the text inside speech bubbles.
The writing direction of Hebrew UI labels and translated text.

These are separate concerns.

7. AI translation system

Translation is a core product feature, not an optional demonstration.

Create a provider-independent translation architecture.

Provider adapter interface

Design a stable interface that allows adding, removing, and configuring translation providers without rewriting the reader.

Potential providers:

Anthropic Claude API.
OpenAI API.
Google Gemini API.
OpenAI-compatible endpoints.
Local inference through Ollama or another suitable local model server.
Future providers through plugins or adapters.

Do not assume every provider supports image input, structured outputs, or the same API format.

Implement each provider using its official, current API documentation.

Allow the user to configure:

Provider.
Model.
API key.
Endpoint when supported.
Translation quality profile.
Maximum concurrent requests.
Request timeout.
Retry policy.
Optional cost limits.
Translation glossary.
Target language.

The default target language should be Hebrew.

Store API credentials securely using the operating system's credential storage when possible. Do not save secrets in source files, logs, browser storage, exported configuration files, or version control.

Never expose provider API keys to an untrusted renderer or public frontend.

If a local server is used, make its address configurable and explain its requirements.

API cost awareness

Before translating a large chapter, estimate the work and, when provider pricing information is available, approximate the cost.

Ask for confirmation when the estimated cost exceeds the user's configured threshold.

Show:

Pages completed.
Pages remaining.
Current provider and model.
Failed pages.
Retry status.
Estimated or reported cost when available.

Do not promise an exact cost when token usage or image pricing cannot be determined reliably.

Provide cancellation and resumability.

8. Translation quality: context-aware Hebrew

Do not translate each speech bubble independently without context.

Create a context-aware translation pipeline that considers:

Multiple speech bubbles from the same page.
Page reading order.
Speaker identity when inferable.
Character names and relationships.
Established terminology.
Honorifics and culturally specific expressions.
Previous translated pages or chapters when useful.
Tone, humor, emotion, slang, and dialogue style.
Hebrew grammar, natural phrasing, and gender agreement where context supports it.
Translation memory

Maintain a local translation memory per manga or series.

Store:

Character names and aliases.
Preferred Hebrew spellings.
Character relationships when known.
Recurring terms.
Attack names, places and organizations.
User corrections.
Style preferences.
Relevant chapter context.

Allow users to edit the glossary manually.

Reuse confirmed translations to improve consistency across chapters.

Do not automatically treat every uncertain OCR result as a confirmed glossary entry.

Translation output

Prefer structured responses with stable IDs linking each translated string to its source text region.

A translation result should include, when applicable:

Source text.
Hebrew translation.
Region identifier.
Reading order.
Text category.
Confidence or uncertainty indicators.
Optional notes for ambiguous text.

Validate structured output before using it.

If the provider returns malformed data, retry or report a recoverable error rather than corrupting the page.

Translation prompt design

Create a dedicated, versioned translation prompt.

The translator should:

Translate naturally into contemporary Hebrew.
Preserve meaning, tone and character voice.
Avoid unnecessary literal translations.
Preserve meaningful humor and wordplay where possible.
Keep dialogue concise enough for speech bubbles.
Preserve names consistently.
Avoid adding explanations to dialogue.
Respect the series glossary.
Distinguish speech from sound effects and signs.
Mark genuinely uncertain text rather than inventing missing content.

Do not claim perfect translation. Provide a practical review workflow for ambiguous dialogue.

9. OCR and image processing pipeline

Build a modular image-processing pipeline.

The expected stages are:

Load the original page.
Detect text regions and speech bubbles.
Recognize source text using OCR.
Associate text with regions and determine reading order.
Translate text with contextual information.
Remove or cover original text when appropriate.
Render Hebrew text into the appropriate regions.
Validate the rendered result.
Save the translated page and metadata.
Display the result in the reader.

Investigate whether Libre Manga Translator or another suitable open-source engine can provide these capabilities without requiring a complete rewrite.

Reuse compatible components where practical.

OCR requirements
Support Japanese source text initially.
Design for future support of Korean, Chinese and English.
Make OCR engines replaceable.
Handle vertical Japanese text when supported by the selected engine.
Preserve region coordinates.
Expose OCR confidence and errors.
Allow correction of recognized text.
Handle pages with no detectable text gracefully.

Do not pretend that a generic OCR model reliably handles every manga layout.

Image cleaning

Use an appropriate text-removal or inpainting component.

Requirements:

Preserve the original page.
Never overwrite the original image.
Avoid destructive repeated edits.
Keep intermediate processing reproducible where practical.
Support retrying a page without re-downloading it.
Handle text on patterned backgrounds as well as plain speech bubbles.
Report areas where removal quality is poor.

If the existing engine provides better text removal than a custom implementation, reuse it.

Hebrew text rendering

Implement proper right-to-left rendering.

Requirements:

Use a text-shaping and rendering solution that supports Hebrew correctly.
Support Unicode and bidirectional text.
Choose readable fonts that support Hebrew.
Handle punctuation, numbers, Latin words and mixed-direction text.
Fit translated text inside speech bubbles without clipping.
Adjust line breaks, font size, alignment and padding.
Support configurable font size and margins.
Preserve the original bubble shape as much as practical.
Keep text readable against the background.
Support manual adjustments when automatic layout fails.

Do not reverse the Hebrew string manually.

Do not assume that ordinary left-to-right canvas text rendering will automatically produce correct Hebrew shaping.

Test mixed Hebrew, English, digits, parentheses and punctuation.

10. Translation editor

Include a practical editor for fixing imperfect translations.

The editor should let me:

View the original page.
View the translated page.
Select a detected text region.
Edit OCR text.
Edit the Hebrew translation.
Adjust text position and size where feasible.
Retry OCR or translation for an individual region.
Reprocess an individual page.
Confirm a correction.
Save changes permanently.
Restore the original page.
Compare previous and current results.

After manual correction, preserve the approved translation in the local translation memory.

Do not silently overwrite approved edits during a background retranslation.

Support selective retranslation when the prompt, model, glossary, or translation settings change.

11. Persistent storage and caching

All library data should remain local by default.

Design a reliable storage system for:

Manga metadata.
Chapter metadata.
Downloaded archives and page images.
Original images.
Translated images.
OCR results.
Translation results.
Glossaries.
Translation memory.
Reading progress.
User settings.
Job queues.
Processing errors.
Model and prompt metadata.

Choose a suitable database such as SQLite unless the chosen framework provides a compelling alternative.

Keep large images as files rather than storing them as huge database blobs without a clear reason.

Cache invalidation

Use a deterministic cache key that considers relevant inputs, such as:

Source image hash.
OCR engine and configuration.
Translation provider.
Model.
Prompt version.
Glossary version.
Image-processing configuration.
Rendering settings.

Avoid retranslating an unchanged page unnecessarily.

Keep original and translated files clearly separated.

Use atomic writes or equivalent safeguards to avoid corrupting saved files if the application closes during processing.

Support recovery from interrupted jobs.

12. Translation queue and background processing

Create a persistent job queue.

Support:

Translate one page.
Translate one chapter.
Translate selected chapters.
Queue multiple chapters.
Pause and resume.
Cancel.
Retry failed pages.
Retry only missing pages.
Prioritize the currently open chapter.
Limit parallel requests.
Recover interrupted jobs after application restart.

Avoid freezing the user interface during OCR or image processing.

Use suitable worker processes or background tasks.

Do not overload the AI provider or local computer with uncontrolled concurrency.

Show meaningful progress and errors rather than a generic spinner.

13. User interface and design system

Build a polished, coherent interface.

Design principles:

Dark mode as the default.
Optional light mode.
Modern desktop layout.
Consistent spacing and typography.
Clear visual hierarchy.
Responsive layout for common desktop window sizes.
Keyboard accessibility.
Useful empty states.
Clear loading and error states.
Accessible contrast.
No fake controls or buttons that do nothing.
No placeholder content masquerading as real data.
No unnecessary animations during reading.

The reader should prioritize the manga image over surrounding UI.

The application should feel like a dedicated reading product rather than an administrative dashboard.

Use reusable components and a consistent design system.

14. Privacy and personal use

The application is intended for a single user on a personal computer.

Requirements:

No mandatory account.
No public server.
No automatic upload of an entire manga library to a third party.
No analytics or telemetry by default.
No hidden background network requests.
No unnecessary external dependencies at runtime.
Clear disclosure when images or OCR text are sent to an AI provider.
Configurable local-only mode where technically supported.
Local storage of library and translation data.
Secure API credential storage.
Clear deletion and export controls.
No exposure of local files to untrusted network clients.

When using cloud translation, send only the data needed for the translation task.

Explain that sending page images or extracted text to an external AI provider is not equivalent to local processing.

Do not claim that cloud translation is completely private or offline.

Bind local services to localhost by default unless a different configuration is explicitly required.

15. Licensing and copyright

Maintain a strict open-source compliance process.

Before copying or adapting code:

Identify the exact repository and license.
Read its license file.
Review applicable attribution requirements.
Review copyleft obligations.
Document modifications to third-party code.
Preserve required notices.
Record third-party dependencies and licenses.
Avoid copying code when compatibility is unclear.

Do not assume that every public GitHub repository is permissively licensed.

Prefer a maintainable architecture that minimizes unnecessary license conflicts.

Provide an appropriate THIRDPARTYNOTICES document and an accurate README.

The application should not circumvent DRM, paywalls, or access controls.

Make clear that the user is responsible for ensuring that their downloads, translations, and other uses comply with applicable laws and source terms.

16. Suggested technical architecture

Choose technologies based on the actual repository and operating system.

For a custom desktop shell, consider:

Tauri for a lightweight native desktop application.
React and TypeScript for the frontend.
SQLite for persistent metadata.
A suitable native or local service layer for filesystem operations.
Python or another suitable worker environment for OCR and image processing.
A provider adapter layer for translation APIs.
A persistent job queue.
Local filesystem storage for manga and translated assets.

These are candidate technologies, not mandatory choices.

If integrating an existing server or reader is clearly better, use it instead of building a redundant replacement.

Keep module boundaries clear:

Desktop shell.
Manga source and download integration.
Library and metadata.
Reader.
OCR.
Translation providers.
Translation orchestration.
Image processing and rendering.
Local storage.
Job queue.
Settings and security.

Define stable interfaces between modules.

Do not place the entire application in a single giant component or script.

17. Error handling and reliability

Handle failures explicitly.

Examples:

Invalid API key.
Rate limit.
Provider outage.
Network interruption.
Corrupted archive.
Unsupported image.
Missing font.
OCR failure.
Translation timeout.
Malformed AI output.
Failed text removal.
Failed page rendering.
Insufficient disk space.
Database migration failure.
Application restart during translation.

Provide useful error messages and recovery actions.

Never mark a chapter as fully translated when pages have failed.

Track page-level status and calculate chapter status from actual results.

Use structured logs, but redact API keys and sensitive content.

18. Testing strategy

Write automated tests throughout development.

Unit tests

Test:

Natural numeric page sorting.
Archive parsing.
File discovery.
Cache-key generation.
Provider selection.
Structured AI output validation.
Retry behavior.
Job state transitions.
Hebrew text direction handling.
Translation memory updates.
Database migrations.
Integration tests

Test:

Importing a sample CBZ.
Opening a chapter.
Persisting reading progress.
Mock translation provider.
Mock OCR engine.
Cache reuse.
Interrupted-job recovery.
Failed-page retries.
Editing and saving a translation.
Switching between original and translated pages.
Visual and manual tests

Use a legally usable sample manga or synthetic test pages.

Include Hebrew test strings containing:

Plain Hebrew.
Hebrew and English mixed together.
Numbers and punctuation.
Parentheses.
Multiple lines.
Long dialogue.
Short dialogue.
Empty or uncertain OCR results.

Verify that the application renders these cases correctly.

Do not spend API credits during routine automated tests. Use mocked providers by default.

19. Development workflow

Follow this workflow continuously:

Phase 1 — Repository inspection
Inspect the current working directory.
Read the existing project structure.
Identify the environment and installed tools.
Check existing project instructions.
Inspect candidate upstream repositories and licenses.
Identify the smallest viable architecture.
Phase 2 — Architecture

Create:

ARCHITECTURE.md
IMPLEMENTATION_PLAN.md
THIRDPARTYNOTICES.md
A documented list of major technical decisions.

Then proceed to implementation. Do not wait for me to approve every routine decision.

Phase 3 — Working application foundation

Implement:

Application startup.
Navigation.
Local settings.
Persistent database.
Local library.
Importing CBZ and image folders.
A functioning manga reader.
Reading progress persistence.

Make this phase usable before implementing the full AI pipeline.

Phase 4 — Download integration

Integrate a suitable existing downloader or source adapter.

Implement persistent downloads, progress, cancellation and error handling.

Phase 5 — AI translation

Implement:

Provider interface.
Secure API configuration.
OCR adapter.
Context-aware Hebrew translation.
Translation memory.
Translation queue.
Translated image storage.
Phase 6 — Page rendering and editing

Implement Hebrew text shaping, text placement, image cleanup, translation review, manual corrections and selective reprocessing.

Phase 7 — Hardening

Run tests, fix failures, improve reliability, verify privacy settings, review license compliance and test a clean installation.

Phase 8 — Packaging

Provide a repeatable build process for the detected operating system.

When practical, provide installers or packaged application artifacts for the target platform.

Document any external runtimes, model downloads, API keys or configuration steps required.

20. Definition of done

The project is not complete merely because the frontend looks attractive.

The minimum successful application must allow me to:

Launch the application on my computer.
Import a real manga archive from disk.
See it in my local library.
Open a chapter.
Navigate pages and preserve reading progress.
Configure an AI translation provider.
Translate a sample page into Hebrew.
Display Hebrew correctly inside translated page regions.
Save the translated page.
Close and reopen the application without losing the result.
Open the translated page again without paying for the same translation again.
Edit a translation and save the correction.
Recover from a failed translation.
Keep the original page unchanged.
Use the reader offline for content already downloaded.

Online discovery and downloading should be completed where a suitable supported integration exists.

If a feature cannot be completed because an external dependency is unavailable, clearly explain the limitation and implement the closest honest, functional alternative.

Never simulate a successful translation with hardcoded demo results.

21. Final instructions

Start by inspecting the current workspace and available tools.

Investigate existing open-source projects before selecting the final architecture.

Then implement the application incrementally.

Do not stop after planning. Do not rewrite a mature subsystem without a clear technical reason. Do not make unsupported claims about upstream capabilities.

After each major phase:

Run the relevant tests.
Fix failures.
Update documentation.
Summarize what is genuinely working.

If credentials are missing, implement a mockable provider interface and explain how I can configure real credentials later.

If a model or OCR dependency is unavailable, create a clear adapter boundary and complete the remaining work rather than blocking the entire project.

At the end, provide:

A concise architecture summary.
The implemented features.
The files and modules created.
The tests executed and their actual results.
Exact installation and launch commands.
API configuration instructions.
Remaining limitations.
Any licensing considerations that require attention.

Begin now by inspecting the workspace, verifying the candidate repositories, and choosing the best implementation strategy for this personal desktop manga reader with high-quality Hebrew AI translation.