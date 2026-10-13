//! Translation editor operations: fix OCR, edit/approve translations, adjust
//! layout, retry single regions/pages, restore the original.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::db::now;
use crate::error::{AppError, AppResult};
use crate::pipeline::{load_page, load_regions, page_state, Engine, PageStateRow, RegionRow};
use crate::providers::TranslationProvider;
use crate::settings::{AppSettings, ProviderConfig};
use crate::translate::memory;

#[derive(Debug, Deserialize, Default)]
pub struct RegionEdit {
    pub source_text: Option<String>,
    pub translation: Option<String>,
    pub layout: Option<Value>,
    pub skip: Option<bool>,
    pub approved: Option<bool>,
    pub category: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EditorPage {
    pub page_id: i64,
    pub chapter_id: i64,
    pub manga_id: i64,
    pub idx: i64,
    pub width: i64,
    pub height: i64,
    pub state: PageStateRow,
    pub regions: Vec<RegionRow>,
    pub has_previous_render: bool,
}

pub fn editor_page(engine: &Engine, page_id: i64) -> AppResult<EditorPage> {
    let p = load_page(&engine.db, page_id, true)?;
    let (width, height): (i64, i64) =
        engine.db.with(|c| Ok(c.query_row("SELECT width, height FROM page WHERE id = ?1", [page_id], |r| Ok((r.get(0)?, r.get(1)?)))?))?;
    let state = page_state(&engine.db, page_id)?;
    let has_prev: Option<String> =
        engine.db.with(|c| Ok(c.query_row("SELECT prev_rendered_path FROM page_state WHERE page_id = ?1", [page_id], |r| r.get(0)).unwrap_or(None)))?;
    Ok(EditorPage {
        page_id,
        chapter_id: p.chapter_id,
        manga_id: p.manga_id,
        idx: p.idx,
        width,
        height,
        state,
        regions: load_regions(&engine.db, page_id)?,
        has_previous_render: has_prev.map(|p| std::path::Path::new(&p).exists()).unwrap_or(false),
    })
}

fn region(engine: &Engine, region_id: i64) -> AppResult<RegionRow> {
    let page_id: i64 = engine.db.with(|c| Ok(c.query_row("SELECT page_id FROM region WHERE id = ?1", [region_id], |r| r.get(0))?))?;
    load_regions(&engine.db, page_id)?
        .into_iter()
        .find(|r| r.id == region_id)
        .ok_or_else(|| AppError::not_found("region"))
}

/// Apply a user edit. Editing the translation approves it and stores it in the
/// series translation memory, so later retranslation never overwrites it.
pub fn update_region(engine: &Engine, region_id: i64, e: RegionEdit) -> AppResult<RegionRow> {
    let r = region(engine, region_id)?;
    let manga_id: i64 = engine.db.with(|c| {
        Ok(c.query_row(
            "SELECT ch.manga_id FROM page p JOIN chapter ch ON ch.id = p.chapter_id WHERE p.id = ?1",
            [r.page_id],
            |x| x.get(0),
        )?)
    })?;
    let ts = now();
    engine.db.with_tx(|tx| {
        if let Some(src) = &e.source_text {
            let v = if src.trim().is_empty() || src.trim() == r.ocr_text.trim() { None } else { Some(src.trim()) };
            tx.execute("UPDATE region SET source_text = ?1, updated_at = ?2 WHERE id = ?3", params![v, ts, region_id])?;
        }
        if let Some(t) = &e.translation {
            let mut history = r.history.as_array().cloned().unwrap_or_default();
            if let Some(prev) = r.translation.as_deref().filter(|p| p.trim() != t.trim()) {
                history.push(json!({"text": prev, "at": ts}));
            }
            tx.execute(
                "UPDATE region SET translation = ?1, approved = 1, confidence = 'high', history = ?2, updated_at = ?3 WHERE id = ?4",
                params![t.trim(), serde_json::to_string(&history)?, ts, region_id],
            )?;
        }
        if let Some(l) = &e.layout {
            let mut layout = r.layout.clone();
            if let (Some(dst), Some(src)) = (layout.as_object_mut(), l.as_object()) {
                for (k, v) in src {
                    if v.is_null() {
                        dst.remove(k);
                    } else {
                        dst.insert(k.clone(), v.clone());
                    }
                }
            }
            tx.execute("UPDATE region SET layout = ?1, updated_at = ?2 WHERE id = ?3", params![layout.to_string(), ts, region_id])?;
        }
        if let Some(s) = e.skip {
            tx.execute("UPDATE region SET skip = ?1, updated_at = ?2 WHERE id = ?3", params![s, ts, region_id])?;
        }
        if let Some(a) = e.approved {
            tx.execute("UPDATE region SET approved = ?1, updated_at = ?2 WHERE id = ?3", params![a, ts, region_id])?;
        }
        if let Some(c) = &e.category {
            tx.execute("UPDATE region SET category = ?1, updated_at = ?2 WHERE id = ?3", params![c, ts, region_id])?;
        }
        Ok(())
    })?;
    let updated = region(engine, region_id)?;
    if updated.approved {
        if let Some(t) = updated.translation.as_deref() {
            memory::remember(&engine.db, manga_id, updated.source(), t, "approved_edit")?;
        }
    }
    Ok(updated)
}

/// Add a region the detector missed (user-drawn box), with OCR.
pub async fn add_region(engine: &Engine, page_id: i64, bbox: [i64; 4], s: &AppSettings) -> AppResult<RegionRow> {
    let p = load_page(&engine.db, page_id, true)?;
    let src = engine.source_file(&p)?;
    let (text, conf) = engine.vision.ocr_region(&src, bbox, &s.ocr.device).await?;
    let existing = load_regions(&engine.db, page_id)?;
    let n = existing.iter().filter_map(|r| r.rid.trim_start_matches('r').parse::<i64>().ok()).max().unwrap_or(0) + 1;
    let order = existing.iter().map(|r| r.order_idx).max().unwrap_or(0) + 1;
    let vertical = (bbox[3] - bbox[1]) > (bbox[2] - bbox[0]);
    engine.db.with(|c| {
        c.execute(
            "INSERT INTO region (page_id, rid, order_idx, kind, bbox, parts, vertical, ocr_text, ocr_confidence, updated_at)
             VALUES (?1, ?2, ?3, 'free', ?4, ?5, ?6, ?7, ?8, ?9)",
            params![page_id, format!("r{n}"), order, serde_json::to_string(&bbox)?, serde_json::to_string(&vec![bbox])?, vertical, text, conf, now()],
        )?;
        Ok(())
    })?;
    let id: i64 = engine.db.with(|c| Ok(c.query_row("SELECT max(id) FROM region WHERE page_id = ?1", [page_id], |r| r.get(0))?))?;
    region(engine, id)
}

pub fn delete_region(engine: &Engine, region_id: i64) -> AppResult<()> {
    engine.db.with(|c| {
        c.execute("DELETE FROM region WHERE id = ?1", [region_id])?;
        Ok(())
    })
}

/// Move a region one step earlier/later in reading order.
pub fn move_region(engine: &Engine, region_id: i64, delta: i64) -> AppResult<()> {
    let r = region(engine, region_id)?;
    let mut regs = load_regions(&engine.db, r.page_id)?;
    let pos = regs.iter().position(|x| x.id == region_id).unwrap_or(0) as i64;
    let to = (pos + delta).clamp(0, regs.len() as i64 - 1) as usize;
    let item = regs.remove(pos as usize);
    regs.insert(to, item);
    engine.db.with_tx(|tx| {
        for (i, x) in regs.iter().enumerate() {
            tx.execute("UPDATE region SET order_idx = ?1 WHERE id = ?2", params![i as i64 + 1, x.id])?;
        }
        Ok(())
    })
}

pub async fn retry_region_ocr(engine: &Engine, region_id: i64, s: &AppSettings) -> AppResult<RegionRow> {
    let r = region(engine, region_id)?;
    let p = load_page(&engine.db, r.page_id, true)?;
    let src = engine.source_file(&p)?;
    let (text, conf) = engine.vision.ocr_region(&src, r.bbox, &s.ocr.device).await?;
    engine.db.with(|c| {
        c.execute(
            "UPDATE region SET ocr_text = ?1, ocr_confidence = ?2, source_text = NULL, updated_at = ?3 WHERE id = ?4",
            params![text, conf, now(), region_id],
        )?;
        Ok(())
    })?;
    region(engine, region_id)
}

/// Retranslate one region with the page's context. Clears approval for that region only.
pub async fn retranslate_region(
    engine: &Engine,
    region_id: i64,
    s: &AppSettings,
    cfg: &ProviderConfig,
    provider: Arc<dyn TranslationProvider>,
) -> AppResult<RegionRow> {
    let r = region(engine, region_id)?;
    engine.db.with(|c| {
        c.execute("UPDATE region SET approved = 0, translation = NULL WHERE id = ?1", [region_id])?;
        Ok(())
    })?;
    let p = load_page(&engine.db, r.page_id, s.reader.direction != "ltr")?;
    if let Err(e) = engine.translate(&[p], s, cfg, provider, None, false).await {
        // put the old text back so the page is not left half-empty
        engine.db.with(|c| {
            c.execute("UPDATE region SET translation = ?1, approved = ?2 WHERE id = ?3", params![r.translation, r.approved, region_id])?;
            Ok(())
        })?;
        return Err(e);
    }
    region(engine, region_id)
}

pub async fn rerender_page(engine: &Engine, page_id: i64, s: &AppSettings) -> AppResult<PageStateRow> {
    let p = load_page(&engine.db, page_id, s.reader.direction != "ltr")?;
    engine.render(&p, s).await?;
    page_state(&engine.db, page_id)
}

/// Show the original again: drop the rendered version but keep regions and translations.
pub fn restore_original(engine: &Engine, page_id: i64) -> AppResult<()> {
    engine.db.with(|c| {
        c.execute(
            "UPDATE page_state SET status = CASE WHEN analysis_key IS NULL THEN 'none' ELSE 'translated' END,
             prev_rendered_path = coalesce(rendered_path, prev_rendered_path), rendered_path = NULL, render_key = NULL, updated_at = ?1
             WHERE page_id = ?2",
            params![now(), page_id],
        )?;
        Ok(())
    })
}

/// Forget everything derived for a page (regions included, except approved ones).
pub fn reset_page(engine: &Engine, page_id: i64) -> AppResult<()> {
    engine.db.with_tx(|tx| {
        tx.execute("DELETE FROM region WHERE page_id = ?1 AND approved = 0", [page_id])?;
        tx.execute(
            "UPDATE page_state SET status = 'none', stage = NULL, error = NULL, analysis_key = NULL, translation_key = NULL,
             clean_key = NULL, render_key = NULL, rendered_path = NULL, updated_at = ?1 WHERE page_id = ?2",
            params![now(), page_id],
        )?;
        Ok(())
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::assets::Paths;
    use crate::db::Db;
    use crate::library::import::import_path;
    use crate::pipeline::NoEvents;
    use crate::providers::test_support::MockProvider;
    use crate::providers::ProviderError;
    use crate::worker::fake::FakeVision;

    pub(crate) fn setup(texts: &[&str]) -> (tempfile::TempDir, Engine, Arc<FakeVision>, i64) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("lib/Series/ch1");
        std::fs::create_dir_all(&root).unwrap();
        for i in 1..=3u8 {
            let mut img = image::RgbImage::new(100, 100);
            img.pixels_mut().for_each(|p| *p = image::Rgb([i * 40, 200, 255]));
            img.save(root.join(format!("{i}.png"))).unwrap();
        }
        let db = Db::open_in_memory().unwrap();
        import_path(&db, &d.path().join("lib/Series")).unwrap();
        let paths = Paths::new(d.path().join("app")).unwrap();
        let fake = Arc::new(FakeVision::new(texts));
        let engine = Engine::new(db, paths, fake.clone(), Arc::new(NoEvents));
        let ch: i64 = engine.db.with(|c| Ok(c.query_row("SELECT id FROM chapter", [], |r| r.get(0))?)).unwrap();
        (d, engine, fake, ch)
    }

    pub(crate) fn tr_json(ids: &[(&str, &str)]) -> Result<String, ProviderError> {
        let items: Vec<Value> = ids
            .iter()
            .map(|(id, t)| json!({"id": id, "translation": t, "category": "dialogue", "speaker": "", "confidence": "high", "note": ""}))
            .collect();
        Ok(json!({"translations": items, "glossary_suggestions": [], "page_notes": "summary"}).to_string())
    }

    pub(crate) fn cfg() -> ProviderConfig {
        ProviderConfig { id: "mock".into(), kind: "ollama".into(), model: "mock".into(), max_retries: 1, ..Default::default() }
    }

    #[tokio::test]
    async fn edit_is_approved_remembered_and_survives_retranslation() {
        let (_d, engine, _fake, _ch) = setup(&["こんにちは", "さようなら"]);
        let mut s = AppSettings::default();
        s.translation.pages_per_request = 1;
        let page_id: i64 = engine.db.with(|c| Ok(c.query_row("SELECT id FROM page WHERE idx = 0", [], |r| r.get(0))?)).unwrap();
        let p = load_page(&engine.db, page_id, true).unwrap();
        let mock = MockProvider::new(vec![tr_json(&[("p1r1", "שלום"), ("p1r2", "להתראות")]), tr_json(&[("p1r2", "ביי")])]);
        engine.process_batch(std::slice::from_ref(&p), &s, &cfg(), mock.clone(), None, false).await.unwrap();
        let regs = load_regions(&engine.db, page_id).unwrap();
        assert_eq!(regs[0].translation.as_deref(), Some("שלום"));

        // user edits r1
        let edited = update_region(&engine, regs[0].id, RegionEdit { translation: Some("היי!".into()), ..Default::default() }).unwrap();
        assert!(edited.approved);
        assert_eq!(memory::lookup(&engine.db, p.manga_id, "こんにちは").unwrap().as_deref(), Some("היי!"));

        // forced retranslation of the page must not touch the approved region
        engine.process_batch(std::slice::from_ref(&p), &s, &cfg(), mock.clone(), None, true).await.unwrap();
        let regs = load_regions(&engine.db, page_id).unwrap();
        assert_eq!(regs[0].translation.as_deref(), Some("היי!"));
        assert_eq!(regs[1].translation.as_deref(), Some("ביי"));
        assert!(!mock.requests.lock()[1].user.contains("[p1r1]"));
        // history kept
        assert_eq!(regs[1].history.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn non_japanese_pages_are_read_from_the_image() {
        let (_d, engine, fake, _ch) = setup(&["ignored", "ignored"]);
        engine.db.with(|c| { c.execute("UPDATE manga SET source_lang = 'en'", [])?; Ok(()) }).unwrap();
        let s = AppSettings::default();
        let page_id: i64 = engine.db.with(|c| Ok(c.query_row("SELECT id FROM page WHERE idx = 0", [], |r| r.get(0))?)).unwrap();
        let p = load_page(&engine.db, page_id, true).unwrap();
        let reply = json!({"translations": [
            {"id": "p1r1", "source": "Where am I?", "translation": "איפה אני?", "category": "dialogue", "speaker": "", "confidence": "high", "note": ""},
            {"id": "p1r2", "source": "", "translation": "", "category": "other", "speaker": "", "confidence": "low", "note": "no text"}
        ], "glossary_suggestions": [], "page_notes": ""}).to_string();
        let mock = MockProvider::new(vec![Ok(reply)]);
        let mut c = cfg();
        c.send_images = true;
        engine.process_batch(&[p], &s, &c, mock.clone(), None, false).await.unwrap();
        assert!(fake.calls.lock().iter().any(|x| x == "analyze:ocr=false"), "no Japanese OCR for English pages");
        let req = mock.requests.lock()[0].clone();
        assert!(req.user.contains("no OCR: read the text in the page image at x"));
        assert_eq!(req.images.len(), 1, "the page image is required");
        let regs = load_regions(&engine.db, page_id).unwrap();
        assert_eq!(regs[0].ocr_text, "Where am I?", "the text the model read is kept for the editor");
        assert_eq!(regs[0].translation.as_deref(), Some("איפה אני?"));
        assert_eq!(page_state(&engine.db, page_id).unwrap().status, "done");
        // reopening must not pay again even though the read text was stored
        let p = load_page(&engine.db, page_id, true).unwrap();
        engine.process_batch(&[p], &s, &c, mock.clone(), None, false).await.unwrap();
        assert_eq!(mock.requests.lock().len(), 1, "cached: no second request");
    }

    #[tokio::test]
    async fn reading_from_image_without_images_fails_honestly() {
        let (_d, engine, _fake, _ch) = setup(&["x"]);
        engine.db.with(|c| { c.execute("UPDATE manga SET source_lang = 'en'", [])?; Ok(()) }).unwrap();
        let page_id: i64 = engine.db.with(|c| Ok(c.query_row("SELECT id FROM page WHERE idx = 0", [], |r| r.get(0))?)).unwrap();
        let p = load_page(&engine.db, page_id, true).unwrap();
        let mock = MockProvider::new(vec![]);
        let mut c = cfg();
        c.send_images = false;
        let out = engine.process_batch(&[p], &AppSettings::default(), &c, mock.clone(), None, false).await.unwrap();
        assert_eq!(out.failed.len(), 1);
        assert!(mock.requests.lock().is_empty(), "nothing sent");
        let st = page_state(&engine.db, page_id).unwrap();
        assert_eq!(st.status, "failed");
        assert!(st.error.unwrap().contains("send page images"));
    }

    #[tokio::test]
    async fn switching_sfx_mode_redraws_without_new_requests() {
        let (_d, engine, fake, _ch) = setup(&["ドーン", "やあ"]);
        let mut s = AppSettings::default();
        s.translation.sfx_mode = "keep".into();
        let page_id: i64 = engine.db.with(|c| Ok(c.query_row("SELECT id FROM page WHERE idx = 0", [], |r| r.get(0))?)).unwrap();
        let p = load_page(&engine.db, page_id, true).unwrap();
        let reply = json!({"translations": [
            {"id": "p1r1", "source": "", "translation": "בום", "category": "sfx", "speaker": "", "confidence": "high", "note": ""},
            {"id": "p1r2", "source": "", "translation": "היי", "category": "dialogue", "speaker": "", "confidence": "high", "note": ""}
        ], "glossary_suggestions": [], "page_notes": ""}).to_string();
        let mock = MockProvider::new(vec![Ok(reply)]);
        engine.process_batch(&[p.clone()], &s, &cfg(), mock.clone(), None, false).await.unwrap();
        assert!(fake.calls.lock().contains(&"render:1".to_string()), "keep: SFX not drawn");
        s.translation.sfx_mode = "translate".into();
        engine.process_batch(&[p], &s, &cfg(), mock.clone(), None, false).await.unwrap();
        assert_eq!(mock.requests.lock().len(), 1, "no new translation request");
        assert!(fake.calls.lock().contains(&"render:2".to_string()), "translate: SFX drawn");
    }

    #[tokio::test]
    async fn restore_original_and_rerender() {
        let (_d, engine, _fake, _ch) = setup(&["あ"]);
        let s = AppSettings::default();
        let page_id: i64 = engine.db.with(|c| Ok(c.query_row("SELECT id FROM page WHERE idx = 0", [], |r| r.get(0))?)).unwrap();
        let p = load_page(&engine.db, page_id, true).unwrap();
        engine.process_batch(&[p], &s, &cfg(), MockProvider::new(vec![tr_json(&[("p1r1", "א")])]), None, false).await.unwrap();
        assert!(page_state(&engine.db, page_id).unwrap().rendered_path.is_some());
        restore_original(&engine, page_id).unwrap();
        let st = page_state(&engine.db, page_id).unwrap();
        assert!(st.rendered_path.is_none());
        assert_eq!(st.status, "translated");
        let st = rerender_page(&engine, page_id, &s).await.unwrap();
        assert_eq!(st.status, "done");
    }

    #[tokio::test]
    async fn region_ops() {
        let (_d, engine, _fake, _ch) = setup(&["あ", "い"]);
        let s = AppSettings::default();
        let page_id: i64 = engine.db.with(|c| Ok(c.query_row("SELECT id FROM page WHERE idx = 0", [], |r| r.get(0))?)).unwrap();
        let p = load_page(&engine.db, page_id, true).unwrap();
        engine.analyze(&p, &s, false).await.unwrap();
        let added = add_region(&engine, page_id, [1, 1, 30, 30], &s).await.unwrap();
        assert_eq!(added.rid, "r3");
        assert_eq!(added.ocr_text, "再OCR");
        move_region(&engine, added.id, -2).unwrap();
        assert_eq!(load_regions(&engine.db, page_id).unwrap()[0].id, added.id);
        let fixed = update_region(&engine, added.id, RegionEdit { source_text: Some("直した".into()), ..Default::default() }).unwrap();
        assert_eq!(fixed.source(), "直した");
        let again = retry_region_ocr(&engine, added.id, &s).await.unwrap();
        assert_eq!(again.source(), "再OCR");
        delete_region(&engine, added.id).unwrap();
        assert_eq!(load_regions(&engine.db, page_id).unwrap().len(), 2);
    }
}
