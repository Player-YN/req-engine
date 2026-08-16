//! MCP stdio server (`req-engine mcp --pair CODE`).
//!
//! Product path binds a process to one project + seat. `--role` + token is debug only.
//! Tools call existing services; no free-form status / complete_review on foreman.
//! Logging goes to **stderr only** (stdout is the MCP wire).

mod auth;
mod dto;
mod foreman;
mod planner;
mod util;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use thiserror::Error;

use crate::domain::models::AgentSeat;
use crate::domain::state::Role;
use crate::paths;
use crate::services::{
    clear_seat_presence, ensure_all_project_pair_codes, lookup_pair_code, touch_seat_presence,
    touch_seat_presence_client, OccupantHint, PairBinding,
};

pub use auth::{AuthContext, resolve_auth};

#[derive(Debug, Error)]
pub enum McpError {
    #[error("{0}")]
    Msg(String),
}

impl McpError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }
}

/// CLI role for MCP tool surface selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpRole {
    Planner,
    Foreman,
}

impl McpRole {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "planner" => Some(Self::Planner),
            "foreman" => Some(Self::Foreman),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Foreman => "foreman",
        }
    }
}

/// Shared MCP server state (DB + authenticated actor + optional project lock).
#[derive(Clone)]
pub struct McpState {
    pub db: Arc<Mutex<Connection>>,
    pub auth: AuthContext,
    pub surface: McpRole,
    /// Set when started with `--pair`. All tools are pinned to this project.
    pub binding: Option<PairBinding>,
}

/// Open DB, authenticate, run stdio MCP server for the requested surface.
pub async fn run(
    home_override: Option<PathBuf>,
    role: Option<McpRole>,
    token: Option<String>,
    pair: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // tracing → stderr only (never stdout; stdout is MCP JSON-RPC)
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .try_init();

    let home = home_override.unwrap_or_else(paths::resolve_home);
    let db_path = paths::db_path(&home);
    if !db_path.exists() {
        return Err(format!(
            "database not found at {} — run `req-engine init` first",
            db_path.display()
        )
        .into());
    }

    let conn = crate::db::open_and_migrate(&db_path)?;
    let _ = ensure_all_project_pair_codes(&conn, &home);

    let (role, auth, binding) = if let Some(code) = pair.filter(|s| !s.trim().is_empty()) {
        let bound = lookup_pair_code(&conn, code.trim())
            .map_err(|e| format!("invalid --pair: {e}"))?;
        let surface = match bound.seat {
            AgentSeat::Discuss => McpRole::Planner,
            AgentSeat::Build => McpRole::Foreman,
        };
        let auth = AuthContext {
            name: format!("{}:{}", bound.seat.as_str(), bound.project_id),
            role: match bound.seat {
                AgentSeat::Discuss => Role::Planner,
                AgentSeat::Build => Role::Foreman,
            },
        };
        (surface, auth, Some(bound))
    } else {
        let role = role.ok_or_else(|| {
            "pass --pair <CODE> (product) or --role + --token (debug)".to_string()
        })?;
        let token = token
            .or_else(|| std::env::var("REQ_ENGINE_TOKEN").ok())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "token required without --pair: pass --token or set REQ_ENGINE_TOKEN".to_string()
            })?;
        let auth = resolve_auth(&conn, &token, role)?;
        (role, auth, None)
    };

    tracing::info!(
        home = %home.display(),
        surface = role.as_str(),
        token_role = auth.role.as_str(),
        actor = %auth.name,
        bound_project = binding.as_ref().map(|b| b.project_id.as_str()).unwrap_or("-"),
        "starting MCP stdio server"
    );

    if let Some(ref bound) = binding {
        let _ = touch_seat_presence(&conn, &bound.project_id, bound.seat);
    }

    let state = McpState {
        db: Arc::new(Mutex::new(conn)),
        auth,
        surface: role,
        binding,
    };

    let heartbeat = state.binding.as_ref().map(|bound| {
        let hb_state = state.clone();
        let project_id = bound.project_id.clone();
        let seat = bound.seat;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(4));
            loop {
                tick.tick().await;
                if let Ok(guard) = hb_state.db.lock() {
                    let _ = touch_seat_presence(&guard, &project_id, seat);
                }
            }
        })
    });

    let serve_result = match role {
        McpRole::Planner => planner::serve(state.clone()).await,
        McpRole::Foreman => foreman::serve(state.clone()).await,
    };

    if let Some(h) = heartbeat {
        h.abort();
    }
    if let Some(bound) = &state.binding {
        if let Ok(guard) = state.db.lock() {
            let _ = clear_seat_presence(&guard, &bound.project_id, bound.seat);
        }
    }
    serve_result?;
    Ok(())
}

/// After MCP initialize: persist self-reported clientInfo onto the seat.
pub fn record_peer_client(
    state: &McpState,
    name: &str,
    title: Option<&str>,
    version: Option<&str>,
) {
    let Some(bound) = state.binding.as_ref() else {
        return;
    };
    let hint = OccupantHint {
        name: name.trim().to_string(),
        title: title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        version: version
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    };
    if hint.name.is_empty() {
        return;
    }
    if let Ok(conn) = state.db.lock() {
        let _ = touch_seat_presence_client(&conn, &bound.project_id, bound.seat, &hint);
    }
}

/// Pure check used by unit tests and startup.
pub fn role_allowed_for_surface(token_role: Role, surface: McpRole) -> bool {
    match token_role {
        Role::Admin => true,
        Role::Planner => surface == McpRole::Planner,
        Role::Foreman => surface == McpRole::Foreman,
    }
}

