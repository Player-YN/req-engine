//! Domain models mapped to SQLite rows.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::state::{Role, Status};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub color: String,
    pub blurb: String,
    /// Optional absolute local folder path (empty = unbound).
    /// Product meaning today: **metadata / future agent workspace root** — not a live runtime yet.
    pub local_path: String,
    /// Soft-archive timestamp (RFC3339). `None` = active in default lists.
    pub archived_at: Option<DateTime<Utc>>,
    /// When the **讨论 Agent** (MCP planner) last acknowledged setup for this project.
    pub discuss_agent_at: Option<DateTime<Utc>>,
    /// When the **实现 Agent** (MCP foreman) last acknowledged setup for this project.
    pub build_agent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Product seat names (two agents per project).
/// Wire protocol still uses planner / foreman MCP roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSeat {
    /// 讨论 Agent — create/refine requirements (MCP role: planner).
    Discuss,
    /// 实现 Agent — claim/implement/submit (MCP role: foreman).
    Build,
}

impl AgentSeat {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentSeat::Discuss => "discuss",
            AgentSeat::Build => "build",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "discuss" | "discussion" | "planner" | "curator" => Some(AgentSeat::Discuss),
            "build" | "builder" | "foreman" | "implement" | "implementation" => {
                Some(AgentSeat::Build)
            }
            _ => None,
        }
    }

    pub fn display_zh(self) -> &'static str {
        match self {
            AgentSeat::Discuss => "讨论 Agent",
            AgentSeat::Build => "实现 Agent",
        }
    }

    pub fn mcp_role(self) -> &'static str {
        match self {
            AgentSeat::Discuss => "planner",
            AgentSeat::Build => "foreman",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub priority: String,
    pub status: Status,
    pub scope_json: String,
    pub non_scope_json: String,
    pub acceptance_json: String,
    pub dependencies_json: String,
    pub claimed_by: Option<String>,
    pub progress_summary: Option<String>,
    pub blocked_reason: Option<String>,
    pub external_run_id: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub project_id: String,
    pub requirement_id: Option<String>,
    pub actor: String,
    pub kind: String,
    pub message: String,
    pub payload_json: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub token_hash: String,
    pub role: Role,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

/// Input for creating a requirement (always starts as `todo`).
#[derive(Debug, Clone)]
pub struct CreateRequirementInput {
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub priority: String,
    pub scope_json: String,
    pub non_scope_json: String,
    pub acceptance_json: String,
    pub dependencies_json: String,
    pub created_by: String,
}
