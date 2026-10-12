// Mirrors the Rust DTOs (serde field names).

export interface AppError { code: string; message: string }

export interface MangaSummary {
  id: number; title: string; favorite: boolean; source_kind: string;
  chapter_count: number; read_count: number; page_total: number; translated_chapters: number;
  last_read_at: string | null; created_at: string; updated_at: string; categories: number[];
  continue_chapter_id: number | null; continue_page: number | null;
  downloaded_chapters: number; translated_pages: number; failed_pages: number;
}

export interface TranslationCounts { total: number; done: number; failed: number; in_progress: number }

export interface ChapterInfo {
  id: number; manga_id: number; title: string; number: number | null; volume: number | null; sort_index: number;
  container: string; page_count: number; read: boolean; last_page: number; last_read_at: string | null;
  downloaded: boolean; available: boolean; translation: TranslationCounts;
}

export interface MangaDetail {
  id: number; title: string; alt_titles: string[]; description: string | null; author: string | null; artist: string | null;
  status: string | null; genres: string[]; source_kind: string; source_id: string | null; import_root: string | null;
  favorite: boolean; reading_mode: string | null; reading_dir: string | null; source_lang: string; translation_notes: string;
  categories: number[]; chapters: ChapterInfo[]; last_read_at: string | null;
}

export interface PageInfo {
  id: number; idx: number; width: number; height: number; sha256: string; status: string;
  stage: string | null; error: string | null; translated_version: string | null; region_count: number; needs_review: number;
}

export interface ChapterView {
  chapter: ChapterInfo; manga_title: string; reading_mode: string | null; reading_dir: string | null;
  prev_chapter_id: number | null; next_chapter_id: number | null; pages: PageInfo[];
}

export interface HistoryEntry {
  manga_id: number; manga_title: string; chapter_id: number; chapter_title: string; page_idx: number; page_count: number; at: string;
}

export interface Category { id: number; name: string; sort: number; count: number }

export interface ImportReport {
  manga_id: number; title: string; chapters_added: number; chapters_updated: number; chapters_unchanged: number;
  pages_added: number; warnings: string[];
}

export interface ProviderConfig {
  id: string; kind: string; label: string; model: string; endpoint: string; max_concurrency: number; timeout_secs: number;
  max_retries: number; send_images: boolean; price_input_per_mtok: number; price_output_per_mtok: number; max_output_tokens: number;
}

export interface Settings {
  theme: "dark" | "light"; library_view: string;
  reader: { mode: "single" | "double" | "vertical"; direction: "rtl" | "ltr"; fit: "height" | "width" | "original";
    show_translated: boolean; preload: number; double_cover_alone: boolean; vertical_gap: number };
  translation: { active_provider: string; providers: ProviderConfig[]; target_lang: string; quality: string;
    pages_per_request: number; context_lines: number; confirm_cost_above_usd: number; job_cost_limit_usd: number;
    sfx_mode: string; local_only: boolean; auto_translate_next: boolean };
  render: { font: string; font_loud: string; min_font_px: number; max_font_px: number; padding: number; line_spacing: number;
    stroke_px: number; inpaint: string };
  ocr: { detector_threshold: number; device: string };
  sources: { suwayomi_enabled: boolean; suwayomi_url: string; suwayomi_user: string; download_concurrency: number;
    managed: boolean; managed_port: number };
  cloud_notice_accepted: boolean;
  onboarding_done: boolean;
}

export interface Job {
  id: number; kind: string; manga_id: number; manga_title: string; chapter_id: number; chapter_title: string;
  page_ids: number[] | null; mode: string; priority: number; status: string; total: number; completed: number; failed: number;
  cost_usd: number; provider: string | null; model: string | null; error: string | null; created_at: string; updated_at: string;
}

export interface Estimate {
  pages_total: number; pages_to_translate: number; regions_estimate: number; input_tokens: number; output_tokens: number;
  cost_usd: number | null; provider: string; model: string; sends_images: boolean; local: boolean; needs_confirmation: boolean;
}

export interface Download {
  id: number; source_id: string; manga_ref: string; manga_title: string; chapter_ref: string; chapter_title: string;
  chapter_number: number | null; manga_id: number | null; chapter_id: number | null; status: string; progress: number;
  total: number; error: string | null; updated_at: string;
}

export interface RecentTranslated {
  chapter_id: number; chapter_title: string; manga_id: number; manga_title: string; pages_done: number; page_count: number; at: string;
}

export interface HomeData {
  continue_reading: HistoryEntry[]; recently_added: MangaSummary[]; recent_translations: RecentTranslated[];
  active_jobs: Job[]; active_downloads: Download[]; library_count: number;
}

export type Box = [number, number, number, number];

export interface Region {
  id: number; page_id: number; rid: string; order_idx: number; kind: string; bbox: Box; bubble: Box | null; parts: Box[];
  vertical: boolean; ocr_text: string; ocr_confidence: number | null; source_text: string | null; translation: string | null;
  category: string | null; speaker: string | null; confidence: string | null; note: string | null; approved: boolean; skip: boolean;
  layout: { font_size?: number; dx?: number; dy?: number; align?: string; font?: string; box?: Box };
  history: { text: string; at: string }[];
}

export interface PageState {
  status: string; stage: string | null; error: string | null; rendered_path: string | null; clean_path: string | null;
  render_key: string | null; provider: string | null; model: string | null; cost_usd: number | null; notes: { kind: string; rid?: string }[];
  summary: string | null;
}

export interface EditorPage {
  page_id: number; chapter_id: number; manga_id: number; idx: number; width: number; height: number;
  state: PageState; regions: Region[]; has_previous_render: boolean;
}

export interface GlossaryEntry {
  id: number; manga_id: number; source: string; target: string; kind: string; gender: string | null; notes: string; status: string;
}

export interface MemoryEntry { id: number; source_text: string; target_text: string; origin: string; created_at: string }

export interface SourceInfo { id: string; name: string; lang: string; supports_latest: boolean; is_nsfw: boolean; icon_url: string | null }
export interface RemoteManga {
  source_id: string; manga_ref: string; title: string; thumbnail_url: string | null; author: string | null; artist: string | null;
  description: string | null; genres: string[]; status: string | null;
}
export interface RemoteChapter {
  chapter_ref: string; title: string; number: number | null; scanlator: string | null; upload_date: number | null; page_count: number | null;
}
export interface RemoteDetail {
  manga: RemoteManga; chapters: RemoteChapter[]; downloads: Record<string, string>; local_chapters: Record<string, number>; local_manga_id: number | null;
}

export interface EngineStatus {
  installed: boolean; python: string; worker_dir: string; models_dir: string; uv: string | null;
  health: { raqm: boolean; torch: string | null; device?: string; python: string;
    models: { detector: boolean; ocr: boolean; lama: boolean; lama_path: string } } | null;
  error: string | null;
}

export interface SearchEntry { source_id: string; source_name: string; lang: string; manga: RemoteManga }
export interface SearchGroup { title: string; author: string | null; thumbnail_url: string | null; description: string | null; entries: SearchEntry[] }
export interface SearchOutcome {
  groups: SearchGroup[]; sources_searched: number; online_sources: number;
  failures: { source_name: string; message: string }[]; cancelled: boolean;
}
export interface SourceHealth {
  server: "ok" | "offline" | "auth" | "disabled" | "error"; server_version: string | null; message: string | null;
  sources: number; online_sources: number; stores: number; installed_extensions: number;
}
export interface RuntimeStatus { supported: boolean; installed: boolean; running: boolean; version: string; install_dir: string; download_size_mb: number | null }
export interface SourceOverview {
  enabled: boolean; url: string; managed: boolean; runtime: RuntimeStatus; runtime_error: string | null;
  health: SourceHealth | null; local_dir: string | null;
}
export interface ExtensionStore { name: string; index_url: string; is_legacy: boolean; badge_label: string; contact_website: string }
export interface Extension {
  pkg_name: string; name: string; lang: string; version_name: string; is_installed: boolean; has_update: boolean;
  is_obsolete: boolean; is_nsfw: boolean; store_index_url: string | null; icon_url: string | null;
}
export interface ProviderStatus { ready: boolean; provider_id: string; label: string; kind: string; local: boolean; missing: string[] }
