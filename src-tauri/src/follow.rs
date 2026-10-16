//! Library updates: ask each series' source for chapters published since they were added,
//! queue them (one version per chapter, like "download all") and report them.

use rusqlite::params;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

use crate::chapter_pick::chapter_key;
use crate::db::Db;
use crate::downloads;
use crate::error::AppResult;
use crate::sources::{RemoteChapter, RemoteManga, SourceAdapter};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NewChapters {
    pub manga_id: i64,
    pub title: String,
    /// Titles of the new chapters (one version each), in the source's order.
    pub chapters: Vec<String>,
    /// They were added to the download queue.
    pub queued: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CheckReport {
    pub checked: usize,
    pub failed: Vec<String>,
    pub new: Vec<NewChapters>,
}

/// Series in the library that came from an online source.
fn followed(db: &Db) -> AppResult<Vec<(i64, RemoteManga)>> {
    db.with(|c| {
        let mut s = c.prepare(
            "SELECT id, source_id, source_ref, title, author, artist, description, status FROM manga
             WHERE source_kind = 'source' AND source_id IS NOT NULL AND source_ref IS NOT NULL ORDER BY title",
        )?;
        let v = s
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    RemoteManga {
                        source_id: r.get(1)?,
                        manga_ref: r.get(2)?,
                        title: r.get(3)?,
                        thumbnail_url: None,
                        author: r.get(4)?,
                        artist: r.get(5)?,
                        description: r.get(6)?,
                        genres: vec![],
                        status: r.get(7)?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })
}

/// Chapters the library already knows in any version or state (done, queued, failed, skipped…),
/// so a chapter that failed is not retried behind the user's back on every check.
fn known_keys(db: &Db, m: &RemoteManga) -> AppResult<HashSet<String>> {
    db.with(|c| {
        let mut s = c.prepare("SELECT chapter_number, chapter_title FROM download WHERE source_id = ?1 AND manga_ref = ?2")?;
        let v = s
            .query_map(params![m.source_id, m.manga_ref], |r| Ok(chapter_key(r.get(0)?, &r.get::<_, String>(1)?)))?
            .collect::<Result<HashSet<_>, _>>()?;
        Ok(v)
    })
}

/// Chapters of `listing` that are new to the library.
pub fn new_chapters(known: &HashSet<String>, listing: &[RemoteChapter]) -> Vec<RemoteChapter> {
    listing.iter().filter(|c| !known.contains(&chapter_key(c.number, &c.title))).cloned().collect()
}

/// Check every followed series. With `download`, new chapters are queued.
pub async fn check(db: &Db, adapter: Arc<dyn SourceAdapter>, download: bool) -> AppResult<CheckReport> {
    let mut report = CheckReport::default();
    for (manga_id, m) in followed(db)? {
        let listing = match adapter.chapters(&m.manga_ref).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(manga_id, "update check failed: {}", e.message);
                report.failed.push(m.title.clone());
                continue;
            }
        };
        report.checked += 1;
        let fresh = new_chapters(&known_keys(db, &m)?, &listing);
        if fresh.is_empty() {
            continue;
        }
        let preferred = downloads::stored_preference(db, &m)?;
        let plan = downloads::plan_bulk(db, &m, &fresh, preferred.as_deref())?;
        if plan.picks.is_empty() {
            continue; // only placeholders or extra versions
        }
        if download {
            downloads::enqueue_picks(db, &m, &plan.picks, true)?;
        }
        report.new.push(NewChapters {
            manga_id,
            title: m.title.clone(),
            chapters: plan.picks.iter().map(|p| p.chapter.title.clone()).collect(),
            queued: download,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::sources::{MangaPage, SourceInfo};
    use async_trait::async_trait;

    struct Listing(Vec<RemoteChapter>);

    #[async_trait]
    impl SourceAdapter for Listing {
        async fn ping(&self) -> AppResult<String> { Ok("l".into()) }
        async fn sources(&self) -> AppResult<Vec<SourceInfo>> { Ok(vec![]) }
        async fn browse(&self, _: &str, _: &str, _: Option<&str>, _: u32) -> AppResult<MangaPage> { Ok(MangaPage { mangas: vec![], has_next_page: false }) }
        async fn manga(&self, _: &str) -> AppResult<RemoteManga> { Err(AppError::invalid("x")) }
        async fn chapters(&self, r: &str) -> AppResult<Vec<RemoteChapter>> {
            if r == "gone" {
                return Err(AppError::new("source_offline", "down"));
            }
            Ok(self.0.clone())
        }
        async fn page_urls(&self, _: &str) -> AppResult<Vec<String>> { Ok(vec![]) }
        async fn fetch(&self, _: &str) -> AppResult<(Vec<u8>, Option<String>)> { Err(AppError::invalid("x")) }
    }

    fn ch(r: &str, n: f64, group: &str, pages: Option<i64>) -> RemoteChapter {
        RemoteChapter { chapter_ref: r.into(), title: format!("Ch. {n}"), number: Some(n), scanlator: Some(group.into()), upload_date: None, page_count: pages }
    }

    fn manga(r: &str) -> RemoteManga {
        RemoteManga { source_id: "s".into(), manga_ref: r.into(), title: format!("T-{r}"), thumbnail_url: None, author: None, artist: None, description: None, genres: vec![], status: None }
    }

    #[tokio::test]
    async fn new_chapters_are_queued_once_one_version_each() {
        let db = Db::open_in_memory().unwrap();
        // the user downloaded chapters 1-2 (group A); chapter 3 failed earlier
        downloads::enqueue(&db, &manga("m"), &[ch("a1", 1.0, "A", Some(10)), ch("a2", 2.0, "A", Some(10)), ch("a3", 3.0, "A", Some(10))]).unwrap();
        db.with(|c| { c.execute("UPDATE download SET status = 'failed' WHERE chapter_ref = 'a3'", [])?; Ok(()) }).unwrap();
        downloads::local_manga(&db, &manga("gone")).unwrap(); // a series whose source fails
        let listing = vec![
            ch("a1", 1.0, "A", Some(10)), ch("b1", 1.0, "B", Some(10)), ch("a2", 2.0, "A", Some(10)), ch("a3", 3.0, "A", Some(10)),
            ch("a4", 4.0, "A", Some(12)), ch("b4", 4.0, "B", Some(12)), // new chapter in two versions
            ch("a5", 5.0, "A", Some(1)),  // new, but only a one-page placeholder
        ];
        let adapter: Arc<dyn SourceAdapter> = Arc::new(Listing(listing));

        let report = check(&db, adapter.clone(), true).await.unwrap();
        assert_eq!(report.checked, 1);
        assert_eq!(report.failed, vec!["T-gone".to_string()]);
        assert_eq!(report.new.len(), 1);
        assert_eq!(report.new[0].chapters, vec!["Ch. 4".to_string()], "one version, no placeholder, failed ch. 3 not retried");
        let queued: Vec<String> = db
            .with(|c| Ok(c.prepare("SELECT chapter_ref FROM download WHERE status = 'queued' ORDER BY id")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?))
            .unwrap();
        assert!(queued.contains(&"a4".to_string()), "the user's group is preferred: {queued:?}");
        assert!(!queued.contains(&"b4".to_string()));

        let again = check(&db, adapter, true).await.unwrap();
        assert!(again.new.is_empty(), "a second check finds nothing new");
    }

    #[tokio::test]
    async fn check_without_download_only_reports() {
        let db = Db::open_in_memory().unwrap();
        downloads::enqueue(&db, &manga("m"), &[ch("a1", 1.0, "A", Some(10))]).unwrap();
        let adapter: Arc<dyn SourceAdapter> = Arc::new(Listing(vec![ch("a1", 1.0, "A", Some(10)), ch("a2", 2.0, "A", Some(9))]));
        let report = check(&db, adapter, false).await.unwrap();
        assert_eq!(report.new[0].chapters, vec!["Ch. 2".to_string()]);
        assert!(!report.new[0].queued);
        let n: i64 = db.with(|c| Ok(c.query_row("SELECT count(*) FROM download", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 1, "nothing queued");
    }
}
