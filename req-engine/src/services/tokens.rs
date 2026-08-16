//! API token generation, hashing, and lookup.

use chrono::Utc;
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::domain::models::ApiToken;
use crate::domain::state::Role;

#[derive(Debug, Clone)]
pub struct GeneratedToken {
    pub role: Role,
    pub name: String,
    /// Plaintext token (print once / write to tokens.txt for local dev).
    pub plaintext: String,
    pub token_hash: String,
}

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub fn hash_token(plaintext: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plaintext.as_bytes());
    hex::encode(hasher.finalize())
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("req_{}", hex::encode(bytes))
}

/// Generate admin / planner / foreman bootstrap tokens, store hashes in DB.
pub fn generate_bootstrap_tokens(conn: &Connection) -> Result<Vec<GeneratedToken>, TokenError> {
    let specs = [
        (Role::Admin, "admin"),
        (Role::Planner, "planner"),
        (Role::Foreman, "foreman"),
    ];

    let mut out = Vec::with_capacity(3);
    let now = Utc::now().to_rfc3339();

    for (role, name) in specs {
        let plaintext = random_token();
        let token_hash = hash_token(&plaintext);

        conn.execute(
            "INSERT INTO api_tokens (token_hash, role, name, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![token_hash, role.as_str(), name, now],
        )?;

        out.push(GeneratedToken {
            role,
            name: name.to_string(),
            plaintext,
            token_hash,
        });
    }

    Ok(out)
}

/// Lookup a bearer token by hashing plaintext and matching `api_tokens.token_hash`.
pub fn lookup_token(conn: &Connection, plaintext: &str) -> Result<Option<ApiToken>, TokenError> {
    let token_hash = hash_token(plaintext);
    let row = conn
        .query_row(
            "SELECT token_hash, role, name, created_at FROM api_tokens WHERE token_hash = ?1",
            [&token_hash],
            |row| {
                Ok((
                    row.get::<_, String>("token_hash")?,
                    row.get::<_, String>("role")?,
                    row.get::<_, String>("name")?,
                    row.get::<_, String>("created_at")?,
                ))
            },
        )
        .optional()?;

    let Some((hash, role_s, name, created_s)) = row else {
        return Ok(None);
    };
    let Some(role) = Role::parse(&role_s) else {
        // Refuse fail-open (previously defaulted to Foreman).
        return Ok(None);
    };
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    Ok(Some(ApiToken {
        token_hash: hash,
        role,
        name,
        created_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn bootstrap_inserts_three_hashed_tokens() {
        let conn = open_in_memory().unwrap();
        let tokens = generate_bootstrap_tokens(&conn).unwrap();
        assert_eq!(tokens.len(), 3);
        for t in &tokens {
            assert!(t.plaintext.starts_with("req_"));
            assert_eq!(hash_token(&t.plaintext), t.token_hash);
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM api_tokens", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn lookup_roundtrip() {
        let conn = open_in_memory().unwrap();
        let tokens = generate_bootstrap_tokens(&conn).unwrap();
        let admin = tokens.iter().find(|t| t.role == Role::Admin).unwrap();
        let found = lookup_token(&conn, &admin.plaintext).unwrap().unwrap();
        assert_eq!(found.role, Role::Admin);
        assert_eq!(found.name, "admin");
        assert!(lookup_token(&conn, "nope").unwrap().is_none());
    }

    #[test]
    fn unknown_role_string_is_rejected() {
        let conn = open_in_memory().unwrap();
        let tokens = generate_bootstrap_tokens(&conn).unwrap();
        let planner = tokens.iter().find(|t| t.role == Role::Planner).unwrap();
        conn.execute(
            "UPDATE api_tokens SET role = 'nope' WHERE name = 'planner'",
            [],
        )
        .unwrap();
        let found = lookup_token(&conn, &planner.plaintext).unwrap();
        assert!(
            found.is_none(),
            "unknown role must not become Foreman: {found:?}"
        );
    }
}
