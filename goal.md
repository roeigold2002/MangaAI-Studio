MangaAI Studio — Make Manga Discovery and Hebrew Translation Effortless

You are working on the existing MangaAI Studio desktop application.

Your task is to transform it into a polished, beginner-friendly manga discovery, download, library, and AI translation application.

The user should never need to understand Suwayomi, extension repositories, GraphQL, source configuration, OCR engines, or API internals to read manga.

The core user experience

The entire product should revolve around this simple workflow:

Discover → Download → Library → Translate → Read

A beginner should be able to launch the app and understand what to do immediately.

The main journey must be:

Open the application.
Search for a manga by title.
Browse results with covers, titles, descriptions, and available chapter counts.
Open a manga details page.
Click "הורדת מנגה" or "הורדת פרקים".
Watch download progress.
Find the manga automatically in "הספרייה שלי".
Open the manga and click "תרגם לעברית".
Wait for translation progress.
Read the translated chapters, with translated pages saved locally for future reading.

Do not require the user to configure an extension repository manually as part of the normal first-run experience.

1. Inspect the existing implementation first

Preserve the current MangaAI Studio codebase and working functionality.

The existing project includes:

src-tauri/src/sources.rs, which integrates with Suwayomi for searching, browsing, retrieving chapters, and downloading pages.
settings.rs, which persists application settings.
Secure storage of the Suwayomi password in the macOS Keychain.
Existing UI screens for the library, sources, downloads, translation queue, translation settings, and application settings.

Inspect the actual code and current application behavior before editing.

Do not replace working functionality with mock data.

Do not rewrite the project from scratch.

Implement the smallest maintainable set of changes that delivers the complete beginner-friendly experience.

2. Make Search the primary entry point

Create a prominent search experience on the Home screen.

The search bar should be visible immediately when the app launches.

Support:

Searching by manga title.
Searching across all available configured sources.
Debounced search input.
Loading indicators.
Search cancellation where practical.
Empty-state messaging.
Retry after errors.
Deduplication of equivalent results when appropriate.
A source indicator when useful.
Cover images and metadata when available.

Display results as attractive cover cards with readable titles and useful metadata.

The interface must not require the user to select a source before searching.

If multiple sources return results for the same title, consolidate them when reliable matching information is available, while preserving the option to open a specific source result.

Do not merge unrelated manga simply because their titles are similar.

3. Solve the source problem behind the scenes

A search interface is useless if no sources are available.

Implement a reliable source bootstrap strategy using the existing Suwayomi integration.

Investigate the current Suwayomi version, supported Extension Store mechanism, compatible extensions, and existing project configuration.

Determine how a fresh installation can obtain usable manga sources with the least possible user effort.

Requirements:

Detect whether Suwayomi is installed and reachable.
Detect whether usable manga sources are available.
Automatically initialize safe, verified defaults where appropriate.
Explain what is missing if no sources are available.
Retry source initialization.
Refresh the source list after an extension is installed.
Preserve existing user configuration.
Never report success when the server has failed.
Never invent available manga results.
Never install arbitrary extensions without the user's consent.

Do not assume the official Suwayomi extension index provides ordinary manga websites. Verify what it actually contains.

If a compatible, maintained extension store is unavailable, present a clear explanation and an optional advanced setup path. Do not block local imports or reading because online sources are unavailable.

Keep advanced extension management under Settings or an "Advanced" section. Ordinary users should not need to open it.

Do not silently install unverified third-party stores.

4. Manga details and one-click downloading

When the user opens a search result, show:

Cover.
Title and alternate titles.
Description.
Available chapters.
Source availability.
Download status.
Translation status.
Reading progress.

Provide clear primary actions:

"הורדת פרק"
"הורדת כל הפרקים"
"קריאה עכשיו"

Support downloading a selected chapter range.

The default download behavior should be sensible and safe. Before downloading a very large series, show the chapter count and approximate disk-space requirement when it can be estimated reliably.

Downloads should:

Run in the background.
Show meaningful progress.
Support cancellation and retry.
Resume safely after recoverable failures.
Avoid unnecessary duplicate downloads.
Store chapters locally.
Update the library automatically.
Show a notification or clear completion indicator.

After a download completes, the user should not need to manually import the chapter.

5. The library must be automatic

Every successfully downloaded manga should appear automatically in "הספרייה שלי".

Organize the library into:

All manga.
Currently reading.
Recently added.
Downloaded.
Translated.
Favorites.

Each manga card should show:

Cover.
Title.
Download status.
Translation status.
Reading progress.
A clear "המשך קריאה" action when applicable.

Persist reading progress and metadata across application restarts.

Support local manga imports as a secondary workflow, but do not make importing files a prerequisite for using online search.

Keep the original files and downloaded content separate from generated translated images.

6. Make translation a primary action

The most important action in the library and manga details screen is:

"תרגם לעברית"

The user should not need to open the translation queue or configure an OCR engine to begin.

When clicked:

Check whether the required translation provider is configured.
If not configured, open a short, beginner-friendly setup dialog.
Explain which providers are available and which require API credentials.
Save credentials securely.
Return the user to the translation workflow.
Show the chapters available for translation.
Allow translation of the current chapter or selected downloaded chapters.
Start a persistent background job.
Display progress and estimated cost when reliable information is available.
Save successful translated pages locally.
Update the library's translation status.
Enable reading of the translated version.

If the provider is already configured, translation should start with one click, using the user's saved preferences.

Do not require the user to configure API credentials for a provider they are not using.

7. Preserve translation quality

Reuse the existing translation engine if it is already implemented.

Inspect the current OCR, translation, image-cleaning, rendering, and storage pipeline before making changes.

Ensure the pipeline supports:

OCR of source text.
Speech-region identification.
Context-aware translation.
Natural Hebrew.
Translation memory and glossary.
Correct right-to-left Hebrew shaping.
Text fitting within speech bubbles.
Image cleanup where supported.
Saving translated images.
Manual corrections.
Selective retries for failed pages.

Do not claim that a chapter is fully translated when some pages have failed.

Show partial completion accurately.

Keep the original page available at all times.

Cache results so reopening a translated chapter does not incur another translation charge.

8. Make the reader seamless

The reading screen should make it obvious which version is being displayed.

Include:

Original / Hebrew toggle.
Previous and next page navigation.
Next and previous chapter.
Page progress.
Full-screen mode.
Fit-to-width.
Fit-to-height.
Reading direction settings.
Remembered reading position.

If a chapter has not been translated, show a prominent "תרגם לעברית" action.

If translation is already complete, open the translated version according to the user's preference.

If translation is partially complete, allow the user to read completed pages and retry the remaining ones.

Do not force the user to leave the reader to manage translation jobs.

9. Redesign navigation around the user's goals

Use a clear main navigation:

בית
חיפוש
הספרייה שלי
הורדות
תור תרגום
הגדרות

The Home screen should prioritize search and recently read manga.

Move technical configuration, source-store management, API diagnostics, and advanced options into Settings.

Do not remove advanced capabilities; simply keep them out of the beginner's main workflow.

Use consistent Hebrew terminology throughout the UI.

10. First-run experience

Implement a lightweight onboarding experience.

On first launch:

Check whether online sources are available.
Check whether downloaded/local content exists.
Let the user configure a translation provider now or later.
Explain the basic workflow in a few concise steps.
Take the user directly to manga search when ready.

Avoid a long mandatory wizard.

The app must remain useful if the user has not configured an AI provider yet: searching, downloading, importing, and reading original manga should still work.

If no usable online sources exist, explain this clearly and offer local import and source setup as separate options.

Do not show a fake empty search result as though no manga exists on the internet. Distinguish between "no results" and "no sources configured".

11. Persistent state and reliability

Use the existing persistence architecture where possible.

Ensure the application correctly tracks:

Download jobs.
Downloaded chapters.
Translation jobs.
Completed translations.
Failed pages.
Provider settings.
Reading progress.
Original and translated assets.

Jobs should recover safely after application restart.

Do not re-download or re-translate completed work unnecessarily.

Protect against corrupted files and interrupted writes.

Keep API keys secure and redact secrets from logs.

12. Tests and real verification

Add tests for:

Search with multiple available sources.
Search with no configured sources.
Search with no results.
Duplicate results.
Download completion and failure.
Automatic library insertion.
Translation with a configured provider.
Translation setup when no provider is configured.
Partial chapter translation.
Cache reuse.
Persistence across restart.
Correct refresh of sources after extension installation.

Use mocks for routine automated tests.

Then run the application and verify the complete flow using a real downloadable sample if a suitable source is available.

Do not fabricate successful network requests or translation results.

Report clearly which steps were tested against real services and which were tested with mocks.

13. Completion criteria

The feature is complete when a beginner can:

Open the app.
Search for a manga without configuring technical settings.
Select a result.
Download a chapter.
Find the manga in the library automatically.
Open the chapter.
Click "תרגם לעברית".
Follow translation progress.
Read the translated chapter.
Close and reopen the app without losing downloads, translations, or reading progress.

If the first-run environment has no usable online sources, the app must explain why and provide the shortest supported setup path. Do not hide the underlying dependency.

Start by inspecting the current implementation, then implement and test this end-to-end workflow. Prioritize a working, polished user journey over adding more configuration screens.