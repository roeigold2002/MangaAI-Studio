use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Ordered list of migrations. Never edit a shipped migration; append a new one.
const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    r#"
CREATE TABLE manga (
    id              INTEGER PRIMARY KEY,
    title           TEXT NOT NULL,
    alt_titles      TEXT NOT NULL DEFAULT '[]',
    description     TEXT,
    author          TEXT,
    artist          TEXT,
    status          TEXT,
    genres          TEXT NOT NULL DEFAULT '[]',
    source_kind     TEXT NOT NULL,              -- 'local' | 'source'
    source_id       TEXT,                       -- adapter source id (e.g. suwayomi:<id>)
    source_ref      TEXT,                       -- adapter manga reference
    import_root     TEXT,                       -- folder/archive path for local imports
    cover_path      TEXT,                       -- explicit cover file, else first page
    reading_mode    TEXT,                       -- per-manga override of reader mode
    reading_dir     TEXT,                       -- 'rtl' | 'ltr' override
    source_lang     TEXT NOT NULL DEFAULT 'ja',
    favorite        INTEGER NOT NULL DEFAULT 0,
    translation_notes TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    last_read_at    TEXT
);
CREATE UNIQUE INDEX manga_import_root ON manga(import_root) WHERE import_root IS NOT NULL;
CREATE UNIQUE INDEX manga_source ON manga(source_id, source_ref) WHERE source_id IS NOT NULL;

CREATE TABLE category (
    id    INTEGER PRIMARY KEY,
    name  TEXT NOT NULL UNIQUE,
    sort  INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE manga_category (
    manga_id    INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    category_id INTEGER NOT NULL REFERENCES category(id) ON DELETE CASCADE,
    PRIMARY KEY (manga_id, category_id)
);

CREATE TABLE chapter (
    id           INTEGER PRIMARY KEY,
    manga_id     INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    number       REAL,
    volume       REAL,
    sort_index   INTEGER NOT NULL,
    container    TEXT NOT NULL,     -- 'folder' | 'archive'
    location     TEXT NOT NULL,     -- folder path, or archive path
    inner_prefix TEXT NOT NULL DEFAULT '',  -- directory inside the archive
    source_ref   TEXT,              -- adapter chapter reference for downloads
    page_count   INTEGER NOT NULL DEFAULT 0,
    read         INTEGER NOT NULL DEFAULT 0,
    last_page    INTEGER NOT NULL DEFAULT 0,
    added_at     TEXT NOT NULL,
    last_read_at TEXT,
    UNIQUE (manga_id, location, inner_prefix)
);
CREATE INDEX chapter_manga ON chapter(manga_id, sort_index);

CREATE TABLE page (
    id            INTEGER PRIMARY KEY,
    chapter_id    INTEGER NOT NULL REFERENCES chapter(id) ON DELETE CASCADE,
    idx           INTEGER NOT NULL,
    path          TEXT NOT NULL,      -- file path (folder) or entry name (archive)
    sha256        TEXT NOT NULL,
    width         INTEGER NOT NULL DEFAULT 0,
    height        INTEGER NOT NULL DEFAULT 0,
    mime          TEXT NOT NULL,
    UNIQUE (chapter_id, idx)
);
CREATE INDEX page_sha ON page(sha256);

CREATE TABLE reading_history (
    id         INTEGER PRIMARY KEY,
    manga_id   INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    chapter_id INTEGER NOT NULL REFERENCES chapter(id) ON DELETE CASCADE,
    page_idx   INTEGER NOT NULL,
    at         TEXT NOT NULL
);
CREATE INDEX history_at ON reading_history(at DESC);

CREATE TABLE setting (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Detection + OCR result per page (one row per page, replaced on re-analysis).
CREATE TABLE page_state (
    page_id          INTEGER PRIMARY KEY REFERENCES page(id) ON DELETE CASCADE,
    status           TEXT NOT NULL DEFAULT 'none', -- none|analyzed|translated|done|failed
    stage            TEXT,                          -- stage that failed or is running
    error            TEXT,
    analysis_key     TEXT,
    translation_key  TEXT,
    clean_key        TEXT,
    render_key       TEXT,
    clean_path       TEXT,
    rendered_path    TEXT,
    prev_rendered_path TEXT,                        -- previous rendered version (compare)
    provider         TEXT,
    model            TEXT,
    prompt_version   TEXT,
    cost_usd         REAL,
    notes            TEXT NOT NULL DEFAULT '[]',     -- warnings (poor inpaint, overflow...)
    summary          TEXT,                          -- model's page summary, context for later pages
    updated_at       TEXT NOT NULL
);

CREATE TABLE region (
    id             INTEGER PRIMARY KEY,
    page_id        INTEGER NOT NULL REFERENCES page(id) ON DELETE CASCADE,
    rid            TEXT NOT NULL,      -- stable id within the page: r1, r2...
    order_idx      INTEGER NOT NULL,
    kind           TEXT NOT NULL,      -- bubble|free
    bbox           TEXT NOT NULL,      -- [x1,y1,x2,y2] text box
    bubble         TEXT,               -- [x1,y1,x2,y2] enclosing bubble, if any
    parts          TEXT NOT NULL DEFAULT '[]', -- detected text boxes merged into this region
    vertical       INTEGER NOT NULL DEFAULT 0,
    ocr_text       TEXT NOT NULL DEFAULT '',
    ocr_confidence REAL,
    source_text    TEXT,               -- user-corrected OCR text, overrides ocr_text
    translation    TEXT,
    category       TEXT,               -- dialogue|thought|narration|sfx|sign|other
    speaker        TEXT,
    confidence     TEXT,               -- high|medium|low
    note           TEXT,
    approved       INTEGER NOT NULL DEFAULT 0,
    skip           INTEGER NOT NULL DEFAULT 0, -- leave original text untouched
    layout         TEXT NOT NULL DEFAULT '{}', -- manual overrides: font_size, dx, dy, align, box
    history        TEXT NOT NULL DEFAULT '[]', -- previous translations
    updated_at     TEXT NOT NULL,
    UNIQUE (page_id, rid)
);
CREATE INDEX region_page ON region(page_id, order_idx);

CREATE TABLE glossary (
    id         INTEGER PRIMARY KEY,
    manga_id   INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    source     TEXT NOT NULL,
    target     TEXT NOT NULL,
    kind       TEXT NOT NULL DEFAULT 'term', -- character|term|place|attack|organization|honorific|other
    gender     TEXT,                         -- m|f|n for characters (Hebrew agreement)
    notes      TEXT NOT NULL DEFAULT '',
    status     TEXT NOT NULL DEFAULT 'confirmed', -- confirmed|suggested
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (manga_id, source)
);

CREATE TABLE translation_memory (
    id          INTEGER PRIMARY KEY,
    manga_id    INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    source_text TEXT NOT NULL,
    target_text TEXT NOT NULL,
    origin      TEXT NOT NULL,       -- approved_edit|confirmed
    created_at  TEXT NOT NULL,
    UNIQUE (manga_id, source_text)
);

CREATE TABLE job (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL,          -- translate
    manga_id    INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    chapter_id  INTEGER NOT NULL REFERENCES chapter(id) ON DELETE CASCADE,
    page_ids    TEXT,                   -- JSON array subset, NULL = whole chapter
    mode        TEXT NOT NULL DEFAULT 'missing', -- missing|failed|all
    priority    INTEGER NOT NULL DEFAULT 0,
    status      TEXT NOT NULL,          -- queued|running|paused|done|failed|cancelled
    total       INTEGER NOT NULL DEFAULT 0,
    completed   INTEGER NOT NULL DEFAULT 0,
    failed      INTEGER NOT NULL DEFAULT 0,
    cost_usd    REAL NOT NULL DEFAULT 0,
    provider    TEXT,
    model       TEXT,
    error       TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX job_status ON job(status, priority DESC, id);

CREATE TABLE usage_log (
    id            INTEGER PRIMARY KEY,
    at            TEXT NOT NULL,
    provider      TEXT NOT NULL,
    model         TEXT NOT NULL,
    input_tokens  INTEGER,
    output_tokens INTEGER,
    cost_usd      REAL,
    job_id        INTEGER
);

CREATE TABLE download (
    id             INTEGER PRIMARY KEY,
    source_id      TEXT NOT NULL,
    manga_ref      TEXT NOT NULL,
    manga_title    TEXT NOT NULL,
    chapter_ref    TEXT NOT NULL,
    chapter_title  TEXT NOT NULL,
    chapter_number REAL,
    manga_id       INTEGER REFERENCES manga(id) ON DELETE SET NULL,
    chapter_id     INTEGER REFERENCES chapter(id) ON DELETE SET NULL,
    status         TEXT NOT NULL,     -- queued|running|done|failed|cancelled
    progress       INTEGER NOT NULL DEFAULT 0,
    total          INTEGER NOT NULL DEFAULT 0,
    error          TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    UNIQUE (source_id, chapter_ref)
);
"#,
    // 2: download priority ("read now" jumps the queue)
    r#"
ALTER TABLE download ADD COLUMN priority INTEGER NOT NULL DEFAULT 0;
"#,
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(AppError::new(
            "db_version",
            format!("database schema v{current} is newer than this app (v{SCHEMA_VERSION})"),
        ));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = i as i64 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(sql).map_err(|e| {
            AppError::new("db_migration", format!("migration {version} failed: {e}"))
        })?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::info!(version, "applied migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_fresh_db_and_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE type='table'", [], |r| r.get(0))
            .unwrap();
        assert!(n >= 14);
    }

    #[test]
    fn refuses_newer_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5).unwrap();
        let err = migrate(&mut conn).unwrap_err();
        assert_eq!(err.code, "db_version");
    }
}
