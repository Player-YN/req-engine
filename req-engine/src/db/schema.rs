//! Embedded migrations.

use rusqlite::Connection;

use super::{DbError, Result};

const MIGRATION_001: &str = include_str!("../../migrations/001_init.sql");
const MIGRATION_002: &str = include_str!("../../migrations/002_project_local_path.sql");
const MIGRATION_003: &str = include_str!("../../migrations/003_project_archived.sql");
const MIGRATION_004: &str = include_str!("../../migrations/004_project_agent_seats.sql");
const MIGRATION_005: &str = include_str!("../../migrations/005_project_pair_codes.sql");
const MIGRATION_006: &str = include_str!("../../migrations/006_seat_presence.sql");
const MIGRATION_007: &str = include_str!("../../migrations/007_seat_client.sql");

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )?;

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current < 1 {
        conn.execute_batch(MIGRATION_001)
            .map_err(|e| DbError::Migration(format!("001_init: {e}")))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (1, ?1)",
            [&now],
        )?;
    }

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current < 2 {
        conn.execute_batch(MIGRATION_002)
            .map_err(|e| DbError::Migration(format!("002_project_local_path: {e}")))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (2, ?1)",
            [&now],
        )?;
    }

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current < 3 {
        conn.execute_batch(MIGRATION_003)
            .map_err(|e| DbError::Migration(format!("003_project_archived: {e}")))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (3, ?1)",
            [&now],
        )?;
    }

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current < 4 {
        conn.execute_batch(MIGRATION_004)
            .map_err(|e| DbError::Migration(format!("004_project_agent_seats: {e}")))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (4, ?1)",
            [&now],
        )?;
    }

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current < 5 {
        conn.execute_batch(MIGRATION_005)
            .map_err(|e| DbError::Migration(format!("005_project_pair_codes: {e}")))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (5, ?1)",
            [&now],
        )?;
    }

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current < 6 {
        conn.execute_batch(MIGRATION_006)
            .map_err(|e| DbError::Migration(format!("006_seat_presence: {e}")))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (6, ?1)",
            [&now],
        )?;
    }

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current < 7 {
        conn.execute_batch(MIGRATION_007)
            .map_err(|e| DbError::Migration(format!("007_seat_client: {e}")))?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (7, ?1)",
            [&now],
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::db::{open_in_memory, table_exists};

    #[test]
    fn migrate_creates_all_tables() {
        let conn = open_in_memory().unwrap();
        for t in ["projects", "requirements", "events", "api_tokens"] {
            assert!(table_exists(&conn, t).unwrap(), "missing table {t}");
        }
    }

    #[test]
    fn migrate_adds_local_path_column() {
        let conn = open_in_memory().unwrap();
        conn.prepare("SELECT local_path FROM projects LIMIT 0")
            .expect("local_path column should exist after migrate");

        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(version >= 2, "migration 002 should be applied, got {version}");
    }

    #[test]
    fn migrate_adds_pair_hash_columns() {
        let conn = open_in_memory().unwrap();
        conn.prepare("SELECT discuss_pair_hash, build_pair_hash FROM projects LIMIT 0")
            .expect("pair hash columns should exist after migrate");
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(version >= 5, "migration 005 should be applied, got {version}");
    }

    #[test]
    fn migrate_adds_seat_presence() {
        let conn = open_in_memory().unwrap();
        conn.prepare("SELECT project_id, seat, last_seen_at, pid FROM seat_presence LIMIT 0")
            .expect("seat_presence should exist after migrate");
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(version >= 6, "migration 006 should be applied, got {version}");
    }

    #[test]
    fn init_without_seed_has_empty_projects() {
        let conn = open_in_memory().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "fresh migrate must not insert demo projects");
    }
}
