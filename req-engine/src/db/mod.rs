//! SQLite access (rusqlite, sync).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;

pub mod schema;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("migration failed: {0}")]
    Migration(String),

    #[error("not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Open (or create) the database and apply migrations.
pub fn open_and_migrate(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")?;
    schema::migrate(&conn)?;
    Ok(conn)
}

/// Open an in-memory DB with migrations (tests).
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    schema::migrate(&conn)?;
    Ok(conn)
}

/// Read a single scalar if present.
pub fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let mut stmt = conn.prepare(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1 LIMIT 1",
    )?;
    let found: Option<i32> = stmt.query_row([name], |r| r.get(0)).optional()?;
    Ok(found.is_some())
}
