//! Per-page translation pipeline: analyze → translate → clean → render.
//! Every stage has a deterministic cache key; unchanged inputs are never
//! reprocessed (and never paid for twice). Status is tracked per page from
//! real results.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use crate::assets::{atomic_write, read_page_bytes, sha256_str, Paths};
use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::library::scan::Container;
use crate::providers::{cost_from_usage, ImagePart, TranslationProvider};
use crate::settings::{AppSettings, ProviderConfig};
use crate::translate::{self, memory, prompt, Context, PageInput, RegionInput, RetryPolicy};
use crate::worker::{DetectedRegion, Vision};

pub trait EventSink: Send + Sync {
    fn emit(&self, event: &str, payload: Value);
}

pub struct NoEvents;
impl EventSink for NoEvents {
    fn emit(&self, _event: &str, _payload: Value) {}
}

#[derive(Clone)]
pub struct Engine {
    pub db: Db,
    pub paths: Paths,
    pub vision: Arc<dyn Vision>,
    pub events: Arc<dyn EventSink>,
}

#[derive(Debug, Clone)]
pub struct PageCtx {
    pub page_id: i64,
    pub chapter_id: i64,
    pub manga_id: i64,
    pub idx: i64,
    pub path: String,
    pub sha256: String,
    pub mime: String,
    pub container: Container,
    pub location: PathBuf,
    pub manga_title: String,
    pub source_lang: String,
    pub rtl: bool,
    pub translation_notes: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionRow {
    pub id: i64,
    pub page_id: i64,
    pub rid: String,
    pub order_idx: i64,
    pub kind: String,
    pub bbox: [i64; 4],
    pub bubble: Option<[i64; 4]>,
    pub parts: Vec<[i64; 4]>,
    pub vertical: bool,
    pub ocr_text: String,
    pub ocr_confidence: Option<f64>,
    pub source_text: Option<String>,
    pub translation: Option<String>,
    pub category: Option<String>,
    pub speaker: Option<String>,
    pub confidence: Option<String>,
    pub note: Option<String>,
    pub approved: bool,
    pub skip: bool,
    pub layout: Value,
    pub history: Value,
}

impl RegionRow {
    pub fn source(&self) -> &str {
        self.source_text.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(&self.ocr_text)
    }
    fn uncertain(&self) -> bool {
        self.source_text.is_none() && self.ocr_confidence.map(|c| c < 0.6).unwrap_or(false)
    }
}

pub fn load_page(db: &Db, page_id: i64, default_rtl: bool) -> AppResult<PageCtx> {
    db.with(|c| {
        Ok(c.query_row(
            "SELECT p.id, p.chapter_id, ch.manga_id, p.idx, p.path, p.sha256, p.mime, ch.container, ch.location,
                    m.title, m.source_lang, m.reading_dir, m.translation_notes
             FROM page p JOIN chapter ch ON ch.id = p.chapter_id JOIN manga m ON m.id = ch.manga_id WHERE p.id = ?1",
            [page_id],
            |r| {
                let dir: Option<String> = r.get(11)?;
                Ok(PageCtx {
                    page_id: r.get(0)?,
                    chapter_id: r.get(1)?,
                    manga_id: r.get(2)?,
                    idx: r.get(3)?,
                    path: r.get(4)?,
                    sha256: r.get(5)?,
                    mime: r.get(6)?,
                    container: Container::parse(&r.get::<_, String>(7)?),
                    location: PathBuf::from(r.get::<_, String>(8)?),
                    manga_title: r.get(9)?,
                    source_lang: r.get(10)?,
                    rtl: dir.map(|d| d != "ltr").unwrap_or(default_rtl),
                    translation_notes: r.get(12)?,
                })
            },
        )?)
    })
}

fn parse_box(v: Option<String>) -> Option<[i64; 4]> {
    v.and_then(|s| serde_json::from_str(&s).ok())
}

pub fn load_regions(db: &Db, page_id: i64) -> AppResult<Vec<RegionRow>> {
    db.with(|c| {
        let mut s = c.prepare(
            "SELECT id, page_id, rid, order_idx, kind, bbox, bubble, parts, vertical, ocr_text, ocr_confidence, source_text,
                    translation, category, speaker, confidence, note, approved, skip, layout, history
             FROM region WHERE page_id = ?1 ORDER BY order_idx, id",
        )?;
        let v = s
            .query_map([page_id], |r| {
                Ok(RegionRow {
                    id: r.get(0)?,
                    page_id: r.get(1)?,
                    rid: r.get(2)?,
                    order_idx: r.get(3)?,
                    kind: r.get(4)?,
                    bbox: parse_box(r.get(5)?).unwrap_or([0, 0, 0, 0]),
                    bubble: parse_box(r.get(6)?),
                    parts: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                    vertical: r.get::<_, i64>(8)? != 0,
                    ocr_text: r.get(9)?,
                    ocr_confidence: r.get(10)?,
                    source_text: r.get(11)?,
                    translation: r.get(12)?,
                    category: r.get(13)?,
                    speaker: r.get(14)?,
                    confidence: r.get(15)?,
                    note: r.get(16)?,
                    approved: r.get::<_, i64>(17)? != 0,
                    skip: r.get::<_, i64>(18)? != 0,
                    layout: serde_json::from_str(&r.get::<_, String>(19)?).unwrap_or(json!({})),
                    history: serde_json::from_str(&r.get::<_, String>(20)?).unwrap_or(json!([])),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PageStateRow {
    pub status: String,
    pub stage: Option<String>,
    pub error: Option<String>,
    pub analysis_key: Option<String>,
    pub translation_key: Option<String>,
    pub clean_key: Option<String>,
    pub render_key: Option<String>,
    pub clean_path: Option<String>,
    pub rendered_path: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
    pub notes: Value,
    pub summary: Option<String>,
}

pub fn page_state(db: &Db, page_id: i64) -> AppResult<PageStateRow> {
    db.with(|c| {
        Ok(c
            .query_row(
                "SELECT status, stage, error, analysis_key, translation_key, clean_key, render_key, clean_path, rendered_path,
                        provider, model, cost_usd, notes, summary FROM page_state WHERE page_id = ?1",
                [page_id],
                |r| {
                    Ok(PageStateRow {
                        status: r.get(0)?,
                        stage: r.get(1)?,
                        error: r.get(2)?,
                        analysis_key: r.get(3)?,
                        translation_key: r.get(4)?,
                        clean_key: r.get(5)?,
                        render_key: r.get(6)?,
                        clean_path: r.get(7)?,
                        rendered_path: r.get(8)?,
                        provider: r.get(9)?,
                        model: r.get(10)?,
                        cost_usd: r.get(11)?,
                        notes: serde_json::from_str(&r.get::<_, String>(12)?).unwrap_or(json!([])),
                        summary: r.get(13)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_else(|| PageStateRow { status: "none".into(), notes: json!([]), ..Default::default() }))
    })
}

fn ensure_state(c: &rusqlite::Connection, page_id: i64) -> AppResult<()> {
    c.execute("INSERT OR IGNORE INTO page_state (page_id, status, updated_at) VALUES (?1, 'none', ?2)", params![page_id, now()])?;
    Ok(())
}

pub fn set_status(db: &Db, page_id: i64, status: &str, stage: Option<&str>, error: Option<&str>) -> AppResult<()> {
    db.with(|c| {
        ensure_state(c, page_id)?;
        c.execute(
            "UPDATE page_state SET status = ?1, stage = ?2, error = ?3, updated_at = ?4 WHERE page_id = ?5",
            params![status, stage, error, now(), page_id],
        )?;
        Ok(())
    })
}

/// Settings that change what the translator receives (part of the translation key).
pub fn translation_key(page: &PageCtx, regions: &[RegionRow], cfg: &ProviderConfig, s: &AppSettings, glossary_version: &str) -> String {
    let mut parts: Vec<String> = vec![
        page.sha256.clone(),
        prompt::PROMPT_VERSION.into(),
        cfg.kind.clone(),
        cfg.model.clone(),
        glossary_version.into(),
        s.translation.target_lang.clone(),
        s.translation.sfx_mode.clone(),
        page.source_lang.clone(),
        format!("img={}", cfg.send_images),
    ];
    for r in regions {
        parts.push(format!("{}|{}|{}", r.rid, r.source(), r.skip));
    }
    let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    sha256_str(&refs)
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct BatchOutcome {
    pub translated_pages: Vec<i64>,
    pub cached_pages: Vec<i64>,
    pub failed: Vec<(i64, String)>,
    pub cost_usd: f64,
    pub provider_calls: u32,
}

impl Engine {
    pub fn new(db: Db, paths: Paths, vision: Arc<dyn Vision>, events: Arc<dyn EventSink>) -> Self {
        Self { db, paths, vision, events }
    }

    fn emit_page(&self, p: &PageCtx) {
        let st = page_state(&self.db, p.page_id).ok();
        self.events.emit(
            "page-updated",
            json!({"page_id": p.page_id, "chapter_id": p.chapter_id, "status": st.as_ref().map(|s| s.status.clone()),
                   "error": st.and_then(|s| s.error)}),
        );
    }

    /// A readable file for the original page (archive entries are extracted once, read-only copy).
    pub fn source_file(&self, p: &PageCtx) -> AppResult<PathBuf> {
        match p.container {
            Container::Folder => {
                let path = PathBuf::from(&p.path);
                if !path.exists() {
                    return Err(AppError::new("source_missing", format!("original file is missing: {}", p.path)));
                }
                Ok(path)
            }
            Container::Archive => {
                let ext = crate::library::scan::ext_of(&p.path);
                let out = self.paths.derived_dir(&p.sha256).join(format!("source.{ext}"));
                if !out.exists() {
                    let bytes = read_page_bytes(Container::Archive, &p.location, &p.path)?;
                    atomic_write(&out, &bytes)?;
                }
                Ok(out)
            }
        }
    }

    // ---------- stage 1: analysis ----------

    pub async fn analyze(&self, p: &PageCtx, s: &AppSettings, force: bool) -> AppResult<()> {
        let (av, _, _) = self.vision.versions();
        let key = sha256_str(&[&p.sha256, &av, &format!("{:.2}", s.ocr.detector_threshold), if p.rtl { "rtl" } else { "ltr" }]);
        let st = page_state(&self.db, p.page_id)?;
        if !force && st.analysis_key.as_deref() == Some(key.as_str()) {
            return Ok(());
        }
        let src = self.source_file(p)?;
        let analysis = self.vision.analyze(&src, p.rtl, s.ocr.detector_threshold, &s.ocr.device).await?;
        self.store_analysis(p.page_id, &analysis.regions, &key)?;
        Ok(())
    }

    /// Replace detected regions. Approved regions are kept; new detections that overlap them are dropped.
    pub fn store_analysis(&self, page_id: i64, regions: &[DetectedRegion], key: &str) -> AppResult<()> {
        let ts = now();
        let keep = load_regions(&self.db, page_id)?.into_iter().filter(|r| r.approved).collect::<Vec<_>>();
        self.db.with_tx(|tx| {
            ensure_state(tx, page_id)?;
            tx.execute("DELETE FROM region WHERE page_id = ?1 AND approved = 0", [page_id])?;
            let mut n = keep.iter().filter_map(|r| r.rid.trim_start_matches('r').parse::<i64>().ok()).max().unwrap_or(0);
            let mut order = 0i64;
            for d in regions {
                if keep.iter().any(|k| iou(k.bbox, d.bbox) > 0.4) {
                    continue;
                }
                n += 1;
                order += 1;
                tx.execute(
                    "INSERT INTO region (page_id, rid, order_idx, kind, bbox, bubble, parts, vertical, ocr_text, ocr_confidence,
                                         category, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        page_id,
                        format!("r{n}"),
                        order,
                        d.kind,
                        serde_json::to_string(&d.bbox)?,
                        d.bubble.map(|b| serde_json::to_string(&b)).transpose()?,
                        serde_json::to_string(&d.parts)?,
                        d.vertical,
                        d.text,
                        d.confidence,
                        if d.punct_only { Some("sfx") } else { None },
                        ts
                    ],
                )?;
            }
            tx.execute(
                "UPDATE page_state SET analysis_key = ?1, translation_key = NULL, status = CASE WHEN status = 'none' OR status = 'failed' THEN 'analyzed' ELSE status END,
                     stage = NULL, error = NULL, updated_at = ?2 WHERE page_id = ?3",
                params![key, ts, page_id],
            )?;
            Ok(())
        })?;
        self.renumber(page_id)?;
        Ok(())
    }

    /// Re-sort regions into reading order positions (approved ones keep their place by bbox).
    fn renumber(&self, page_id: i64) -> AppResult<()> {
        let mut regs = load_regions(&self.db, page_id)?;
        regs.sort_by_key(|r| (r.order_idx, r.id));
        self.db.with_tx(|tx| {
            for (i, r) in regs.iter().enumerate() {
                tx.execute("UPDATE region SET order_idx = ?1 WHERE id = ?2", params![i as i64 + 1, r.id])?;
            }
            Ok(())
        })
    }

    // ---------- stage 2: translation ----------

    fn context(&self, first: &PageCtx, s: &AppSettings) -> AppResult<Context> {
        let glossary = memory::list_glossary(&self.db, first.manga_id)?
            .into_iter()
            .filter(|g| g.status == "confirmed")
            .take(300)
            .map(|g| {
                let mut line = format!("{} → {} ({}", g.source, g.target, g.kind);
                if let Some(gd) = g.gender.as_deref().filter(|x| !x.is_empty()) {
                    line.push_str(&format!(", {}", match gd { "m" => "male", "f" => "female", _ => "neutral" }));
                }
                line.push(')');
                if !g.notes.is_empty() {
                    line.push_str(&format!(" — {}", g.notes));
                }
                line
            })
            .collect();
        let limit = s.translation.context_lines as i64;
        let (lines, summaries): (Vec<String>, Vec<String>) = self.db.with(|c| {
            let mut st = c.prepare(
                "SELECT coalesce(r.speaker, ''), coalesce(nullif(r.source_text, ''), r.ocr_text), r.translation
                 FROM region r JOIN page p ON p.id = r.page_id
                 WHERE p.chapter_id = ?1 AND p.idx < ?2 AND r.translation IS NOT NULL AND r.translation != ''
                 ORDER BY p.idx DESC, r.order_idx DESC LIMIT ?3",
            )?;
            let mut lines: Vec<String> = st
                .query_map(params![first.chapter_id, first.idx, limit], |r| {
                    let sp: String = r.get(0)?;
                    let src: String = r.get(1)?;
                    let tr: String = r.get(2)?;
                    Ok(if sp.is_empty() { format!("{src} → {tr}") } else { format!("[{sp}] {src} → {tr}") })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            lines.reverse();
            let mut st = c.prepare(
                "SELECT s.summary FROM page_state s JOIN page p ON p.id = s.page_id
                 WHERE p.chapter_id = ?1 AND p.idx < ?2 AND s.summary IS NOT NULL AND s.summary != ''
                 ORDER BY p.idx DESC LIMIT 8",
            )?;
            let mut sums: Vec<String> = st.query_map(params![first.chapter_id, first.idx], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;
            sums.reverse();
            Ok((lines, sums))
        })?;
        Ok(Context {
            series_title: first.manga_title.clone(),
            series_notes: first.translation_notes.clone(),
            glossary,
            previous_lines: lines,
            previous_summaries: summaries,
            memory_hints: vec![],
        })
    }

    fn page_image(&self, p: &PageCtx) -> AppResult<ImagePart> {
        let src = self.source_file(p)?;
        let bytes = std::fs::read(&src)?;
        // Downscale for the provider: long side ≤ 1568 px keeps tokens bounded and text legible.
        let img = image::load_from_memory(&bytes).map_err(|e| AppError::new("bad_image", e.to_string()))?;
        let img = if img.width().max(img.height()) > 1568 { img.resize(1568, 1568, image::imageops::FilterType::Triangle) } else { img };
        let mut out = std::io::Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
            .encode_image(&img.to_rgb8())
            .map_err(|e| AppError::new("bad_image", e.to_string()))?;
        Ok(ImagePart { mime: "image/jpeg".into(), data: out.into_inner(), path: Some(src) })
    }

    /// Translate a batch of (already analyzed) pages. Pages whose key is unchanged are skipped.
    pub async fn translate(
        &self,
        pages: &[PageCtx],
        s: &AppSettings,
        cfg: &ProviderConfig,
        provider: Arc<dyn TranslationProvider>,
        job_id: Option<i64>,
        force: bool,
    ) -> AppResult<BatchOutcome> {
        let mut outcome = BatchOutcome::default();
        if pages.is_empty() {
            return Ok(outcome);
        }
        let gv = memory::glossary_version(&self.db, pages[0].manga_id)?;
        let mut work: Vec<(PageCtx, Vec<RegionRow>, String)> = vec![];
        let mut inputs: Vec<PageInput> = vec![];
        let mut memory_hints = vec![];
        for p in pages {
            let regions = load_regions(&self.db, p.page_id)?;
            let key = translation_key(p, &regions, cfg, s, &gv);
            let st = page_state(&self.db, p.page_id)?;
            let pending: Vec<&RegionRow> = regions
                .iter()
                .filter(|r| !r.approved && !r.skip && !r.source().trim().is_empty())
                .filter(|r| force || st.translation_key.as_deref() != Some(key.as_str()) || r.translation.is_none())
                .collect();
            if pending.is_empty() {
                outcome.cached_pages.push(p.page_id);
                self.db.with(|c| {
                    ensure_state(c, p.page_id)?;
                    c.execute(
                        "UPDATE page_state SET translation_key = ?1, status = CASE WHEN status IN ('none','analyzed','failed') THEN 'translated' ELSE status END,
                         stage = NULL, error = NULL, updated_at = ?2 WHERE page_id = ?3",
                        params![key, now(), p.page_id],
                    )?;
                    Ok(())
                })?;
                continue;
            }
            // Exact matches in translation memory are reused without asking the model.
            let mut ask = vec![];
            for r in pending {
                if let Some(tm) = memory::lookup(&self.db, p.manga_id, r.source())? {
                    if !force {
                        self.save_translation(r, &tm, None, None, Some("high"), Some("translation memory"))?;
                        continue;
                    }
                    memory_hints.push(format!("{} → {}", r.source(), tm));
                }
                ask.push(RegionInput {
                    id: format!("p{}{}", p.idx + 1, r.rid),
                    text: r.source().to_string(),
                    kind: r.kind.clone(),
                    vertical: r.vertical,
                    uncertain: r.uncertain(),
                });
            }
            let image = if cfg.send_images && provider.capabilities().vision && !ask.is_empty() {
                match self.page_image(p) {
                    Ok(i) => Some(i),
                    Err(e) => {
                        tracing::warn!("page image not sent: {}", e.message);
                        None
                    }
                }
            } else {
                None
            };
            if !ask.is_empty() {
                inputs.push(PageInput { page_no: p.idx + 1, regions: ask, image });
            }
            work.push((p.clone(), regions, key));
        }

        if !inputs.is_empty() {
            let mut ctx = self.context(&work[0].0, s)?;
            ctx.memory_hints = memory_hints;
            let system = prompt::system_prompt(&work[0].0.source_lang, &s.translation.target_lang, &s.translation.sfx_mode);
            let effort = match s.translation.quality.as_str() {
                "fast" => "low",
                "best" => "high",
                _ => "medium",
            };
            let policy = RetryPolicy { max_retries: cfg.max_retries, base_delay: std::time::Duration::from_secs(2) };
            let res = translate::translate_batch(provider.clone(), &system, &ctx, &inputs, cfg.max_output_tokens, effort, &policy).await;
            let res = match res {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("{:?}: {}", e.kind, e.message);
                    for (p, _, _) in &work {
                        set_status(&self.db, p.page_id, "failed", Some("translate"), Some(&msg))?;
                        outcome.failed.push((p.page_id, msg.clone()));
                        self.emit_page(p);
                    }
                    return Err(AppError::new(&format!("provider_{}", serde_json::to_value(&e.kind)?.as_str().unwrap_or("error")), e.message));
                }
            };
            outcome.provider_calls = res.responses.len() as u32;
            let mut cost = 0.0;
            for r in &res.responses {
                let c = cost_from_usage(cfg, &r.usage);
                cost += c.unwrap_or(0.0);
                self.db.with(|c2| {
                    c2.execute(
                        "INSERT INTO usage_log (at, provider, model, input_tokens, output_tokens, cost_usd, job_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![now(), cfg.id, if r.model.is_empty() { &cfg.model } else { &r.model }, r.usage.input_tokens, r.usage.output_tokens, c, job_id],
                    )?;
                    Ok(())
                })?;
            }
            outcome.cost_usd = cost;
            let v = res.validated;
            memory::add_suggestions(&self.db, work[0].0.manga_id, &v.out.glossary_suggestions)?;
            let per_page_cost = cost / work.len() as f64;
            for (p, regions, key) in &work {
                let prefix = format!("p{}", p.idx + 1);
                let mut missing = 0;
                for r in regions.iter().filter(|r| !r.approved && !r.skip) {
                    let id = format!("{prefix}{}", r.rid);
                    if let Some(item) = v.out.translations.iter().find(|t| t.id == id) {
                        self.save_translation(r, &item.translation, Some(&item.category), Some(&item.speaker), Some(&item.confidence), Some(&item.note))?;
                    } else if v.missing.contains(&id) {
                        missing += 1;
                    }
                }
                self.db.with(|c| {
                    ensure_state(c, p.page_id)?;
                    let summary = if v.out.page_notes.is_empty() { None } else { Some(v.out.page_notes.clone()) };
                    if missing > 0 {
                        c.execute(
                            "UPDATE page_state SET status = 'failed', stage = 'translate', error = ?1, provider = ?2, model = ?3,
                             prompt_version = ?4, cost_usd = coalesce(cost_usd,0) + ?5, updated_at = ?6 WHERE page_id = ?7",
                            params![format!("{missing} text region(s) were not translated"), cfg.id, cfg.model, prompt::PROMPT_VERSION, per_page_cost, now(), p.page_id],
                        )?;
                    } else {
                        c.execute(
                            "UPDATE page_state SET status = 'translated', stage = NULL, error = NULL, translation_key = ?1, provider = ?2, model = ?3,
                             prompt_version = ?4, cost_usd = coalesce(cost_usd,0) + ?5, summary = coalesce(?6, summary), updated_at = ?7 WHERE page_id = ?8",
                            params![key, cfg.id, cfg.model, prompt::PROMPT_VERSION, per_page_cost, summary, now(), p.page_id],
                        )?;
                    }
                    Ok(())
                })?;
                if missing > 0 {
                    outcome.failed.push((p.page_id, format!("{missing} region(s) untranslated")));
                } else {
                    outcome.translated_pages.push(p.page_id);
                }
                self.emit_page(p);
            }
        } else {
            for (p, _, key) in &work {
                self.db.with(|c| {
                    c.execute(
                        "UPDATE page_state SET status = 'translated', stage = NULL, error = NULL, translation_key = ?1, updated_at = ?2 WHERE page_id = ?3",
                        params![key, now(), p.page_id],
                    )?;
                    Ok(())
                })?;
                outcome.translated_pages.push(p.page_id);
            }
        }
        Ok(outcome)
    }

    fn save_translation(
        &self,
        r: &RegionRow,
        text: &str,
        category: Option<&str>,
        speaker: Option<&str>,
        confidence: Option<&str>,
        note: Option<&str>,
    ) -> AppResult<()> {
        let mut history = r.history.as_array().cloned().unwrap_or_default();
        if let Some(prev) = r.translation.as_deref().filter(|p| *p != text) {
            history.push(json!({"text": prev, "at": now()}));
            if history.len() > 10 {
                history.remove(0);
            }
        }
        self.db.with(|c| {
            c.execute(
                "UPDATE region SET translation = ?1, category = coalesce(?2, category), speaker = nullif(?3, ''), confidence = ?4,
                     note = nullif(?5, ''), history = ?6, updated_at = ?7 WHERE id = ?8 AND approved = 0",
                params![text, category, speaker.unwrap_or(""), confidence, note.unwrap_or(""), serde_json::to_string(&history)?, now(), r.id],
            )?;
            Ok(())
        })
    }

    // ---------- stages 3 + 4: clean and render ----------

    pub async fn render(&self, p: &PageCtx, s: &AppSettings) -> AppResult<()> {
        let regions = load_regions(&self.db, p.page_id)?;
        let (_, iv, rv) = self.vision.versions();
        let keep_sfx = s.translation.sfx_mode != "translate";
        let to_draw: Vec<&RegionRow> = regions
            .iter()
            .filter(|r| !r.skip)
            .filter(|r| r.translation.as_deref().map(|t| !t.trim().is_empty()).unwrap_or(false))
            .filter(|r| !(keep_sfx && r.category.as_deref() == Some("sfx") && !r.approved))
            .collect();
        let st = page_state(&self.db, p.page_id)?;
        if to_draw.is_empty() {
            self.db.with(|c| {
                ensure_state(c, p.page_id)?;
                c.execute(
                    "UPDATE page_state SET status = 'done', stage = NULL, error = NULL, rendered_path = NULL, render_key = NULL,
                     notes = ?1, updated_at = ?2 WHERE page_id = ?3",
                    params![if regions.is_empty() { "[{\"kind\":\"no_text\"}]" } else { "[]" }, now(), p.page_id],
                )?;
                Ok(())
            })?;
            return Ok(());
        }
        let src = self.source_file(p)?;
        let dir = self.paths.derived_dir(&p.sha256);

        let clean_regions: Vec<Value> = to_draw
            .iter()
            .map(|r| json!({"rid": r.rid, "kind": r.kind, "bbox": r.bbox, "bubble": r.bubble, "parts": r.parts}))
            .collect();
        let clean_key = sha256_str(&[&p.sha256, &iv, &s.render.inpaint, &serde_json::to_string(&clean_regions)?]);
        let clean_path = dir.join(format!("clean-{}.png", &clean_key[..16]));
        let mut notes: Vec<Value> = vec![];
        if !clean_path.exists() {
            let res = self.vision.clean(&src, &clean_path, json!(clean_regions), &s.render.inpaint, &s.ocr.device).await?;
            notes.extend(res.notes);
        } else if st.clean_key.as_deref() == Some(clean_key.as_str()) {
            notes.extend(st.notes.as_array().cloned().unwrap_or_default().into_iter().filter(|n| n["stage"] != "render"));
        }

        let render_regions: Vec<Value> = to_draw
            .iter()
            .map(|r| {
                let mut v = json!({
                    "rid": r.rid, "kind": r.kind, "bbox": r.bbox, "bubble": r.bubble,
                    "text": r.translation, "category": r.category,
                });
                if let Some(obj) = r.layout.as_object() {
                    for k in ["font", "font_size", "align", "dx", "dy", "box"] {
                        if let Some(val) = obj.get(k).filter(|v| !v.is_null()) {
                            v[k] = val.clone();
                        }
                    }
                }
                v
            })
            .collect();
        let settings_json = serde_json::to_value(&s.render)?;
        let render_key = sha256_str(&[&clean_key, &rv, &settings_json.to_string(), &serde_json::to_string(&render_regions)?]);
        let out = dir.join(format!("tr-{}.png", &render_key[..16]));
        if !out.exists() {
            let res = self.vision.render(&clean_path, &out, json!(render_regions), settings_json).await?;
            notes.extend(res.notes.into_iter().map(|mut n| {
                n["stage"] = json!("render");
                n
            }));
        } else if st.render_key.as_deref() == Some(render_key.as_str()) {
            notes = st.notes.as_array().cloned().unwrap_or_default();
        }
        self.db.with(|c| {
            ensure_state(c, p.page_id)?;
            c.execute(
                "UPDATE page_state SET status = 'done', stage = NULL, error = NULL, clean_key = ?1, clean_path = ?2,
                 prev_rendered_path = CASE WHEN rendered_path IS NOT NULL AND rendered_path != ?4 THEN rendered_path ELSE prev_rendered_path END,
                 render_key = ?3, rendered_path = ?4, notes = ?5, updated_at = ?6 WHERE page_id = ?7",
                params![
                    clean_key,
                    clean_path.to_string_lossy(),
                    render_key,
                    out.to_string_lossy(),
                    serde_json::to_string(&notes)?,
                    now(),
                    p.page_id
                ],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    /// Run all stages for a set of pages (one provider request per batch).
    pub async fn process_batch(
        &self,
        pages: &[PageCtx],
        s: &AppSettings,
        cfg: &ProviderConfig,
        provider: Arc<dyn TranslationProvider>,
        job_id: Option<i64>,
        force: bool,
    ) -> AppResult<BatchOutcome> {
        let mut ready = vec![];
        let mut outcome = BatchOutcome::default();
        for p in pages {
            match self.analyze(p, s, false).await {
                Ok(()) => ready.push(p.clone()),
                Err(e) => {
                    set_status(&self.db, p.page_id, "failed", Some("analyze"), Some(&e.message))?;
                    outcome.failed.push((p.page_id, e.message.clone()));
                    self.emit_page(p);
                    if matches!(e.code.as_str(), "engine_missing" | "model_missing" | "engine_start") {
                        return Err(e);
                    }
                }
            }
        }
        let t = self.translate(&ready, s, cfg, provider, job_id, force).await;
        let t = match t {
            Ok(t) => t,
            Err(e) => {
                outcome.failed.extend(ready.iter().map(|p| (p.page_id, e.message.clone())));
                return Err(e);
            }
        };
        outcome.cost_usd = t.cost_usd;
        outcome.provider_calls = t.provider_calls;
        outcome.failed.extend(t.failed.iter().cloned());
        for p in &ready {
            if t.failed.iter().any(|(id, _)| *id == p.page_id) {
                continue;
            }
            match self.render(p, s).await {
                Ok(()) => {
                    if t.cached_pages.contains(&p.page_id) {
                        outcome.cached_pages.push(p.page_id);
                    } else {
                        outcome.translated_pages.push(p.page_id);
                    }
                }
                Err(e) => {
                    set_status(&self.db, p.page_id, "failed", Some("render"), Some(&e.message))?;
                    outcome.failed.push((p.page_id, e.message.clone()));
                }
            }
            self.emit_page(p);
        }
        Ok(outcome)
    }
}

fn iou(a: [i64; 4], b: [i64; 4]) -> f64 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0) as f64;
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0) as f64;
    let inter = w * h;
    let area = |x: [i64; 4]| ((x[2] - x[0]).max(0) * (x[3] - x[1]).max(0)) as f64;
    let u = area(a) + area(b) - inter;
    if u > 0.0 { inter / u } else { 0.0 }
}
