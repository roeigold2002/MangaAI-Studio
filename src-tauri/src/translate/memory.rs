//! Per-series glossary and translation memory.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::assets::sha256_str;
use crate::db::{now, Db};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlossaryEntry {
    pub id: i64,
    pub manga_id: i64,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub gender: Option<String>,
    pub notes: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryInput {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub gender: Option<String>,
    pub notes: String,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryEntry {
    pub id: i64,
    pub source_text: String,
    pub target_text: String,
    pub origin: String,
    pub created_at: String,
}

pub fn list_glossary(db: &Db, manga_id: i64) -> AppResult<Vec<GlossaryEntry>> {
    db.with(|c| {
        let mut s = c.prepare(
            "SELECT id, manga_id, source, target, kind, gender, notes, status FROM glossary
             WHERE manga_id = ?1 ORDER BY status, kind, source",
        )?;
        let v = s
            .query_map([manga_id], |r| {
                Ok(GlossaryEntry {
                    id: r.get(0)?,
                    manga_id: r.get(1)?,
                    source: r.get(2)?,
                    target: r.get(3)?,
                    kind: r.get(4)?,
                    gender: r.get(5)?,
                    notes: r.get(6)?,
                    status: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })
}

pub fn upsert_glossary(db: &Db, manga_id: i64, e: &GlossaryInput) -> AppResult<i64> {
    let source = e.source.trim();
    let target = e.target.trim();
    if source.is_empty() || target.is_empty() {
        return Err(AppError::invalid("source and target are required"));
    }
    let status = e.status.clone().unwrap_or_else(|| "confirmed".into());
    let ts = now();
    db.with(|c| {
        c.execute(
            "INSERT INTO glossary (manga_id, source, target, kind, gender, notes, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(manga_id, source) DO UPDATE SET target = excluded.target, kind = excluded.kind,
                 gender = excluded.gender, notes = excluded.notes, status = excluded.status, updated_at = excluded.updated_at",
            params![manga_id, source, target, e.kind, e.gender.as_deref().filter(|g| !g.is_empty()), e.notes, status, ts],
        )?;
        Ok(c.query_row("SELECT id FROM glossary WHERE manga_id = ?1 AND source = ?2", params![manga_id, source], |r| r.get(0))?)
    })
}

pub fn delete_glossary(db: &Db, id: i64) -> AppResult<()> {
    db.with(|c| {
        c.execute("DELETE FROM glossary WHERE id = ?1", [id])?;
        Ok(())
    })
}

/// Model suggestions are stored as `suggested` and never override a user entry.
pub fn add_suggestions(db: &Db, manga_id: i64, items: &[super::validate::GlossaryOut]) -> AppResult<usize> {
    let ts = now();
    db.with(|c| {
        let mut n = 0;
        for g in items {
            n += c.execute(
                "INSERT OR IGNORE INTO glossary (manga_id, source, target, kind, gender, notes, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'suggested', ?7, ?7)",
                params![manga_id, g.source.trim(), g.target.trim(), g.kind, Some(&g.gender).filter(|s| !s.is_empty()), g.note, ts],
            )?;
        }
        Ok(n)
    })
}

/// Hash of the confirmed glossary + series notes: part of the translation cache key.
pub fn glossary_version(db: &Db, manga_id: i64) -> AppResult<String> {
    let (entries, notes): (Vec<String>, String) = db.with(|c| {
        let mut s = c.prepare(
            "SELECT source || '\u{1f}' || target || '\u{1f}' || kind || '\u{1f}' || coalesce(gender,'') || '\u{1f}' || notes
             FROM glossary WHERE manga_id = ?1 AND status = 'confirmed' ORDER BY source",
        )?;
        let v = s.query_map([manga_id], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
        let notes: String = c.query_row("SELECT translation_notes FROM manga WHERE id = ?1", [manga_id], |r| r.get(0))?;
        Ok((v, notes))
    })?;
    let mut parts: Vec<&str> = entries.iter().map(|s| s.as_str()).collect();
    parts.push(&notes);
    Ok(sha256_str(&parts)[..16].to_string())
}

pub fn remember(db: &Db, manga_id: i64, source_text: &str, target_text: &str, origin: &str) -> AppResult<()> {
    let s = source_text.trim();
    let t = target_text.trim();
    if s.is_empty() || t.is_empty() {
        return Ok(());
    }
    db.with(|c| {
        c.execute(
            "INSERT INTO translation_memory (manga_id, source_text, target_text, origin, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(manga_id, source_text) DO UPDATE SET target_text = excluded.target_text, origin = excluded.origin",
            params![manga_id, s, t, origin, now()],
        )?;
        Ok(())
    })
}

pub fn lookup(db: &Db, manga_id: i64, source_text: &str) -> AppResult<Option<String>> {
    db.with(|c| {
        Ok(c.query_row(
            "SELECT target_text FROM translation_memory WHERE manga_id = ?1 AND source_text = ?2",
            params![manga_id, source_text.trim()],
            |r| r.get(0),
        )
        .optional()?)
    })
}

pub fn list_memory(db: &Db, manga_id: i64) -> AppResult<Vec<MemoryEntry>> {
    db.with(|c| {
        let mut s = c.prepare(
            "SELECT id, source_text, target_text, origin, created_at FROM translation_memory WHERE manga_id = ?1 ORDER BY created_at DESC",
        )?;
        let v = s
            .query_map([manga_id], |r| {
                Ok(MemoryEntry { id: r.get(0)?, source_text: r.get(1)?, target_text: r.get(2)?, origin: r.get(3)?, created_at: r.get(4)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    })
}

pub fn delete_memory(db: &Db, id: i64) -> AppResult<()> {
    db.with(|c| {
        c.execute("DELETE FROM translation_memory WHERE id = ?1", [id])?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manga(db: &Db) -> i64 {
        db.with(|c| {
            c.execute("INSERT INTO manga (title, source_kind, created_at, updated_at) VALUES ('M','local','x','x')", [])?;
            Ok(c.last_insert_rowid())
        })
        .unwrap()
    }

    #[test]
    fn suggestions_never_override_confirmed_and_dont_change_version() {
        let db = Db::open_in_memory().unwrap();
        let m = manga(&db);
        let v0 = glossary_version(&db, m).unwrap();
        upsert_glossary(&db, m, &GlossaryInput { source: "田中".into(), target: "טנאקה".into(), kind: "character".into(), gender: Some("m".into()), notes: "".into(), status: None }).unwrap();
        let v1 = glossary_version(&db, m).unwrap();
        assert_ne!(v0, v1);
        let sugg = vec![
            super::super::validate::GlossaryOut { source: "田中".into(), target: "טאנקה".into(), kind: "character".into(), gender: "m".into(), note: "".into() },
            super::super::validate::GlossaryOut { source: "魔法".into(), target: "קסם".into(), kind: "term".into(), gender: "".into(), note: "".into() },
        ];
        assert_eq!(add_suggestions(&db, m, &sugg).unwrap(), 1);
        let g = list_glossary(&db, m).unwrap();
        assert_eq!(g.iter().find(|e| e.source == "田中").unwrap().target, "טנאקה");
        assert_eq!(g.iter().find(|e| e.source == "魔法").unwrap().status, "suggested");
        assert_eq!(glossary_version(&db, m).unwrap(), v1, "suggestions are not part of the version");
    }

    #[test]
    fn memory_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let m = manga(&db);
        remember(&db, m, "おはよう", "בוקר טוב", "approved_edit").unwrap();
        remember(&db, m, "おはよう", "בוקר אור", "approved_edit").unwrap();
        assert_eq!(lookup(&db, m, " おはよう ").unwrap().as_deref(), Some("בוקר אור"));
        assert_eq!(list_memory(&db, m).unwrap().len(), 1);
    }
}
