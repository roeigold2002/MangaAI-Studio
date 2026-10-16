//! End-to-end check with the real components (no mocks):
//! import a CBZ → detect/OCR (Python worker) → translate with the configured
//! provider → clean + render Hebrew → reopen without new provider calls →
//! edit + approve → re-render → original archive unchanged.
//!
//! Usage: cargo run --example e2e -- <cbz-or-folder> [provider-id] [model] [pages]
//! Spends real provider credits for the pages processed.

use std::sync::Arc;

use mangaai_lib::assets::Paths;
use mangaai_lib::db::Db;
use mangaai_lib::editor::{self, RegionEdit};
use mangaai_lib::jobs::{self, Runner};
use mangaai_lib::library::import::import_path;
use mangaai_lib::pipeline::{load_regions, page_state, Engine, NoEvents};
use mangaai_lib::secrets::KeyringStore;
use mangaai_lib::settings;
use mangaai_lib::worker::{self, PythonWorker, Vision, WorkerConfig};
use sha2::Digest;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let input = std::path::PathBuf::from(args.get(1).expect("path to a CBZ or folder"));
    let provider_id = args.get(2).cloned().unwrap_or_else(|| "claude_cli".into());
    let model = args.get(3).cloned();
    let pages: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);

    let before = sha2::Sha256::digest(std::fs::read(&input).unwrap_or_default());
    let tmp = tempfile::tempdir()?;
    let data_root = std::env::var_os("MANGAAI_DATA_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| tmp.path().join("app"));
    let paths = Paths::new(data_root)?;
    let db = Db::open(&paths.db_path())?;
    let app_data = std::env::var_os("MANGAAI_DATA_DIR").map(std::path::PathBuf::from).unwrap_or_else(mangaai_lib::platform::default_data_dir);
    let (worker_dir, python) = worker::resolve(&app_data, None);
    let fonts = mangaai_lib::platform::canonicalize(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/fonts"))?;
    let vision = Arc::new(PythonWorker::new(WorkerConfig {
        python,
        worker_dir,
        models_dir: app_data.join("models"),
        fonts_dir: fonts,
        log_path: paths.logs_dir().join("worker.log"),
        native_dir: worker::resolve_native_dir(None),
    }));
    println!("worker health: {}", vision.health().await.map_err(|e| anyhow::anyhow!(e.message))?);

    // settings: chosen provider, cloud notice accepted for this run
    let mut s = settings::load(&db)?;
    s.translation.active_provider = provider_id.clone();
    s.translation.pages_per_request = pages.min(4) as u32;
    s.cloud_notice_accepted = true;
    if let Some(m) = &model {
        for p in s.translation.providers.iter_mut().filter(|p| p.id == provider_id) {
            p.model = m.clone();
        }
    }
    settings::save(&db, &s)?;

    let report = import_path(&db, &input).map_err(|e| anyhow::anyhow!(e.message))?;
    println!("imported: {} ({} pages, {} chapters)", report.title, report.pages_added, report.chapters_added);
    db.with(|c| {
        // Pepper&Carrot is a western comic: left-to-right
        c.execute("UPDATE manga SET reading_dir = 'ltr'", [])?;
        // MANGAAI_E2E_LANG=en: treat the pages as non-Japanese (text read from the image by the model)
        if let Ok(l) = std::env::var("MANGAAI_E2E_LANG") {
            c.execute("UPDATE manga SET source_lang = ?1", [l])?;
        }
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!(e.message))?;
    let (chapter_id, page_ids): (i64, Vec<i64>) = db
        .with(|c| {
            let ch: i64 = c.query_row("SELECT id FROM chapter ORDER BY sort_index LIMIT 1", [], |r| r.get(0))?;
            let mut st = c.prepare("SELECT id FROM page WHERE chapter_id = ?1 ORDER BY idx LIMIT ?2")?;
            let ids = st.query_map(rusqlite::params![ch, pages as i64], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
            Ok((ch, ids))
        })
        .map_err(|e| anyhow::anyhow!(e.message))?;

    let engine = Engine::new(db.clone(), paths.clone(), vision.clone(), Arc::new(NoEvents));
    let runner = Runner { engine: engine.clone(), secrets: Arc::new(KeyringStore), notify: Arc::new(tokio::sync::Notify::new()), provider_override: None };

    // Failure + recovery: first run with a model name the provider rejects.
    let good = settings::load(&db)?;
    let mut bad = good.clone();
    for p in bad.translation.providers.iter_mut().filter(|p| p.id == provider_id) {
        p.model = "no-such-model-e2e".into();
        p.max_retries = 0;
    }
    settings::save(&db, &bad)?;
    let failing = jobs::enqueue(&db, chapter_id, Some(page_ids.clone()), "missing", 0).map_err(|e| anyhow::anyhow!(e.message))?;
    runner.run_job(failing).await.map_err(|e| anyhow::anyhow!(e.message))?;
    let fj = jobs::get(&db, failing).map_err(|e| anyhow::anyhow!(e.message))?;
    let fp = page_state(&db, page_ids[0]).map_err(|e| anyhow::anyhow!(e.message))?;
    println!("failing job: status={} failed={} error={:?}", fj.status, fj.failed, fj.error);
    println!("  page {} status={} stage={:?} error={:?}", page_ids[0], fp.status, fp.stage, fp.error);
    assert_eq!(fj.status, "failed");
    assert_eq!(fp.status, "failed", "a failed page must not be marked translated");
    settings::save(&db, &good)?;

    // Recover: retry only the failed pages with the working configuration.
    let t0 = std::time::Instant::now();
    let job = jobs::retry_failed(&db, failing).map_err(|e| anyhow::anyhow!(e.message))?;
    runner.run_job(job).await.map_err(|e| anyhow::anyhow!(e.message))?;
    let j = jobs::get(&db, job).map_err(|e| anyhow::anyhow!(e.message))?;
    println!("retry job: status={} completed={} failed={} cost=${:.4} error={:?} ({:.1}s)", j.status, j.completed, j.failed, j.cost_usd, j.error, t0.elapsed().as_secs_f32());
    assert_eq!(j.status, "done", "retry after failure must succeed");

    for pid in &page_ids {
        let st = page_state(&db, *pid).map_err(|e| anyhow::anyhow!(e.message))?;
        println!("page {pid}: {} rendered={:?} notes={}", st.status, st.rendered_path, st.notes);
        for r in load_regions(&db, *pid).map_err(|e| anyhow::anyhow!(e.message))? {
            println!("  {} [{}] {:?} | {} => {}", r.rid, r.category.clone().unwrap_or_default(), r.confidence, r.source(), r.translation.clone().unwrap_or_default());
        }
        if let Some(p) = &st.rendered_path {
            let out = std::env::temp_dir().join(format!("mangaai-e2e-page-{pid}.png"));
            std::fs::copy(p, &out)?;
            println!("  copy of rendered page: {}", out.display());
        }
    }

    // Reopen: the same pages again must not call the provider.
    let calls_before: i64 = db.with(|c| Ok(c.query_row("SELECT count(*) FROM usage_log", [], |r| r.get(0))?)).map_err(|e| anyhow::anyhow!(e.message))?;
    let job2 = jobs::enqueue(&db, chapter_id, Some(page_ids.clone()), "missing", 0).map_err(|e| anyhow::anyhow!(e.message))?;
    runner.run_job(job2).await.map_err(|e| anyhow::anyhow!(e.message))?;
    let calls_after: i64 = db.with(|c| Ok(c.query_row("SELECT count(*) FROM usage_log", [], |r| r.get(0))?)).map_err(|e| anyhow::anyhow!(e.message))?;
    println!("job 2 (cached): status={} provider calls before={} after={}", jobs::get(&db, job2).unwrap().status, calls_before, calls_after);
    assert_eq!(calls_before, calls_after, "cached pages must not be sent again");

    // Edit + approve + re-render
    let first = load_regions(&db, page_ids[0]).map_err(|e| anyhow::anyhow!(e.message))?;
    if let Some(r) = first.first() {
        let edited = editor::update_region(&engine, r.id, RegionEdit { translation: Some("תיקון ידני (בדיקה) 123!".into()), ..Default::default() })
            .map_err(|e| anyhow::anyhow!(e.message))?;
        assert!(edited.approved);
        let st = editor::rerender_page(&engine, page_ids[0], &settings::load(&db)?).await.map_err(|e| anyhow::anyhow!(e.message))?;
        println!("after edit: status={} rendered={:?}", st.status, st.rendered_path);
        if let Some(p) = &st.rendered_path {
            let out = std::env::temp_dir().join("mangaai-e2e-edited.png");
            std::fs::copy(p, &out)?;
            println!("  copy of edited page: {}", out.display());
        }
    }

    let after = sha2::Sha256::digest(std::fs::read(&input).unwrap_or_default());
    assert_eq!(before, after, "original file must be unchanged");
    println!("original unchanged: yes");
    vision.shutdown().await;
    Ok(())
}
