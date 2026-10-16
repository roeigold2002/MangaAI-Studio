//! Insert scanned manga into the database; rescan without duplicating chapters.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::path::Path;

use super::scan::{scan_manga, ChapterSource, MangaSource};
use crate::assets::{image_dims, mime_for, read_page_bytes, sha256_hex};
use crate::db::{now, Db};
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, Clone)]
pub struct ImportReport {
    pub manga_id: i64,
    pub title: String,
    pub chapters_added: usize,
    pub chapters_updated: usize,
    pub chapters_unchanged: usize,
    pub pages_added: usize,
    pub warnings: Vec<String>,
}

struct PageRecord {
    path: String,
    sha256: String,
    width: u32,
    height: u32,
    mime: &'static str,
}

/// Read every page once: hash + dimensions. Unreadable pages become warnings.
fn read_chapter_pages(ch: &ChapterSource, warnings: &mut Vec<String>) -> Vec<PageRecord> {
    let mut out = Vec::with_capacity(ch.pages.len());
    for p in &ch.pages {
        match read_page_bytes(ch.container, &ch.location, p) {
            Ok(bytes) => {
                let (width, height) = image_dims(&bytes);
                if width == 0 {
                    warnings.push(format!("{}: unsupported or corrupt image: {p}", ch.title));
                    continue;
                }
                out.push(PageRecord { path: p.clone(), sha256: sha256_hex(&bytes), width, height, mime: mime_for(p) });
            }
            Err(e) => warnings.push(format!("{}: {p}: {}", ch.title, e.message)),
        }
    }
    out
}

fn insert_pages(tx: &rusqlite::Transaction, chapter_id: i64, pages: &[PageRecord]) -> AppResult<()> {
    let mut stmt = tx.prepare(
        "INSERT INTO page (chapter_id, idx, path, sha256, width, height, mime) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for (i, p) in pages.iter().enumerate() {
        stmt.execute(params![chapter_id, i as i64, p.path, p.sha256, p.width, p.height, p.mime])?;
    }
    Ok(())
}

/// Import (or rescan) a manga folder / archive. Identified by its absolute path,
/// so importing the same path twice updates the existing entry.
pub fn import_path(db: &Db, root: &Path) -> AppResult<ImportReport> {
    let root = crate::platform::canonicalize(root).map_err(|e| AppError::new("not_found", format!("{}: {e}", root.display())))?;
    let src = scan_manga(&root)?;
    import_scanned(db, src)
}

pub fn import_scanned(db: &Db, src: MangaSource) -> AppResult<ImportReport> {
    let mut warnings = src.warnings.clone();
    let root_str = src.root.to_string_lossy().into_owned();

    // Hash pages outside the DB lock (this is the slow part).
    let existing: Vec<(i64, String, String, i64)> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT ch.id, ch.location, ch.inner_prefix, ch.page_count FROM chapter ch
             JOIN manga m ON m.id = ch.manga_id WHERE m.import_root = ?1",
        )?;
        let rows = stmt
            .query_map([&root_str], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;

    let mut scanned: Vec<(ChapterSource, Vec<PageRecord>)> = Vec::new();
    for ch in &src.chapters {
        let pages = read_chapter_pages(ch, &mut warnings);
        if pages.is_empty() {
            warnings.push(format!("{}: no readable pages, skipped", ch.title));
            continue;
        }
        scanned.push((ch.clone(), pages));
    }
    if scanned.is_empty() {
        return Err(AppError::new("empty", "no readable pages found"));
    }

    let ts = now();
    db.with_tx(|tx| {
        let manga_id: i64 = match tx
            .query_row("SELECT id FROM manga WHERE import_root = ?1", [&root_str], |r| r.get(0))
            .optional()?
        {
            Some(id) => {
                tx.execute("UPDATE manga SET updated_at = ?1 WHERE id = ?2", params![ts, id])?;
                id
            }
            None => {
                tx.execute(
                    "INSERT INTO manga (title, source_kind, import_root, cover_path, created_at, updated_at)
                     VALUES (?1, 'local', ?2, ?3, ?4, ?4)",
                    params![src.title, root_str, src.cover.as_ref().map(|p| p.to_string_lossy().into_owned()), ts],
                )?;
                tx.last_insert_rowid()
            }
        };

        let (mut added, mut updated, mut unchanged, mut pages_added) = (0, 0, 0, 0);
        for (i, (ch, pages)) in scanned.iter().enumerate() {
            let loc = ch.location.to_string_lossy().into_owned();
            let found = existing.iter().find(|(_, l, p, _)| *l == loc && *p == ch.inner_prefix);
            match found {
                Some((id, _, _, count)) => {
                    // Same chapter: refresh order; rebuild pages only if content changed.
                    tx.execute("UPDATE chapter SET sort_index = ?1 WHERE id = ?2", params![i as i64, id])?;
                    let old: Vec<String> = {
                        let mut s = tx.prepare("SELECT sha256 FROM page WHERE chapter_id = ?1 ORDER BY idx")?;
                        let v = s.query_map([id], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
                        v
                    };
                    let new: Vec<String> = pages.iter().map(|p| p.sha256.clone()).collect();
                    if old == new && *count as usize == pages.len() {
                        unchanged += 1;
                    } else {
                        // Pages changed on disk: keep per-page state for pages whose content is unchanged.
                        tx.execute(
                            "UPDATE page SET idx = -1 - idx WHERE chapter_id = ?1",
                            [id],
                        )?;
                        for (j, p) in pages.iter().enumerate() {
                            let moved = tx.execute(
                                "UPDATE page SET idx = ?1, path = ?2 WHERE id = (
                                    SELECT id FROM page WHERE chapter_id = ?3 AND sha256 = ?4 AND idx < 0 LIMIT 1)",
                                params![j as i64, p.path, id, p.sha256],
                            )?;
                            if moved == 0 {
                                tx.execute(
                                    "INSERT INTO page (chapter_id, idx, path, sha256, width, height, mime)
                                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                                    params![id, j as i64, p.path, p.sha256, p.width, p.height, p.mime],
                                )?;
                                pages_added += 1;
                            }
                        }
                        tx.execute("DELETE FROM page WHERE chapter_id = ?1 AND idx < 0", [id])?;
                        tx.execute(
                            "UPDATE chapter SET page_count = ?1, last_page = MIN(last_page, ?1 - 1) WHERE id = ?2",
                            params![pages.len() as i64, id],
                        )?;
                        updated += 1;
                    }
                }
                None => {
                    tx.execute(
                        "INSERT INTO chapter (manga_id, title, number, volume, sort_index, container, location,
                                              inner_prefix, page_count, added_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            manga_id,
                            ch.title,
                            ch.number,
                            ch.volume,
                            i as i64,
                            ch.container.as_str(),
                            loc,
                            ch.inner_prefix,
                            pages.len() as i64,
                            ts
                        ],
                    )?;
                    let cid = tx.last_insert_rowid();
                    insert_pages(tx, cid, pages)?;
                    added += 1;
                    pages_added += pages.len();
                }
            }
        }
        Ok(ImportReport {
            manga_id,
            title: src.title.clone(),
            chapters_added: added,
            chapters_updated: updated,
            chapters_unchanged: unchanged,
            pages_added,
            warnings: warnings.clone(),
        })
    })
}

/// Add a downloaded chapter (a folder of page images inside the app library).
/// Idempotent per (manga, folder).
pub fn add_folder_chapter(
    db: &Db,
    manga_id: i64,
    dir: &Path,
    title: &str,
    number: Option<f64>,
    source_ref: &str,
) -> AppResult<i64> {
    let mut files: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && super::scan::is_image_name(&p.to_string_lossy()))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    files.sort_by(|a, b| super::natsort::natural_path_cmp(a, b));
    let ch = ChapterSource {
        container: super::scan::Container::Folder,
        location: dir.to_path_buf(),
        inner_prefix: String::new(),
        title: title.to_string(),
        number,
        volume: None,
        pages: files,
    };
    let mut warnings = vec![];
    let pages = read_chapter_pages(&ch, &mut warnings);
    if pages.is_empty() {
        return Err(AppError::new("empty", "downloaded chapter has no readable pages"));
    }
    let loc = dir.to_string_lossy().into_owned();
    let ts = now();
    db.with_tx(|tx| {
        if let Some(id) = tx
            .query_row("SELECT id FROM chapter WHERE manga_id = ?1 AND location = ?2", params![manga_id, loc], |r| r.get::<_, i64>(0))
            .optional()?
        {
            return Ok(id);
        }
        // Order by chapter number among existing chapters.
        let sort_index: i64 = match number {
            Some(n) => tx.query_row(
                "SELECT count(*) FROM chapter WHERE manga_id = ?1 AND coalesce(number, 1e9) <= ?2",
                params![manga_id, n],
                |r| r.get(0),
            )?,
            None => tx.query_row("SELECT count(*) FROM chapter WHERE manga_id = ?1", [manga_id], |r| r.get(0))?,
        };
        tx.execute("UPDATE chapter SET sort_index = sort_index + 1 WHERE manga_id = ?1 AND sort_index >= ?2", params![manga_id, sort_index])?;
        tx.execute(
            "INSERT INTO chapter (manga_id, title, number, sort_index, container, location, source_ref, page_count, added_at)
             VALUES (?1, ?2, ?3, ?4, 'folder', ?5, ?6, ?7, ?8)",
            params![manga_id, title, number, sort_index, loc, source_ref, pages.len() as i64, ts],
        )?;
        let cid = tx.last_insert_rowid();
        insert_pages(tx, cid, &pages)?;
        tx.execute("UPDATE manga SET updated_at = ?1 WHERE id = ?2", params![ts, manga_id])?;
        Ok(cid)
    })
}

/// Chapters whose files disappeared since import (reported, never auto-deleted).
pub fn missing_chapters(db: &Db, manga_id: i64) -> AppResult<Vec<(i64, String)>> {
    let rows: Vec<(i64, String, String)> = db.with(|c| {
        let mut s = c.prepare("SELECT id, title, location FROM chapter WHERE manga_id = ?1")?;
        let v = s.query_map([manga_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })?;
    Ok(rows.into_iter().filter(|(_, _, loc)| !Path::new(loc).exists()).map(|(id, t, _)| (id, t)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn png(seed: u8) -> Vec<u8> {
        let mut img = image::RgbImage::new(8, 12);
        img.pixels_mut().for_each(|p| *p = image::Rgb([seed, 255 - seed, 7]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn write(p: &Path, b: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b).unwrap();
    }

    pub fn make_cbz(path: &Path, n: u8) {
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        for i in 1..=n {
            z.start_file(format!("{i}.png"), zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&png(i)).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn import_cbz_then_read_pages_back() {
        let d = tempfile::tempdir().unwrap();
        let cbz = d.path().join("Sample.cbz");
        make_cbz(&cbz, 12);
        let db = Db::open_in_memory().unwrap();
        let r = import_path(&db, &cbz).unwrap();
        assert_eq!(r.chapters_added, 1);
        assert_eq!(r.pages_added, 12);
        let (loc, path): (String, String) = db
            .with(|c| Ok(c.query_row(
                "SELECT ch.location, p.path FROM page p JOIN chapter ch ON ch.id = p.chapter_id WHERE p.idx = 9",
                [], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap();
        assert_eq!(path, "10.png"); // numeric order, not "10" after "1"
        let bytes = read_page_bytes(crate::library::scan::Container::Archive, Path::new(&loc), &path).unwrap();
        assert_eq!(image_dims(&bytes), (8, 12));
    }

    #[test]
    fn rescan_detects_new_chapters_without_duplicates() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Series");
        write(&root.join("ch1/1.png"), &png(1));
        write(&root.join("ch1/2.png"), &png(2));
        let db = Db::open_in_memory().unwrap();
        let r1 = import_path(&db, &root).unwrap();
        assert_eq!(r1.chapters_added, 1);

        write(&root.join("ch2/1.png"), &png(3));
        let r2 = import_path(&db, &root).unwrap();
        assert_eq!(r2.manga_id, r1.manga_id);
        assert_eq!((r2.chapters_added, r2.chapters_unchanged), (1, 1));

        // A page added to an existing chapter keeps the old page rows.
        let old_id: i64 = db.with(|c| Ok(c.query_row("SELECT id FROM page WHERE path LIKE '%ch1_2.png'", [], |r| r.get(0))?)).unwrap();
        write(&root.join("ch1/3.png"), &png(4));
        let r3 = import_path(&db, &root).unwrap();
        assert_eq!(r3.chapters_updated, 1);
        assert_eq!(r3.pages_added, 1);
        let same_id: i64 = db.with(|c| Ok(c.query_row("SELECT id FROM page WHERE path LIKE '%ch1_2.png'", [], |r| r.get(0))?)).unwrap();
        assert_eq!(old_id, same_id);
        let count: i64 = db.with(|c| Ok(c.query_row("SELECT count(*) FROM manga", [], |r| r.get(0))?)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn corrupt_image_is_skipped_with_warning() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("S");
        write(&root.join("1.png"), &png(1));
        write(&root.join("2.png"), b"garbage");
        let db = Db::open_in_memory().unwrap();
        let r = import_path(&db, &root).unwrap();
        assert_eq!(r.pages_added, 1);
        assert!(r.warnings.iter().any(|w| w.contains("2.png")));
    }

    #[test]
    fn originals_are_not_modified() {
        let d = tempfile::tempdir().unwrap();
        let cbz = d.path().join("Keep.cbz");
        make_cbz(&cbz, 3);
        let before = std::fs::read(&cbz).unwrap();
        let db = Db::open_in_memory().unwrap();
        import_path(&db, &cbz).unwrap();
        import_path(&db, &cbz).unwrap();
        assert_eq!(std::fs::read(&cbz).unwrap(), before);
    }
}
