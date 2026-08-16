//! JSON request/response DTOs (snake_case for frontend).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::models::{Event, Project, Requirement};
use crate::domain::state::Status;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub color: String,
    pub blurb: String,
    /// Optional absolute local folder path (empty = unbound).
    pub local_path: String,
    /// Soft-archived projects are omitted from GET /projects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// 讨论 Agent (MCP planner) linked?
    pub discuss_agent_configured: bool,
    /// 实现 Agent (MCP foreman) linked?
    pub build_agent_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discuss_agent_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_agent_at: Option<String>,
    /// Live MCP stdio process is heartbeating this seat (TTL window).
    pub discuss_seated: bool,
    pub build_seated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discuss_occupant: Option<OccupantDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_occupant: Option<OccupantDto>,
}

#[derive(Debug, Serialize, Clone)]
pub struct OccupantDto {
    pub raw_name: String,
    pub label: String,
    pub key: String,
    pub initials: String,
    pub hue: u16,
    pub known: bool,
}

impl From<Project> for ProjectDto {
    fn from(p: Project) -> Self {
        Self {
            id: p.id,
            name: p.name,
            color: p.color,
            blurb: p.blurb,
            local_path: p.local_path,
            archived_at: p.archived_at.map(|t| t.to_rfc3339()),
            discuss_agent_configured: p.discuss_agent_at.is_some(),
            build_agent_configured: p.build_agent_at.is_some(),
            discuss_agent_at: p.discuss_agent_at.map(|t| t.to_rfc3339()),
            build_agent_at: p.build_agent_at.map(|t| t.to_rfc3339()),
            discuss_seated: false,
            build_seated: false,
            discuss_occupant: None,
            build_occupant: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateProjectBody {
    pub name: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub blurb: String,
    /// Optional local folder path (empty = unbound). Stored trimmed; need not exist.
    #[serde(default)]
    pub local_path: String,
    /// Optional stable id (seed / admin convenience).
    pub id: Option<String>,
}

/// Partial project update (admin). Omitted fields are left unchanged.
#[derive(Debug, Deserialize)]
pub struct PatchProjectBody {
    pub name: Option<String>,
    pub color: Option<String>,
    pub blurb: Option<String>,
    pub local_path: Option<String>,
}

fn default_color() -> String {
    "#6366f1".into()
}

#[derive(Debug, Serialize)]
pub struct SeatPairDto {
    pub code: String,
    pub copy_text: String,
    pub seated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occupant: Option<OccupantDto>,
}

#[derive(Debug, Serialize)]
pub struct ProjectPairCodesDto {
    pub discuss: SeatPairDto,
    pub build: SeatPairDto,
}

#[derive(Debug, Serialize)]
pub struct RequirementDto {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub priority: String,
    pub status: Status,
    pub scope: Value,
    pub non_scope: Value,
    pub acceptance_criteria: Value,
    pub dependencies: Value,
    pub claimed_by: Option<String>,
    pub progress_summary: Option<String>,
    pub blocked_reason: Option<String>,
    pub external_run_id: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn parse_json_field(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Array(vec![]))
}

impl From<Requirement> for RequirementDto {
    fn from(r: Requirement) -> Self {
        Self {
            id: r.id,
            project_id: r.project_id,
            title: r.title,
            description: r.description,
            priority: r.priority,
            status: r.status,
            scope: parse_json_field(&r.scope_json),
            non_scope: parse_json_field(&r.non_scope_json),
            acceptance_criteria: parse_json_field(&r.acceptance_json),
            dependencies: parse_json_field(&r.dependencies_json),
            claimed_by: r.claimed_by,
            progress_summary: r.progress_summary,
            blocked_reason: r.blocked_reason,
            external_run_id: r.external_run_id,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RequirementDetailDto {
    #[serde(flatten)]
    pub requirement: RequirementDto,
    pub events: Vec<EventDto>,
}

#[derive(Debug, Serialize)]
pub struct EventDto {
    pub id: String,
    pub project_id: String,
    pub requirement_id: Option<String>,
    pub actor: String,
    pub kind: String,
    pub message: String,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

impl From<Event> for EventDto {
    fn from(e: Event) -> Self {
        Self {
            id: e.id,
            project_id: e.project_id,
            requirement_id: e.requirement_id,
            actor: e.actor,
            kind: e.kind,
            message: e.message,
            payload: serde_json::from_str(&e.payload_json).unwrap_or(Value::Object(Default::default())),
            created_at: e.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateRequirementBody {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    #[serde(default = "empty_array")]
    pub scope: Value,
    #[serde(default = "empty_array")]
    pub non_scope: Value,
    #[serde(default = "empty_array")]
    pub acceptance_criteria: Value,
    #[serde(default = "empty_array")]
    pub dependencies: Value,
}

fn default_priority() -> String {
    "medium".into()
}

fn empty_array() -> Value {
    Value::Array(vec![])
}

impl CreateRequirementBody {
    pub fn to_json_strings(&self) -> (String, String, String, String) {
        (
            self.scope.to_string(),
            self.non_scope.to_string(),
            self.acceptance_criteria.to_string(),
            self.dependencies.to_string(),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct ProgressBody {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteReviewBody {
    pub pass: bool,
    #[serde(default)]
    pub reason: Option<String>,
}
