//! Compare translation models on the same pages with the real pipeline (worker OCR → provider →
//! clean + render). Prints time per page and every line, and writes them as JSON for side-by-side
//! review. Nothing is retried on purpose: a model that breaks the JSON schema shows up as failed.
//!
//! Usage: cargo run --example bench_translate -- <cbz> <provider-id> <model> <pages> <lang ja|en> <out.json>
//! MANGAAI_E2E_IMAGES=0: text only. MANGAAI_GPU_LAYERS=n: Ollama layers on the GPU.
//! MANGAAI_OCR_DEVICE=cpu|cuda|auto: where the OCR/inpainting worker runs. Spends provider credits for cloud providers (none for Ollama).

use std::sync::Arc;

use mangaai_lib::assets::Paths;
use mangaai_lib::db::Db;
use mangaai_lib::jobs::{self, Runner};
use mangaai_lib::library::import::import_path;
use mangaai_lib::pipeline::{load_regions, page_state, Engine, NoEvents};
use mangaai_lib::secrets::KeyringStore;
use mangaai_lib::settings;
use mangaai_lib::worker::{self, PythonWorker, Vision, WorkerConfig};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (input, provider_id, model, pages, lang, out) =
        (std::path::PathBuf::from(&a[1]), a[2].clone(), a[3].clone(), a[4].parse::<usize>()?, a[5].clone(), a[6].clone());
    let e = |e: mangaai_lib::error::AppError| anyhow::anyhow!("{}: {}", e.code, e.message);

    let tmp = tempfile::tempdir()?;
    let paths = Paths::new(tmp.path().join("app"))?;
    let db = Db::open(&paths.db_path())?;
    let app_data = mangaai_lib::platform::default_data_dir();
    let (worker_dir, python) = worker::resolve(&app_data, None);
    let vision = Arc::new(PythonWorker::new(WorkerConfig {
        python,
        worker_dir,
        models_dir: app_data.join("models"),
        fonts_dir: mangaai_lib::platform::canonicalize(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/fonts"))?,
        log_path: paths.logs_dir().join("worker.log"),
        native_dir: worker::resolve_native_dir(None),
    }));
    vision.health().await.map_err(e)?;

    let mut s = settings::load(&db)?;
    s.translation.active_provider = provider_id.clone();
    s.translation.pages_per_request = 1;
    s.cloud_notice_accepted = true;
    if let Ok(d) = std::env::var("MANGAAI_OCR_DEVICE") {
        s.ocr.device = d;
    }
    for p in s.translation.providers.iter_mut().filter(|p| p.id == provider_id) {
        p.model = model.clone();
        p.max_retries = 0;
        p.send_images = std::env::var("MANGAAI_E2E_IMAGES").as_deref() != Ok("0");
        p.gpu_layers = std::env::var("MANGAAI_GPU_LAYERS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    }
    settings::save(&db, &s)?;

    import_path(&db, &input).map_err(e)?;
    db.with(|c| {
        c.execute("UPDATE manga SET source_lang = ?1", [&lang])?;
        Ok(())
    })
    .map_err(e)?;
    let (chapter_id, page_ids): (i64, Vec<i64>) = db
        .with(|c| {
            let ch: i64 = c.query_row("SELECT id FROM chapter ORDER BY sort_index LIMIT 1", [], |r| r.get(0))?;
            let mut st = c.prepare("SELECT id FROM page WHERE chapter_id = ?1 ORDER BY idx LIMIT ?2")?;
            let ids = st.query_map(rusqlite::params![ch, pages as i64], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
            Ok((ch, ids))
        })
        .map_err(e)?;

    let engine = Engine::new(db.clone(), paths.clone(), vision.clone(), Arc::new(NoEvents));
    let runner = Runner { engine, secrets: Arc::new(KeyringStore), notify: Arc::new(tokio::sync::Notify::new()), provider_override: None };

    // analysis first (shared by every model), so the timing below is the translation step
    let job = jobs::enqueue(&db, chapter_id, Some(page_ids.clone()), "missing", 0).map_err(e)?;
    let t0 = std::time::Instant::now();
    runner.run_job(job).await.map_err(e)?;
    let secs = t0.elapsed().as_secs_f32();
    let j = jobs::get(&db, job).map_err(e)?;
    println!("{model}: status={} completed={} failed={} {:.1}s ({:.1}s/page) error={:?}", j.status, j.completed, j.failed, secs, secs / pages as f32, j.error);

    let mut lines = vec![];
    for pid in &page_ids {
        let st = page_state(&db, *pid).map_err(e)?;
        if st.status != "done" {
            println!("  page {pid}: {} stage={:?} error={:?}", st.status, st.stage, st.error);
        }
        if let Some(p) = &st.rendered_path {
            let copy = std::path::PathBuf::from(&out).with_extension(format!("p{pid}.png"));
            std::fs::copy(p, &copy)?;
        }
        for r in load_regions(&db, *pid).map_err(e)? {
            println!("  {} | {} => {}", r.rid, r.source(), r.translation.clone().unwrap_or_default());
            lines.push(json!({"page": pid, "rid": r.rid, "source": r.source(), "translation": r.translation, "category": r.category}));
        }
    }
    std::fs::write(&out, serde_json::to_string_pretty(&json!({
        "model": model, "seconds": secs, "pages": pages, "status": j.status, "failed": j.failed, "error": j.error, "lines": lines
    }))?)?;
    vision.shutdown().await;
    Ok(())
}
