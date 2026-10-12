//! Tauri commands: the only API the webview can call. Secrets never cross it.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Notify;

use crate::assets::Paths;
use crate::db::Db;
use crate::downloads;
use crate::editor::{self, RegionEdit};
use crate::error::{AppError, AppResult};
use crate::jobs;
use crate::library::{import, queries, scan};
use crate::pipeline::{load_page, Engine};
use crate::providers::{self, find_claude, Ollama};
use crate::secrets::SecretStore;
use crate::settings::{self, AppSettings};
use crate::sources::{RemoteChapter, RemoteManga, SourceAdapter, Suwayomi};
use crate::translate::memory::{self, GlossaryInput};
use crate::worker::{find_uv, PythonWorker, Vision};

pub struct AppState {
    pub runtime: Arc<crate::runtime::Runtime>,
    pub search_gen: Arc<std::sync::atomic::AtomicU64>,
    pub db: Db,
    pub paths: Paths,
    pub engine: Engine,
    pub worker: Arc<PythonWorker>,
    pub secrets: Arc<dyn SecretStore>,
    pub job_notify: Arc<Notify>,
    pub dl_notify: Arc<Notify>,
    pub resource_dir: Option<PathBuf>,
}

type R<T> = Result<T, AppError>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> { tokio::task::spawn_blocking(f).await.map_err(|e| AppError::new("internal", e.to_string()))? }

// ---------------- library ----------------

#[tauri::command]
pub fn list_manga(st: State<'_, AppState>) -> R<Vec<queries::MangaSummary>> {
    queries::list_manga(&st.db)
}

#[tauri::command]
pub fn manga_detail(st: State<'_, AppState>, id: i64) -> R<queries::MangaDetail> {
    queries::manga_detail(&st.db, id)
}

#[tauri::command]
pub fn chapter_view(st: State<'_, AppState>, chapter_id: i64) -> R<queries::ChapterView> {
    queries::chapter_view(&st.db, chapter_id)
}

#[derive(Serialize)]
pub struct ImportResult {
    pub reports: Vec<import::ImportReport>,
    pub errors: Vec<String>,
}

/// mode: "manga" (each path is one manga) or "library" (each sub-folder of the path is a manga)
#[tauri::command]
pub async fn import_paths(st: State<'_, AppState>, paths: Vec<String>, mode: String) -> R<ImportResult> {
    let db = st.db.clone();
    blocking(move || {
        let mut targets: Vec<PathBuf> = vec![];
        let mut errors = vec![];
        for p in paths {
            let p = PathBuf::from(p);
            if mode == "library" {
                match scan::scan_library_root(&p) {
                    Ok(list) => targets.extend(list),
                    Err(e) => errors.push(format!("{}: {}", p.display(), e.message)),
                }
            } else {
                targets.push(p);
            }
        }
        let mut reports = vec![];
        for t in targets {
            match import::import_path(&db, &t) {
                Ok(r) => reports.push(r),
                Err(e) => errors.push(format!("{}: {}", t.display(), e.message)),
            }
        }
        Ok(ImportResult { reports, errors })
    })
    .await
}

#[tauri::command]
pub async fn rescan_manga(st: State<'_, AppState>, id: i64) -> R<import::ImportReport> {
    let db = st.db.clone();
    blocking(move || {
        let root: Option<String> = db.with(|c| Ok(c.query_row("SELECT import_root FROM manga WHERE id = ?1", [id], |r| r.get(0))?))?;
        let root = root.ok_or_else(|| AppError::invalid("only imported manga can be rescanned"))?;
        let mut r = import::import_path(&db, std::path::Path::new(&root))?;
        for (_, title) in import::missing_chapters(&db, id)? {
            r.warnings.push(format!("files missing for chapter: {title}"));
        }
        Ok(r)
    })
    .await
}

#[tauri::command]
pub fn update_manga(st: State<'_, AppState>, id: i64, update: queries::MangaUpdate) -> R<()> {
    queries::update_manga(&st.db, id, update)
}

#[tauri::command]
pub fn remove_manga(st: State<'_, AppState>, id: i64, delete_downloads: bool) -> R<()> {
    queries::remove_manga(&st.db, &st.paths.library_dir(), id, delete_downloads)
}

#[tauri::command]
pub fn set_chapters_read(st: State<'_, AppState>, chapter_ids: Vec<i64>, read: bool) -> R<()> {
    queries::set_chapters_read(&st.db, &chapter_ids, read)
}

#[tauri::command]
pub fn save_progress(st: State<'_, AppState>, chapter_id: i64, page_idx: i64) -> R<()> {
    queries::save_progress(&st.db, chapter_id, page_idx)
}

#[tauri::command]
pub fn reading_history(st: State<'_, AppState>, limit: Option<i64>) -> R<Vec<queries::HistoryEntry>> {
    queries::history(&st.db, limit.unwrap_or(200))
}

#[tauri::command]
pub fn clear_history(st: State<'_, AppState>) -> R<()> {
    queries::clear_history(&st.db)
}

#[tauri::command]
pub fn list_categories(st: State<'_, AppState>) -> R<Vec<queries::Category>> {
    queries::list_categories(&st.db)
}

#[tauri::command]
pub fn create_category(st: State<'_, AppState>, name: String) -> R<i64> {
    queries::create_category(&st.db, &name)
}

#[tauri::command]
pub fn rename_category(st: State<'_, AppState>, id: i64, name: String) -> R<()> {
    queries::rename_category(&st.db, id, &name)
}

#[tauri::command]
pub fn delete_category(st: State<'_, AppState>, id: i64) -> R<()> {
    queries::delete_category(&st.db, id)
}

#[derive(Serialize)]
pub struct RecentTranslated {
    pub chapter_id: i64,
    pub chapter_title: String,
    pub manga_id: i64,
    pub manga_title: String,
    pub pages_done: i64,
    pub page_count: i64,
    pub at: String,
}

#[derive(Serialize)]
pub struct HomeData {
    pub continue_reading: Vec<queries::HistoryEntry>,
    pub recently_added: Vec<queries::MangaSummary>,
    pub recent_translations: Vec<RecentTranslated>,
    pub active_jobs: Vec<jobs::Job>,
    pub active_downloads: Vec<downloads::Download>,
    pub library_count: usize,
}

#[tauri::command]
pub fn home_data(st: State<'_, AppState>) -> R<HomeData> {
    let mut seen = std::collections::HashSet::new();
    let continue_reading: Vec<_> =
        queries::history(&st.db, 60)?.into_iter().filter(|h| seen.insert(h.manga_id)).take(8).collect();
    let mut all = queries::list_manga(&st.db)?;
    let library_count = all.len();
    all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let recently_added = all.into_iter().take(10).collect();
    let recent_translations = st.db.with(|c| {
        let mut s = c.prepare(
            "SELECT ch.id, ch.title, m.id, m.title,
                    sum(CASE WHEN s.status = 'done' THEN 1 ELSE 0 END), ch.page_count, max(s.updated_at)
             FROM page_state s JOIN page p ON p.id = s.page_id JOIN chapter ch ON ch.id = p.chapter_id JOIN manga m ON m.id = ch.manga_id
             WHERE s.status = 'done' GROUP BY ch.id ORDER BY max(s.updated_at) DESC LIMIT 8",
        )?;
        let v = s
            .query_map([], |r| {
                Ok(RecentTranslated {
                    chapter_id: r.get(0)?,
                    chapter_title: r.get(1)?,
                    manga_id: r.get(2)?,
                    manga_title: r.get(3)?,
                    pages_done: r.get(4)?,
                    page_count: r.get(5)?,
                    at: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })?;
    let active_jobs = jobs::list(&st.db, false)?.into_iter().filter(|j| j.status != "failed").collect();
    let active_downloads = downloads::list(&st.db)?.into_iter().filter(|d| matches!(d.status.as_str(), "queued" | "running")).collect();
    Ok(HomeData { continue_reading, recently_added, recent_translations, active_jobs, active_downloads, library_count })
}

// ---------------- settings & secrets ----------------

#[tauri::command]
pub fn get_settings(st: State<'_, AppState>) -> R<AppSettings> {
    settings::load(&st.db)
}

#[tauri::command]
pub fn save_settings(st: State<'_, AppState>, settings: AppSettings) -> R<AppSettings> {
    settings::save(&st.db, &settings)?;
    st.dl_notify.notify_one();
    settings::load(&st.db)
}

#[tauri::command]
pub fn set_api_key(st: State<'_, AppState>, provider_id: String, key: String) -> R<()> {
    let key = key.trim();
    if key.is_empty() {
        return st.secrets.delete(&provider_id);
    }
    st.secrets.set(&provider_id, key)
}

#[tauri::command]
pub fn api_key_status(st: State<'_, AppState>) -> R<Value> {
    let s = settings::load(&st.db)?;
    let mut out = serde_json::Map::new();
    for p in s.translation.providers.iter().map(|p| p.id.clone()).chain(std::iter::once("suwayomi".to_string())) {
        let has = st.secrets.get(&p).map(|k| k.is_some()).unwrap_or(false);
        out.insert(p, json!(has));
    }
    Ok(Value::Object(out))
}

/// Small real request to check a provider works (user-initiated; may cost a fraction of a cent).
#[tauri::command]
pub async fn test_provider(st: State<'_, AppState>, provider_id: String) -> R<Value> {
    let s = settings::load(&st.db)?;
    let cfg = s.provider(&provider_id).cloned().ok_or_else(|| AppError::not_found("provider"))?;
    let p = providers::build(&cfg, st.secrets.as_ref()).map_err(|e| AppError::new("provider", e.message))?;
    let req = providers::ProviderRequest {
        system: "You are a test endpoint. Reply with JSON only.".into(),
        user: "Translate the Japanese word こんにちは into Hebrew. Return {\"translation\": \"...\"}.".into(),
        images: vec![],
        schema: json!({"type":"object","additionalProperties":false,"required":["translation"],"properties":{"translation":{"type":"string"}}}),
        max_output_tokens: 1024,
        effort: "low".into(),
    };
    let started = std::time::Instant::now();
    let r = p.complete(&req).await.map_err(|e| AppError::new("provider", format!("{:?}: {}", e.kind, e.message)))?;
    Ok(json!({"text": r.text, "model": r.model, "ms": started.elapsed().as_millis() as u64,
              "cost_usd": providers::cost_from_usage(&cfg, &r.usage)}))
}

#[tauri::command]
pub async fn list_ollama_models(endpoint: String) -> R<Vec<String>> {
    Ollama::list_models(&endpoint).await.map_err(|e| AppError::new("provider", e.message))
}

#[tauri::command]
pub fn claude_cli_status(path: Option<String>) -> R<Value> {
    let found = find_claude(path.as_deref().unwrap_or(""));
    Ok(json!({"found": found.is_some(), "path": found.map(|p| p.to_string_lossy().into_owned())}))
}

// ---------------- AI engine ----------------

#[tauri::command]
pub async fn engine_status(st: State<'_, AppState>) -> R<Value> {
    let cfg = st.worker.config().clone();
    let installed = cfg.python.exists();
    let health = if installed { st.worker.health().await.map_err(|e| e.message) } else { Err("not installed".into()) };
    Ok(json!({
        "installed": installed,
        "python": cfg.python,
        "worker_dir": cfg.worker_dir,
        "models_dir": cfg.models_dir,
        "uv": find_uv(),
        "health": health.as_ref().ok(),
        "error": health.err(),
    }))
}

/// Create/refresh the worker's Python environment with uv (network: PyPI).
#[tauri::command]
pub async fn install_engine(st: State<'_, AppState>) -> R<String> {
    let uv = find_uv().ok_or_else(|| AppError::new("uv_missing", "uv is required: https://docs.astral.sh/uv/ (brew install uv)"))?;
    let cfg = st.worker.config().clone();
    let venv = st.paths.root.join("worker-venv");
    st.worker.shutdown().await;
    let out = tokio::process::Command::new(uv)
        .args(["sync", "--managed-python", "--python", "3.12", "--no-dev", "--frozen", "--project"])
        .arg(&cfg.worker_dir)
        .env("UV_PROJECT_ENVIRONMENT", &venv)
        .output()
        .await?;
    let log = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        return Err(AppError::new("install_failed", log.chars().rev().take(1500).collect::<String>().chars().rev().collect::<String>()));
    }
    Ok("installed — restart the app to use the new engine".into())
}

#[tauri::command]
pub async fn prepare_models(st: State<'_, AppState>) -> R<Value> {
    st.worker.prepare_models().await
}

// ---------------- translation queue ----------------

#[tauri::command]
pub fn estimate_translation(st: State<'_, AppState>, chapter_ids: Vec<i64>) -> R<jobs::Estimate> {
    jobs::estimate(&st.db, &chapter_ids)
}

#[tauri::command]
pub fn enqueue_translation(
    st: State<'_, AppState>,
    chapter_ids: Vec<i64>,
    page_ids: Option<Vec<i64>>,
    mode: Option<String>,
    priority: Option<i64>,
) -> R<Vec<i64>> {
    let s = settings::load(&st.db)?;
    let cfg = s.active_provider().cloned().ok_or_else(|| AppError::new("no_provider", "no provider selected"))?;
    jobs::check_provider_allowed(&s, &cfg)?;
    let mode = mode.unwrap_or_else(|| "missing".into());
    let mut ids = vec![];
    for ch in &chapter_ids {
        let subset = page_ids.clone().filter(|_| chapter_ids.len() == 1);
        ids.push(jobs::enqueue(&st.db, *ch, subset, &mode, priority.unwrap_or(0))?);
    }
    st.job_notify.notify_one();
    Ok(ids)
}

#[tauri::command]
pub fn list_jobs(st: State<'_, AppState>, include_finished: Option<bool>) -> R<Vec<jobs::Job>> {
    jobs::list(&st.db, include_finished.unwrap_or(true))
}

#[tauri::command]
pub fn job_action(st: State<'_, AppState>, id: i64, action: String) -> R<Option<i64>> {
    let r = match action.as_str() {
        "pause" => jobs::set_status(&st.db, id, "paused").map(|_| None),
        "resume" => jobs::set_status(&st.db, id, "queued").map(|_| None),
        "cancel" => jobs::set_status(&st.db, id, "cancelled").map(|_| None),
        "retry_failed" => jobs::retry_failed(&st.db, id).map(Some),
        _ => Err(AppError::invalid("unknown action")),
    };
    st.job_notify.notify_one();
    r
}

#[tauri::command]
pub fn clear_finished_jobs(st: State<'_, AppState>) -> R<usize> {
    jobs::clear_finished(&st.db)
}

#[tauri::command]
pub fn prioritize_chapter(st: State<'_, AppState>, chapter_id: i64) -> R<()> {
    jobs::prioritize_chapter(&st.db, chapter_id)?;
    st.job_notify.notify_one();
    Ok(())
}

#[tauri::command]
pub fn usage_summary(st: State<'_, AppState>) -> R<Value> {
    st.db.with(|c| {
        let mut s = c.prepare(
            "SELECT provider, model, count(*), coalesce(sum(input_tokens),0), coalesce(sum(output_tokens),0), sum(cost_usd)
             FROM usage_log GROUP BY provider, model ORDER BY max(at) DESC",
        )?;
        let rows: Vec<Value> = s
            .query_map([], |r| {
                Ok(json!({"provider": r.get::<_, String>(0)?, "model": r.get::<_, String>(1)?, "requests": r.get::<_, i64>(2)?,
                          "input_tokens": r.get::<_, i64>(3)?, "output_tokens": r.get::<_, i64>(4)?, "cost_usd": r.get::<_, Option<f64>>(5)?}))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!(rows))
    })
}

// ---------------- editor ----------------

#[tauri::command]
pub fn editor_page(st: State<'_, AppState>, page_id: i64) -> R<editor::EditorPage> {
    editor::editor_page(&st.engine, page_id)
}

#[tauri::command]
pub fn update_region(st: State<'_, AppState>, region_id: i64, edit: RegionEditDto) -> R<crate::pipeline::RegionRow> {
    editor::update_region(&st.engine, region_id, edit.into())
}

#[derive(Deserialize)]
pub struct RegionEditDto {
    pub source_text: Option<String>,
    pub translation: Option<String>,
    pub layout: Option<Value>,
    pub skip: Option<bool>,
    pub approved: Option<bool>,
    pub category: Option<String>,
}

impl From<RegionEditDto> for RegionEdit {
    fn from(d: RegionEditDto) -> Self {
        RegionEdit { source_text: d.source_text, translation: d.translation, layout: d.layout, skip: d.skip, approved: d.approved, category: d.category }
    }
}

#[tauri::command]
pub async fn add_region(st: State<'_, AppState>, page_id: i64, bbox: [i64; 4]) -> R<crate::pipeline::RegionRow> {
    let s = settings::load(&st.db)?;
    editor::add_region(&st.engine, page_id, bbox, &s).await
}

#[tauri::command]
pub fn delete_region(st: State<'_, AppState>, region_id: i64) -> R<()> {
    editor::delete_region(&st.engine, region_id)
}

#[tauri::command]
pub fn move_region(st: State<'_, AppState>, region_id: i64, delta: i64) -> R<()> {
    editor::move_region(&st.engine, region_id, delta)
}

#[tauri::command]
pub async fn retry_region_ocr(st: State<'_, AppState>, region_id: i64) -> R<crate::pipeline::RegionRow> {
    let s = settings::load(&st.db)?;
    editor::retry_region_ocr(&st.engine, region_id, &s).await
}

#[tauri::command]
pub async fn retranslate_region(st: State<'_, AppState>, region_id: i64) -> R<crate::pipeline::RegionRow> {
    let s = settings::load(&st.db)?;
    let cfg = s.active_provider().cloned().ok_or_else(|| AppError::new("no_provider", "no provider selected"))?;
    jobs::check_provider_allowed(&s, &cfg)?;
    let p = providers::build(&cfg, st.secrets.as_ref()).map_err(|e| AppError::new("provider", e.message))?;
    editor::retranslate_region(&st.engine, region_id, &s, &cfg, p).await
}

#[tauri::command]
pub async fn rerender_page(st: State<'_, AppState>, page_id: i64) -> R<crate::pipeline::PageStateRow> {
    let s = settings::load(&st.db)?;
    editor::rerender_page(&st.engine, page_id, &s).await
}

#[tauri::command]
pub async fn reanalyze_page(st: State<'_, AppState>, page_id: i64) -> R<editor::EditorPage> {
    let s = settings::load(&st.db)?;
    let p = load_page(&st.db, page_id, s.reader.direction != "ltr")?;
    st.engine.analyze(&p, &s, true).await?;
    editor::editor_page(&st.engine, page_id)
}

#[tauri::command]
pub fn restore_original(st: State<'_, AppState>, page_id: i64) -> R<()> {
    editor::restore_original(&st.engine, page_id)
}

#[tauri::command]
pub fn reset_page(st: State<'_, AppState>, page_id: i64) -> R<()> {
    editor::reset_page(&st.engine, page_id)
}

// ---------------- glossary & memory ----------------

#[tauri::command]
pub fn list_glossary(st: State<'_, AppState>, manga_id: i64) -> R<Vec<memory::GlossaryEntry>> {
    memory::list_glossary(&st.db, manga_id)
}

#[tauri::command]
pub fn upsert_glossary(st: State<'_, AppState>, manga_id: i64, entry: GlossaryInput) -> R<i64> {
    memory::upsert_glossary(&st.db, manga_id, &entry)
}

#[tauri::command]
pub fn delete_glossary(st: State<'_, AppState>, id: i64) -> R<()> {
    memory::delete_glossary(&st.db, id)
}

#[tauri::command]
pub fn list_memory(st: State<'_, AppState>, manga_id: i64) -> R<Vec<memory::MemoryEntry>> {
    memory::list_memory(&st.db, manga_id)
}

#[tauri::command]
pub fn delete_memory(st: State<'_, AppState>, id: i64) -> R<()> {
    memory::delete_memory(&st.db, id)
}

// ---------------- sources & downloads ----------------

pub fn make_adapter(db: &Db, secrets: &dyn SecretStore) -> AppResult<Arc<dyn SourceAdapter>> {
    let s = settings::load(db)?;
    if !s.sources.suwayomi_enabled {
        return Err(AppError::new("sources_disabled", "Online sources are off. Enable Suwayomi in Settings."));
    }
    let auth = if s.sources.suwayomi_user.trim().is_empty() {
        None
    } else {
        Some((s.sources.suwayomi_user.clone(), secrets.get("suwayomi")?.unwrap_or_default()))
    };
    Ok(Arc::new(Suwayomi::new(&s.sources.suwayomi_url, auth)?))
}

#[tauri::command]
pub async fn source_status(st: State<'_, AppState>) -> R<String> {
    make_adapter(&st.db, st.secrets.as_ref())?.ping().await
}

#[tauri::command]
pub async fn list_sources(st: State<'_, AppState>) -> R<Vec<crate::sources::SourceInfo>> {
    make_adapter(&st.db, st.secrets.as_ref())?.sources().await
}

#[tauri::command]
pub async fn browse_source(st: State<'_, AppState>, source_id: String, kind: String, query: Option<String>, page: u32) -> R<crate::sources::MangaPage> {
    make_adapter(&st.db, st.secrets.as_ref())?.browse(&source_id, &kind, query.as_deref(), page).await
}

#[derive(Serialize)]
pub struct RemoteDetail {
    pub manga: RemoteManga,
    pub chapters: Vec<RemoteChapter>,
    /// chapter_ref -> download status
    pub downloads: std::collections::HashMap<String, String>,
    /// chapter_ref -> chapter id in the local library (downloaded chapters)
    pub local_chapters: std::collections::HashMap<String, i64>,
    pub local_manga_id: Option<i64>,
}

#[tauri::command]
pub async fn remote_manga(st: State<'_, AppState>, manga_ref: String) -> R<RemoteDetail> {
    let a = make_adapter(&st.db, st.secrets.as_ref())?;
    let manga = a.manga(&manga_ref).await?;
    let chapters = a.chapters(&manga_ref).await?;
    let (downloads, local_chapters, local) = st.db.with(|c| {
        let mut s = c.prepare("SELECT chapter_ref, status FROM download WHERE source_id = ?1 AND manga_ref = ?2")?;
        let map = s
            .query_map(rusqlite::params![manga.source_id, manga.manga_ref], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
        let mut s = c.prepare(
            "SELECT d.chapter_ref, d.chapter_id FROM download d JOIN chapter ch ON ch.id = d.chapter_id
             WHERE d.source_id = ?1 AND d.manga_ref = ?2 AND d.status = 'done'",
        )?;
        let chapters = s
            .query_map(rusqlite::params![manga.source_id, manga.manga_ref], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
        use rusqlite::OptionalExtension;
        let local: Option<i64> = c
            .query_row("SELECT id FROM manga WHERE source_id = ?1 AND source_ref = ?2", rusqlite::params![manga.source_id, manga.manga_ref], |r| r.get(0))
            .optional()?;
        Ok((map, chapters, local))
    })?;
    Ok(RemoteDetail { manga, chapters, downloads, local_chapters, local_manga_id: local })
}

#[tauri::command]
pub async fn enqueue_downloads(st: State<'_, AppState>, manga: RemoteManga, chapters: Vec<RemoteChapter>) -> R<Value> {
    let (added, skipped) = downloads::enqueue(&st.db, &manga, &chapters)?;
    let manga_id = downloads::local_manga(&st.db, &manga)?;
    // cover in the background
    if let Some(url) = manga.thumbnail_url.clone() {
        let db = st.db.clone();
        let paths = st.paths.clone();
        let secrets = st.secrets.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(a) = make_adapter(&db, secrets.as_ref()) {
                if let Ok((bytes, _)) = a.fetch(&url).await {
                    let p = paths.library_dir().join(manga_id.to_string()).join("cover.img");
                    if crate::assets::atomic_write(&p, &bytes).is_ok() {
                        let _ = db.with(|c| {
                            c.execute(
                                "UPDATE manga SET cover_path = ?1 WHERE id = ?2 AND cover_path IS NULL",
                                rusqlite::params![p.to_string_lossy(), manga_id],
                            )?;
                            Ok(())
                        });
                    }
                }
            }
        });
    }
    st.dl_notify.notify_one();
    Ok(json!({"added": added, "skipped": skipped, "manga_id": manga_id}))
}

#[tauri::command]
pub fn list_downloads(st: State<'_, AppState>) -> R<Vec<downloads::Download>> {
    downloads::list(&st.db)
}

#[tauri::command]
pub fn download_action(st: State<'_, AppState>, id: i64, action: String) -> R<()> {
    match action.as_str() {
        "cancel" => downloads::cancel(&st.db, id)?,
        "retry" => downloads::retry(&st.db, id)?,
        _ => return Err(AppError::invalid("unknown action")),
    }
    st.dl_notify.notify_one();
    Ok(())
}

#[tauri::command]
pub fn clear_finished_downloads(st: State<'_, AppState>) -> R<usize> {
    downloads::clear_finished(&st.db)
}

#[tauri::command]
pub fn set_source_password(st: State<'_, AppState>, password: String) -> R<()> {
    if password.is_empty() {
        st.secrets.delete("suwayomi")
    } else {
        st.secrets.set("suwayomi", &password)
    }
}

// ---------------- data ----------------

#[tauri::command]
pub fn app_paths(st: State<'_, AppState>) -> R<Value> {
    Ok(json!({"data": st.paths.root, "library": st.paths.library_dir(), "logs": st.paths.logs_dir(), "resources": st.resource_dir}))
}

/// Disk usage of derived files (cleaned + rendered pages, thumbnails).
#[tauri::command]
pub async fn cache_size(st: State<'_, AppState>) -> R<Value> {
    let paths = st.paths.clone();
    blocking(move || {
        let size = |d: PathBuf| -> u64 {
            walkdir::WalkDir::new(d).into_iter().filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
        };
        Ok(json!({"derived": size(paths.derived_root()), "thumbs": size(paths.thumbs_dir()), "library": size(paths.library_dir())}))
    })
    .await
}

#[tauri::command]
pub fn clear_thumbnails(st: State<'_, AppState>) -> R<()> {
    let d = st.paths.thumbs_dir();
    if d.exists() {
        std::fs::remove_dir_all(&d)?;
    }
    std::fs::create_dir_all(&d)?;
    Ok(())
}

/// Export library metadata, progress, glossaries and translations as JSON (no secrets, no images).
#[tauri::command]
pub async fn export_data(st: State<'_, AppState>, path: String) -> R<String> {
    let db = st.db.clone();
    blocking(move || {
        let dump = db.with(|c| {
            let table = |sql: &str| -> AppResult<Vec<Value>> {
                let mut s = c.prepare(sql)?;
                let cols: Vec<String> = s.column_names().iter().map(|s| s.to_string()).collect();
                let rows = s
                    .query_map([], |r| {
                        let mut m = serde_json::Map::new();
                        for (i, name) in cols.iter().enumerate() {
                            let v: rusqlite::types::Value = r.get(i)?;
                            m.insert(
                                name.clone(),
                                match v {
                                    rusqlite::types::Value::Null => Value::Null,
                                    rusqlite::types::Value::Integer(n) => json!(n),
                                    rusqlite::types::Value::Real(f) => json!(f),
                                    rusqlite::types::Value::Text(t) => json!(t),
                                    rusqlite::types::Value::Blob(_) => Value::Null,
                                },
                            );
                        }
                        Ok(Value::Object(m))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            };
            Ok(json!({
                "format": "mangaai-studio-export/1",
                "manga": table("SELECT * FROM manga")?,
                "categories": table("SELECT * FROM category")?,
                "manga_categories": table("SELECT * FROM manga_category")?,
                "chapters": table("SELECT id, manga_id, title, number, volume, sort_index, container, location, inner_prefix, page_count, read, last_page, last_read_at FROM chapter")?,
                "glossary": table("SELECT * FROM glossary")?,
                "translation_memory": table("SELECT * FROM translation_memory")?,
                "regions": table("SELECT r.*, p.sha256 AS page_sha256 FROM region r JOIN page p ON p.id = r.page_id")?,
                "history": table("SELECT * FROM reading_history")?,
            }))
        })?;
        crate::assets::atomic_write(std::path::Path::new(&path), serde_json::to_string_pretty(&dump)?.as_bytes())?;
        Ok(path)
    })
    .await
}

/// Debug builds only: tour progress lines into logs/dev-tour.log.
#[tauri::command]
pub fn dev_log(st: State<'_, AppState>, message: String) {
    if cfg!(debug_assertions) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(st.paths.logs_dir().join("dev-tour.log")) {
            let _ = writeln!(f, "{} {message}", crate::db::now());
        }
    }
}

/// Debug builds only: initial route from MANGAAI_START_ROUTE (used for screenshots during development).
#[tauri::command]
pub fn dev_start_route() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var("MANGAAI_START_ROUTE").ok()
    } else {
        None
    }
}

// ---------------- discovery: health, managed server, search, extensions ----------------

fn suwayomi(st: &AppState) -> AppResult<Suwayomi> {
    let s = settings::load(&st.db)?;
    let auth = if s.sources.suwayomi_user.trim().is_empty() {
        None
    } else {
        Some((s.sources.suwayomi_user.clone(), st.secrets.get("suwayomi")?.unwrap_or_default()))
    };
    Suwayomi::new(&s.sources.suwayomi_url, auth)
}

#[derive(Serialize)]
pub struct SourceOverview {
    pub enabled: bool,
    pub url: String,
    pub managed: bool,
    pub runtime: crate::runtime::RuntimeStatus,
    pub runtime_error: Option<String>,
    pub health: Option<crate::catalog::SourceHealth>,
    /// Local manga folder of the managed server ("Local source").
    pub local_dir: Option<String>,
}

#[tauri::command]
pub async fn source_overview(st: State<'_, AppState>) -> R<SourceOverview> {
    let s = settings::load(&st.db)?;
    let runtime = st.runtime.status().await;
    let runtime_error = if s.sources.managed { st.runtime.exited().await } else { None };
    let health = if s.sources.suwayomi_enabled { Some(crate::catalog::health(&suwayomi(&st)?).await) } else { None };
    Ok(SourceOverview {
        enabled: s.sources.suwayomi_enabled,
        url: s.sources.suwayomi_url.clone(),
        managed: s.sources.managed,
        runtime,
        runtime_error,
        health,
        local_dir: s.sources.managed.then(|| st.runtime.local_dir().to_string_lossy().into_owned()),
    })
}

async fn wait_reachable(s: &Suwayomi, secs: u64) -> bool {
    for _ in 0..secs {
        if s.ping().await.is_ok() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    false
}

/// One-click setup: download + verify the pinned official release, start it on
/// 127.0.0.1, register the official (verified) extension store. Installs no extensions.
#[tauri::command]
pub async fn setup_sources(app: tauri::AppHandle, st: State<'_, AppState>) -> R<SourceOverview> {
    use tauri::Emitter;
    let mut s = settings::load(&st.db)?;
    if !st.runtime.is_installed() {
        let a = app.clone();
        st.runtime
            .install(move |done, total| {
                let _ = a.emit("runtime-progress", json!({"done": done, "total": total}));
            })
            .await?;
    }
    s.sources.managed = true;
    s.sources.suwayomi_enabled = true;
    s.sources.suwayomi_url = format!("http://127.0.0.1:{}", s.sources.managed_port);
    settings::save(&st.db, &s)?;
    start_managed(&st).await?;
    let sw = suwayomi(&st)?;
    if crate::catalog::list_stores(&sw).await.map(|l| l.is_empty()).unwrap_or(false) {
        match crate::catalog::add_store(&sw, crate::catalog::OFFICIAL_STORE_URL).await {
            Ok(_) => {
                let _ = crate::catalog::refresh_extensions(&sw).await;
            }
            Err(e) => tracing::warn!("official store not added: {}", e.message),
        }
    }
    st.dl_notify.notify_one();
    source_overview(st).await
}

async fn start_managed(st: &AppState) -> AppResult<()> {
    let s = settings::load(&st.db)?;
    let sw = suwayomi(st)?;
    if sw.ping().await.is_ok() {
        return Ok(()); // something already answers on this address
    }
    st.runtime.start(s.sources.managed_port).await?;
    if !wait_reachable(&sw, 120).await {
        let why = st.runtime.exited().await.unwrap_or_else(|| "Suwayomi did not start within 2 minutes".into());
        return Err(AppError::new("start_failed", why));
    }
    Ok(())
}

#[tauri::command]
pub async fn start_sources(st: State<'_, AppState>) -> R<SourceOverview> {
    start_managed(&st).await?;
    source_overview(st).await
}

#[tauri::command]
pub async fn stop_sources(st: State<'_, AppState>) -> R<()> {
    st.runtime.stop().await;
    Ok(())
}

#[tauri::command]
pub async fn search_all(st: State<'_, AppState>, query: String, generation: u64) -> R<crate::catalog::SearchOutcome> {
    st.search_gen.fetch_max(generation, std::sync::atomic::Ordering::SeqCst);
    let adapter = make_adapter(&st.db, st.secrets.as_ref())?;
    crate::catalog::search_all(adapter, &query, generation, st.search_gen.clone()).await
}

#[tauri::command]
pub async fn remote_chapter_count(st: State<'_, AppState>, manga_ref: String) -> R<usize> {
    Ok(make_adapter(&st.db, st.secrets.as_ref())?.chapters(&manga_ref).await?.len())
}

#[tauri::command]
pub async fn list_stores(st: State<'_, AppState>) -> R<Vec<crate::catalog::ExtensionStore>> {
    crate::catalog::list_stores(&suwayomi(&st)?).await
}

#[tauri::command]
pub async fn add_store(st: State<'_, AppState>, url: String) -> R<crate::catalog::ExtensionStore> {
    let sw = suwayomi(&st)?;
    let store = crate::catalog::add_store(&sw, &url).await?;
    let _ = crate::catalog::refresh_extensions(&sw).await;
    Ok(store)
}

#[tauri::command]
pub fn validate_store_url(url: String) -> R<String> {
    crate::catalog::validate_store_url(&url)
}

#[tauri::command]
pub async fn remove_store(st: State<'_, AppState>, index_url: String) -> R<()> {
    crate::catalog::remove_store(&suwayomi(&st)?, &index_url).await
}

#[tauri::command]
pub async fn list_extensions(st: State<'_, AppState>, refresh: bool) -> R<Vec<crate::catalog::Extension>> {
    let sw = suwayomi(&st)?;
    if refresh {
        crate::catalog::refresh_extensions(&sw).await
    } else {
        crate::catalog::list_extensions(&sw).await
    }
}

/// Install / uninstall / update one extension (explicit user action), then report the source list.
#[tauri::command]
pub async fn change_extension(st: State<'_, AppState>, pkg_name: String, action: String) -> R<Value> {
    let sw = suwayomi(&st)?;
    let (ext, sources) = crate::catalog::change_extension_and_sources(&sw, &pkg_name, &action).await?;
    Ok(json!({"extension": ext, "sources": sources.len(),
              "online_sources": sources.iter().filter(|s| s.id != crate::catalog::LOCAL_SOURCE_ID).count()}))
}

#[tauri::command]
pub async fn installed_sources(st: State<'_, AppState>) -> R<Vec<Value>> {
    let sw = suwayomi(&st)?;
    let d = sw.graphql("query { sources { nodes { id name displayName lang isConfigurable isNsfw iconUrl } } }", json!({})).await?;
    Ok(d["sources"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut v| {
            if let Some(u) = v["iconUrl"].as_str().map(|u| sw.absolute_url(u)) {
                v["iconUrl"] = json!(u);
            }
            v
        })
        .collect())
}

#[tauri::command]
pub async fn source_preferences(st: State<'_, AppState>, source_id: String) -> R<Vec<crate::catalog::SourcePreference>> {
    crate::catalog::source_preferences(&suwayomi(&st)?, &source_id).await
}

#[tauri::command]
pub async fn set_source_preference(
    st: State<'_, AppState>,
    source_id: String,
    position: usize,
    kind: String,
    value: Value,
) -> R<Vec<crate::catalog::SourcePreference>> {
    crate::catalog::set_source_preference(&suwayomi(&st)?, &source_id, position, &kind, value).await
}

/// Queue one chapter at the front of the download queue ("read now"). Returns the download id.
#[tauri::command]
pub fn download_now(st: State<'_, AppState>, manga: RemoteManga, chapter: RemoteChapter) -> R<i64> {
    downloads::enqueue(&st.db, &manga, std::slice::from_ref(&chapter))?;
    let id: i64 = st.db.with(|c| {
        Ok(c.query_row(
            "SELECT id FROM download WHERE source_id = ?1 AND chapter_ref = ?2",
            rusqlite::params![manga.source_id, chapter.chapter_ref],
            |r| r.get(0),
        )?)
    })?;
    downloads::prioritize(&st.db, id)?;
    st.dl_notify.notify_one();
    Ok(id)
}

/// Average size of a downloaded chapter, from what is already on disk (None if nothing yet).
#[tauri::command]
pub async fn download_size_hint(st: State<'_, AppState>) -> R<Option<u64>> {
    let db = st.db.clone();
    let lib = st.paths.library_dir();
    blocking(move || {
        let dirs: Vec<String> = db.with(|c| {
            let mut s = c.prepare("SELECT location FROM chapter WHERE source_ref IS NOT NULL")?;
            let v = s.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
            Ok(v)
        })?;
        let mut total = 0u64;
        let mut n = 0u64;
        for d in dirs.iter().filter(|d| std::path::Path::new(d).starts_with(&lib)) {
            let size: u64 = walkdir::WalkDir::new(d).into_iter().filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum();
            if size > 0 {
                total += size;
                n += 1;
            }
        }
        Ok((n > 0).then(|| total / n))
    })
    .await
}

#[derive(Serialize)]
pub struct ProviderStatus {
    pub ready: bool,
    pub provider_id: String,
    pub label: String,
    pub kind: String,
    pub local: bool,
    /// no_provider | no_key | no_model | cli_missing | cloud_notice | local_only
    pub missing: Vec<String>,
}

/// Can translation start right now with the saved settings?
#[tauri::command]
pub fn provider_status(st: State<'_, AppState>) -> R<ProviderStatus> {
    provider_status_for(&settings::load(&st.db)?, st.secrets.as_ref())
}

pub fn provider_status_for(s: &AppSettings, secrets: &dyn SecretStore) -> AppResult<ProviderStatus> {
    let Some(cfg) = s.active_provider().cloned() else {
        return Ok(ProviderStatus { ready: false, provider_id: String::new(), label: String::new(), kind: String::new(), local: false, missing: vec!["no_provider".into()] });
    };
    let caps = providers::capabilities_for_kind(&cfg.kind, &cfg.endpoint);
    let mut missing = vec![];
    if caps.needs_key && secrets.get(&cfg.id)?.map(|k| k.trim().is_empty()).unwrap_or(true) {
        missing.push("no_key".to_string());
    }
    if cfg.kind != "claude_cli" && cfg.model.trim().is_empty() {
        missing.push("no_model".into());
    }
    if cfg.kind == "claude_cli" && find_claude(&cfg.endpoint).is_none() {
        missing.push("cli_missing".into());
    }
    if !caps.local && !s.cloud_notice_accepted {
        missing.push("cloud_notice".into());
    }
    if s.translation.local_only && !caps.local {
        missing.push("local_only".into());
    }
    Ok(ProviderStatus { ready: missing.is_empty(), provider_id: cfg.id, label: cfg.label, kind: cfg.kind, local: caps.local, missing })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryStore;

    #[test]
    fn translation_setup_needed_until_provider_configured() {
        let secrets = MemoryStore::default();
        let mut s = AppSettings::default();
        s.translation.active_provider = "anthropic".into();
        let st = provider_status_for(&s, &secrets).unwrap();
        assert!(!st.ready);
        assert!(st.missing.contains(&"no_key".to_string()) && st.missing.contains(&"cloud_notice".to_string()));
        secrets.set("anthropic", "k").unwrap();
        s.cloud_notice_accepted = true;
        assert!(provider_status_for(&s, &secrets).unwrap().ready);
        // a local provider never asks for a key or the cloud notice
        let mut s = AppSettings::default();
        s.translation.active_provider = "ollama".into();
        let st = provider_status_for(&s, &secrets).unwrap();
        assert_eq!(st.missing, vec!["no_model".to_string()]);
        s.translation.active_provider = "nope".into();
        assert_eq!(provider_status_for(&s, &secrets).unwrap().missing, vec!["no_provider".to_string()]);
    }

    #[test]
    fn library_progress_and_jobs_survive_restart() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("M/ch1");
        std::fs::create_dir_all(&root).unwrap();
        image::RgbImage::new(10, 14).save(root.join("1.png")).unwrap();
        image::RgbImage::new(10, 14).save(root.join("2.png")).unwrap();
        let db_path = d.path().join("library.db");
        {
            let db = Db::open(&db_path).unwrap();
            import::import_path(&db, &d.path().join("M")).unwrap();
            let ch: i64 = db.with(|c| Ok(c.query_row("SELECT id FROM chapter", [], |r| r.get(0))?)).unwrap();
            queries::save_progress(&db, ch, 1).unwrap();
            let id = jobs::enqueue(&db, ch, None, "missing", 0).unwrap();
            db.with(|c| { c.execute("UPDATE job SET status = 'running' WHERE id = ?1", [id])?; Ok(()) }).unwrap();
            let mut s = settings::load(&db).unwrap();
            s.onboarding_done = true;
            settings::save(&db, &s).unwrap();
        } // "app closed" mid-job
        let db = Db::open(&db_path).unwrap();
        assert_eq!(jobs::recover(&db).unwrap(), 1);
        let m = queries::list_manga(&db).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].continue_page, Some(1));
        assert!(m[0].continue_chapter_id.is_some());
        assert_eq!(jobs::list(&db, false).unwrap()[0].status, "queued");
        assert!(settings::load(&db).unwrap().onboarding_done);
    }
}
