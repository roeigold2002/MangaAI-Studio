import { invoke } from "@tauri-apps/api/core";
import type {
  BackupReport, CheckReport, MergeReport, SetupNeeds, SetupStep,
  Category, ChapterView, Download, EditorPage, EngineStatus, Estimate, GlossaryEntry, HistoryEntry, HomeData, ImportReport,
  Job, MangaDetail, MangaSummary, MemoryEntry, PageState, Region, RemoteChapter, RemoteDetail, RemoteManga, Settings, SourceInfo,
  SearchOutcome, SourceOverview, ExtensionStore, Extension, ProviderStatus, InstalledSource, SourcePreference,
  DownloadPlan, CleanupPlan, CleanupResult,
} from "./types";

export const api = {
  // library
  listManga: () => invoke<MangaSummary[]>("list_manga"),
  mangaDetail: (id: number) => invoke<MangaDetail>("manga_detail", { id }),
  chapterView: (chapterId: number) => invoke<ChapterView>("chapter_view", { chapterId }),
  importPaths: (paths: string[], mode: "manga" | "library") =>
    invoke<{ reports: ImportReport[]; errors: string[] }>("import_paths", { paths, mode }),
  rescanManga: (id: number) => invoke<ImportReport>("rescan_manga", { id }),
  updateManga: (id: number, update: Record<string, unknown>) => invoke<void>("update_manga", { id, update }),
  removeManga: (id: number, deleteDownloads: boolean) => invoke<void>("remove_manga", { id, deleteDownloads }),
  setChaptersRead: (chapterIds: number[], read: boolean) => invoke<void>("set_chapters_read", { chapterIds, read }),
  saveProgress: (chapterId: number, pageIdx: number) => invoke<void>("save_progress", { chapterId, pageIdx }),
  history: (limit?: number) => invoke<HistoryEntry[]>("reading_history", { limit }),
  clearHistory: () => invoke<void>("clear_history"),
  categories: () => invoke<Category[]>("list_categories"),
  createCategory: (name: string) => invoke<number>("create_category", { name }),
  renameCategory: (id: number, name: string) => invoke<void>("rename_category", { id, name }),
  deleteCategory: (id: number) => invoke<void>("delete_category", { id }),
  home: () => invoke<HomeData>("home_data"),
  // settings
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  setApiKey: (providerId: string, key: string) => invoke<void>("set_api_key", { providerId, key }),
  apiKeyStatus: () => invoke<Record<string, boolean>>("api_key_status"),
  testProvider: (providerId: string) =>
    invoke<{ text: string; model: string; ms: number; cost_usd: number | null }>("test_provider", { providerId }),
  ollamaModels: (endpoint: string) => invoke<string[]>("list_ollama_models", { endpoint }),
  claudeCliStatus: (path?: string) => invoke<{ found: boolean; path: string | null }>("claude_cli_status", { path }),
  engineStatus: () => invoke<EngineStatus>("engine_status"),
  installEngine: (gpu = false) => invoke<string>("install_engine", { gpu }),
  prepareModels: () => invoke<unknown>("prepare_models"),
  // translation
  estimate: (chapterIds: number[]) => invoke<Estimate>("estimate_translation", { chapterIds }),
  enqueue: (chapterIds: number[], opts: { pageIds?: number[]; mode?: string; priority?: number } = {}) =>
    invoke<number[]>("enqueue_translation", { chapterIds, pageIds: opts.pageIds, mode: opts.mode, priority: opts.priority }),
  jobs: (includeFinished = true) => invoke<Job[]>("list_jobs", { includeFinished }),
  jobAction: (id: number, action: "pause" | "resume" | "cancel" | "retry_failed") => invoke<number | null>("job_action", { id, action }),
  clearFinishedJobs: () => invoke<number>("clear_finished_jobs"),
  prioritizeChapter: (chapterId: number) => invoke<void>("prioritize_chapter", { chapterId }),
  usage: () => invoke<{ provider: string; model: string; requests: number; input_tokens: number; output_tokens: number; cost_usd: number | null }[]>("usage_summary"),
  // editor
  editorPage: (pageId: number) => invoke<EditorPage>("editor_page", { pageId }),
  updateRegion: (regionId: number, edit: Partial<Pick<Region, "source_text" | "translation" | "skip" | "approved" | "category">> & { layout?: Record<string, unknown> }) =>
    invoke<Region>("update_region", { regionId, edit }),
  addRegion: (pageId: number, bbox: [number, number, number, number]) => invoke<Region>("add_region", { pageId, bbox }),
  deleteRegion: (regionId: number) => invoke<void>("delete_region", { regionId }),
  moveRegion: (regionId: number, delta: number) => invoke<void>("move_region", { regionId, delta }),
  retryRegionOcr: (regionId: number) => invoke<Region>("retry_region_ocr", { regionId }),
  retranslateRegion: (regionId: number) => invoke<Region>("retranslate_region", { regionId }),
  rerenderPage: (pageId: number) => invoke<PageState>("rerender_page", { pageId }),
  reanalyzePage: (pageId: number) => invoke<EditorPage>("reanalyze_page", { pageId }),
  restoreOriginal: (pageId: number) => invoke<void>("restore_original", { pageId }),
  resetPage: (pageId: number) => invoke<void>("reset_page", { pageId }),
  // glossary
  glossary: (mangaId: number) => invoke<GlossaryEntry[]>("list_glossary", { mangaId }),
  upsertGlossary: (mangaId: number, entry: { source: string; target: string; kind: string; gender: string | null; notes: string; status?: string }) =>
    invoke<number>("upsert_glossary", { mangaId, entry }),
  deleteGlossary: (id: number) => invoke<void>("delete_glossary", { id }),
  memory: (mangaId: number) => invoke<MemoryEntry[]>("list_memory", { mangaId }),
  deleteMemory: (id: number) => invoke<void>("delete_memory", { id }),
  // sources
  sourceStatus: () => invoke<string>("source_status"),
  sources: () => invoke<SourceInfo[]>("list_sources"),
  browse: (sourceId: string, kind: string, query: string | null, page: number) =>
    invoke<{ mangas: RemoteManga[]; has_next_page: boolean }>("browse_source", { sourceId, kind, query, page }),
  remoteManga: (mangaRef: string) => invoke<RemoteDetail>("remote_manga", { mangaRef }),
  /** `bestOnly`: one version per chapter (download all / a range); otherwise exactly the chapters given. */
  enqueueDownloads: (manga: RemoteManga, chapters: RemoteChapter[], opts: { bestOnly?: boolean; preferred?: string | null } = {}) =>
    invoke<{ added: number; skipped: number; manga_id: number; plan: DownloadPlan | null }>("enqueue_downloads", {
      manga, chapters, bestOnly: opts.bestOnly ?? false, preferred: opts.preferred ?? null,
    }),
  planDownloads: (manga: RemoteManga, chapters: RemoteChapter[], preferred: string | null) =>
    invoke<DownloadPlan>("plan_downloads", { manga, chapters, preferred }),
  duplicateCleanupPlan: (mangaId: number, placeholders: boolean) => invoke<CleanupPlan>("duplicate_cleanup_plan", { mangaId, placeholders }),
  cleanUpDuplicates: (mangaId: number, placeholders: boolean, chapterIds: number[], downloadIds: number[]) =>
    invoke<CleanupResult>("clean_up_duplicates", { mangaId, placeholders, chapterIds, downloadIds }),
  downloads: () => invoke<Download[]>("list_downloads"),
  downloadAction: (id: number, action: "cancel" | "retry") => invoke<void>("download_action", { id, action }),
  clearFinishedDownloads: () => invoke<number>("clear_finished_downloads"),
  setSourcePassword: (password: string) => invoke<void>("set_source_password", { password }),
  // discovery
  sourceOverview: () => invoke<SourceOverview>("source_overview"),
  setupSources: () => invoke<SourceOverview>("setup_sources"),
  startSources: () => invoke<SourceOverview>("start_sources"),
  stopSources: () => invoke<void>("stop_sources"),
  searchAll: (query: string, generation: number) => invoke<SearchOutcome>("search_all", { query, generation }),
  remoteChapterCount: (mangaRef: string) => invoke<number>("remote_chapter_count", { mangaRef }),
  stores: () => invoke<ExtensionStore[]>("list_stores"),
  addStore: (url: string) => invoke<ExtensionStore>("add_store", { url }),
  checkNewChapters: () => invoke<CheckReport>("check_new_chapters"),
  syncNow: () => invoke<MergeReport>("sync_now"),
  setupNeeds: () => invoke<SetupNeeds>("setup_needs"),
  setupEverything: (gpu: boolean) => invoke<SetupStep[]>("setup_everything", { gpu }),
  createBackup: (path: string, includeChapters: boolean) => invoke<BackupReport>("create_backup", { path, includeChapters }),
  restoreBackup: (path: string) => invoke<number>("restore_backup", { path }),
  applySourceDefaults: () => invoke<{ stores_added: string[]; extensions_installed: string[] }>("apply_source_defaults"),
  validateStoreUrl: (url: string) => invoke<string>("validate_store_url", { url }),
  removeStore: (indexUrl: string) => invoke<void>("remove_store", { indexUrl }),
  extensions: (refresh: boolean) => invoke<Extension[]>("list_extensions", { refresh }),
  changeExtension: (pkgName: string, action: "install" | "uninstall" | "update") =>
    invoke<{ extension: Extension; sources: number; online_sources: number }>("change_extension", { pkgName, action }),
  downloadNow: (manga: RemoteManga, chapter: RemoteChapter) => invoke<number>("download_now", { manga, chapter }),
  downloadSizeHint: () => invoke<number | null>("download_size_hint"),
  providerStatus: () => invoke<ProviderStatus>("provider_status"),
  installedSources: () => invoke<InstalledSource[]>("installed_sources"),
  sourcePreferences: (sourceId: string) => invoke<SourcePreference[]>("source_preferences", { sourceId }),
  setSourcePreference: (sourceId: string, position: number, kind: string, value: unknown) =>
    invoke<SourcePreference[]>("set_source_preference", { sourceId, position, kind, value }),
  // data
  appPaths: () => invoke<{ data: string; library: string; logs: string }>("app_paths"),
  cacheSize: () => invoke<{ derived: number; thumbs: number; library: number }>("cache_size"),
  clearThumbnails: () => invoke<void>("clear_thumbnails"),
  exportData: (path: string) => invoke<string>("export_data", { path }),
};

export function errMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}

export function errCode(e: unknown): string | null {
  if (e && typeof e === "object" && "code" in e) return String((e as { code: unknown }).code);
  return null;
}
