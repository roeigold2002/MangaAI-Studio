//! Read models for the UI and simple library mutations.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize)]
pub struct MangaSummary {
    pub id: i64,
    pub title: String,
    pub favorite: bool,
    pub source_kind: String,
    pub chapter_count: i64,
    pub read_count: i64,
    pub page_total: i64,
    pub translated_chapters: i64,
    pub last_read_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub categories: Vec<i64>,
    pub continue_chapter_id: Option<i64>,
    /// Page to resume at in `continue_chapter_id`.
    pub continue_page: Option<i64>,
    pub downloaded_chapters: i64,
    pub translated_pages: i64,
    pub failed_pages: i64,
}

#[derive(Debug, Serialize)]
pub struct TranslationCounts {
    pub total: i64,
    pub done: i64,
    pub failed: i64,
    pub in_progress: i64,
}

#[derive(Debug, Serialize)]
pub struct ChapterInfo {
    pub id: i64,
    pub manga_id: i64,
    pub title: String,
    pub number: Option<f64>,
    pub volume: Option<f64>,
    pub sort_index: i64,
    pub container: String,
    pub page_count: i64,
    pub read: bool,
    pub last_page: i64,
    pub last_read_at: Option<String>,
    pub downloaded: bool,
    pub available: bool,
    pub translation: TranslationCounts,
}

#[derive(Debug, Serialize)]
pub struct MangaDetail {
    pub id: i64,
    pub title: String,
    pub alt_titles: Vec<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub status: Option<String>,
    pub genres: Vec<String>,
    pub source_kind: String,
    pub source_id: Option<String>,
    pub import_root: Option<String>,
    pub favorite: bool,
    pub reading_mode: Option<String>,
    pub reading_dir: Option<String>,
    pub source_lang: String,
    pub translation_notes: String,
    pub categories: Vec<i64>,
    pub chapters: Vec<ChapterInfo>,
    pub last_read_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PageInfo {
    pub id: i64,
    pub idx: i64,
    pub width: i64,
    pub height: i64,
    pub sha256: String,
    pub status: String,
    pub stage: Option<String>,
    pub error: Option<String>,
    /// Changes whenever the rendered file changes (cache-busting for the image URL).
    pub translated_version: Option<String>,
    pub region_count: i64,
    pub needs_review: i64,
}

#[derive(Debug, Serialize)]
pub struct ChapterView {
    pub chapter: ChapterInfo,
    pub manga_title: String,
    pub reading_mode: Option<String>,
    pub reading_dir: Option<String>,
    pub prev_chapter_id: Option<i64>,
    pub next_chapter_id: Option<i64>,
    pub pages: Vec<PageInfo>,
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub manga_id: i64,
    pub manga_title: String,
    pub chapter_id: i64,
    pub chapter_title: String,
    pub page_idx: i64,
    pub page_count: i64,
    pub at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub sort: i64,
    pub count: i64,
}

fn json_vec(s: String) -> Vec<String> {
    serde_json::from_str(&s).unwrap_or_default()
}

fn categories_of(c: &Connection, manga_id: i64) -> AppResult<Vec<i64>> {
    let mut s = c.prepare_cached("SELECT category_id FROM manga_category WHERE manga_id = ?1")?;
    let v = s.query_map([manga_id], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
    Ok(v)
}

const CHAPTER_SELECT: &str = "
    SELECT ch.id, ch.manga_id, ch.title, ch.number, ch.volume, ch.sort_index, ch.container, ch.page_count,
           ch.read, ch.last_page, ch.last_read_at, ch.source_ref, ch.location,
           (SELECT count(*) FROM page p JOIN page_state s ON s.page_id = p.id WHERE p.chapter_id = ch.id AND s.status = 'done'),
           (SELECT count(*) FROM page p JOIN page_state s ON s.page_id = p.id WHERE p.chapter_id = ch.id AND s.status = 'failed'),
           (SELECT count(*) FROM page p JOIN page_state s ON s.page_id = p.id WHERE p.chapter_id = ch.id AND s.status IN ('analyzed','translated','running'))
    FROM chapter ch";

fn chapter_row(r: &Row) -> rusqlite::Result<ChapterInfo> {
    let location: String = r.get(12)?;
    let page_count: i64 = r.get(7)?;
    Ok(ChapterInfo {
        id: r.get(0)?,
        manga_id: r.get(1)?,
        title: r.get(2)?,
        number: r.get(3)?,
        volume: r.get(4)?,
        sort_index: r.get(5)?,
        container: r.get(6)?,
        page_count,
        read: r.get::<_, i64>(8)? != 0,
        last_page: r.get(9)?,
        last_read_at: r.get(10)?,
        downloaded: r.get::<_, Option<String>>(11)?.is_some(),
        available: std::path::Path::new(&location).exists(),
        translation: TranslationCounts { total: page_count, done: r.get(13)?, failed: r.get(14)?, in_progress: r.get(15)? },
    })
}

pub fn chapter_info(c: &Connection, chapter_id: i64) -> AppResult<ChapterInfo> {
    Ok(c.query_row(&format!("{CHAPTER_SELECT} WHERE ch.id = ?1"), [chapter_id], chapter_row)?)
}

pub fn list_manga(db: &Db) -> AppResult<Vec<MangaSummary>> {
    db.with(|c| {
        let mut s = c.prepare(
            "SELECT m.id, m.title, m.favorite, m.source_kind, m.last_read_at, m.created_at, m.updated_at,
                    (SELECT count(*) FROM chapter WHERE manga_id = m.id),
                    (SELECT count(*) FROM chapter WHERE manga_id = m.id AND read = 1),
                    (SELECT coalesce(sum(page_count),0) FROM chapter WHERE manga_id = m.id),
                    (SELECT count(*) FROM chapter ch WHERE ch.manga_id = m.id AND ch.page_count > 0 AND ch.page_count =
                        (SELECT count(*) FROM page p JOIN page_state st ON st.page_id = p.id
                          WHERE p.chapter_id = ch.id AND st.status = 'done')),
                    (SELECT id FROM chapter WHERE manga_id = m.id AND last_read_at IS NOT NULL ORDER BY last_read_at DESC LIMIT 1),
                    (SELECT last_page FROM chapter WHERE manga_id = m.id AND last_read_at IS NOT NULL ORDER BY last_read_at DESC LIMIT 1),
                    (SELECT count(*) FROM chapter WHERE manga_id = m.id AND source_ref IS NOT NULL),
                    (SELECT count(*) FROM page p JOIN chapter ch ON ch.id = p.chapter_id JOIN page_state st ON st.page_id = p.id
                       WHERE ch.manga_id = m.id AND st.status = 'done'),
                    (SELECT count(*) FROM page p JOIN chapter ch ON ch.id = p.chapter_id JOIN page_state st ON st.page_id = p.id
                       WHERE ch.manga_id = m.id AND st.status = 'failed')
             FROM manga m ORDER BY m.title COLLATE NOCASE",
        )?;
        let rows = s
            .query_map([], |r| {
                Ok(MangaSummary {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    favorite: r.get::<_, i64>(2)? != 0,
                    source_kind: r.get(3)?,
                    last_read_at: r.get(4)?,
                    created_at: r.get(5)?,
                    updated_at: r.get(6)?,
                    chapter_count: r.get(7)?,
                    read_count: r.get(8)?,
                    page_total: r.get(9)?,
                    translated_chapters: r.get(10)?,
                    continue_chapter_id: r.get(11)?,
                    continue_page: r.get(12)?,
                    downloaded_chapters: r.get(13)?,
                    translated_pages: r.get(14)?,
                    failed_pages: r.get(15)?,
                    categories: vec![],
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = rows;
        for m in out.iter_mut() {
            m.categories = categories_of(c, m.id)?;
        }
        Ok(out)
    })
}

pub fn manga_detail(db: &Db, id: i64) -> AppResult<MangaDetail> {
    db.with(|c| {
        let mut d = c.query_row(
            "SELECT id, title, alt_titles, description, author, artist, status, genres, source_kind, source_id,
                    import_root, favorite, reading_mode, reading_dir, source_lang, translation_notes, last_read_at
             FROM manga WHERE id = ?1",
            [id],
            |r| {
                Ok(MangaDetail {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    alt_titles: json_vec(r.get(2)?),
                    description: r.get(3)?,
                    author: r.get(4)?,
                    artist: r.get(5)?,
                    status: r.get(6)?,
                    genres: json_vec(r.get(7)?),
                    source_kind: r.get(8)?,
                    source_id: r.get(9)?,
                    import_root: r.get(10)?,
                    favorite: r.get::<_, i64>(11)? != 0,
                    reading_mode: r.get(12)?,
                    reading_dir: r.get(13)?,
                    source_lang: r.get(14)?,
                    translation_notes: r.get(15)?,
                    last_read_at: r.get(16)?,
                    categories: vec![],
                    chapters: vec![],
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("manga"))?;
        d.categories = categories_of(c, id)?;
        let mut s = c.prepare(&format!("{CHAPTER_SELECT} WHERE ch.manga_id = ?1 ORDER BY ch.sort_index, ch.id"))?;
        d.chapters = s.query_map([id], chapter_row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(d)
    })
}

pub fn chapter_view(db: &Db, chapter_id: i64) -> AppResult<ChapterView> {
    db.with(|c| {
        let chapter = chapter_info(c, chapter_id)?;
        let (manga_title, reading_mode, reading_dir): (String, Option<String>, Option<String>) = c.query_row(
            "SELECT title, reading_mode, reading_dir FROM manga WHERE id = ?1",
            [chapter.manga_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let neighbor = |dir: &str| -> AppResult<Option<i64>> {
            let sql = if dir == "prev" {
                "SELECT id FROM chapter WHERE manga_id = ?1 AND (sort_index < ?2 OR (sort_index = ?2 AND id < ?3))
                 ORDER BY sort_index DESC, id DESC LIMIT 1"
            } else {
                "SELECT id FROM chapter WHERE manga_id = ?1 AND (sort_index > ?2 OR (sort_index = ?2 AND id > ?3))
                 ORDER BY sort_index, id LIMIT 1"
            };
            Ok(c.query_row(sql, params![chapter.manga_id, chapter.sort_index, chapter.id], |r| r.get(0)).optional()?)
        };
        let prev_chapter_id = neighbor("prev")?;
        let next_chapter_id = neighbor("next")?;
        let mut s = c.prepare(
            "SELECT p.id, p.idx, p.width, p.height, p.sha256, coalesce(s.status, 'none'), s.stage, s.error, s.render_key,
                    s.rendered_path,
                    (SELECT count(*) FROM region r WHERE r.page_id = p.id),
                    (SELECT count(*) FROM region r WHERE r.page_id = p.id AND r.approved = 0 AND r.skip = 0
                       AND (r.confidence = 'low' OR r.translation IS NULL))
             FROM page p LEFT JOIN page_state s ON s.page_id = p.id
             WHERE p.chapter_id = ?1 ORDER BY p.idx",
        )?;
        let pages = s
            .query_map([chapter_id], |r| {
                let rendered: Option<String> = r.get(9)?;
                let key: Option<String> = r.get(8)?;
                Ok(PageInfo {
                    id: r.get(0)?,
                    idx: r.get(1)?,
                    width: r.get(2)?,
                    height: r.get(3)?,
                    sha256: r.get(4)?,
                    status: r.get(5)?,
                    stage: r.get(6)?,
                    error: r.get(7)?,
                    translated_version: rendered.and(key).map(|k| k[..12.min(k.len())].to_string()),
                    region_count: r.get(10)?,
                    needs_review: r.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ChapterView { chapter, manga_title, reading_mode, reading_dir, prev_chapter_id, next_chapter_id, pages })
    })
}

/// Save reading position. Marks the chapter read when the last page is reached.
pub fn save_progress(db: &Db, chapter_id: i64, page_idx: i64) -> AppResult<()> {
    let ts = now();
    db.with_tx(|tx| {
        let (manga_id, page_count): (i64, i64) =
            tx.query_row("SELECT manga_id, page_count FROM chapter WHERE id = ?1", [chapter_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let page_idx = page_idx.clamp(0, (page_count - 1).max(0));
        let finished = page_idx >= page_count - 1;
        tx.execute(
            "UPDATE chapter SET last_page = ?1, last_read_at = ?2, read = CASE WHEN ?3 THEN 1 ELSE read END WHERE id = ?4",
            params![page_idx, ts, finished, chapter_id],
        )?;
        tx.execute("UPDATE manga SET last_read_at = ?1 WHERE id = ?2", params![ts, manga_id])?;
        // Collapse consecutive history rows for the same chapter into one.
        let updated = tx.execute(
            "UPDATE reading_history SET page_idx = ?1, at = ?2
             WHERE id = (SELECT id FROM reading_history ORDER BY at DESC, id DESC LIMIT 1) AND chapter_id = ?3",
            params![page_idx, ts, chapter_id],
        )?;
        if updated == 0 {
            tx.execute(
                "INSERT INTO reading_history (manga_id, chapter_id, page_idx, at) VALUES (?1, ?2, ?3, ?4)",
                params![manga_id, chapter_id, page_idx, ts],
            )?;
        }
        Ok(())
    })
}

pub fn set_chapters_read(db: &Db, chapter_ids: &[i64], read: bool) -> AppResult<()> {
    db.with_tx(|tx| {
        for id in chapter_ids {
            tx.execute(
                "UPDATE chapter SET read = ?1, last_page = CASE WHEN ?1 THEN max(page_count - 1, 0) ELSE 0 END WHERE id = ?2",
                params![read, id],
            )?;
        }
        Ok(())
    })
}

pub fn history(db: &Db, limit: i64) -> AppResult<Vec<HistoryEntry>> {
    db.with(|c| {
        let mut s = c.prepare(
            "SELECT h.manga_id, m.title, h.chapter_id, ch.title, h.page_idx, ch.page_count, h.at
             FROM reading_history h JOIN manga m ON m.id = h.manga_id JOIN chapter ch ON ch.id = h.chapter_id
             ORDER BY h.at DESC, h.id DESC LIMIT ?1",
        )?;
        let v = s
            .query_map([limit], |r| {
                Ok(HistoryEntry {
                    manga_id: r.get(0)?,
                    manga_title: r.get(1)?,
                    chapter_id: r.get(2)?,
                    chapter_title: r.get(3)?,
                    page_idx: r.get(4)?,
                    page_count: r.get(5)?,
                    at: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })
}

pub fn clear_history(db: &Db) -> AppResult<()> {
    db.with(|c| {
        c.execute("DELETE FROM reading_history", [])?;
        Ok(())
    })
}

#[derive(Debug, Deserialize)]
pub struct MangaUpdate {
    pub title: Option<String>,
    pub favorite: Option<bool>,
    pub reading_mode: Option<Option<String>>,
    pub reading_dir: Option<Option<String>>,
    pub source_lang: Option<String>,
    pub translation_notes: Option<String>,
    pub description: Option<String>,
    pub categories: Option<Vec<i64>>,
}

pub fn update_manga(db: &Db, id: i64, u: MangaUpdate) -> AppResult<()> {
    let ts = now();
    db.with_tx(|tx| {
        if let Some(t) = u.title.filter(|t| !t.trim().is_empty()) {
            tx.execute("UPDATE manga SET title = ?1 WHERE id = ?2", params![t.trim(), id])?;
        }
        if let Some(f) = u.favorite {
            tx.execute("UPDATE manga SET favorite = ?1 WHERE id = ?2", params![f, id])?;
        }
        if let Some(m) = u.reading_mode {
            tx.execute("UPDATE manga SET reading_mode = ?1 WHERE id = ?2", params![m, id])?;
        }
        if let Some(d) = u.reading_dir {
            tx.execute("UPDATE manga SET reading_dir = ?1 WHERE id = ?2", params![d, id])?;
        }
        if let Some(l) = u.source_lang {
            tx.execute("UPDATE manga SET source_lang = ?1 WHERE id = ?2", params![l, id])?;
        }
        if let Some(n) = u.translation_notes {
            tx.execute("UPDATE manga SET translation_notes = ?1 WHERE id = ?2", params![n, id])?;
        }
        if let Some(d) = u.description {
            tx.execute("UPDATE manga SET description = ?1 WHERE id = ?2", params![d, id])?;
        }
        if let Some(cats) = u.categories {
            tx.execute("DELETE FROM manga_category WHERE manga_id = ?1", [id])?;
            for c in cats {
                tx.execute("INSERT OR IGNORE INTO manga_category (manga_id, category_id) VALUES (?1, ?2)", params![id, c])?;
            }
        }
        tx.execute("UPDATE manga SET updated_at = ?1 WHERE id = ?2", params![ts, id])?;
        Ok(())
    })
}

/// Remove a manga from the library. Imported originals are never touched;
/// downloaded pages (inside the app library dir) are deleted only if `delete_downloads`.
pub fn remove_manga(db: &Db, library_dir: &std::path::Path, id: i64, delete_downloads: bool) -> AppResult<()> {
    if delete_downloads {
        let dir = library_dir.join(id.to_string());
        if dir.starts_with(library_dir) && dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
    }
    db.with(|c| {
        c.execute("DELETE FROM manga WHERE id = ?1", [id])?;
        Ok(())
    })
}

pub fn list_categories(db: &Db) -> AppResult<Vec<Category>> {
    db.with(|c| {
        let mut s = c.prepare(
            "SELECT c.id, c.name, c.sort, (SELECT count(*) FROM manga_category mc WHERE mc.category_id = c.id)
             FROM category c ORDER BY c.sort, c.name",
        )?;
        let v = s
            .query_map([], |r| Ok(Category { id: r.get(0)?, name: r.get(1)?, sort: r.get(2)?, count: r.get(3)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })
}

pub fn create_category(db: &Db, name: &str) -> AppResult<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::invalid("category name is empty"));
    }
    db.with(|c| {
        c.execute(
            "INSERT INTO category (name, sort) VALUES (?1, (SELECT coalesce(max(sort),0)+1 FROM category))",
            [name],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
                AppError::invalid("a category with this name already exists")
            }
            e => e.into(),
        })?;
        Ok(c.last_insert_rowid())
    })
}

pub fn delete_category(db: &Db, id: i64) -> AppResult<()> {
    db.with(|c| {
        c.execute("DELETE FROM category WHERE id = ?1", [id])?;
        Ok(())
    })
}

pub fn rename_category(db: &Db, id: i64, name: &str) -> AppResult<()> {
    db.with(|c| {
        c.execute("UPDATE category SET name = ?1 WHERE id = ?2", params![name.trim(), id])?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(db: &Db) -> (i64, i64) {
        db.with(|c| {
            c.execute(
                "INSERT INTO manga (title, source_kind, created_at, updated_at) VALUES ('M', 'local', 'x', 'x')",
                [],
            )?;
            let m = c.last_insert_rowid();
            c.execute(
                "INSERT INTO chapter (manga_id, title, sort_index, container, location, page_count, added_at)
                 VALUES (?1, 'c1', 0, 'folder', '/nonexistent', 3, 'x')",
                [m],
            )?;
            let ch = c.last_insert_rowid();
            for i in 0..3 {
                c.execute(
                    "INSERT INTO page (chapter_id, idx, path, sha256, mime) VALUES (?1, ?2, 'p', 'h', 'image/png')",
                    params![ch, i],
                )?;
            }
            Ok((m, ch))
        })
        .unwrap()
    }

    #[test]
    fn progress_persists_and_marks_read_at_end() {
        let db = Db::open_in_memory().unwrap();
        let (_, ch) = seed(&db);
        save_progress(&db, ch, 1).unwrap();
        let v = chapter_view(&db, ch).unwrap();
        assert_eq!(v.chapter.last_page, 1);
        assert!(!v.chapter.read);
        save_progress(&db, ch, 2).unwrap();
        save_progress(&db, ch, 99).unwrap(); // clamped
        let v = chapter_view(&db, ch).unwrap();
        assert_eq!(v.chapter.last_page, 2);
        assert!(v.chapter.read);
        // consecutive saves for one chapter collapse into a single history row
        assert_eq!(history(&db, 10).unwrap().len(), 1);
    }

    #[test]
    fn categories_and_favorites() {
        let db = Db::open_in_memory().unwrap();
        let (m, _) = seed(&db);
        let cat = create_category(&db, "Seinen").unwrap();
        assert_eq!(create_category(&db, "Seinen").unwrap_err().code, "invalid");
        update_manga(
            &db,
            m,
            MangaUpdate {
                title: None,
                favorite: Some(true),
                reading_mode: None,
                reading_dir: Some(Some("ltr".into())),
                source_lang: None,
                translation_notes: None,
                description: None,
                categories: Some(vec![cat]),
            },
        )
        .unwrap();
        let list = list_manga(&db).unwrap();
        assert!(list[0].favorite);
        assert_eq!(list[0].categories, vec![cat]);
        assert_eq!(manga_detail(&db, m).unwrap().reading_dir.as_deref(), Some("ltr"));
    }
}
