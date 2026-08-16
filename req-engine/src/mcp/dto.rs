//! MCP tool parameter types (JSON Schema via schemars).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::models::{Project, Requirement};
use crate::domain::state::Status;

// ── Shared / common ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmptyArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RequirementIdArgs {
    #[schemars(description = "Requirement id")]
    pub id: String,
}

// ── Planner ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateRequirementArgs {
    #[serde(default)]
    #[schemars(description = "Ignored when MCP is started with --pair (bound project is used)")]
    pub project_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    #[schemars(description = "JSON array of scope strings, or omit")]
    pub scope: Option<Value>,
    #[serde(default)]
    #[schemars(description = "JSON array of acceptance criteria, or omit")]
    pub acceptance_criteria: Option<Value>,
    #[serde(default)]
    #[schemars(description = "JSON array of dependency requirement ids, or omit")]
    pub dependencies: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListRequirementsArgs {
    #[serde(default)]
    #[schemars(description = "Ignored when MCP is started with --pair (bound project is used)")]
    pub project_id: Option<String>,
    #[serde(default)]
    #[schemars(description = "Optional status filter: todo|in_progress|review|done|cancelled")]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateRequirementArgs {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub scope: Option<Value>,
    #[serde(default)]
    pub acceptance_criteria: Option<Value>,
    #[serde(default)]
    pub dependencies: Option<Value>,
}

// ── Foreman ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListReadyTasksArgs {
    #[serde(default)]
    #[schemars(description = "Optional project filter; omit for all projects")]
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportProgressArgs {
    pub id: String,
    #[schemars(description = "Progress summary text")]
    pub summary: String,
    #[serde(default)]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReleaseTaskArgs {
    pub id: String,
    #[serde(default)]
    #[schemars(description = "Optional reason (logged in tool result; domain verb has no reason field)")]
    pub reason: Option<String>,
}

// ── Response helpers ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub color: String,
    pub blurb: String,
    pub local_path: String,
}

impl From<Project> for ProjectView {
    fn from(p: Project) -> Self {
        Self {
            id: p.id,
            name: p.name,
            color: p.color,
            blurb: p.blurb,
            local_path: p.local_path,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RequirementView {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub priority: String,
    pub status: Status,
    pub scope: Value,
    pub acceptance_criteria: Value,
    pub dependencies: Value,
    pub claimed_by: Option<String>,
    pub progress_summary: Option<String>,
    pub blocked_reason: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

fn parse_json_field(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Array(vec![]))
}

impl From<Requirement> for RequirementView {
    fn from(r: Requirement) -> Self {
        Self {
            id: r.id,
            project_id: r.project_id,
            title: r.title,
            description: r.description,
            priority: r.priority,
            status: r.status,
            scope: parse_json_field(&r.scope_json),
            acceptance_criteria: parse_json_field(&r.acceptance_json),
            dependencies: parse_json_field(&r.dependencies_json),
            claimed_by: r.claimed_by,
            progress_summary: r.progress_summary,
            blocked_reason: r.blocked_reason,
            created_by: r.created_by,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

pub fn value_to_json_array_string(v: Option<Value>) -> String {
    match v {
        Some(Value::Array(_)) | Some(Value::Object(_)) => v.unwrap().to_string(),
        Some(other) => Value::Array(vec![other]).to_string(),
        None => "[]".into(),
    }
}
