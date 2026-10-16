//! Backup and sync between computers.
//!
//! Sync: a folder both computers can reach (OneDrive, Google Drive, Dropbox, a network share)
//! holds one state file. Syncing merges that file into the local library, then writes the merged
//! state back, so every computer converges. What travels is what a person builds up while reading:
//! progress and read marks, favorites and categories, the glossary and translation memory, and the
//! list of online series. Pages and translations stay on each computer (downloads and the
//! new-chapter check fetch them). Merging is by stable identity: a series by its source and
//! reference (or title for local imports), a chapter by its number/title key.
//!
//! Backup: a ZIP with a consistent snapshot of the database (and optionally the downloaded
//! chapters). Restoring stages the database; it replaces the live one on the next start.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::chapter_pick::chapter_key;
use crate::db::{now, Db};
use crate::error::{AppError, AppResult};

pub const FORMAT: &str = "mangaai-sync/1";
pub const STATE_FILE: &str = "mangaai-sync.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SyncState {
    pub format: String,
    pub device: String,
    pub written_at: String,
    pub manga: Vec<MangaState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MangaState {
    /// Online series: source id and reference. Local imports: None (matched by title).
    pub source_id: Option<String>,
    pub source_ref: Option<String>,
    pub title: String,
    pub source_lang: String,
    pub reading_dir: Option<String>,
    pub translation_notes: String,
    pub favorite: bool,
    pub updated_at: String,
    pub last_read_at: Option<String>,
    pub categories: Vec<String>,
    pub chapters: Vec<ChapterState>,
    pub glossary: Vec<GlossaryState>,
    pub memory: Vec<MemoryState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ChapterState {
    pub key: String,
    pub read: bool,
    pub last_page: i64,
    pub last_read_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GlossaryState {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub gender: Option<String>,
    pub notes: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MemoryState {
    pub source_text: String,
    pub target_text: String,
    pub origin: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct MergeReport {
    /// Online series that were only on the other computer and were added here.
    pub series_added: Vec<String>,
    /// Local imports that exist only on the other computer (their files are not here).
    pub series_elsewhere: Vec<String>,
    pub chapters_updated: usize,
    pub glossary_updated: usize,
    pub memory_added: usize,
    pub favorites_changed: usize,
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "this computer".into())
}

fn newer(a: &Option<String>, b: &Option<String>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        _ => false,
    }
}

/// Everything worth carrying to another computer.
pub fn export_state(db: &Db) -> AppResult<SyncState> {
    db.with(|c| {
        let mut out = SyncState { format: FORMAT.into(), device: device_name(), written_at: now(), manga: vec![] };
        let mut ms = c.prepare(
            "SELECT id, source_kind, source_id, source_ref, title, source_lang, reading_dir, translation_notes, favorite, updated_at, last_read_at
             FROM manga ORDER BY id",
        )?;
        let rows = ms
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    MangaState {
                        source_id: if r.get::<_, String>(1)? == "source" { r.get(2)? } else { None },
                        source_ref: if r.get::<_, String>(1)? == "source" { r.get(3)? } else { None },
                        title: r.get(4)?,
                        source_lang: r.get(5)?,
                        reading_dir: r.get(6)?,
                        translation_notes: r.get(7)?,
                        favorite: r.get::<_, i64>(8)? != 0,
                        updated_at: r.get(9)?,
                        last_read_at: r.get(10)?,
                        ..Default::default()
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, mut m) in rows {
            let mut s = c.prepare("SELECT c.name FROM manga_category mc JOIN category c ON c.id = mc.category_id WHERE mc.manga_id = ?1 ORDER BY c.name")?;
            m.categories = s.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
            // one entry per chapter key: the most advanced of its versions
            let mut s = c.prepare("SELECT number, title, read, last_page, last_read_at FROM chapter WHERE manga_id = ?1 ORDER BY sort_index")?;
            let mut by_key: Vec<ChapterState> = vec![];
            for row in s.query_map([id], |r| {
                Ok(ChapterState {
                    key: chapter_key(r.get(0)?, &r.get::<_, String>(1)?),
                    read: r.get::<_, i64>(2)? != 0,
                    last_page: r.get(3)?,
                    last_read_at: r.get(4)?,
                })
            })? {
                let ch = row?;
                match by_key.iter_mut().find(|x| x.key == ch.key) {
                    Some(x) => {
                        if newer(&ch.last_read_at, &x.last_read_at) {
                            x.last_page = ch.last_page;
                            x.last_read_at = ch.last_read_at.clone();
                        }
                        x.read |= ch.read;
                    }
                    None => by_key.push(ch),
                }
            }
            m.chapters = by_key.into_iter().filter(|c| c.read || c.last_read_at.is_some()).collect();
            let mut s = c.prepare("SELECT source, target, kind, gender, notes, status, updated_at FROM glossary WHERE manga_id = ?1 ORDER BY id")?;
            m.glossary = s
                .query_map([id], |r| {
                    Ok(GlossaryState {
                        source: r.get(0)?,
                        target: r.get(1)?,
                        kind: r.get(2)?,
                        gender: r.get(3)?,
                        notes: r.get(4)?,
                        status: r.get(5)?,
                        updated_at: r.get(6)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            let mut s = c.prepare("SELECT source_text, target_text, origin, created_at FROM translation_memory WHERE manga_id = ?1 ORDER BY id")?;
            m.memory = s
                .query_map([id], |r| Ok(MemoryState { source_text: r.get(0)?, target_text: r.get(1)?, origin: r.get(2)?, created_at: r.get(3)? }))?
                .collect::<Result<_, _>>()?;
            out.manga.push(m);
        }
        Ok(out)
    })
}

/// Merge another computer's state into this library. Newer wins per item; read marks,
/// categories, glossary entries and memory are never removed by a merge.
pub fn merge_state(db: &Db, remote: &SyncState) -> AppResult<MergeReport> {
    if remote.format != FORMAT {
        return Err(AppError::invalid(format!("unknown sync format '{}'", remote.format)));
    }
    let mut rep = MergeReport::default();
    db.with_tx(|tx| {
        for m in &remote.manga {
            let local: Option<(i64, bool, String)> = match (&m.source_id, &m.source_ref) {
                (Some(sid), Some(sref)) => tx
                    .query_row("SELECT id, favorite, updated_at FROM manga WHERE source_id = ?1 AND source_ref = ?2", params![sid, sref], |r| {
                        Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get(2)?))
                    })
                    .optional()?,
                _ => tx
                    .query_row(
                        "SELECT id, favorite, updated_at FROM manga WHERE source_kind = 'local' AND lower(title) = lower(?1)",
                        [&m.title],
                        |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get(2)?)),
                    )
                    .optional()?,
            };
            let manga_id = match local {
                Some((id, fav, updated)) => {
                    if fav != m.favorite && m.updated_at > updated {
                        tx.execute("UPDATE manga SET favorite = ?1 WHERE id = ?2", params![m.favorite as i64, id])?;
                        rep.favorites_changed += 1;
                    }
                    if m.updated_at > updated {
                        tx.execute(
                            "UPDATE manga SET translation_notes = ?1, reading_dir = coalesce(?2, reading_dir), source_lang = ?3, updated_at = ?4 WHERE id = ?5",
                            params![m.translation_notes, m.reading_dir, m.source_lang, m.updated_at, id],
                        )?;
                    }
                    id
                }
                None if m.source_id.is_some() && m.source_ref.is_some() => {
                    tx.execute(
                        "INSERT INTO manga (title, source_kind, source_id, source_ref, source_lang, reading_dir, translation_notes, favorite, created_at, updated_at, last_read_at)
                         VALUES (?1, 'source', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![m.title, m.source_id, m.source_ref, m.source_lang, m.reading_dir, m.translation_notes, m.favorite as i64, now(), m.updated_at, m.last_read_at],
                    )?;
                    rep.series_added.push(m.title.clone());
                    tx.last_insert_rowid()
                }
                None => {
                    rep.series_elsewhere.push(m.title.clone());
                    continue;
                }
            };
            tx.execute(
                "UPDATE manga SET last_read_at = ?1 WHERE id = ?2 AND (last_read_at IS NULL OR last_read_at < ?1)",
                params![m.last_read_at, manga_id],
            )?;
            for name in &m.categories {
                tx.execute("INSERT OR IGNORE INTO category (name) VALUES (?1)", [name])?;
                tx.execute(
                    "INSERT OR IGNORE INTO manga_category (manga_id, category_id) SELECT ?1, id FROM category WHERE name = ?2",
                    params![manga_id, name],
                )?;
            }
            // chapters: every local version of a chapter key takes the newer progress
            let mut s = tx.prepare("SELECT id, number, title, read, last_read_at FROM chapter WHERE manga_id = ?1")?;
            let local_chapters: Vec<(i64, String, bool, Option<String>)> = s
                .query_map([manga_id], |r| Ok((r.get(0)?, chapter_key(r.get(1)?, &r.get::<_, String>(2)?), r.get::<_, i64>(3)? != 0, r.get(4)?)))?
                .collect::<Result<_, _>>()?;
            drop(s);
            let wanted: HashMap<&str, &ChapterState> = m.chapters.iter().map(|c| (c.key.as_str(), c)).collect();
            for (id, key, read, last_read_at) in local_chapters {
                let Some(rc) = wanted.get(key.as_str()) else { continue };
                let take_progress = newer(&rc.last_read_at, &last_read_at);
                if take_progress || (rc.read && !read) {
                    tx.execute(
                        "UPDATE chapter SET read = max(read, ?1),
                                last_page = CASE WHEN ?2 THEN ?3 ELSE last_page END,
                                last_read_at = CASE WHEN ?2 THEN ?4 ELSE last_read_at END
                         WHERE id = ?5",
                        params![rc.read as i64, take_progress, rc.last_page, rc.last_read_at, id],
                    )?;
                    rep.chapters_updated += 1;
                }
            }
            for g in &m.glossary {
                let cur: Option<(i64, String)> = tx
                    .query_row("SELECT id, updated_at FROM glossary WHERE manga_id = ?1 AND source = ?2", params![manga_id, g.source], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?;
                match cur {
                    Some((_, updated)) if updated >= g.updated_at => {}
                    Some((gid, _)) => {
                        tx.execute(
                            "UPDATE glossary SET target = ?1, kind = ?2, gender = ?3, notes = ?4, status = ?5, updated_at = ?6 WHERE id = ?7",
                            params![g.target, g.kind, g.gender, g.notes, g.status, g.updated_at, gid],
                        )?;
                        rep.glossary_updated += 1;
                    }
                    None => {
                        tx.execute(
                            "INSERT INTO glossary (manga_id, source, target, kind, gender, notes, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                            params![manga_id, g.source, g.target, g.kind, g.gender, g.notes, g.status, g.updated_at],
                        )?;
                        rep.glossary_updated += 1;
                    }
                }
            }
            for mm in &m.memory {
                rep.memory_added += tx.execute(
                    "INSERT OR IGNORE INTO translation_memory (manga_id, source_text, target_text, origin, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![manga_id, mm.source_text, mm.target_text, mm.origin, mm.created_at],
                )?;
            }
        }
        Ok(())
    })?;
    Ok(rep)
}

/// Merge the folder's state into this library, then write the merged state back.
pub fn sync_folder(db: &Db, folder: &Path) -> AppResult<MergeReport> {
    if !folder.is_dir() {
        return Err(AppError::new("sync_folder", format!("the sync folder is not reachable: {}", folder.display())));
    }
    let file = folder.join(STATE_FILE);
    let rep = match std::fs::read(&file) {
        Ok(bytes) => {
            let remote: SyncState = serde_json::from_slice(&bytes).map_err(|e| AppError::new("sync_file", format!("{}: {e}", file.display())))?;
            merge_state(db, &remote)?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => MergeReport::default(),
        Err(e) => return Err(e.into()),
    };
    let state = export_state(db)?;
    crate::assets::atomic_write(&file, serde_json::to_string_pretty(&state)?.as_bytes())?;
    Ok(rep)
}

// ---------------------------------------------------------------- backup

#[derive(Debug, Clone, Serialize)]
pub struct BackupReport {
    pub path: String,
    pub bytes: u64,
    pub files: usize,
}

/// Write a backup ZIP: a consistent copy of the database, the readable sync state, and with
/// `include_chapters` the downloaded chapters (`library/`). Secrets are never included.
pub fn create_backup(db: &Db, root: &Path, out: &Path, include_chapters: bool) -> AppResult<BackupReport> {
    let tmp = tempfile::tempdir()?;
    let snapshot = tmp.path().join("library.db");
    db.with(|c| {
        c.execute("VACUUM INTO ?1", [snapshot.to_string_lossy()])?;
        Ok(())
    })?;
    let part = out.with_extension("zip.part");
    let f = std::fs::File::create(&part)?;
    let mut zip = zip::ZipWriter::new(f);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut files = 0;
    let mut add = |zip: &mut zip::ZipWriter<std::fs::File>, name: &str, path: &Path, opts| -> AppResult<()> {
        zip.start_file(name, opts).map_err(|e| AppError::new("backup", e.to_string()))?;
        let mut src = std::fs::File::open(path)?;
        std::io::copy(&mut src, zip)?;
        files += 1;
        Ok(())
    };
    add(&mut zip, "library.db", &snapshot, deflated)?;
    zip.start_file(STATE_FILE, deflated).map_err(|e| AppError::new("backup", e.to_string()))?;
    zip.write_all(serde_json::to_string_pretty(&export_state(db)?)?.as_bytes())?;
    if include_chapters {
        let lib = root.join("library");
        for e in walkdir::WalkDir::new(&lib).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
            let rel = e.path().strip_prefix(root).unwrap_or(e.path());
            let name = rel.to_string_lossy().replace('\\', "/");
            if name.contains(".part/") || name.ends_with(".tmp") {
                continue;
            }
            add(&mut zip, &name, e.path(), stored)?; // images are already compressed
        }
    }
    zip.finish().map_err(|e| AppError::new("backup", e.to_string()))?;
    std::fs::rename(&part, out)?;
    Ok(BackupReport { path: out.to_string_lossy().into_owned(), bytes: std::fs::metadata(out)?.len(), files })
}

const PENDING_RESTORE: &str = "library.db.restore";

/// Check a backup and stage it. The database is swapped in on the next start (it is open now);
/// chapter files are extracted next to the existing ones (nothing is overwritten).
pub fn stage_restore(root: &Path, backup: &Path) -> AppResult<usize> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(backup)?).map_err(|e| AppError::new("backup", format!("not a backup file: {e}")))?;
    let mut db_bytes = vec![];
    zip.by_name("library.db")
        .map_err(|_| AppError::new("backup", "this ZIP is not a MangaAI Studio backup (library.db is missing)"))?
        .read_to_end(&mut db_bytes)?;
    if !db_bytes.starts_with(b"SQLite format 3\0") {
        return Err(AppError::new("backup", "the database in this backup is damaged"));
    }
    let mut extracted = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| AppError::new("backup", e.to_string()))?;
        let Some(rel) = f.enclosed_name() else { continue }; // refuses ../ paths
        if !rel.starts_with("library") || f.is_dir() {
            continue;
        }
        let dest = root.join(&rel);
        if dest.exists() {
            continue;
        }
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut f, &mut out)?;
        extracted += 1;
    }
    crate::assets::atomic_write(&root.join(PENDING_RESTORE), &db_bytes)?;
    Ok(extracted)
}

/// Called before the database is opened: swap in a staged restore, keeping the old database.
pub fn apply_pending_restore(root: &Path) -> AppResult<Option<PathBuf>> {
    let staged = root.join(PENDING_RESTORE);
    if !staged.exists() {
        return Ok(None);
    }
    let db = root.join("library.db");
    let keep = root.join(format!("library.db.before-restore-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
    if db.exists() {
        std::fs::rename(&db, &keep)?;
        for side in ["-wal", "-shm"] {
            let p = root.join(format!("library.db{side}"));
            if p.exists() {
                let _ = std::fs::rename(&p, root.join(format!("{}{side}", keep.file_name().unwrap().to_string_lossy())));
            }
        }
    }
    std::fs::rename(&staged, &db)?;
    Ok(Some(keep))
}

/// Categories of a state (for display).
pub fn category_names(s: &SyncState) -> BTreeSet<String> {
    s.manga.iter().flat_map(|m| m.categories.iter().cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::RemoteManga;

    fn online(db: &Db, r: &str, title: &str) -> i64 {
        let m = RemoteManga {
            source_id: "src".into(),
            manga_ref: r.into(),
            title: title.into(),
            thumbnail_url: None,
            author: None,
            artist: None,
            description: None,
            genres: vec![],
            status: None,
        };
        crate::downloads::local_manga(db, &m).unwrap()
    }

    fn chapter(db: &Db, manga: i64, n: f64) -> i64 {
        db.with(|c| {
            c.execute(
                "INSERT INTO chapter (manga_id, title, number, sort_index, container, location, added_at) VALUES (?1, ?2, ?3, ?4, 'folder', ?5, ?6)",
                params![manga, format!("Ch. {n}"), n, (n * 10.0) as i64, format!("dl-{manga}-{n}"), now()],
            )?;
            Ok(c.last_insert_rowid())
        })
        .unwrap()
    }

    fn progress(db: &Db, id: i64) -> (bool, i64) {
        db.with(|c| Ok(c.query_row("SELECT read, last_page FROM chapter WHERE id = ?1", [id], |r| Ok((r.get::<_, i64>(0)? != 0, r.get(1)?)))?)).unwrap()
    }

    #[test]
    fn two_computers_converge_through_a_folder() {
        let folder = tempfile::tempdir().unwrap();
        let (a, b) = (Db::open_in_memory().unwrap(), Db::open_in_memory().unwrap());

        // computer A: reads the series, keeps a glossary, favorites it
        let ma = online(&a, "7", "Lemon");
        let (a1, a2) = (chapter(&a, ma, 1.0), chapter(&a, ma, 2.0));
        a.with(|c| {
            c.execute("UPDATE chapter SET read = 1, last_page = 17, last_read_at = '2026-10-16T08:00:00Z' WHERE id = ?1", [a1])?;
            c.execute("UPDATE chapter SET last_page = 5, last_read_at = '2026-10-16T09:00:00Z' WHERE id = ?1", [a2])?;
            c.execute("UPDATE manga SET favorite = 1, updated_at = '2026-10-16T09:00:00Z', last_read_at = '2026-10-16T09:00:00Z' WHERE id = ?1", [ma])?;
            c.execute("INSERT INTO glossary (manga_id, source, target, kind, created_at, updated_at) VALUES (?1, 'Akiyoshi', 'אקיושי', 'character', 'x', '2026-10-16T09:00:00Z')", [ma])?;
            c.execute("INSERT INTO translation_memory (manga_id, source_text, target_text, origin, created_at) VALUES (?1, 'HEY!', 'היי!', 'approved_edit', 'x')", [ma])?;
            c.execute("INSERT INTO category (name) VALUES ('Reading')", [])?;
            c.execute("INSERT INTO manga_category (manga_id, category_id) SELECT ?1, id FROM category", [ma])?;
            c.execute(
                "INSERT INTO manga (title, source_kind, import_root, created_at, updated_at) VALUES ('My Scans', 'local', 'C:/scans', 'x', 'x')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        // computer B: has the series with one more chapter, plus a series A does not have
        let mb = online(&b, "7", "Lemon");
        let (b1, b2, b3) = (chapter(&b, mb, 1.0), chapter(&b, mb, 2.0), chapter(&b, mb, 3.0));
        b.with(|c| {
            c.execute("UPDATE manga SET updated_at = '2026-10-01T00:00:00Z' WHERE id = ?1", [mb])?;
            Ok(())
        })
        .unwrap();
        online(&b, "9", "Other");

        let first = sync_folder(&a, folder.path()).unwrap();
        assert_eq!(first, MergeReport::default(), "nothing in the folder yet");
        let rep = sync_folder(&b, folder.path()).unwrap();
        assert_eq!(rep.chapters_updated, 2);
        assert_eq!(rep.series_elsewhere, vec!["My Scans".to_string()], "local files cannot travel");
        assert_eq!(progress(&b, b1), (true, 17));
        assert_eq!(progress(&b, b2), (false, 5));
        assert_eq!(progress(&b, b3), (false, 0));
        let (fav, cats, gloss, mem): (i64, i64, String, i64) = b
            .with(|c| {
                Ok((
                    c.query_row("SELECT favorite FROM manga WHERE id = ?1", [mb], |r| r.get(0))?,
                    c.query_row("SELECT count(*) FROM manga_category", [], |r| r.get(0))?,
                    c.query_row("SELECT target FROM glossary WHERE manga_id = ?1", [mb], |r| r.get(0))?,
                    c.query_row("SELECT count(*) FROM translation_memory", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!((fav, cats, gloss.as_str(), mem), (1, 1, "אקיושי", 1));

        // back on A: the series that was only on B is added; a second sync changes nothing
        let rep = sync_folder(&a, folder.path()).unwrap();
        assert_eq!(rep.series_added, vec!["Other".to_string()]);
        assert_eq!(rep.chapters_updated, 0, "A already had the newest progress");
        let again = sync_folder(&a, folder.path()).unwrap();
        assert!(again.series_added.is_empty() && again.chapters_updated == 0 && again.glossary_updated == 0 && again.memory_added == 0);
    }

    #[test]
    fn newer_progress_wins_and_read_marks_are_kept() {
        let (a, b) = (Db::open_in_memory().unwrap(), Db::open_in_memory().unwrap());
        let (ma, mb) = (online(&a, "1", "S"), online(&b, "1", "S"));
        let (a1, b1) = (chapter(&a, ma, 1.0), chapter(&b, mb, 1.0));
        a.with(|c| { c.execute("UPDATE chapter SET read = 1, last_page = 3, last_read_at = '2026-10-10' WHERE id = ?1", [a1])?; Ok(()) }).unwrap();
        b.with(|c| { c.execute("UPDATE chapter SET last_page = 9, last_read_at = '2026-10-12' WHERE id = ?1", [b1])?; Ok(()) }).unwrap();
        merge_state(&b, &export_state(&a).unwrap()).unwrap();
        assert_eq!(progress(&b, b1), (true, 9), "older remote: the page stays, the read mark is added");
        merge_state(&a, &export_state(&b).unwrap()).unwrap();
        assert_eq!(progress(&a, a1), (true, 9), "newer remote: its page wins");
    }

    #[test]
    fn backup_restores_on_next_start() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("library.db")).unwrap();
        let m = online(&db, "1", "Kept");
        let file = root.path().join("library").join(m.to_string()).join("dl-1").join("001.jpg");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"jpeg").unwrap();
        let out = root.path().join("backup.zip");
        let rep = create_backup(&db, root.path(), &out, true).unwrap();
        assert_eq!(rep.files, 2, "database + one page");

        // a fresh computer: empty folder, restore the backup, then "restart"
        let other = tempfile::tempdir().unwrap();
        drop(Db::open(&other.path().join("library.db")).unwrap());
        assert_eq!(stage_restore(other.path(), &out).unwrap(), 1);
        assert!(other.path().join("library").join(m.to_string()).join("dl-1").join("001.jpg").exists());
        let kept = apply_pending_restore(other.path()).unwrap().expect("the old database is kept");
        assert!(kept.exists());
        let restored = Db::open(&other.path().join("library.db")).unwrap();
        let title: String = restored.with(|c| Ok(c.query_row("SELECT title FROM manga", [], |r| r.get(0))?)).unwrap();
        assert_eq!(title, "Kept");
        assert_eq!(apply_pending_restore(other.path()).unwrap(), None, "applied once");

        let junk = other.path().join("junk.zip");
        std::fs::write(&junk, b"not a zip").unwrap();
        assert!(stage_restore(other.path(), &junk).is_err());
    }
}
