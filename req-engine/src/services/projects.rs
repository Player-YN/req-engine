//! Project helpers.

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::models::Project;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("project not found: {0}")]
    NotFound(String),

    #[error("name must not be empty")]
    EmptyName,

    #[error("project already exists: {0}")]
    AlreadyExists(String),

    #[error("invalid local_path: {0}")]
    InvalidLocalPath(String),

    #[error("project is archived: {0}")]
    Archived(String),

    #[error("invalid project id: {0}")]
    InvalidId(String),
}

/// Normalize and validate an optional local folder path for storage.
///
/// - Trim whitespace; empty after trim → `""` (unbound).
/// - Reject null bytes.
/// - Path need not exist (user may bind before creating the folder).
/// - On Windows, if non-empty and not absolute, still accept but prefer absolute
///   (MVP stores trimmed path as-is when absolute-or-relative; no hard reject
///   for relative paths so portable configs work in tests).
pub fn normalize_local_path(raw: &str) -> Result<String, ProjectError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.contains('\0') {
        return Err(ProjectError::InvalidLocalPath(
            "path must not contain null bytes".into(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Stable project ids: slug `[A-Za-z0-9][A-Za-z0-9_-]{0,63}` (covers seed ids and UUIDs).
pub fn validate_project_id(id: &str) -> Result<(), ProjectError> {
    let id = id.trim();
    if id.is_empty() {
        return Err(ProjectError::InvalidId("id must not be empty".into()));
    }
    if id.len() > 64 {
        return Err(ProjectError::InvalidId("id must be at most 64 characters".into()));
    }
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err(ProjectError::InvalidId("id must not be empty".into()));
    };
    if !first.is_ascii_alphanumeric() {
        return Err(ProjectError::InvalidId(
            "id must start with a letter or digit".into(),
        ));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(ProjectError::InvalidId(
            "id may only contain letters, digits, '_' and '-'".into(),
        ));
    }
    Ok(())
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn opt_dt(row: &rusqlite::Row<'_>, col: &str) -> rusqlite::Result<Option<chrono::DateTime<Utc>>> {
    let s: Option<String> = row.get(col)?;
    Ok(s.filter(|x| !x.is_empty()).map(|x| parse_dt(&x)))
}

fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get("id")?,
        name: row.get("name")?,
        color: row.get("color")?,
        blurb: row.get("blurb")?,
        local_path: row.get("local_path")?,
        archived_at: opt_dt(row, "archived_at")?,
        discuss_agent_at: opt_dt(row, "discuss_agent_at")?,
        build_agent_at: opt_dt(row, "build_agent_at")?,
        created_at: parse_dt(&row.get::<_, String>("created_at")?),
        updated_at: parse_dt(&row.get::<_, String>("updated_at")?),
    })
}

const PROJECT_SELECT: &str = "SELECT id, name, color, blurb, local_path, archived_at, \
     discuss_agent_at, build_agent_at, created_at, updated_at FROM projects";

pub fn create_project(
    conn: &Connection,
    name: &str,
    color: &str,
    blurb: &str,
    local_path: &str,
) -> Result<Project, ProjectError> {
    let id = Uuid::new_v4().to_string();
    create_project_with_id(conn, &id, name, color, blurb, local_path)
}

pub fn create_project_with_id(
    conn: &Connection,
    id: &str,
    name: &str,
    color: &str,
    blurb: &str,
    local_path: &str,
) -> Result<Project, ProjectError> {
    if name.trim().is_empty() {
        return Err(ProjectError::EmptyName);
    }
    validate_project_id(id)?;
    let local_path = normalize_local_path(local_path)?;
    let now = Utc::now();
    let now_s = now.to_rfc3339();

    match conn.execute(
        "INSERT INTO projects (id, name, color, blurb, local_path, archived_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7)",
        rusqlite::params![id, name, color, blurb, local_path, now_s, now_s],
    ) {
        Ok(_) => Ok(Project {
            id: id.to_string(),
            name: name.to_string(),
            color: color.to_string(),
            blurb: blurb.to_string(),
            local_path,
            archived_at: None,
            discuss_agent_at: None,
            build_agent_at: None,
            created_at: now,
            updated_at: now,
        }),
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err(ProjectError::AlreadyExists(id.to_string()))
        }
        Err(e) => Err(ProjectError::Sqlite(e)),
    }
}

/// Load a project and reject if it is missing or soft-archived.
pub fn ensure_project_writable(conn: &Connection, id: &str) -> Result<Project, ProjectError> {
    let existing = get_project(conn, id)?.ok_or_else(|| ProjectError::NotFound(id.to_string()))?;
    if existing.archived_at.is_some() {
        return Err(ProjectError::Archived(id.to_string()));
    }
    Ok(existing)
}

/// Partial update of project metadata fields. Only non-`None` fields are applied.
pub fn update_project(
    conn: &Connection,
    id: &str,
    name: Option<&str>,
    color: Option<&str>,
    blurb: Option<&str>,
    local_path: Option<&str>,
) -> Result<Project, ProjectError> {
    let existing = ensure_project_writable(conn, id)?;

    let new_name = match name {
        Some(n) if n.trim().is_empty() => return Err(ProjectError::EmptyName),
        Some(n) => n.to_string(),
        None => existing.name,
    };
    let new_color = color.map(|c| c.to_string()).unwrap_or(existing.color);
    let new_blurb = blurb.map(|b| b.to_string()).unwrap_or(existing.blurb);
    let new_local_path = match local_path {
        Some(p) => normalize_local_path(p)?,
        None => existing.local_path,
    };

    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let n = conn.execute(
        "UPDATE projects SET name = ?1, color = ?2, blurb = ?3, local_path = ?4, updated_at = ?5
         WHERE id = ?6",
        rusqlite::params![new_name, new_color, new_blurb, new_local_path, now_s, id],
    )?;
    if n == 0 {
        return Err(ProjectError::NotFound(id.to_string()));
    }

    Ok(Project {
        id: id.to_string(),
        name: new_name,
        color: new_color,
        blurb: new_blurb,
        local_path: new_local_path,
        archived_at: existing.archived_at,
        discuss_agent_at: existing.discuss_agent_at,
        build_agent_at: existing.build_agent_at,
        created_at: existing.created_at,
        updated_at: now,
    })
}

/// Mark a product agent seat as linked (after MCP setup succeeds).
pub fn ack_agent_seat(
    conn: &Connection,
    id: &str,
    seat: crate::domain::models::AgentSeat,
) -> Result<Project, ProjectError> {
    let existing = ensure_project_writable(conn, id)?;
    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let col = match seat {
        crate::domain::models::AgentSeat::Discuss => "discuss_agent_at",
        crate::domain::models::AgentSeat::Build => "build_agent_at",
    };
    let sql = format!("UPDATE projects SET {col} = ?1, updated_at = ?1 WHERE id = ?2");
    let n = conn.execute(&sql, rusqlite::params![now_s, id])?;
    if n == 0 {
        return Err(ProjectError::NotFound(id.to_string()));
    }
    let mut p = existing;
    p.updated_at = now;
    match seat {
        crate::domain::models::AgentSeat::Discuss => p.discuss_agent_at = Some(now),
        crate::domain::models::AgentSeat::Build => p.build_agent_at = Some(now),
    }
    Ok(p)
}

/// Soft-archive a project (hidden from default list). Requirements are kept.
pub fn archive_project(conn: &Connection, id: &str) -> Result<Project, ProjectError> {
    let existing = get_project(conn, id)?.ok_or_else(|| ProjectError::NotFound(id.to_string()))?;
    if existing.archived_at.is_some() {
        return Ok(existing);
    }
    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let n = conn.execute(
        "UPDATE projects SET archived_at = ?1, updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now_s, id],
    )?;
    if n == 0 {
        return Err(ProjectError::NotFound(id.to_string()));
    }
    Ok(Project {
        archived_at: Some(now),
        updated_at: now,
        discuss_agent_at: existing.discuss_agent_at,
        build_agent_at: existing.build_agent_at,
        ..existing
    })
}

/// Clear soft-archive so the project returns to the default list and accepts writes.
pub fn unarchive_project(conn: &Connection, id: &str) -> Result<Project, ProjectError> {
    let existing = get_project(conn, id)?.ok_or_else(|| ProjectError::NotFound(id.to_string()))?;
    if existing.archived_at.is_none() {
        return Ok(existing);
    }
    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let n = conn.execute(
        "UPDATE projects SET archived_at = NULL, updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now_s, id],
    )?;
    if n == 0 {
        return Err(ProjectError::NotFound(id.to_string()));
    }
    Ok(Project {
        archived_at: None,
        updated_at: now,
        discuss_agent_at: existing.discuss_agent_at,
        build_agent_at: existing.build_agent_at,
        ..existing
    })
}

pub fn get_project(conn: &Connection, id: &str) -> Result<Option<Project>, ProjectError> {
    Ok(conn
        .query_row(
            &format!("{PROJECT_SELECT} WHERE id = ?1"),
            [id],
            row_to_project,
        )
        .optional()?)
}

/// Active (non-archived) projects only — product default list.
pub fn list_projects(conn: &Connection) -> Result<Vec<Project>, ProjectError> {
    list_projects_filtered(conn, false)
}

/// `include_archived = true` returns every project (archived last).
pub fn list_projects_filtered(
    conn: &Connection,
    include_archived: bool,
) -> Result<Vec<Project>, ProjectError> {
    let sql = if include_archived {
        format!("{PROJECT_SELECT} ORDER BY (archived_at IS NULL OR archived_at = '') DESC, name ASC")
    } else {
        format!("{PROJECT_SELECT} WHERE archived_at IS NULL OR archived_at = '' ORDER BY name ASC")
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_project)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn create_and_list_with_local_path() {
        let conn = open_in_memory().unwrap();
        let p = create_project(
            &conn,
            "Bound",
            "#111",
            "has folder",
            r"C:\Users\demo\work\app",
        )
        .unwrap();
        assert_eq!(p.local_path, r"C:\Users\demo\work\app");

        let list = list_projects(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].local_path, r"C:\Users\demo\work\app");
    }

    #[test]
    fn empty_local_path_is_unbound() {
        let conn = open_in_memory().unwrap();
        let p = create_project(&conn, "Unbound", "#000", "", "   ").unwrap();
        assert_eq!(p.local_path, "");
    }

    #[test]
    fn archive_hides_from_list() {
        let conn = open_in_memory().unwrap();
        let p = create_project(&conn, "Gone", "#000", "", "").unwrap();
        assert_eq!(list_projects(&conn).unwrap().len(), 1);
        archive_project(&conn, &p.id).unwrap();
        assert_eq!(list_projects(&conn).unwrap().len(), 0);
        assert!(get_project(&conn, &p.id).unwrap().unwrap().archived_at.is_some());
    }

    #[test]
    fn ack_agent_seats() {
        use crate::domain::models::AgentSeat;
        let conn = open_in_memory().unwrap();
        let p = create_project(&conn, "P", "#000", "", "").unwrap();
        assert!(p.discuss_agent_at.is_none());
        let p2 = ack_agent_seat(&conn, &p.id, AgentSeat::Discuss).unwrap();
        assert!(p2.discuss_agent_at.is_some());
        let p3 = ack_agent_seat(&conn, &p.id, AgentSeat::Build).unwrap();
        assert!(p3.build_agent_at.is_some());
    }

    #[test]
    fn rejects_empty_and_unsafe_project_ids() {
        let conn = open_in_memory().unwrap();
        assert!(matches!(
            create_project_with_id(&conn, "", "N", "#000", "", "").unwrap_err(),
            ProjectError::InvalidId(_)
        ));
        assert!(matches!(
            create_project_with_id(&conn, "../etc/passwd", "N", "#000", "", "").unwrap_err(),
            ProjectError::InvalidId(_)
        ));
        assert!(matches!(
            create_project_with_id(&conn, "has space", "N", "#000", "", "").unwrap_err(),
            ProjectError::InvalidId(_)
        ));
        let ok = create_project_with_id(&conn, "demo-shop", "N", "#000", "", "").unwrap();
        assert_eq!(ok.id, "demo-shop");
        let uuid = create_project(&conn, "Auto", "#000", "", "").unwrap();
        assert!(uuid.id.contains('-'));
    }

    #[test]
    fn rejects_null_byte_in_local_path() {
        let conn = open_in_memory().unwrap();
        let err = create_project(&conn, "Bad", "#000", "", "C:\\foo\0bar").unwrap_err();
        assert!(matches!(err, ProjectError::InvalidLocalPath(_)));
    }

    #[test]
    fn update_project_local_path() {
        let conn = open_in_memory().unwrap();
        let p = create_project(&conn, "P", "#000", "", "").unwrap();
        let updated = update_project(
            &conn,
            &p.id,
            None,
            None,
            None,
            Some(r"D:\repos\my-app"),
        )
        .unwrap();
        assert_eq!(updated.local_path, r"D:\repos\my-app");
        assert_eq!(updated.name, "P");
    }

    #[test]
    fn archive_rejects_metadata_writes_until_unarchive() {
        use crate::domain::models::AgentSeat;
        let conn = open_in_memory().unwrap();
        let p = create_project(&conn, "Freeze", "#000", "", "").unwrap();
        archive_project(&conn, &p.id).unwrap();

        let err = update_project(&conn, &p.id, Some("Nope"), None, None, None).unwrap_err();
        assert!(matches!(err, ProjectError::Archived(_)), "got {err:?}");

        let err = ack_agent_seat(&conn, &p.id, AgentSeat::Discuss).unwrap_err();
        assert!(matches!(err, ProjectError::Archived(_)), "got {err:?}");

        let restored = unarchive_project(&conn, &p.id).unwrap();
        assert!(restored.archived_at.is_none());
        assert_eq!(list_projects(&conn).unwrap().len(), 1);

        let updated = update_project(&conn, &p.id, Some("Thawed"), None, None, None).unwrap();
        assert_eq!(updated.name, "Thawed");
    }
}
