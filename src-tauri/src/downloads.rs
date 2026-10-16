//! Persistent download queue: fetches chapters from a source adapter into the
//! app's library folder, then registers them like any local chapter.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Notify;

use crate::assets::{atomic_write, ensure_space, Paths};
use crate::chapter_pick::{self, Held, Pick, Plan, Removal};
use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::library::import::add_folder_chapter;
use crate::pipeline::EventSink;
use crate::sources::{RemoteChapter, RemoteManga, SourceAdapter};

#[derive(Debug, Clone, Serialize)]
pub struct Download {
    pub id: i64,
    pub source_id: String,
    pub manga_ref: String,
    pub manga_title: String,
    pub chapter_ref: String,
    pub chapter_title: String,
    pub chapter_number: Option<f64>,
    pub manga_id: Option<i64>,
    pub chapter_id: Option<i64>,
    pub status: String,
    pub progress: i64,
    pub total: i64,
    pub error: Option<String>,
    pub updated_at: String,
    pub scanlator: Option<String>,
    /// Picked by a bulk download (one version per chapter) rather than chosen by hand.
    pub auto: bool,
}

const SELECT: &str = "SELECT id, source_id, manga_ref, manga_title, chapter_ref, chapter_title, chapter_number, manga_id, chapter_id,
    status, progress, total, error, updated_at, scanlator, auto FROM download";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Download> {
    Ok(Download {
        id: r.get(0)?,
        source_id: r.get(1)?,
        manga_ref: r.get(2)?,
        manga_title: r.get(3)?,
        chapter_ref: r.get(4)?,
        chapter_title: r.get(5)?,
        chapter_number: r.get(6)?,
        manga_id: r.get(7)?,
        chapter_id: r.get(8)?,
        status: r.get(9)?,
        progress: r.get(10)?,
        total: r.get(11)?,
        error: r.get(12)?,
        updated_at: r.get(13)?,
        scanlator: r.get(14)?,
        auto: r.get(15)?,
    })
}

pub fn list(db: &Db) -> AppResult<Vec<Download>> {
    db.with(|c| {
        let mut s = c.prepare(&format!(
            "{SELECT} ORDER BY CASE status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 WHEN 'failed' THEN 2 ELSE 3 END, id DESC LIMIT 500"
        ))?;
        let v = s.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })
}

pub fn get(db: &Db, id: i64) -> AppResult<Download> {
    db.with(|c| Ok(c.query_row(&format!("{SELECT} WHERE id = ?1"), [id], row)?))
}

/// Find or create the local manga entry for a remote manga.
pub fn local_manga(db: &Db, m: &RemoteManga) -> AppResult<i64> {
    let ts = now();
    db.with_tx(|tx| {
        if let Some(id) = tx
            .query_row("SELECT id FROM manga WHERE source_id = ?1 AND source_ref = ?2", params![m.source_id, m.manga_ref], |r| r.get(0))
            .optional()?
        {
            return Ok(id);
        }
        tx.execute(
            "INSERT INTO manga (title, description, author, artist, status, genres, source_kind, source_id, source_ref, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'source', ?7, ?8, ?9, ?9)",
            params![m.title, m.description, m.author, m.artist, m.status, serde_json::to_string(&m.genres)?, m.source_id, m.manga_ref, ts],
        )?;
        Ok(tx.last_insert_rowid())
    })
}

/// Queue chapters exactly as chosen. Chapters already downloaded are skipped;
/// failed/cancelled/skipped ones are re-queued.
pub fn enqueue(db: &Db, m: &RemoteManga, chapters: &[RemoteChapter]) -> AppResult<(usize, usize)> {
    let items: Vec<Pick> = chapters.iter().map(|c| Pick { chapter: c.clone(), alternates: vec![] }).collect();
    enqueue_picks(db, m, &items, false)
}

/// Queue picked versions. `auto` marks them as chosen by the bulk download, which lets the
/// downloader switch to an alternate version when the pick turns out to be a placeholder.
pub fn enqueue_picks(db: &Db, m: &RemoteManga, picks: &[Pick], auto: bool) -> AppResult<(usize, usize)> {
    let manga_id = local_manga(db, m)?;
    let ts = now();
    db.with_tx(|tx| {
        let (mut added, mut skipped) = (0, 0);
        for Pick { chapter: ch, alternates } in picks {
            let alts = serde_json::to_string(alternates)?;
            let existing: Option<(i64, String, Option<i64>)> = tx
                .query_row(
                    "SELECT id, status, chapter_id FROM download WHERE source_id = ?1 AND chapter_ref = ?2",
                    params![m.source_id, ch.chapter_ref],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            match existing {
                Some((_, st, _)) if st == "queued" || st == "running" => skipped += 1,
                Some((_, st, Some(_))) if st == "done" => skipped += 1,
                Some((id, _, _)) => {
                    tx.execute(
                        "UPDATE download SET status = 'queued', error = NULL, progress = 0, updated_at = ?1, auto = ?2, alternates = ?3,
                                scanlator = coalesce(?4, scanlator), manga_id = ?5 WHERE id = ?6",
                        params![ts, auto, alts, ch.scanlator, manga_id, id],
                    )?;
                    added += 1;
                }
                None => {
                    tx.execute(
                        "INSERT INTO download (source_id, manga_ref, manga_title, chapter_ref, chapter_title, chapter_number, manga_id,
                                               status, total, created_at, updated_at, scanlator, auto, alternates)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, ?9, ?9, ?10, ?11, ?12)",
                        params![m.source_id, m.manga_ref, m.title, ch.chapter_ref, ch.title, ch.number, manga_id, ch.page_count.unwrap_or(0), ts,
                                ch.scanlator, auto, alts],
                    )?;
                    added += 1;
                }
            }
        }
        Ok((added, skipped))
    })
}

/// The stored preferred scanlation group of a remote manga (if it is in the library).
pub fn stored_preference(db: &Db, m: &RemoteManga) -> AppResult<Option<String>> {
    db.with(|c| {
        Ok(c.query_row(
            "SELECT preferred_scanlator FROM manga WHERE source_id = ?1 AND source_ref = ?2",
            params![m.source_id, m.manga_ref],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
    })
}

pub fn set_preference(db: &Db, manga_id: i64, scanlator: Option<&str>) -> AppResult<()> {
    let s = scanlator.map(str::trim).filter(|s| !s.is_empty());
    db.with(|c| {
        c.execute("UPDATE manga SET preferred_scanlator = ?1 WHERE id = ?2", params![s, manga_id])?;
        Ok(())
    })
}

/// Fill in the scanlation group of downloads made before it was recorded.
pub fn backfill_scanlators(db: &Db, source_id: &str, chapters: &[RemoteChapter]) -> AppResult<()> {
    db.with_tx(|tx| {
        let mut s = tx.prepare("UPDATE download SET scanlator = ?1 WHERE source_id = ?2 AND chapter_ref = ?3 AND scanlator IS NULL")?;
        for c in chapters.iter().filter(|c| c.scanlator.is_some()) {
            s.execute(params![c.scanlator, source_id, c.chapter_ref])?;
        }
        Ok(())
    })
}

/// Pick one version per chapter for a bulk download of `chapters` (the source's listing,
/// or a range of it). `preferred` is the user's chosen group; without one, the group most
/// of this manga's downloads came from is preferred.
pub fn plan_bulk(db: &Db, m: &RemoteManga, chapters: &[RemoteChapter], preferred: Option<&str>) -> AppResult<Plan> {
    type Row = (String, String, Option<f64>, String, Option<i64>, Option<String>);
    let by_ref: HashMap<&str, &RemoteChapter> = chapters.iter().map(|c| (c.chapter_ref.as_str(), c)).collect();
    let rows: Vec<Row> = db.with(|c| {
        let mut s = c.prepare(
            "SELECT chapter_ref, chapter_title, chapter_number, status, chapter_id, scanlator FROM download WHERE source_id = ?1 AND manga_ref = ?2",
        )?;
        let v = s
            .query_map(params![m.source_id, m.manga_ref], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })?;
    let held = |r: &Row| r.3 == "queued" || r.3 == "running" || (r.3 == "done" && r.4.is_some());
    let have: HashSet<String> = rows.iter().filter(|r| held(r)).map(|r| chapter_pick::chapter_key(r.2, &r.1)).collect();
    let preferred = preferred.map(str::trim).filter(|p| !p.is_empty()).map(String::from).or_else(|| {
        chapter_pick::most_common_group(
            rows.iter()
                .filter(|r| held(r))
                .filter_map(|r| r.5.as_deref().or_else(|| by_ref.get(r.0.as_str()).and_then(|c| c.scanlator.as_deref()))),
        )
    });
    // A version the downloader already found to be a placeholder counts as a 1-page one.
    let skipped: HashSet<&str> = rows.iter().filter(|r| r.3 == "skipped").map(|r| r.0.as_str()).collect();
    let chapters: Vec<RemoteChapter> = chapters
        .iter()
        .map(|c| {
            let mut c = c.clone();
            if skipped.contains(c.chapter_ref.as_str()) {
                c.page_count = Some(1);
            }
            c
        })
        .collect();
    Ok(chapter_pick::plan(&chapters, &have, preferred.as_deref()))
}

/// Move a queued download to the front ("read now").
pub fn prioritize(db: &Db, id: i64) -> AppResult<()> {
    db.with(|c| {
        c.execute("UPDATE download SET priority = (SELECT coalesce(max(priority),0) + 1 FROM download) WHERE id = ?1", [id])?;
        Ok(())
    })
}

pub fn cancel(db: &Db, id: i64) -> AppResult<()> {
    db.with(|c| {
        c.execute(
            "UPDATE download SET status = 'cancelled', updated_at = ?1 WHERE id = ?2 AND status IN ('queued','running','failed')",
            params![now(), id],
        )?;
        Ok(())
    })
}

/// Retry a download. A skipped placeholder is then downloaded as it is (the user insisted).
pub fn retry(db: &Db, id: i64) -> AppResult<()> {
    db.with(|c| {
        c.execute(
            "UPDATE download SET status = 'queued', error = NULL, progress = 0, updated_at = ?1,
                    auto = CASE status WHEN 'skipped' THEN 0 ELSE auto END
             WHERE id = ?2 AND status IN ('failed','cancelled','skipped')",
            params![now(), id],
        )?;
        Ok(())
    })
}

pub fn clear_finished(db: &Db) -> AppResult<usize> {
    db.with(|c| Ok(c.execute("DELETE FROM download WHERE status IN ('done','cancelled','skipped')", [])?))
}

pub fn recover(db: &Db) -> AppResult<usize> {
    db.with(|c| Ok(c.execute("UPDATE download SET status = 'queued' WHERE status = 'running'", [])?))
}

fn ext_for(ct: Option<&str>, bytes: &[u8]) -> &'static str {
    match ct.unwrap_or("") {
        c if c.contains("png") => "png",
        c if c.contains("webp") => "webp",
        c if c.contains("gif") => "gif",
        c if c.contains("jpeg") || c.contains("jpg") => "jpg",
        _ => match image::guess_format(bytes) {
            Ok(image::ImageFormat::Png) => "png",
            Ok(image::ImageFormat::WebP) => "webp",
            Ok(image::ImageFormat::Gif) => "gif",
            _ => "jpg",
        },
    }
}

pub struct DownloadRunner {
    pub db: Db,
    pub paths: Paths,
    pub events: Arc<dyn EventSink>,
    pub notify: Arc<Notify>,
    /// Builds an adapter from current settings (None if sources are disabled).
    pub adapter: Arc<dyn Fn() -> AppResult<Arc<dyn SourceAdapter>> + Send + Sync>,
}

impl DownloadRunner {
    fn emit(&self, id: i64) {
        if let Ok(d) = get(&self.db, id) {
            self.events.emit("download-updated", json!(d));
        }
    }

    fn status(&self, id: i64) -> AppResult<String> {
        self.db.with(|c| Ok(c.query_row("SELECT status FROM download WHERE id = ?1", [id], |r| r.get(0))?))
    }

    fn set(&self, id: i64, status: &str, error: Option<&str>) -> AppResult<()> {
        self.db.with(|c| {
            c.execute(
                "UPDATE download SET status = ?1, error = ?2, updated_at = ?3 WHERE id = ?4",
                params![status, error, now(), id],
            )?;
            Ok(())
        })?;
        self.emit(id);
        Ok(())
    }

    pub async fn run_one(&self, id: i64) -> AppResult<()> {
        let d = get(&self.db, id)?;
        let manga_id = d.manga_id.ok_or_else(|| AppError::invalid("download has no manga"))?;
        self.set(id, "running", None)?;
        let adapter = (self.adapter)()?;
        let mut d = d;
        let mut urls = adapter.page_urls(&d.chapter_ref).await?;
        // A bulk pick that turns out to be a one-page placeholder ("read it on the
        // publisher's site") is swapped for the next version of the same chapter.
        while d.auto && chapter_pick::is_placeholder(d.chapter_number, &d.chapter_title, Some(urls.len() as i64)) {
            match self.next_alternate(id)? {
                Some(next) => {
                    d = next;
                    urls = adapter.page_urls(&d.chapter_ref).await?;
                }
                None => {
                    self.db.with(|c| {
                        c.execute("UPDATE download SET total = ?1, progress = 0 WHERE id = ?2", params![urls.len() as i64, id])?;
                        Ok(())
                    })?;
                    return self.set(id, "skipped", Some("only a one-page placeholder is available for this chapter"));
                }
            }
        }
        self.db.with(|c| {
            c.execute("UPDATE download SET total = ?1, progress = 0 WHERE id = ?2", params![urls.len() as i64, id])?;
            Ok(())
        })?;
        let manga_dir = self.paths.library_dir().join(manga_id.to_string());
        let final_dir = manga_dir.join(format!("dl-{id}"));
        let tmp_dir = manga_dir.join(format!(".dl-{id}.part"));
        std::fs::create_dir_all(&tmp_dir)?;
        ensure_space(&manga_dir, 50 * 1024 * 1024)?;
        let width = urls.len().to_string().len().max(3);
        for (i, url) in urls.iter().enumerate() {
            if self.status(id)? != "running" {
                let _ = std::fs::remove_dir_all(&tmp_dir);
                self.emit(id);
                return Ok(());
            }
            // Resume: pages already saved by an interrupted attempt are kept.
            let stem = format!("{:0width$}.", i + 1, width = width);
            let already = std::fs::read_dir(&tmp_dir)?
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with(&stem) && !e.file_name().to_string_lossy().contains(".tmp"));
            if already {
                self.db.with(|c| {
                    c.execute("UPDATE download SET progress = ?1, updated_at = ?2 WHERE id = ?3", params![i as i64 + 1, now(), id])?;
                    Ok(())
                })?;
                continue;
            }
            let mut last_err = None;
            let mut got = None;
            for attempt in 0..3u32 {
                match adapter.fetch(url).await {
                    Ok(v) => {
                        got = Some(v);
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt))).await;
                    }
                }
            }
            let (bytes, ct) = match got {
                Some(v) => v,
                None => return Err(last_err.unwrap_or_else(|| AppError::new("network", "download failed"))),
            };
            if image::guess_format(&bytes).is_err() {
                return Err(AppError::new("bad_image", format!("page {} is not an image", i + 1)));
            }
            let name = format!("{:0width$}.{}", i + 1, ext_for(ct.as_deref(), &bytes), width = width);
            atomic_write(&tmp_dir.join(name), &bytes)?;
            self.db.with(|c| {
                c.execute("UPDATE download SET progress = ?1, updated_at = ?2 WHERE id = ?3", params![i as i64 + 1, now(), id])?;
                Ok(())
            })?;
            self.emit(id);
        }
        if final_dir.exists() {
            std::fs::remove_dir_all(&final_dir)?;
        }
        std::fs::rename(&tmp_dir, &final_dir)?;
        let chapter_id = add_folder_chapter(&self.db, manga_id, &final_dir, &d.chapter_title, d.chapter_number, &d.chapter_ref)?;
        self.db.with(|c| {
            c.execute("UPDATE download SET chapter_id = ?1 WHERE id = ?2", params![chapter_id, id])?;
            Ok(())
        })?;
        self.set(id, "done", None)
    }

    /// Switch a download to its next alternate version (one not already queued on its own).
    fn next_alternate(&self, id: i64) -> AppResult<Option<Download>> {
        let switched = self.db.with_tx(|tx| {
            let (source_id, alts): (String, String) =
                tx.query_row("SELECT source_id, alternates FROM download WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let mut alts: Vec<RemoteChapter> = serde_json::from_str(&alts).unwrap_or_default();
            while !alts.is_empty() {
                let next = alts.remove(0);
                let taken: bool = tx.query_row(
                    "SELECT count(*) > 0 FROM download WHERE source_id = ?1 AND chapter_ref = ?2",
                    params![source_id, next.chapter_ref],
                    |r| r.get(0),
                )?;
                if taken {
                    continue;
                }
                tx.execute(
                    "UPDATE download SET chapter_ref = ?1, chapter_title = ?2, scanlator = ?3, alternates = ?4, total = ?5, progress = 0,
                            updated_at = ?6 WHERE id = ?7",
                    params![next.chapter_ref, next.title, next.scanlator, serde_json::to_string(&alts)?, next.page_count.unwrap_or(0), now(), id],
                )?;
                return Ok(true);
            }
            tx.execute("UPDATE download SET alternates = '[]' WHERE id = ?1", [id])?;
            Ok(false)
        })?;
        if !switched {
            return Ok(None);
        }
        tracing::info!(download = id, "placeholder chapter; trying another version");
        self.emit(id);
        Ok(Some(get(&self.db, id)?))
    }

    pub async fn run_forever(self: Arc<Self>, concurrency: usize) {
        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        loop {
            let next: Option<i64> = self
                .db
                .with(|c| Ok(c.query_row("SELECT id FROM download WHERE status = 'queued' ORDER BY priority DESC, id LIMIT 1", [], |r| r.get(0)).optional()?))
                .unwrap_or(None);
            let Some(id) = next else {
                self.notify.notified().await;
                continue;
            };
            let permit = sem.clone().acquire_owned().await.expect("semaphore");
            // mark running before spawning so the loop doesn't pick it again
            let _ = self.set(id, "running", None);
            let me = self.clone();
            tokio::spawn(async move {
                me.run_waiting_for_source(id, OFFLINE_RETRY, OFFLINE_PATIENCE).await;
                drop(permit);
                me.notify.notify_one();
            });
        }
    }

    /// Run one download. While the source server is unreachable (typically the managed
    /// Suwayomi still booting after an app restart, when interrupted downloads resume),
    /// the download waits and retries instead of failing; after `patience` it fails.
    pub async fn run_waiting_for_source(&self, id: i64, retry: std::time::Duration, patience: std::time::Duration) {
        let started = std::time::Instant::now();
        loop {
            match self.run_one(id).await {
                Ok(()) => return,
                Err(e) if e.code == "source_offline" && started.elapsed() < patience => {
                    let _ = self.set(id, "running", Some("waiting for the sources engine to start…"));
                    tokio::time::sleep(retry).await;
                    if self.status(id).ok().as_deref() != Some("running") {
                        return; // cancelled or paused while waiting
                    }
                }
                Err(e) => {
                    tracing::warn!(download = id, "download failed: {}", e.message);
                    // the partial folder is kept so a retry resumes where it stopped
                    let _ = self.set(id, "failed", Some(&e.message));
                    return;
                }
            }
        }
    }
}

// ---------------- duplicate cleanup ----------------

/// One chapter (or pending download) the cleanup would remove.
#[derive(Debug, Clone, Serialize)]
pub struct CleanupEntry {
    /// Set for downloaded chapters (removed from the library and disk).
    pub chapter_id: Option<i64>,
    /// Set for downloads not finished yet (cancelled).
    pub download_id: Option<i64>,
    pub title: String,
    pub number: Option<f64>,
    pub scanlator: Option<String>,
    pub pages: Option<i64>,
    pub status: String,
    pub reason: Removal,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupPlan {
    pub manga_id: i64,
    pub remove: Vec<CleanupEntry>,
    /// Downloaded chapters that stay.
    pub kept: usize,
    pub preferred: Option<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CleanupResult {
    pub chapters_removed: usize,
    pub downloads_cancelled: usize,
    pub bytes_freed: u64,
}

/// The folder of a chapter this app downloaded (`library/<manga>/dl-<id>`), or None
/// for anything else — imported chapters are never touched.
fn downloaded_dir(library_dir: &Path, manga_id: i64, location: &str) -> Option<std::path::PathBuf> {
    let p = Path::new(location);
    let manga_dir = library_dir.join(manga_id.to_string());
    let name = p.file_name()?.to_string_lossy();
    (p.parent() == Some(manga_dir.as_path()) && name.starts_with("dl-") && name[3..].chars().all(|c| c.is_ascii_digit()))
        .then(|| p.to_path_buf())
}

fn dir_size(p: &Path) -> u64 {
    walkdir::WalkDir::new(p).into_iter().filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
}

/// What a duplicate cleanup of one manga would remove: extra versions of the same chapter
/// (downloaded or still queued), and with `placeholders` also one-page stand-in chapters.
pub fn cleanup_plan(db: &Db, library_dir: &Path, manga_id: i64, placeholders: bool) -> AppResult<CleanupPlan> {
    struct Row {
        entry: CleanupEntry,
        held: Held,
    }
    let (stored, chapters, pending) = db.with(|c| {
        let stored: Option<String> = c.query_row("SELECT preferred_scanlator FROM manga WHERE id = ?1", [manga_id], |r| r.get(0))?;
        let mut s = c.prepare(
            "SELECT ch.id, ch.title, ch.number, ch.page_count, ch.location, ch.source_ref, d.id, d.scanlator,
                    (SELECT count(*) FROM page p JOIN page_state ps ON ps.page_id = p.id
                      WHERE p.chapter_id = ch.id AND ps.status IN ('translated','done')),
                    (SELECT count(*) FROM job j WHERE j.chapter_id = ch.id AND j.status IN ('queued','running','paused'))
             FROM chapter ch LEFT JOIN download d ON d.chapter_id = ch.id
             WHERE ch.manga_id = ?1 ORDER BY ch.sort_index",
        )?;
        let chapters = s
            .query_map([manga_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, i64>(8)? + r.get::<_, i64>(9)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut s = c.prepare(
            "SELECT id, chapter_title, chapter_number, scanlator, total, status FROM download
             WHERE manga_id = ?1 AND status IN ('queued','running','failed') ORDER BY id",
        )?;
        let pending = s
            .query_map([manga_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok((stored, chapters, pending))
    })?;

    let mut rows: Vec<Row> = vec![];
    for (id, title, number, pages, location, source_ref, dl_id, scanlator, work) in chapters {
        let Some(dir) = source_ref.and(downloaded_dir(library_dir, manga_id, &location)) else { continue };
        rows.push(Row {
            entry: CleanupEntry {
                chapter_id: Some(id),
                download_id: None,
                title: title.clone(),
                number,
                scanlator: scanlator.clone(),
                pages: Some(pages),
                status: "done".into(),
                reason: Removal::Duplicate,
                bytes: dir_size(&dir),
            },
            held: Held { number, title, scanlator, pages: Some(pages), done: true, protected: work > 0, seq: dl_id.unwrap_or(0) },
        });
    }
    for (id, title, number, scanlator, total, status) in pending {
        let pages = (total > 0).then_some(total);
        rows.push(Row {
            entry: CleanupEntry {
                chapter_id: None,
                download_id: Some(id),
                title: title.clone(),
                number,
                scanlator: scanlator.clone(),
                pages,
                status,
                reason: Removal::Duplicate,
                bytes: 0,
            },
            held: Held { number, title, scanlator, pages, done: false, protected: false, seq: id },
        });
    }
    let preferred = stored.filter(|s| !s.trim().is_empty()).or_else(|| {
        chapter_pick::most_common_group(rows.iter().filter(|r| r.held.done).filter_map(|r| r.held.scanlator.as_deref()))
    });
    let held: Vec<Held> = rows.iter().map(|r| r.held.clone()).collect();
    let verdicts = chapter_pick::cleanup(&held, preferred.as_deref(), placeholders);
    let downloaded = rows.iter().filter(|r| r.held.done).count();
    let remove: Vec<CleanupEntry> = rows
        .into_iter()
        .zip(verdicts)
        .filter_map(|(r, v)| v.map(|reason| CleanupEntry { reason, ..r.entry }))
        .collect();
    let removed_chapters = remove.iter().filter(|e| e.chapter_id.is_some()).count();
    Ok(CleanupPlan {
        manga_id,
        kept: downloaded - removed_chapters,
        preferred,
        bytes: remove.iter().map(|e| e.bytes).sum(),
        remove,
    })
}

/// Remove what the user confirmed. Only entries the cleanup still proposes are acted on,
/// so a stale confirmation can never delete a chapter that is now the best copy.
pub fn cleanup_apply(
    db: &Db,
    library_dir: &Path,
    manga_id: i64,
    placeholders: bool,
    chapter_ids: &[i64],
    download_ids: &[i64],
) -> AppResult<CleanupResult> {
    let plan = cleanup_plan(db, library_dir, manga_id, placeholders)?;
    let chapters: Vec<&CleanupEntry> = plan.remove.iter().filter(|e| e.chapter_id.is_some_and(|id| chapter_ids.contains(&id))).collect();
    let pending: Vec<&CleanupEntry> = plan.remove.iter().filter(|e| e.download_id.is_some_and(|id| download_ids.contains(&id))).collect();
    let mut dirs = vec![];
    let ts = now();
    db.with_tx(|tx| {
        for e in &chapters {
            let id = e.chapter_id.unwrap_or_default();
            let location: String = tx.query_row("SELECT location FROM chapter WHERE id = ?1", [id], |r| r.get(0))?;
            dirs.push(downloaded_dir(library_dir, manga_id, &location));
            // The download record goes too: the chapter can still be downloaded again by hand.
            tx.execute("DELETE FROM download WHERE chapter_id = ?1", [id])?;
            tx.execute("DELETE FROM chapter WHERE id = ?1 AND manga_id = ?2", params![id, manga_id])?;
        }
        for e in &pending {
            tx.execute(
                "UPDATE download SET status = 'cancelled', error = 'another version of this chapter is kept', updated_at = ?1
                 WHERE id = ?2 AND status IN ('queued','running','failed')",
                params![ts, e.download_id],
            )?;
        }
        Ok(())
    })?;
    let mut out = CleanupResult { chapters_removed: chapters.len(), downloads_cancelled: pending.len(), bytes_freed: 0 };
    for (e, dir) in chapters.iter().zip(dirs) {
        if let Some(dir) = dir.filter(|d| d.exists()) {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => out.bytes_freed += e.bytes,
                Err(err) => tracing::warn!("could not delete {}: {err}", dir.display()),
            }
        }
    }
    // Partial pages of cancelled downloads (a running one cleans up after itself).
    for e in pending.iter().filter(|e| e.status != "running") {
        let part = library_dir.join(manga_id.to_string()).join(format!(".dl-{}.part", e.download_id.unwrap_or_default()));
        let _ = std::fs::remove_dir_all(part);
    }
    Ok(out)
}

const OFFLINE_RETRY: std::time::Duration = std::time::Duration::from_secs(3);
const OFFLINE_PATIENCE: std::time::Duration = std::time::Duration::from_secs(180);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::NoEvents;
    use crate::sources::{MangaPage, SourceInfo};
    use async_trait::async_trait;

    struct FakeSource {
        fail_page: Option<usize>,
    }

    fn png() -> Vec<u8> {
        let img = image::RgbImage::new(4, 6);
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[async_trait]
    impl SourceAdapter for FakeSource {
        async fn ping(&self) -> AppResult<String> {
            Ok("fake".into())
        }
        async fn sources(&self) -> AppResult<Vec<SourceInfo>> {
            Ok(vec![])
        }
        async fn browse(&self, _: &str, _: &str, _: Option<&str>, _: u32) -> AppResult<MangaPage> {
            Ok(MangaPage { mangas: vec![], has_next_page: false })
        }
        async fn manga(&self, _: &str) -> AppResult<RemoteManga> {
            unimplemented!()
        }
        async fn chapters(&self, _: &str) -> AppResult<Vec<RemoteChapter>> {
            Ok(vec![])
        }
        async fn page_urls(&self, _: &str) -> AppResult<Vec<String>> {
            Ok((0..12).map(|i| format!("fake://{i}")).collect())
        }
        async fn fetch(&self, url: &str) -> AppResult<(Vec<u8>, Option<String>)> {
            let i: usize = url.trim_start_matches("fake://").parse().unwrap();
            if Some(i) == self.fail_page {
                return Err(AppError::new("network", "nope"));
            }
            Ok((png(), Some("image/png".into())))
        }
    }

    fn remote() -> (RemoteManga, Vec<RemoteChapter>) {
        (
            RemoteManga {
                source_id: "s1".into(),
                manga_ref: "7".into(),
                title: "Remote".into(),
                thumbnail_url: None,
                author: None,
                artist: None,
                description: None,
                genres: vec![],
                status: None,
            },
            vec![RemoteChapter { chapter_ref: "70".into(), title: "Ch 1".into(), number: Some(1.0), scanlator: None, upload_date: None, page_count: Some(12) }],
        )
    }

    fn runner(dir: &std::path::Path, fail_page: Option<usize>) -> DownloadRunner {
        let db = Db::open_in_memory().unwrap();
        DownloadRunner {
            db,
            paths: Paths::new(dir.to_path_buf()).unwrap(),
            events: Arc::new(NoEvents),
            notify: Arc::new(Notify::new()),
            adapter: Arc::new(move || Ok(Arc::new(FakeSource { fail_page }) as Arc<dyn SourceAdapter>)),
        }
    }

    #[tokio::test]
    async fn download_registers_chapter_in_numeric_order_and_dedupes() {
        let d = tempfile::tempdir().unwrap();
        let r = runner(d.path(), None);
        let (m, chs) = remote();
        assert_eq!(enqueue(&r.db, &m, &chs).unwrap(), (1, 0));
        assert_eq!(enqueue(&r.db, &m, &chs).unwrap(), (0, 1), "no duplicate download");
        let id = list(&r.db).unwrap()[0].id;
        r.run_one(id).await.unwrap();
        let dl = get(&r.db, id).unwrap();
        assert_eq!((dl.status.as_str(), dl.progress, dl.total), ("done", 12, 12));
        let view = crate::library::queries::chapter_view(&r.db, dl.chapter_id.unwrap()).unwrap();
        assert_eq!(view.pages.len(), 12);
        assert!(view.chapter.downloaded);
        assert_eq!(enqueue(&r.db, &m, &chs).unwrap(), (0, 1), "done chapters are skipped");
    }

    /// Fails on one page the first time only (simulates a network hiccup).
    struct FlakySource {
        fetched: parking_lot::Mutex<Vec<usize>>,
        fail_once: parking_lot::Mutex<Option<usize>>,
    }

    #[async_trait]
    impl SourceAdapter for FlakySource {
        async fn ping(&self) -> AppResult<String> { Ok("f".into()) }
        async fn sources(&self) -> AppResult<Vec<SourceInfo>> { Ok(vec![]) }
        async fn browse(&self, _: &str, _: &str, _: Option<&str>, _: u32) -> AppResult<MangaPage> { Ok(MangaPage { mangas: vec![], has_next_page: false }) }
        async fn manga(&self, _: &str) -> AppResult<RemoteManga> { unimplemented!() }
        async fn chapters(&self, _: &str) -> AppResult<Vec<RemoteChapter>> { Ok(vec![]) }
        async fn page_urls(&self, _: &str) -> AppResult<Vec<String>> { Ok((0..6).map(|i| format!("f://{i}")).collect()) }
        async fn fetch(&self, url: &str) -> AppResult<(Vec<u8>, Option<String>)> {
            let i: usize = url.trim_start_matches("f://").parse().unwrap();
            if *self.fail_once.lock() == Some(i) {
                return Err(AppError::new("network", "hiccup"));
            }
            self.fetched.lock().push(i);
            Ok((png(), Some("image/png".into())))
        }
    }

    /// Unreachable for the first `down` calls, like a server that is still starting.
    struct BootingSource {
        down: parking_lot::Mutex<usize>,
    }

    #[async_trait]
    impl SourceAdapter for BootingSource {
        async fn ping(&self) -> AppResult<String> { Ok("b".into()) }
        async fn sources(&self) -> AppResult<Vec<SourceInfo>> { Ok(vec![]) }
        async fn browse(&self, _: &str, _: &str, _: Option<&str>, _: u32) -> AppResult<MangaPage> { Ok(MangaPage { mangas: vec![], has_next_page: false }) }
        async fn manga(&self, _: &str) -> AppResult<RemoteManga> { unimplemented!() }
        async fn chapters(&self, _: &str) -> AppResult<Vec<RemoteChapter>> { Ok(vec![]) }
        async fn page_urls(&self, _: &str) -> AppResult<Vec<String>> {
            let mut d = self.down.lock();
            if *d > 0 {
                *d -= 1;
                return Err(AppError::new("source_offline", "Suwayomi is not reachable"));
            }
            Ok((0..3).map(|i| format!("b://{i}")).collect())
        }
        async fn fetch(&self, _: &str) -> AppResult<(Vec<u8>, Option<String>)> { Ok((png(), Some("image/png".into()))) }
    }

    fn booting_runner(dir: &std::path::Path, down: usize) -> DownloadRunner {
        let src = Arc::new(BootingSource { down: parking_lot::Mutex::new(down) });
        DownloadRunner {
            db: Db::open_in_memory().unwrap(),
            paths: Paths::new(dir.to_path_buf()).unwrap(),
            events: Arc::new(NoEvents),
            notify: Arc::new(Notify::new()),
            adapter: Arc::new(move || Ok(src.clone() as Arc<dyn SourceAdapter>)),
        }
    }

    #[tokio::test]
    async fn resumed_download_waits_for_the_source_server_to_start() {
        // Regression: after a restart the queue resumed before the managed server answered,
        // and interrupted downloads were marked failed.
        let d = tempfile::tempdir().unwrap();
        let r = booting_runner(d.path(), 3);
        let (m, chs) = remote();
        enqueue(&r.db, &m, &chs).unwrap();
        let id = list(&r.db).unwrap()[0].id;
        let ms = std::time::Duration::from_millis;
        r.run_waiting_for_source(id, ms(5), ms(5_000)).await;
        let dl = get(&r.db, id).unwrap();
        assert_eq!((dl.status.as_str(), dl.progress), ("done", 3));
        assert_eq!(dl.error, None);
    }

    #[tokio::test]
    async fn download_fails_when_the_source_server_never_starts() {
        let d = tempfile::tempdir().unwrap();
        let r = booting_runner(d.path(), usize::MAX);
        let (m, chs) = remote();
        enqueue(&r.db, &m, &chs).unwrap();
        let id = list(&r.db).unwrap()[0].id;
        let ms = std::time::Duration::from_millis;
        r.run_waiting_for_source(id, ms(5), ms(40)).await;
        let dl = get(&r.db, id).unwrap();
        assert_eq!(dl.status, "failed");
        assert!(dl.error.unwrap().contains("not reachable"));
    }

    #[tokio::test]
    async fn interrupted_download_resumes_and_lands_in_library() {
        let d = tempfile::tempdir().unwrap();
        let flaky = Arc::new(FlakySource { fetched: parking_lot::Mutex::new(vec![]), fail_once: parking_lot::Mutex::new(Some(3)) });
        let f2 = flaky.clone();
        let db = Db::open_in_memory().unwrap();
        let r = DownloadRunner {
            db: db.clone(),
            paths: Paths::new(d.path().to_path_buf()).unwrap(),
            events: Arc::new(NoEvents),
            notify: Arc::new(Notify::new()),
            adapter: Arc::new(move || Ok(f2.clone() as Arc<dyn SourceAdapter>)),
        };
        let (m, chs) = remote();
        enqueue(&db, &m, &chs).unwrap();
        let id = list(&db).unwrap()[0].id;
        assert!(r.run_one(id).await.is_err());
        assert_eq!(flaky.fetched.lock().clone(), vec![0, 1, 2]);
        *flaky.fail_once.lock() = None;
        r.run_one(id).await.unwrap();
        assert_eq!(flaky.fetched.lock().clone(), vec![0, 1, 2, 3, 4, 5], "pages 0-2 were not fetched again");
        // automatic library insertion
        let lib = crate::library::queries::list_manga(&db).unwrap();
        assert_eq!(lib.len(), 1);
        assert_eq!((lib[0].title.as_str(), lib[0].downloaded_chapters), ("Remote", 1));
        assert_eq!(crate::library::queries::chapter_view(&db, get(&db, id).unwrap().chapter_id.unwrap()).unwrap().pages.len(), 6);
    }

    /// A multi-scanlator listing: each chapter ref has its own number of pages.
    struct ListingSource {
        pages: HashMap<String, usize>,
    }

    #[async_trait]
    impl SourceAdapter for ListingSource {
        async fn ping(&self) -> AppResult<String> { Ok("l".into()) }
        async fn sources(&self) -> AppResult<Vec<SourceInfo>> { Ok(vec![]) }
        async fn browse(&self, _: &str, _: &str, _: Option<&str>, _: u32) -> AppResult<MangaPage> { Ok(MangaPage { mangas: vec![], has_next_page: false }) }
        async fn manga(&self, _: &str) -> AppResult<RemoteManga> { unimplemented!() }
        async fn chapters(&self, _: &str) -> AppResult<Vec<RemoteChapter>> { Ok(vec![]) }
        async fn page_urls(&self, chapter_ref: &str) -> AppResult<Vec<String>> {
            Ok((0..self.pages[chapter_ref]).map(|i| format!("l://{chapter_ref}/{i}")).collect())
        }
        async fn fetch(&self, _: &str) -> AppResult<(Vec<u8>, Option<String>)> { Ok((png(), Some("image/png".into()))) }
    }

    fn rc(r: &str, n: f64, title: &str, group: &str, known_pages: Option<i64>, up: i64) -> RemoteChapter {
        RemoteChapter { chapter_ref: r.into(), title: title.into(), number: Some(n), scanlator: Some(group.into()), upload_date: Some(up), page_count: known_pages }
    }

    fn listing_runner(dir: &std::path::Path, pages: &[(&str, usize)]) -> DownloadRunner {
        let src = Arc::new(ListingSource { pages: pages.iter().map(|(r, n)| (r.to_string(), *n)).collect() });
        DownloadRunner {
            db: Db::open_in_memory().unwrap(),
            paths: Paths::new(dir.to_path_buf()).unwrap(),
            events: Arc::new(NoEvents),
            notify: Arc::new(Notify::new()),
            adapter: Arc::new(move || Ok(src.clone() as Arc<dyn SourceAdapter>)),
        }
    }

    async fn run_queued(r: &DownloadRunner) {
        let ids: Vec<i64> = list(&r.db).unwrap().into_iter().filter(|d| d.status == "queued").map(|d| d.id).collect();
        for id in ids {
            r.run_one(id).await.unwrap();
        }
    }

    fn by_title(db: &Db, title: &str) -> Download {
        list(db).unwrap().into_iter().find(|d| d.chapter_title == title).unwrap()
    }

    #[tokio::test]
    async fn bulk_download_queues_one_version_and_swaps_placeholders() {
        let d = tempfile::tempdir().unwrap();
        // Page counts are unknown in the listing (Suwayomi reports -1 until fetched):
        // the newest Ch. 1 and the only Ch. 4 turn out to be one-page placeholders.
        let r = listing_runner(d.path(), &[("1a", 4), ("1b", 1), ("2a", 5), ("3a", 1), ("3.5", 1), ("4s", 1)]);
        let (m, _) = remote();
        let listing = vec![
            rc("1a", 1.0, "Ch. 1", "fans", None, 1),
            rc("1b", 1.0, "Ch. 1", "official", None, 2),
            rc("2a", 2.0, "Ch. 2", "fans", None, 3),
            rc("3a", 3.0, "Ch. 3", "official", Some(1), 4), // known stub
            rc("3.5", 3.5, "Ch. 3.5 - Illustration", "official", None, 5),
            rc("4s", 4.0, "Ch. 4", "official", None, 6),
        ];
        let plan = plan_bulk(&r.db, &m, &listing, None).unwrap();
        assert_eq!(plan.picks.iter().map(|p| p.chapter.chapter_ref.as_str()).collect::<Vec<_>>(), vec!["1b", "2a", "3.5", "4s"]);
        assert_eq!((plan.summary.chapters, plan.summary.duplicates, plan.summary.placeholders), (4, 1, 1));
        assert_eq!(enqueue_picks(&r.db, &m, &plan.picks, true).unwrap(), (4, 0));
        run_queued(&r).await;

        let ch1 = by_title(&r.db, "Ch. 1");
        assert_eq!((ch1.status.as_str(), ch1.chapter_ref.as_str(), ch1.total), ("done", "1a", 4), "switched to the real version");
        assert_eq!(ch1.scanlator.as_deref(), Some("fans"));
        let extra = by_title(&r.db, "Ch. 3.5 - Illustration");
        assert_eq!((extra.status.as_str(), extra.total), ("done", 1), "one-page extras are real chapters");
        let ch4 = by_title(&r.db, "Ch. 4");
        assert_eq!(ch4.status, "skipped");
        assert!(ch4.chapter_id.is_none());
        let lib = crate::library::queries::list_manga(&r.db).unwrap();
        assert_eq!(lib[0].downloaded_chapters, 3);

        // Downloading everything again adds nothing: every chapter is held or a placeholder.
        let again = plan_bulk(&r.db, &m, &listing, None).unwrap();
        assert!(again.picks.is_empty(), "{:?}", again.picks);
        assert_eq!((again.summary.already, again.summary.placeholders), (3, 2));
        assert_eq!(again.summary.preferred.as_deref(), Some("fans"), "the group most downloads came from");

        // Picking the placeholder by hand downloads it as it is.
        retry(&r.db, ch4.id).unwrap();
        run_queued(&r).await;
        let ch4 = get(&r.db, ch4.id).unwrap();
        assert_eq!((ch4.status.as_str(), ch4.total, ch4.auto), ("done", 1, false));
    }

    #[tokio::test]
    async fn single_chapter_downloads_stay_exactly_as_chosen() {
        let d = tempfile::tempdir().unwrap();
        let r = listing_runner(d.path(), &[("9a", 3), ("9b", 1)]);
        let (m, _) = remote();
        let stub = rc("9b", 9.0, "Ch. 9", "official", Some(1), 2);
        assert_eq!(enqueue(&r.db, &m, &[rc("9a", 9.0, "Ch. 9", "fans", Some(3), 1), stub]).unwrap(), (2, 0));
        run_queued(&r).await;
        let all = list(&r.db).unwrap();
        assert!(all.iter().all(|d| d.status == "done" && !d.auto), "{all:?}");
    }

    #[tokio::test]
    async fn cleanup_keeps_one_copy_and_never_touches_imports() {
        let d = tempfile::tempdir().unwrap();
        let r = listing_runner(d.path(), &[("85a", 4), ("85b", 4), ("85c", 4), ("85d", 4), ("104a", 4), ("104b", 1), ("104.5", 1), ("99", 1)]);
        let (m, _) = remote();
        // How the library looked before: every version downloaded by hand.
        let versions = vec![
            rc("85a", 85.0, "Ch. 85", "official", None, 1),
            rc("85b", 85.0, "Ch. 85", "unofficial", None, 2),
            rc("85c", 85.0, "Ch. 85", "official", None, 3),
            rc("104a", 104.0, "Ch. 104", "official", None, 4),
            rc("104b", 104.0, "Ch. 104", "unofficial", None, 5),
            rc("104.5", 104.5, "Ch. 104.5 - ILLUSTRATION", "unofficial", None, 6),
            rc("99", 99.0, "Ch. 99", "unofficial", None, 7),
        ];
        enqueue(&r.db, &m, &versions).unwrap();
        run_queued(&r).await;
        enqueue(&r.db, &m, &[rc("85d", 85.0, "Ch. 85", "official", None, 8)]).unwrap(); // still queued
        let manga_id = local_manga(&r.db, &m).unwrap();
        set_preference(&r.db, manga_id, Some("official")).unwrap();
        let lib = r.paths.library_dir();

        // A chapter the user imported themselves, with the same number.
        let imported = d.path().join("my-scans").join("85");
        std::fs::create_dir_all(&imported).unwrap();
        std::fs::write(imported.join("001.png"), png()).unwrap();
        r.db.with(|c| {
            c.execute(
                "INSERT INTO chapter (manga_id, title, number, sort_index, container, location, page_count, added_at)
                 VALUES (?1, 'Ch. 85 (mine)', 85, 0, 'folder', ?2, 1, ?3)",
                params![manga_id, imported.to_string_lossy(), now()],
            )?;
            Ok(())
        })
        .unwrap();
        // Ch. 85 from 85a has a translated page: that copy is never removed.
        let ch85a = by_title_ref(&r.db, "85a").chapter_id.unwrap();
        r.db.with(|c| {
            c.execute(
                "INSERT INTO page_state (page_id, status, updated_at) SELECT id, 'translated', ?2 FROM page WHERE chapter_id = ?1 LIMIT 1",
                params![ch85a, now()],
            )?;
            Ok(())
        })
        .unwrap();

        let plan = cleanup_plan(&r.db, &lib, manga_id, true).unwrap();
        let mut removed: Vec<(String, Removal)> = plan
            .remove
            .iter()
            .map(|e| {
                let id = e.download_id.unwrap_or_else(|| downloads_of_chapter(&r.db, e.chapter_id.unwrap()));
                (get(&r.db, id).unwrap().chapter_ref, e.reason)
            })
            .collect();
        removed.sort_by(|a, b| a.0.cmp(&b.0));
        use Removal::*;
        assert_eq!(
            removed,
            vec![("104b".into(), Duplicate), ("85b".into(), Duplicate), ("85c".into(), Duplicate), ("85d".into(), Duplicate), ("99".into(), Placeholder)]
        );
        assert_eq!(plan.kept, 3, "85a, 104a, 104.5");
        assert!(plan.bytes > 0);

        // Apply, also passing the protected chapter: it is not in the plan, so it stays.
        let chapter_ids: Vec<i64> = plan.remove.iter().filter_map(|e| e.chapter_id).chain([ch85a]).collect();
        let download_ids: Vec<i64> = plan.remove.iter().filter_map(|e| e.download_id).collect();
        let res = cleanup_apply(&r.db, &lib, manga_id, true, &chapter_ids, &download_ids).unwrap();
        assert_eq!((res.chapters_removed, res.downloads_cancelled), (4, 1));
        assert_eq!(res.bytes_freed, plan.bytes);

        let remaining: Vec<String> = r
            .db
            .with(|c| {
                let mut s = c.prepare("SELECT title FROM chapter WHERE manga_id = ?1 ORDER BY title")?;
                let v = s.query_map([manga_id], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
                Ok(v)
            })
            .unwrap();
        assert_eq!(remaining, vec!["Ch. 104", "Ch. 104.5 - ILLUSTRATION", "Ch. 85", "Ch. 85 (mine)"]);
        assert!(imported.join("001.png").exists(), "imported files are untouched");
        let dirs: Vec<String> = std::fs::read_dir(lib.join(manga_id.to_string()))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("dl-"))
            .collect();
        assert_eq!(dirs.len(), 3, "{dirs:?}");
        let refs: Vec<(String, String)> = list(&r.db).unwrap().into_iter().map(|d| (d.chapter_ref, d.status)).collect();
        assert!(refs.contains(&("85d".into(), "cancelled".into())));
        assert!(!refs.iter().any(|(r, _)| r == "85b" || r == "99"), "records of removed chapters are gone: {refs:?}");
        assert_eq!(crate::library::queries::chapter_view(&r.db, ch85a).unwrap().pages.len(), 4);

        // Nothing left to clean.
        assert!(cleanup_plan(&r.db, &lib, manga_id, true).unwrap().remove.is_empty());
    }

    fn by_title_ref(db: &Db, chapter_ref: &str) -> Download {
        list(db).unwrap().into_iter().find(|d| d.chapter_ref == chapter_ref).unwrap()
    }

    fn downloads_of_chapter(db: &Db, chapter_id: i64) -> i64 {
        db.with(|c| Ok(c.query_row("SELECT id FROM download WHERE chapter_id = ?1", [chapter_id], |r| r.get(0))?)).unwrap()
    }

    #[tokio::test]
    async fn failure_leaves_no_partial_chapter_and_can_retry() {
        let d = tempfile::tempdir().unwrap();
        let r = runner(d.path(), Some(5));
        let (m, chs) = remote();
        enqueue(&r.db, &m, &chs).unwrap();
        let id = list(&r.db).unwrap()[0].id;
        assert!(r.run_one(id).await.is_err());
        let n: i64 = r.db.with(|c| Ok(c.query_row("SELECT count(*) FROM chapter", [], |x| x.get(0))?)).unwrap();
        assert_eq!(n, 0);
        r.set(id, "failed", Some("x")).unwrap();
        retry(&r.db, id).unwrap();
        assert_eq!(get(&r.db, id).unwrap().status, "queued");
    }
}
