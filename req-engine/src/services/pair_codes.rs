//! Per-project MCP pairing codes.
//!
//! SQLite stores SHA-256 hashes. Plaintext is written to `{home}/pair-codes.json`
//! (gitignored, same class as tokens.txt).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rand::RngCore;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::models::AgentSeat;
use crate::services::tokens::hash_token;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairBinding {
    pub project_id: String,
    pub seat: AgentSeat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatPlaintexts {
    pub discuss: String,
    pub build: String,
}

#[derive(Debug, Error)]
pub enum PairError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid pair code")]
    InvalidCode,
    #[error("project not found: {0}")]
    NotFound(String),
    #[error("plaintext pair codes missing for project {0}; rotate to issue new ones")]
    PlaintextLost(String),
    #[error("pair-codes file: {0}")]
    File(String),
}

pub fn pair_codes_path(home: &Path) -> PathBuf {
    home.join("pair-codes.json")
}

pub fn hash_pair_code(plaintext: &str) -> String {
    hash_token(plaintext)
}

pub fn generate_pair_code(seat: AgentSeat) -> String {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    let prefix = match seat {
        AgentSeat::Discuss => "disc_",
        AgentSeat::Build => "build_",
    };
    format!("{prefix}{}", hex::encode(bytes))
}

pub fn seat_from_code_prefix(code: &str) -> Option<AgentSeat> {
    if code.starts_with("disc_") {
        Some(AgentSeat::Discuss)
    } else if code.starts_with("build_") {
        Some(AgentSeat::Build)
    } else {
        None
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PairFile {
    #[serde(default)]
    projects: BTreeMap<String, SeatPlaintexts>,
}

fn load_file(home: &Path) -> Result<PairFile, PairError> {
    let path = pair_codes_path(home);
    if !path.exists() {
        return Ok(PairFile::default());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(PairFile::default());
    }
    serde_json::from_str(&raw).map_err(|e| PairError::File(e.to_string()))
}

fn save_file(home: &Path, file: &PairFile) -> Result<(), PairError> {
    fs::create_dir_all(home)?;
    let path = pair_codes_path(home);
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(file).map_err(|e| PairError::File(e.to_string()))?;
    fs::write(&tmp, json)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn hash_column(seat: AgentSeat) -> &'static str {
    match seat {
        AgentSeat::Discuss => "discuss_pair_hash",
        AgentSeat::Build => "build_pair_hash",
    }
}

fn project_exists(conn: &Connection, project_id: &str) -> Result<bool, PairError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE id = ?1",
        [project_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn write_hashes(
    conn: &Connection,
    project_id: &str,
    discuss_hash: Option<&str>,
    build_hash: Option<&str>,
) -> Result<(), PairError> {
    match (discuss_hash, build_hash) {
        (Some(d), Some(b)) => {
            conn.execute(
                "UPDATE projects SET discuss_pair_hash = ?1, build_pair_hash = ?2 WHERE id = ?3",
                rusqlite::params![d, b, project_id],
            )?;
        }
        (Some(d), None) => {
            conn.execute(
                "UPDATE projects SET discuss_pair_hash = ?1 WHERE id = ?2",
                rusqlite::params![d, project_id],
            )?;
        }
        (None, Some(b)) => {
            conn.execute(
                "UPDATE projects SET build_pair_hash = ?1 WHERE id = ?2",
                rusqlite::params![b, project_id],
            )?;
        }
        (None, None) => {}
    }
    Ok(())
}

/// Issue missing codes for one project. Existing hashes are left alone.
pub fn ensure_project_pair_codes(
    conn: &Connection,
    home: &Path,
    project_id: &str,
) -> Result<SeatPlaintexts, PairError> {
    if !project_exists(conn, project_id)? {
        return Err(PairError::NotFound(project_id.to_string()));
    }

    let (discuss_hash, build_hash): (Option<String>, Option<String>) = conn.query_row(
        "SELECT discuss_pair_hash, build_pair_hash FROM projects WHERE id = ?1",
        [project_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    let mut file = load_file(home)?;
    let entry = file
        .projects
        .entry(project_id.to_string())
        .or_insert_with(|| SeatPlaintexts {
            discuss: String::new(),
            build: String::new(),
        });

    let mut new_discuss_hash = None;
    let mut new_build_hash = None;

    if discuss_hash.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
        let code = generate_pair_code(AgentSeat::Discuss);
        new_discuss_hash = Some(hash_pair_code(&code));
        entry.discuss = code;
    }
    if build_hash.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
        let code = generate_pair_code(AgentSeat::Build);
        new_build_hash = Some(hash_pair_code(&code));
        entry.build = code;
    }

    if new_discuss_hash.is_some() || new_build_hash.is_some() {
        write_hashes(
            conn,
            project_id,
            new_discuss_hash.as_deref(),
            new_build_hash.as_deref(),
        )?;
        save_file(home, &file)?;
    }

    let discuss = file
        .projects
        .get(project_id)
        .map(|s| s.discuss.clone())
        .unwrap_or_default();
    let build = file
        .projects
        .get(project_id)
        .map(|s| s.build.clone())
        .unwrap_or_default();

    if discuss.is_empty() || build.is_empty() {
        return Err(PairError::PlaintextLost(project_id.to_string()));
    }
    Ok(SeatPlaintexts { discuss, build })
}

/// Issue codes for every project missing a hash.
pub fn ensure_all_project_pair_codes(conn: &Connection, home: &Path) -> Result<(), PairError> {
    let mut stmt = conn.prepare("SELECT id FROM projects")?;
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        // Ignore plaintext-lost here: hashes already exist, file may be missing.
        match ensure_project_pair_codes(conn, home, &id) {
            Ok(_) | Err(PairError::PlaintextLost(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub fn read_plaintext_codes(home: &Path, project_id: &str) -> Result<SeatPlaintexts, PairError> {
    let file = load_file(home)?;
    match file.projects.get(project_id) {
        Some(s) if !s.discuss.is_empty() && !s.build.is_empty() => Ok(s.clone()),
        _ => Err(PairError::PlaintextLost(project_id.to_string())),
    }
}

/// Resolve a plaintext pair code to project + seat. DB hashes are source of truth.
pub fn lookup_pair_code(conn: &Connection, code: &str) -> Result<PairBinding, PairError> {
    let seat = seat_from_code_prefix(code).ok_or(PairError::InvalidCode)?;
    let hash = hash_pair_code(code.trim());
    let col = hash_column(seat);
    let sql = format!("SELECT id FROM projects WHERE {col} = ?1");
    let id: Option<String> = conn
        .query_row(&sql, [&hash], |r| r.get(0))
        .optional_row()?;
    match id {
        Some(project_id) => Ok(PairBinding { project_id, seat }),
        None => Err(PairError::InvalidCode),
    }
}

pub fn rotate_pair_code(
    conn: &Connection,
    home: &Path,
    project_id: &str,
    seat: AgentSeat,
) -> Result<String, PairError> {
    if !project_exists(conn, project_id)? {
        return Err(PairError::NotFound(project_id.to_string()));
    }
    let code = generate_pair_code(seat);
    let hash = hash_pair_code(&code);
    match seat {
        AgentSeat::Discuss => write_hashes(conn, project_id, Some(&hash), None)?,
        AgentSeat::Build => write_hashes(conn, project_id, None, Some(&hash))?,
    }

    let mut file = load_file(home)?;
    let entry = file
        .projects
        .entry(project_id.to_string())
        .or_insert_with(|| SeatPlaintexts {
            discuss: String::new(),
            build: String::new(),
        });
    match seat {
        AgentSeat::Discuss => entry.discuss = code.clone(),
        AgentSeat::Build => entry.build = code.clone(),
    }
    save_file(home, &file)?;
    Ok(code)
}

trait OptionalRow<T> {
    fn optional_row(self) -> Result<Option<T>, PairError>;
}

impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional_row(self) -> Result<Option<T>, PairError> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(PairError::Sqlite(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::services::create_project;

    #[test]
    fn generate_uses_seat_prefix() {
        assert!(generate_pair_code(AgentSeat::Discuss).starts_with("disc_"));
        assert!(generate_pair_code(AgentSeat::Build).starts_with("build_"));
        assert_eq!(seat_from_code_prefix("disc_aa"), Some(AgentSeat::Discuss));
        assert_eq!(seat_from_code_prefix("build_aa"), Some(AgentSeat::Build));
        assert!(seat_from_code_prefix("req_aa").is_none());
    }

    #[test]
    fn issue_lookup_rotate() {
        let conn = open_in_memory().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let p = create_project(&conn, "Alpha", "#000", "", "").unwrap();

        let codes = ensure_project_pair_codes(&conn, home, &p.id).unwrap();
        assert!(codes.discuss.starts_with("disc_"));
        assert!(codes.build.starts_with("build_"));

        let d = lookup_pair_code(&conn, &codes.discuss).unwrap();
        assert_eq!(d.project_id, p.id);
        assert_eq!(d.seat, AgentSeat::Discuss);

        let b = lookup_pair_code(&conn, &codes.build).unwrap();
        assert_eq!(b.seat, AgentSeat::Build);

        assert!(lookup_pair_code(&conn, "disc_deadbeef").is_err());

        // Prefix from the other seat must not resolve even if we swapped strings
        // (hashes live in different columns).
        assert!(lookup_pair_code(&conn, &codes.discuss.replace("disc_", "build_")).is_err());

        let old = codes.discuss.clone();
        let new_d = rotate_pair_code(&conn, home, &p.id, AgentSeat::Discuss).unwrap();
        assert_ne!(old, new_d);
        assert!(lookup_pair_code(&conn, &old).is_err());
        assert_eq!(
            lookup_pair_code(&conn, &new_d).unwrap().seat,
            AgentSeat::Discuss
        );
        // Build code still valid.
        assert_eq!(
            lookup_pair_code(&conn, &codes.build).unwrap().seat,
            AgentSeat::Build
        );
    }

    #[test]
    fn ensure_is_idempotent_when_hashes_exist() {
        let conn = open_in_memory().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let p = create_project(&conn, "Beta", "#000", "", "").unwrap();
        let a = ensure_project_pair_codes(&conn, home, &p.id).unwrap();
        let b = ensure_project_pair_codes(&conn, home, &p.id).unwrap();
        assert_eq!(a, b);
    }
}
