//! Token → role resolution for MCP.

use rusqlite::Connection;

use crate::domain::state::Role;
use crate::mcp::{McpError, McpRole, role_allowed_for_surface};
use crate::services::tokens::lookup_token;

/// Authenticated actor for MCP tool calls.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Token name (used as actor / claimed_by).
    pub name: String,
    /// Role from DB (Admin keeps full domain privileges).
    pub role: Role,
}

/// Resolve bearer token and ensure CLI `--role` matches token role
/// (admin tokens may use either surface).
pub fn resolve_auth(
    conn: &Connection,
    plaintext_token: &str,
    surface: McpRole,
) -> Result<AuthContext, McpError> {
    let row = lookup_token(conn, plaintext_token)
        .map_err(|e| McpError::msg(e.to_string()))?
        .ok_or_else(|| McpError::msg("invalid token"))?;

    if !role_allowed_for_surface(row.role, surface) {
        return Err(McpError::msg(format!(
            "CLI --role {} does not match token role {} (admin tokens may use either)",
            surface.as_str(),
            row.role.as_str()
        )));
    }

    Ok(AuthContext {
        name: row.name,
        role: row.role,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::services::tokens::generate_bootstrap_tokens;

    #[test]
    fn planner_token_matches_planner_surface() {
        let conn = open_in_memory().unwrap();
        let tokens = generate_bootstrap_tokens(&conn).unwrap();
        let pt = tokens.iter().find(|t| t.role == Role::Planner).unwrap();
        let auth = resolve_auth(&conn, &pt.plaintext, McpRole::Planner).unwrap();
        assert_eq!(auth.role, Role::Planner);
        assert_eq!(auth.name, "planner");
    }

    #[test]
    fn planner_token_rejected_for_foreman_surface() {
        let conn = open_in_memory().unwrap();
        let tokens = generate_bootstrap_tokens(&conn).unwrap();
        let pt = tokens.iter().find(|t| t.role == Role::Planner).unwrap();
        let err = resolve_auth(&conn, &pt.plaintext, McpRole::Foreman).unwrap_err();
        assert!(err.to_string().contains("does not match"));
    }

    #[test]
    fn admin_token_allowed_for_either_surface() {
        let conn = open_in_memory().unwrap();
        let tokens = generate_bootstrap_tokens(&conn).unwrap();
        let at = tokens.iter().find(|t| t.role == Role::Admin).unwrap();
        assert!(resolve_auth(&conn, &at.plaintext, McpRole::Planner).is_ok());
        assert!(resolve_auth(&conn, &at.plaintext, McpRole::Foreman).is_ok());
    }

    #[test]
    fn invalid_token_rejected() {
        let conn = open_in_memory().unwrap();
        generate_bootstrap_tokens(&conn).unwrap();
        let err = resolve_auth(&conn, "nope", McpRole::Planner).unwrap_err();
        assert!(err.to_string().contains("invalid"));
    }
}
