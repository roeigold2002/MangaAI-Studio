//! Persistent translation queue. Jobs live in SQLite, so they survive
//! restarts; one runner task processes them in priority order.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Notify;

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::pipeline::{load_page, page_state, Engine, PageCtx};
use crate::providers::{self, TranslationProvider};
use crate::secrets::SecretStore;
use crate::settings::{self, AppSettings, ProviderConfig};

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub manga_id: i64,
    pub manga_title: String,
    pub chapter_id: i64,
    pub chapter_title: String,
    pub page_ids: Option<Vec<i64>>,
    pub mode: String,
    pub priority: i64,
    pub status: String,
    pub total: i64,
    pub completed: i64,
    pub failed: i64,
    pub cost_usd: f64,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const JOB_SELECT: &str = "SELECT j.id, j.kind, j.manga_id, m.title, j.chapter_id, ch.title, j.page_ids, j.mode, j.priority, j.status,
    j.total, j.completed, j.failed, j.cost_usd, j.provider, j.model, j.error, j.created_at, j.updated_at
    FROM job j JOIN manga m ON m.id = j.manga_id JOIN chapter ch ON ch.id = j.chapter_id";

fn job_row(r: &rusqlite::Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        kind: r.get(1)?,
        manga_id: r.get(2)?,
        manga_title: r.get(3)?,
        chapter_id: r.get(4)?,
        chapter_title: r.get(5)?,
        page_ids: r.get::<_, Option<String>>(6)?.and_then(|s| serde_json::from_str(&s).ok()),
        mode: r.get(7)?,
        priority: r.get(8)?,
        status: r.get(9)?,
        total: r.get(10)?,
        completed: r.get(11)?,
        failed: r.get(12)?,
        cost_usd: r.get(13)?,
        provider: r.get(14)?,
        model: r.get(15)?,
        error: r.get(16)?,
        created_at: r.get(17)?,
        updated_at: r.get(18)?,
    })
}

pub fn get(db: &Db, id: i64) -> AppResult<Job> {
    db.with(|c| Ok(c.query_row(&format!("{JOB_SELECT} WHERE j.id = ?1"), [id], job_row)?))
}

pub fn list(db: &Db, include_finished: bool) -> AppResult<Vec<Job>> {
    db.with(|c| {
        let filter = if include_finished { "" } else { "WHERE j.status IN ('queued','running','paused','failed')" };
        let mut s = c.prepare(&format!(
            "{JOB_SELECT} {filter} ORDER BY CASE j.status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 WHEN 'paused' THEN 2 ELSE 3 END,
             j.priority DESC, j.id DESC LIMIT 300"
        ))?;
        let v = s.query_map([], job_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })
}

/// Queue a chapter (or some of its pages). An unfinished job for the same
/// chapter and page set is reused instead of creating a duplicate.
pub fn enqueue(db: &Db, chapter_id: i64, page_ids: Option<Vec<i64>>, mode: &str, priority: i64) -> AppResult<i64> {
    if !matches!(mode, "missing" | "failed" | "all") {
        return Err(AppError::invalid("mode must be missing|failed|all"));
    }
    let pages_json = page_ids.as_ref().map(serde_json::to_string).transpose()?;
    let ts = now();
    db.with_tx(|tx| {
        let manga_id: i64 = tx.query_row("SELECT manga_id FROM chapter WHERE id = ?1", [chapter_id], |r| r.get(0))?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM job WHERE chapter_id = ?1 AND coalesce(page_ids,'') = coalesce(?2,'') AND mode = ?3
                 AND status IN ('queued','running','paused') LIMIT 1",
                params![chapter_id, pages_json, mode],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            tx.execute(
                "UPDATE job SET priority = max(priority, ?1), status = CASE WHEN status = 'paused' THEN 'queued' ELSE status END, updated_at = ?2 WHERE id = ?3",
                params![priority, ts, id],
            )?;
            return Ok(id);
        }
        let total: i64 = match &page_ids {
            Some(p) => p.len() as i64,
            None => tx.query_row("SELECT count(*) FROM page WHERE chapter_id = ?1", [chapter_id], |r| r.get(0))?,
        };
        tx.execute(
            "INSERT INTO job (kind, manga_id, chapter_id, page_ids, mode, priority, status, total, created_at, updated_at)
             VALUES ('translate', ?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?7)",
            params![manga_id, chapter_id, pages_json, mode, priority, total, ts],
        )?;
        Ok(tx.last_insert_rowid())
    })
}

pub fn set_status(db: &Db, id: i64, status: &str) -> AppResult<()> {
    let allowed_from: &[&str] = match status {
        "paused" => &["queued", "running"],
        "queued" => &["paused", "failed", "cancelled"],
        "cancelled" => &["queued", "running", "paused", "failed"],
        _ => return Err(AppError::invalid("bad status")),
    };
    db.with(|c| {
        let cur: String = c.query_row("SELECT status FROM job WHERE id = ?1", [id], |r| r.get(0))?;
        if !allowed_from.contains(&cur.as_str()) {
            return Err(AppError::invalid(format!("cannot change a {cur} job to {status}")));
        }
        c.execute(
            "UPDATE job SET status = ?1, error = CASE WHEN ?1 = 'queued' THEN NULL ELSE error END, updated_at = ?2 WHERE id = ?3",
            params![status, now(), id],
        )?;
        Ok(())
    })
}

/// Queue a retry for only the failed pages of a job.
pub fn retry_failed(db: &Db, id: i64) -> AppResult<i64> {
    let job = get(db, id)?;
    let failed: Vec<i64> = db.with(|c| {
        let mut s = c.prepare(
            "SELECT p.id FROM page p JOIN page_state s ON s.page_id = p.id WHERE p.chapter_id = ?1 AND s.status = 'failed' ORDER BY p.idx",
        )?;
        let v = s.query_map([job.chapter_id], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
        Ok(v)
    })?;
    let failed: Vec<i64> = match &job.page_ids {
        Some(subset) => failed.into_iter().filter(|p| subset.contains(p)).collect(),
        None => failed,
    };
    if failed.is_empty() {
        return Err(AppError::invalid("no failed pages to retry"));
    }
    let new_id = enqueue(db, job.chapter_id, Some(failed), "missing", job.priority + 1)?;
    if matches!(job.status.as_str(), "failed") {
        db.with(|c| {
            c.execute("UPDATE job SET status = 'cancelled', updated_at = ?1 WHERE id = ?2", params![now(), id])?;
            Ok(())
        })?;
    }
    Ok(new_id)
}

pub fn prioritize_chapter(db: &Db, chapter_id: i64) -> AppResult<()> {
    db.with(|c| {
        c.execute(
            "UPDATE job SET priority = (SELECT coalesce(max(priority),0) + 1 FROM job) WHERE chapter_id = ?1 AND status IN ('queued','paused')",
            [chapter_id],
        )?;
        Ok(())
    })
}

pub fn clear_finished(db: &Db) -> AppResult<usize> {
    db.with(|c| Ok(c.execute("DELETE FROM job WHERE status IN ('done','cancelled')", [])?))
}

/// After a crash/quit: running jobs go back to the queue, half-processed pages are retried.
pub fn recover(db: &Db) -> AppResult<usize> {
    db.with(|c| {
        let n = c.execute("UPDATE job SET status = 'queued', updated_at = ?1 WHERE status = 'running'", [now()])?;
        c.execute("UPDATE page_state SET status = 'none' WHERE status = 'running'", [])?;
        Ok(n)
    })
}

fn next_job(db: &Db) -> AppResult<Option<i64>> {
    db.with(|c| {
        Ok(c.query_row("SELECT id FROM job WHERE status = 'queued' ORDER BY priority DESC, id LIMIT 1", [], |r| r.get(0))
            .optional()?)
    })
}

fn job_pages(db: &Db, job: &Job, default_rtl: bool) -> AppResult<Vec<PageCtx>> {
    let ids: Vec<i64> = match &job.page_ids {
        Some(p) => p.clone(),
        None => db.with(|c| {
            let mut s = c.prepare("SELECT id FROM page WHERE chapter_id = ?1 ORDER BY idx")?;
            let v = s.query_map([job.chapter_id], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
            Ok(v)
        })?,
    };
    let mut out = vec![];
    for id in ids {
        let st = page_state(db, id)?;
        let wanted = match job.mode.as_str() {
            "failed" => st.status == "failed",
            _ => true, // 'missing': up-to-date pages are skipped cheaply by the cache keys
        };
        if wanted {
            out.push(load_page(db, id, default_rtl)?);
        }
    }
    Ok(out)
}

pub struct Runner {
    pub engine: Engine,
    pub secrets: Arc<dyn SecretStore>,
    pub notify: Arc<Notify>,
    /// Test hook: use this provider instead of building one from settings.
    pub provider_override: Option<Arc<dyn TranslationProvider>>,
}

pub fn check_provider_allowed(s: &AppSettings, cfg: &ProviderConfig) -> AppResult<()> {
    let caps = providers::capabilities_for_kind(&cfg.kind, &cfg.endpoint);
    if s.translation.local_only && !caps.local {
        return Err(AppError::new(
            "local_only",
            format!("{} sends data off this computer, but local-only mode is on", cfg.label),
        ));
    }
    if !caps.local && !s.cloud_notice_accepted {
        return Err(AppError::new(
            "cloud_notice",
            "Cloud translation sends page text/images to the provider. Accept the notice in Translation settings first.",
        ));
    }
    Ok(())
}

impl Runner {
    fn emit_job(&self, id: i64) {
        if let Ok(j) = get(&self.engine.db, id) {
            self.engine.events.emit("job-updated", json!(j));
        }
    }

    fn finish(&self, id: i64, status: &str, error: Option<String>) -> AppResult<()> {
        self.engine.db.with(|c| {
            c.execute(
                "UPDATE job SET status = ?1, error = ?2, updated_at = ?3 WHERE id = ?4 AND status = 'running'",
                params![status, error, now(), id],
            )?;
            Ok(())
        })?;
        self.emit_job(id);
        Ok(())
    }

    fn current_status(&self, id: i64) -> AppResult<String> {
        self.engine.db.with(|c| Ok(c.query_row("SELECT status FROM job WHERE id = ?1", [id], |r| r.get(0))?))
    }

    /// Process one job until done, paused, cancelled, failed or preempted.
    pub async fn run_job(&self, id: i64) -> AppResult<()> {
        let db = &self.engine.db;
        let s = settings::load(db)?;
        let cfg = s
            .active_provider()
            .cloned()
            .ok_or_else(|| AppError::new("no_provider", "no translation provider selected"))?;
        db.with(|c| {
            c.execute(
                "UPDATE job SET status = 'running', provider = ?1, model = ?2, error = NULL, updated_at = ?3 WHERE id = ?4",
                params![cfg.id, cfg.model, now(), id],
            )?;
            Ok(())
        })?;
        self.emit_job(id);

        let provider = match &self.provider_override {
            Some(p) => p.clone(),
            None => {
                if let Err(e) = check_provider_allowed(&s, &cfg) {
                    self.finish(id, "failed", Some(e.message))?;
                    return Ok(());
                }
                match providers::build(&cfg, self.secrets.as_ref()) {
                    Ok(p) => p,
                    Err(e) => {
                        self.finish(id, "failed", Some(e.message))?;
                        return Ok(());
                    }
                }
            }
        };

        let job = get(db, id)?;
        let pages = job_pages(db, &job, s.reader.direction != "ltr")?;
        let force = job.mode == "all";
        let batch = s.translation.pages_per_request.clamp(1, 8) as usize;
        db.with(|c| {
            c.execute("UPDATE job SET total = ?1, completed = 0, failed = 0 WHERE id = ?2", params![pages.len() as i64, id])?;
            Ok(())
        })?;
        let (mut completed, mut failed, mut cost) = (0i64, 0i64, job.cost_usd);
        // Batches in one wave run concurrently (provider requests overlap; the vision
        // worker serialises itself). With max_concurrency = 1 each batch also sees the
        // previous batch's lines as context.
        let conc = cfg.max_concurrency.clamp(1, 8) as usize;
        let chunks: Vec<&[PageCtx]> = pages.chunks(batch).collect();
        for wave in chunks.chunks(conc) {
            // pause / cancel / preemption checks between waves
            let st = self.current_status(id)?;
            if st != "running" {
                self.emit_job(id);
                return Ok(());
            }
            let higher: Option<i64> = db.with(|c| {
                Ok(c.query_row(
                    "SELECT id FROM job WHERE status = 'queued' AND priority > ?1 LIMIT 1",
                    [job.priority],
                    |r| r.get(0),
                )
                .optional()?)
            })?;
            if higher.is_some() {
                db.with(|c| {
                    c.execute("UPDATE job SET status = 'queued' WHERE id = ?1 AND status = 'running'", [id])?;
                    Ok(())
                })?;
                self.emit_job(id);
                return Ok(());
            }
            if s.translation.job_cost_limit_usd > 0.0 && cost >= s.translation.job_cost_limit_usd {
                db.with(|c| {
                    c.execute(
                        "UPDATE job SET status = 'paused', error = ?1, updated_at = ?2 WHERE id = ?3",
                        params![format!("cost limit reached (${:.2})", cost), now(), id],
                    )?;
                    Ok(())
                })?;
                self.emit_job(id);
                return Ok(());
            }

            let results = futures_util::future::join_all(
                wave.iter().map(|chunk| self.engine.process_batch(chunk, &s, &cfg, provider.clone(), Some(id), force)),
            )
            .await;
            for (chunk, res) in wave.iter().zip(results) {
                match res {
                    Ok(o) => {
                        completed += (o.translated_pages.len() + o.cached_pages.len()) as i64;
                        failed += o.failed.len() as i64;
                        cost += o.cost_usd;
                    }
                    Err(e) => {
                        failed += chunk.len() as i64;
                        let fatal = matches!(
                            e.code.as_str(),
                            "engine_missing" | "model_missing" | "engine_start" | "provider_auth" | "provider_unavailable" | "provider_bad_request"
                        );
                        db.with(|c| {
                            c.execute(
                                "UPDATE job SET completed = ?1, failed = ?2, cost_usd = ?3, error = ?4, updated_at = ?5 WHERE id = ?6",
                                params![completed, failed, cost, e.message, now(), id],
                            )?;
                            Ok(())
                        })?;
                        if fatal {
                            self.finish(id, "failed", Some(e.message))?;
                            return Ok(());
                        }
                    }
                }
            }
            db.with(|c| {
                c.execute(
                    "UPDATE job SET completed = ?1, failed = ?2, cost_usd = ?3, updated_at = ?4 WHERE id = ?5",
                    params![completed, failed, cost, now(), id],
                )?;
                Ok(())
            })?;
            self.emit_job(id);
        }
        if failed > 0 {
            self.finish(id, "failed", Some(format!("{failed} page(s) failed — retry failed pages from the queue")))
        } else {
            self.finish(id, "done", None)
        }
    }

    pub async fn run_forever(self: Arc<Self>) {
        loop {
            match next_job(&self.engine.db) {
                Ok(Some(id)) => {
                    if let Err(e) = self.run_job(id).await {
                        tracing::error!(job = id, "job error: {}", e.message);
                        let _ = self.finish(id, "failed", Some(e.message));
                    }
                }
                Ok(None) => self.notify.notified().await,
                Err(e) => {
                    tracing::error!("queue error: {}", e.message);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Estimate {
    pub pages_total: i64,
    pub pages_to_translate: i64,
    pub regions_estimate: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: Option<f64>,
    pub provider: String,
    pub model: String,
    pub sends_images: bool,
    pub local: bool,
    pub needs_confirmation: bool,
}

/// Rough cost estimate. Pages already translated with the current settings count as free.
pub fn estimate(db: &Db, chapter_ids: &[i64]) -> AppResult<Estimate> {
    let s = settings::load(db)?;
    let cfg = s.active_provider().cloned().ok_or_else(|| AppError::new("no_provider", "no provider selected"))?;
    let caps = providers::capabilities_for_kind(&cfg.kind, &cfg.endpoint);
    let (mut total, mut todo, mut regions) = (0i64, 0i64, 0i64);
    db.with(|c| {
        for ch in chapter_ids {
            let (t, d, r): (i64, i64, i64) = c.query_row(
                "SELECT count(*),
                        sum(CASE WHEN s.status IS NULL OR s.status != 'done' OR s.model IS NOT ?2 THEN 1 ELSE 0 END),
                        coalesce(sum((SELECT count(*) FROM region r WHERE r.page_id = p.id)), 0)
                 FROM page p LEFT JOIN page_state s ON s.page_id = p.id WHERE p.chapter_id = ?1",
                params![ch, cfg.model],
                |r| Ok((r.get(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0), r.get(2)?)),
            )?;
            total += t;
            todo += d;
            regions += r;
        }
        Ok(())
    })?;
    // Unknown pages are assumed to hold ~8 text regions.
    let per_page_regions = if regions > 0 && total > 0 { (regions as f64 / total as f64).max(4.0) } else { 8.0 };
    let image_tokens = if cfg.send_images && caps.vision { 1600.0 } else { 0.0 };
    let per_page_in = 900.0 + image_tokens + per_page_regions * 45.0;
    let per_page_out = 150.0 + per_page_regions * 70.0;
    let batch = s.translation.pages_per_request.max(1) as f64;
    let system_overhead = 1200.0 / batch; // prompt + glossary + context, shared per request
    let input_tokens = (todo as f64 * (per_page_in + system_overhead)) as i64;
    let output_tokens = (todo as f64 * per_page_out) as i64;
    let cost = if cfg.kind == "ollama" || caps.local {
        Some(0.0)
    } else if cfg.price_input_per_mtok > 0.0 || cfg.price_output_per_mtok > 0.0 {
        Some(input_tokens as f64 / 1e6 * cfg.price_input_per_mtok + output_tokens as f64 / 1e6 * cfg.price_output_per_mtok)
    } else {
        None
    };
    let needs_confirmation = cost.map(|c| c > s.translation.confirm_cost_above_usd).unwrap_or(todo > 50);
    Ok(Estimate {
        pages_total: total,
        pages_to_translate: todo,
        regions_estimate: (per_page_regions * todo as f64) as i64,
        input_tokens,
        output_tokens,
        cost_usd: cost,
        provider: cfg.label.clone(),
        model: cfg.model.clone(),
        sends_images: cfg.send_images && caps.vision,
        local: caps.local,
        needs_confirmation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::tests::{setup, tr_json};
    use crate::providers::test_support::MockProvider;
    use crate::providers::{ErrorKind, ProviderError};
    use crate::secrets::MemoryStore;

    fn runner(engine: &Engine, provider: Arc<dyn TranslationProvider>) -> Runner {
        Runner {
            engine: engine.clone(),
            secrets: Arc::new(MemoryStore::default()),
            notify: Arc::new(Notify::new()),
            provider_override: Some(provider),
        }
    }

    fn settings_for_test(engine: &Engine) {
        let mut s = settings::load(&engine.db).unwrap();
        s.translation.pages_per_request = 1;
        s.translation.active_provider = "ollama".into();
        for p in s.translation.providers.iter_mut() {
            p.max_retries = 1;
            if p.id == "ollama" {
                p.model = "m".into();
            }
        }
        settings::save(&engine.db, &s).unwrap();
    }

    #[tokio::test]
    async fn full_chapter_then_cache_reuse_without_new_calls() {
        let (_d, engine, _fake, ch) = setup(&["こんにちは"]);
        settings_for_test(&engine);
        let mock = MockProvider::new(vec![tr_json(&[("p1r1", "שלום")]), tr_json(&[("p2r1", "שלום")]), tr_json(&[("p3r1", "שלום")])]);
        let r = runner(&engine, mock.clone());
        let id = enqueue(&engine.db, ch, None, "missing", 0).unwrap();
        r.run_job(id).await.unwrap();
        let j = get(&engine.db, id).unwrap();
        assert_eq!((j.status.as_str(), j.completed, j.failed), ("done", 3, 0));
        assert_eq!(mock.requests.lock().len(), 3);

        // Second run: everything cached -> no provider calls, still done.
        let id2 = enqueue(&engine.db, ch, None, "missing", 0).unwrap();
        assert_ne!(id, id2);
        r.run_job(id2).await.unwrap();
        assert_eq!(get(&engine.db, id2).unwrap().status, "done");
        assert_eq!(mock.requests.lock().len(), 3, "translations were reused, not paid for again");
    }

    #[tokio::test]
    async fn failed_page_is_not_reported_done_and_can_be_retried() {
        let (_d, engine, _fake, ch) = setup(&["あ"]);
        settings_for_test(&engine);
        let bad = || Err(ProviderError::new(ErrorKind::Server, "boom"));
        let mock = MockProvider::new(vec![tr_json(&[("p1r1", "א")]), bad(), bad(), tr_json(&[("p3r1", "ג")]), tr_json(&[("p2r1", "ב")])]);
        let r = runner(&engine, mock.clone());
        let id = enqueue(&engine.db, ch, None, "missing", 0).unwrap();
        r.run_job(id).await.unwrap();
        let j = get(&engine.db, id).unwrap();
        assert_eq!(j.status, "failed");
        assert_eq!((j.completed, j.failed), (2, 1));
        let view = crate::library::queries::chapter_view(&engine.db, ch).unwrap();
        assert_eq!(view.chapter.translation.done, 2);
        assert_eq!(view.chapter.translation.failed, 1);

        let retry = retry_failed(&engine.db, id).unwrap();
        r.run_job(retry).await.unwrap();
        let j = get(&engine.db, retry).unwrap();
        assert_eq!((j.status.as_str(), j.total, j.completed), ("done", 1, 1));
        let view = crate::library::queries::chapter_view(&engine.db, ch).unwrap();
        assert_eq!(view.chapter.translation.done, 3);
    }

    #[tokio::test]
    async fn interrupted_job_recovers_and_finishes() {
        let (_d, engine, _fake, ch) = setup(&["あ"]);
        settings_for_test(&engine);
        let id = enqueue(&engine.db, ch, None, "missing", 0).unwrap();
        // simulate a crash mid-job
        engine.db.with(|c| { c.execute("UPDATE job SET status = 'running' WHERE id = ?1", [id])?; Ok(()) }).unwrap();
        assert_eq!(recover(&engine.db).unwrap(), 1);
        assert_eq!(get(&engine.db, id).unwrap().status, "queued");
        let mock = MockProvider::new(vec![tr_json(&[("p1r1", "א")]), tr_json(&[("p2r1", "ב")]), tr_json(&[("p3r1", "ג")])]);
        runner(&engine, mock).run_job(next_job(&engine.db).unwrap().unwrap()).await.unwrap();
        assert_eq!(get(&engine.db, id).unwrap().status, "done");
    }

    #[tokio::test]
    async fn pause_cancel_and_dedupe() {
        let (_d, engine, _fake, ch) = setup(&["あ"]);
        settings_for_test(&engine);
        let id = enqueue(&engine.db, ch, None, "missing", 0).unwrap();
        assert_eq!(enqueue(&engine.db, ch, None, "missing", 5).unwrap(), id, "no duplicate job");
        assert_eq!(get(&engine.db, id).unwrap().priority, 5);
        set_status(&engine.db, id, "paused").unwrap();
        assert!(set_status(&engine.db, id, "paused").is_err());
        assert_eq!(next_job(&engine.db).unwrap(), None);
        set_status(&engine.db, id, "queued").unwrap();
        set_status(&engine.db, id, "cancelled").unwrap();
        assert_eq!(next_job(&engine.db).unwrap(), None);
        // a paused job stops before the next batch
        let id = enqueue(&engine.db, ch, None, "missing", 0).unwrap();
        engine.db.with(|c| { c.execute("UPDATE job SET status = 'paused' WHERE id = ?1", [id])?; Ok(()) }).unwrap();
        let mock = MockProvider::new(vec![]);
        let r = runner(&engine, mock.clone());
        // run_job flips it to running; emulate the user pausing right away
        engine.db.with(|c| { c.execute("CREATE TRIGGER pause_now AFTER UPDATE OF status ON job WHEN NEW.status = 'running' BEGIN UPDATE job SET status = 'paused' WHERE id = NEW.id; END", [])?; Ok(()) }).unwrap();
        r.run_job(id).await.unwrap();
        assert_eq!(get(&engine.db, id).unwrap().status, "paused");
        assert!(mock.requests.lock().is_empty());
    }

    #[test]
    fn local_only_and_cloud_notice() {
        let mut s = AppSettings::default();
        let cloud = s.provider("anthropic").unwrap().clone();
        let local = s.provider("ollama").unwrap().clone();
        assert_eq!(check_provider_allowed(&s, &cloud).unwrap_err().code, "cloud_notice");
        s.cloud_notice_accepted = true;
        assert!(check_provider_allowed(&s, &cloud).is_ok());
        s.translation.local_only = true;
        assert_eq!(check_provider_allowed(&s, &cloud).unwrap_err().code, "local_only");
        assert!(check_provider_allowed(&s, &local).is_ok());
    }

    #[test]
    fn estimate_counts_only_untranslated_pages() {
        let (_d, engine, _fake, ch) = setup(&["あ"]);
        let mut s = settings::load(&engine.db).unwrap();
        s.translation.active_provider = "anthropic".into();
        settings::save(&engine.db, &s).unwrap();
        let e = estimate(&engine.db, &[ch]).unwrap();
        assert_eq!((e.pages_total, e.pages_to_translate), (3, 3));
        assert!(e.cost_usd.unwrap() > 0.0);
        assert!(e.sends_images);
    }
}
