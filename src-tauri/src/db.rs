use crate::models::{Settings, Wallpaper};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    // journal_mode returns a row, so it must be queried rather than executed.
    let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    // Columns added after the first release are applied to existing databases
    // below; the CREATE statement carries them for fresh installs.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS wallpapers (
            id            TEXT PRIMARY KEY,
            provider      TEXT NOT NULL,
            remote_id     TEXT NOT NULL,
            file_name     TEXT NOT NULL,
            width         INTEGER NOT NULL,
            height        INTEGER NOT NULL,
            source_width  INTEGER NOT NULL DEFAULT 0,
            source_height INTEGER NOT NULL DEFAULT 0,
            format        TEXT NOT NULL DEFAULT '',
            bytes         INTEGER NOT NULL,
            downloaded_at INTEGER NOT NULL,
            category      TEXT NOT NULL,
            author_name   TEXT,
            author_url    TEXT,
            source_url    TEXT,
            is_favorite   INTEGER NOT NULL DEFAULT 0,
            last_used_at  INTEGER,
            UNIQUE(provider, remote_id)
         );
         CREATE INDEX IF NOT EXISTS idx_wallpapers_downloaded
            ON wallpapers(downloaded_at);
         CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
         );",
    )?;

    // Existing installs predate these columns; a duplicate-column error just
    // means the migration already ran.
    for stmt in [
        "ALTER TABLE wallpapers ADD COLUMN source_width INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE wallpapers ADD COLUMN source_height INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE wallpapers ADD COLUMN format TEXT NOT NULL DEFAULT ''",
    ] {
        let _ = conn.execute(stmt, []);
    }
    Ok(())
}

pub fn load_settings(conn: &Connection) -> Result<Settings> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'app'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match raw {
        Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        None => Settings::default(),
    })
}

pub fn save_settings(conn: &Connection, settings: &Settings) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('app', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![serde_json::to_string(settings)?],
    )?;
    Ok(())
}

fn row_to_wallpaper(row: &rusqlite::Row) -> rusqlite::Result<Wallpaper> {
    Ok(Wallpaper {
        id: row.get("id")?,
        provider: row.get("provider")?,
        remote_id: row.get("remote_id")?,
        file_name: row.get("file_name")?,
        width: row.get("width")?,
        height: row.get("height")?,
        source_width: row.get("source_width")?,
        source_height: row.get("source_height")?,
        format: row.get("format")?,
        bytes: row.get::<_, i64>("bytes")? as u64,
        downloaded_at: row.get("downloaded_at")?,
        category: row.get("category")?,
        author_name: row.get("author_name")?,
        author_url: row.get("author_url")?,
        source_url: row.get("source_url")?,
        is_favorite: row.get::<_, i64>("is_favorite")? != 0,
        last_used_at: row.get("last_used_at")?,
    })
}

const SELECT_COLS: &str = "id, provider, remote_id, file_name, width, height, \
     source_width, source_height, format, bytes, \
     downloaded_at, category, author_name, author_url, source_url, is_favorite, last_used_at";

pub fn insert_wallpaper(conn: &Connection, w: &Wallpaper) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO wallpapers
         (id, provider, remote_id, file_name, width, height,
          source_width, source_height, format, bytes, downloaded_at,
          category, author_name, author_url, source_url, is_favorite, last_used_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
            w.id,
            w.provider,
            w.remote_id,
            w.file_name,
            w.width,
            w.height,
            w.source_width,
            w.source_height,
            w.format,
            w.bytes as i64,
            w.downloaded_at,
            w.category,
            w.author_name,
            w.author_url,
            w.source_url,
            w.is_favorite as i64,
            w.last_used_at,
        ],
    )?;
    Ok(())
}

pub fn list_wallpapers(conn: &Connection) -> Result<Vec<Wallpaper>> {
    let sql = format!("SELECT {SELECT_COLS} FROM wallpapers ORDER BY downloaded_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_wallpaper)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_wallpaper(conn: &Connection, id: &str) -> Result<Option<Wallpaper>> {
    let sql = format!("SELECT {SELECT_COLS} FROM wallpapers WHERE id = ?1");
    Ok(conn
        .query_row(&sql, params![id], row_to_wallpaper)
        .optional()?)
}

/// Least recently shown cached wallpaper for a category — the offline pick.
/// Falls back across categories if the requested one has nothing.
pub fn pick_cached(conn: &Connection, category: &str, exclude_id: Option<&str>) -> Result<Option<Wallpaper>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM wallpapers
         WHERE (?1 = '' OR category = ?1) AND (?2 IS NULL OR id != ?2)
         ORDER BY last_used_at IS NOT NULL, last_used_at ASC, downloaded_at DESC
         LIMIT 1"
    );
    let found = conn
        .query_row(&sql, params![category, exclude_id], row_to_wallpaper)
        .optional()?;
    if found.is_some() {
        return Ok(found);
    }
    Ok(conn
        .query_row(&sql, params!["", exclude_id], row_to_wallpaper)
        .optional()?)
}

pub fn known_remote_ids(conn: &Connection, provider: &str, limit: usize) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT remote_id FROM wallpapers WHERE provider = ?1
         ORDER BY downloaded_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![provider, limit as i64], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn has_remote_id(conn: &Connection, provider: &str, remote_id: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM wallpapers WHERE provider = ?1 AND remote_id = ?2",
        params![provider, remote_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn set_favorite(conn: &Connection, id: &str, favorite: bool) -> Result<()> {
    conn.execute(
        "UPDATE wallpapers SET is_favorite = ?2 WHERE id = ?1",
        params![id, favorite as i64],
    )?;
    Ok(())
}

pub fn mark_used(conn: &Connection, id: &str, at: i64) -> Result<()> {
    conn.execute(
        "UPDATE wallpapers SET last_used_at = ?2 WHERE id = ?1",
        params![id, at],
    )?;
    Ok(())
}

pub fn delete_wallpaper(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM wallpapers WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn storage_stats(conn: &Connection) -> Result<(u64, u32)> {
    let (bytes, count): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(bytes), 0), COUNT(*) FROM wallpapers",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((bytes as u64, count as u32))
}

/// Oldest non-favorite wallpapers, excluding the protected ids, for eviction.
pub fn eviction_candidates(
    conn: &Connection,
    protected: &[String],
) -> Result<Vec<Wallpaper>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM wallpapers
         WHERE is_favorite = 0 ORDER BY downloaded_at ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_wallpaper)?;
    let all = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(all
        .into_iter()
        .filter(|w| !protected.contains(&w.id))
        .collect())
}

