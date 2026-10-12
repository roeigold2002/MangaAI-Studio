//! Persistent download queue: fetches chapters from a source adapter into the
//! app's library folder, then registers them like any local chapter.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Notify;

use crate::assets::{atomic_write, ensure_space, Paths};
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
}

const SELECT: &str = "SELECT id, source_id, manga_ref, manga_title, chapter_ref, chapter_title, chapter_number, manga_id, chapter_id,
    status, progress, total, error, updated_at FROM download";

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

/// Queue chapters. Chapters already downloaded are skipped; failed/cancelled ones are re-queued.
pub fn enqueue(db: &Db, m: &RemoteManga, chapters: &[RemoteChapter]) -> AppResult<(usize, usize)> {
    let manga_id = local_manga(db, m)?;
    let ts = now();
    db.with_tx(|tx| {
        let (mut added, mut skipped) = (0, 0);
        for ch in chapters {
            let existing: Option<(i64, String)> = tx
                .query_row(
                    "SELECT id, status FROM download WHERE source_id = ?1 AND chapter_ref = ?2",
                    params![m.source_id, ch.chapter_ref],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match existing {
                Some((_, st)) if st == "done" || st == "queued" || st == "running" => skipped += 1,
                Some((id, _)) => {
                    tx.execute(
                        "UPDATE download SET status = 'queued', error = NULL, progress = 0, updated_at = ?1 WHERE id = ?2",
                        params![ts, id],
                    )?;
                    added += 1;
                }
                None => {
                    tx.execute(
                        "INSERT INTO download (source_id, manga_ref, manga_title, chapter_ref, chapter_title, chapter_number, manga_id,
                                               status, total, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, ?9, ?9)",
                        params![m.source_id, m.manga_ref, m.title, ch.chapter_ref, ch.title, ch.number, manga_id, ch.page_count.unwrap_or(0), ts],
                    )?;
                    added += 1;
                }
            }
        }
        Ok((added, skipped))
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

pub fn retry(db: &Db, id: i64) -> AppResult<()> {
    db.with(|c| {
        c.execute(
            "UPDATE download SET status = 'queued', error = NULL, progress = 0, updated_at = ?1 WHERE id = ?2 AND status IN ('failed','cancelled')",
            params![now(), id],
        )?;
        Ok(())
    })
}

pub fn clear_finished(db: &Db) -> AppResult<usize> {
    db.with(|c| Ok(c.execute("DELETE FROM download WHERE status IN ('done','cancelled')", [])?))
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
        let urls = adapter.page_urls(&d.chapter_ref).await?;
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

    pub async fn run_forever(self: Arc<Self>, concurrency: usize) {
        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        loop {
            let next: Option<i64> = self
                .db
                .with(|c| Ok(c.query_row("SELECT id FROM download WHERE status = 'queued' ORDER BY id LIMIT 1", [], |r| r.get(0)).optional()?))
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
                if let Err(e) = me.run_one(id).await {
                    tracing::warn!(download = id, "download failed: {}", e.message);
                    let _ = me.set(id, "failed", Some(&e.message));
                    let _ = std::fs::remove_dir_all(
                        me.paths.library_dir().join(get(&me.db, id).ok().and_then(|d| d.manga_id).unwrap_or(0).to_string()).join(format!(".dl-{id}.part")),
                    );
                }
                drop(permit);
                me.notify.notify_one();
            });
        }
    }
}

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
