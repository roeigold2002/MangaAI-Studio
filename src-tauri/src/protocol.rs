//! `mangaimg://` image protocol. Serves only images that are registered in the
//! database (by id), never arbitrary paths.

use rusqlite::OptionalExtension;
use std::path::PathBuf;

use crate::assets::{atomic_write, make_thumbnail, mime_for, read_page_bytes, Paths};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::library::scan::Container;

pub struct Reply {
    pub status: u16,
    pub mime: String,
    pub body: Vec<u8>,
    pub cache: &'static str,
}

impl Reply {
    fn ok(mime: &str, body: Vec<u8>, cache: &'static str) -> Self {
        Self { status: 200, mime: mime.to_string(), body, cache }
    }
    pub fn error(status: u16, msg: &str) -> Self {
        Self { status, mime: "text/plain".into(), body: msg.as_bytes().to_vec(), cache: "no-store" }
    }
}

struct PageRow {
    path: String,
    sha: String,
    mime: String,
    container: Container,
    location: PathBuf,
}

fn page_row(db: &Db, id: i64) -> AppResult<PageRow> {
    db.with(|c| {
        Ok(c.query_row(
            "SELECT p.path, p.sha256, p.mime, ch.container, ch.location FROM page p JOIN chapter ch ON ch.id = p.chapter_id WHERE p.id = ?1",
            [id],
            |r| {
                Ok(PageRow {
                    path: r.get(0)?,
                    sha: r.get(1)?,
                    mime: r.get(2)?,
                    container: Container::parse(&r.get::<_, String>(3)?),
                    location: PathBuf::from(r.get::<_, String>(4)?),
                })
            },
        )?)
    })
}

fn thumb(paths: &Paths, key: &str, width: u32, load: impl FnOnce() -> AppResult<Vec<u8>>) -> AppResult<Vec<u8>> {
    let p = paths.thumbs_dir().join(format!("{key}_{width}.jpg"));
    if let Ok(b) = std::fs::read(&p) {
        return Ok(b);
    }
    let t = make_thumbnail(&load()?, width)?;
    atomic_write(&p, &t)?;
    Ok(t)
}

fn state_path(db: &Db, page_id: i64, col: &str) -> AppResult<Option<String>> {
    let sql = match col {
        "rendered" => "SELECT rendered_path FROM page_state WHERE page_id = ?1",
        "previous" => "SELECT prev_rendered_path FROM page_state WHERE page_id = ?1",
        "clean" => "SELECT clean_path FROM page_state WHERE page_id = ?1",
        _ => return Err(AppError::invalid("bad column")),
    };
    db.with(|c| Ok(c.query_row(sql, [page_id], |r| r.get::<_, Option<String>>(0)).optional()?.flatten()))
}

/// Path is like `/page/12`, `/page/12/translated`, `/thumb/page/12`, `/cover/3`.
pub fn handle(db: &Db, paths: &Paths, path: &str) -> Reply {
    match handle_inner(db, paths, path) {
        Ok(r) => r,
        Err(e) if e.code == "not_found" => Reply::error(404, &e.message),
        Err(e) => {
            tracing::warn!("image request {path} failed: {}", e.message);
            Reply::error(500, &e.message)
        }
    }
}

fn handle_inner(db: &Db, paths: &Paths, path: &str) -> AppResult<Reply> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let id = |s: &str| s.parse::<i64>().map_err(|_| AppError::not_found("image"));
    match parts.as_slice() {
        ["page", pid] => {
            let p = page_row(db, id(pid)?)?;
            let bytes = read_page_bytes(p.container, &p.location, &p.path)?;
            Ok(Reply::ok(&p.mime, bytes, "max-age=86400"))
        }
        ["page", pid, kind @ ("translated" | "previous" | "clean")] => {
            let col = if *kind == "translated" { "rendered" } else { kind };
            let f = state_path(db, id(pid)?, col)?.ok_or_else(|| AppError::not_found("translated page"))?;
            let bytes = std::fs::read(&f).map_err(|_| AppError::not_found("translated file"))?;
            Ok(Reply::ok(mime_for(&f), bytes, "max-age=86400"))
        }
        ["thumb", "page", pid] => {
            let p = page_row(db, id(pid)?)?;
            let sha = p.sha.clone();
            let b = thumb(paths, &sha, 360, || read_page_bytes(p.container, &p.location, &p.path))?;
            Ok(Reply::ok("image/jpeg", b, "max-age=86400"))
        }
        ["cover", mid] => {
            let mid = id(mid)?;
            let cover: Option<String> = db.with(|c| Ok(c.query_row("SELECT cover_path FROM manga WHERE id = ?1", [mid], |r| r.get(0))?))?;
            if let Some(cp) = cover.filter(|p| std::path::Path::new(p).exists()) {
                let key = crate::assets::sha256_str(&[&cp]);
                let b = thumb(paths, &key, 480, || Ok(std::fs::read(&cp)?))?;
                return Ok(Reply::ok("image/jpeg", b, "max-age=3600"));
            }
            let first: Option<i64> = db.with(|c| {
                Ok(c.query_row(
                    "SELECT p.id FROM page p JOIN chapter ch ON ch.id = p.chapter_id WHERE ch.manga_id = ?1
                     ORDER BY ch.sort_index, ch.id, p.idx LIMIT 1",
                    [mid],
                    |r| r.get(0),
                )
                .optional()?)
            })?;
            let pid = first.ok_or_else(|| AppError::not_found("cover"))?;
            let p = page_row(db, pid)?;
            let sha = p.sha.clone();
            let b = thumb(paths, &sha, 480, || read_page_bytes(p.container, &p.location, &p.path))?;
            Ok(Reply::ok("image/jpeg", b, "max-age=3600"))
        }
        _ => Err(AppError::not_found("image")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::import::import_path;

    #[test]
    fn serves_registered_pages_only() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("M");
        std::fs::create_dir_all(&root).unwrap();
        image::RgbImage::new(20, 30).save(root.join("1.png")).unwrap();
        let db = Db::open_in_memory().unwrap();
        import_path(&db, &root).unwrap();
        let paths = Paths::new(d.path().join("app")).unwrap();
        let pid: i64 = db.with(|c| Ok(c.query_row("SELECT id FROM page", [], |r| r.get(0))?)).unwrap();
        let r = handle(&db, &paths, &format!("/page/{pid}"));
        assert_eq!((r.status, r.mime.as_str()), (200, "image/png"));
        assert_eq!(handle(&db, &paths, "/thumb/page/1").status, 200);
        assert_eq!(handle(&db, &paths, "/cover/1").status, 200);
        assert_eq!(handle(&db, &paths, &format!("/page/{pid}/translated")).status, 404);
        assert_eq!(handle(&db, &paths, "/page/999").status, 404);
        assert_eq!(handle(&db, &paths, "/../../etc/passwd").status, 404);
    }
}
