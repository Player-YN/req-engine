//! Requirement services: create → todo and transactional lifecycle verbs.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::models::{CreateRequirementInput, Event, Requirement};
use crate::domain::state::{
    Role, Status, Transition, TransitionContext, TransitionError, apply_transition,
    status_on_create,
};

#[derive(Debug, Error)]
pub enum CreateRequirementError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("title must not be empty")]
    EmptyTitle,

    #[error("project is archived: {0}")]
    ProjectArchived(String),
}

#[derive(Debug, Error)]
pub enum UpdateRequirementError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("requirement not found: {0}")]
    NotFound(String),

    #[error("update only allowed when status is todo (current: {0})")]
    NotTodo(Status),

    #[error("title must not be empty")]
    EmptyTitle,

    #[error("project is archived: {0}")]
    ProjectArchived(String),
}

/// Partial field updates for a requirement still in `todo`.
#[derive(Debug, Clone, Default)]
pub struct UpdateRequirementInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub scope_json: Option<String>,
    pub non_scope_json: Option<String>,
    pub acceptance_json: Option<String>,
    pub dependencies_json: Option<String>,
}

/// Shared error for lifecycle verbs (claim, progress, review, release, cancel).
#[derive(Debug, Error)]
pub enum VerbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("requirement not found: {0}")]
    NotFound(String),

    #[error(transparent)]
    Transition(#[from] TransitionError),

    #[error("concurrent claim lost (already claimed or status changed)")]
    ConcurrentClaimLost,

    #[error("project is archived: {0}")]
    ProjectArchived(String),

    #[error("dependencies not satisfied")]
    DependenciesNotMet,

    #[error("reject reason is required")]
    RejectReasonRequired,
}

/// Back-compat alias used by Task A call sites / tests.
pub type ClaimError = VerbError;

fn parse_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn row_to_requirement(row: &rusqlite::Row<'_>) -> rusqlite::Result<Requirement> {
    let status_s: String = row.get("status")?;
    let status = Status::parse(&status_s).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("invalid requirement status: {status_s}").into(),
        )
    })?;
    Ok(Requirement {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        priority: row.get("priority")?,
        status,
        scope_json: row.get("scope_json")?,
        non_scope_json: row.get("non_scope_json")?,
        acceptance_json: row.get("acceptance_json")?,
        dependencies_json: row.get("dependencies_json")?,
        claimed_by: row.get("claimed_by")?,
        progress_summary: row.get("progress_summary")?,
        blocked_reason: row.get("blocked_reason")?,
        external_run_id: row.get("external_run_id")?,
        created_by: row.get("created_by")?,
        created_at: parse_dt(&row.get::<_, String>("created_at")?),
        updated_at: parse_dt(&row.get::<_, String>("updated_at")?),
    })
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        requirement_id: row.get("requirement_id")?,
        actor: row.get("actor")?,
        kind: row.get("kind")?,
        message: row.get("message")?,
        payload_json: row.get("payload_json")?,
        created_at: parse_dt(&row.get::<_, String>("created_at")?),
    })
}

const REQ_SELECT: &str = "SELECT id, project_id, title, description, priority, status,
                scope_json, non_scope_json, acceptance_json, dependencies_json,
                claimed_by, progress_summary, blocked_reason, external_run_id,
                created_by, created_at, updated_at
         FROM requirements";

pub fn get_requirement(conn: &Connection, id: &str) -> Result<Option<Requirement>, rusqlite::Error> {
    conn.query_row(
        &format!("{REQ_SELECT} WHERE id = ?1"),
        [id],
        row_to_requirement,
    )
    .optional()
}

pub fn list_requirements_for_project(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<Requirement>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "{REQ_SELECT} WHERE project_id = ?1 ORDER BY created_at ASC"
    ))?;
    let rows = stmt.query_map([project_id], row_to_requirement)?;
    rows.collect()
}

/// List requirements for a project, optionally filtered by status.
pub fn list_requirements_for_project_filtered(
    conn: &Connection,
    project_id: &str,
    status: Option<Status>,
) -> Result<Vec<Requirement>, rusqlite::Error> {
    let all = list_requirements_for_project(conn, project_id)?;
    Ok(match status {
        Some(s) => all.into_iter().filter(|r| r.status == s).collect(),
        None => all,
    })
}

/// Update editable fields of a requirement. Only allowed while status is `todo`.
pub fn update_requirement(
    conn: &Connection,
    id: &str,
    actor: &str,
    input: UpdateRequirementInput,
) -> Result<Requirement, UpdateRequirementError> {
    let mut req = get_requirement(conn, id)?
        .ok_or_else(|| UpdateRequirementError::NotFound(id.to_string()))?;

    reject_archived_for_update(conn, &req.project_id)?;

    if req.status != Status::Todo {
        return Err(UpdateRequirementError::NotTodo(req.status));
    }

    if let Some(ref t) = input.title {
        if t.trim().is_empty() {
            return Err(UpdateRequirementError::EmptyTitle);
        }
        req.title = t.clone();
    }
    if let Some(ref d) = input.description {
        req.description = d.clone();
    }
    if let Some(ref p) = input.priority {
        req.priority = p.clone();
    }
    if let Some(ref s) = input.scope_json {
        req.scope_json = s.clone();
    }
    if let Some(ref s) = input.non_scope_json {
        req.non_scope_json = s.clone();
    }
    if let Some(ref s) = input.acceptance_json {
        req.acceptance_json = s.clone();
    }
    if let Some(ref s) = input.dependencies_json {
        req.dependencies_json = s.clone();
    }

    let now = Utc::now();
    let now_s = now.to_rfc3339();

    conn.execute(
        "UPDATE requirements
         SET title = ?1,
             description = ?2,
             priority = ?3,
             scope_json = ?4,
             non_scope_json = ?5,
             acceptance_json = ?6,
             dependencies_json = ?7,
             updated_at = ?8
         WHERE id = ?9
           AND status = 'todo'",
        rusqlite::params![
            req.title,
            req.description,
            req.priority,
            req.scope_json,
            req.non_scope_json,
            req.acceptance_json,
            req.dependencies_json,
            now_s,
            id,
        ],
    )?;

    let payload = serde_json::json!({
        "title": input.title,
        "description": input.description,
        "priority": input.priority,
    })
    .to_string();

    append_event(
        conn,
        &req.project_id,
        Some(id),
        actor,
        "update",
        &format!("{actor} updated requirement"),
        &payload,
    )?;

    req.updated_at = now;
    Ok(req)
}

/// Parse dependencies_json as a JSON array of requirement id strings.
fn parse_dependency_ids(dependencies_json: &str) -> Vec<String> {
    match serde_json::from_str::<serde_json::Value>(dependencies_json) {
        Ok(serde_json::Value::Array(arr)) => arr
            .into_iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s),
                other => other.as_str().map(|s| s.to_string()),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether every dependency id exists and has status `done`.
fn dependencies_satisfied(
    conn: &Connection,
    dependencies_json: &str,
) -> Result<bool, rusqlite::Error> {
    let deps = parse_dependency_ids(dependencies_json);
    for dep_id in deps {
        match get_requirement(conn, &dep_id)? {
            Some(dep) if dep.status == Status::Done => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// List tasks ready to claim: status=`todo` and all dependencies are `done`.
///
/// If `project_id` is `None`, scans all projects.
pub fn list_ready_tasks(
    conn: &Connection,
    project_id: Option<&str>,
) -> Result<Vec<Requirement>, rusqlite::Error> {
    let candidates: Vec<Requirement> = if let Some(pid) = project_id {
        list_requirements_for_project_filtered(conn, pid, Some(Status::Todo))?
    } else {
        let mut stmt = conn.prepare(&format!(
            "{REQ_SELECT} WHERE status = 'todo' ORDER BY created_at ASC"
        ))?;
        let rows = stmt.query_map([], row_to_requirement)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut ready = Vec::new();
    for req in candidates {
        match crate::services::projects::get_project(conn, &req.project_id) {
            Ok(Some(p)) if p.archived_at.is_none() => {}
            _ => continue,
        }
        if dependencies_satisfied(conn, &req.dependencies_json)? {
            ready.push(req);
        }
    }
    Ok(ready)
}

pub fn list_events_for_requirement(
    conn: &Connection,
    requirement_id: &str,
) -> Result<Vec<Event>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, requirement_id, actor, kind, message, payload_json, created_at
         FROM events
         WHERE requirement_id = ?1
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([requirement_id], row_to_event)?;
    rows.collect()
}

fn append_event(
    conn: &Connection,
    project_id: &str,
    requirement_id: Option<&str>,
    actor: &str,
    kind: &str,
    message: &str,
    payload_json: &str,
) -> Result<(), rusqlite::Error> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO events (id, project_id, requirement_id, actor, kind, message, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            id,
            project_id,
            requirement_id,
            actor,
            kind,
            message,
            payload_json,
            now
        ],
    )?;
    Ok(())
}

fn reject_archived_for_create(
    conn: &Connection,
    project_id: &str,
) -> Result<(), CreateRequirementError> {
    match crate::services::projects::ensure_project_writable(conn, project_id) {
        Ok(_) => Ok(()),
        Err(crate::services::projects::ProjectError::Archived(id)) => {
            Err(CreateRequirementError::ProjectArchived(id))
        }
        Err(crate::services::projects::ProjectError::NotFound(id)) => {
            Err(CreateRequirementError::ProjectNotFound(id))
        }
        Err(crate::services::projects::ProjectError::Sqlite(e)) => {
            Err(CreateRequirementError::Sqlite(e))
        }
        Err(e) => Err(CreateRequirementError::ProjectNotFound(e.to_string())),
    }
}

fn reject_archived_for_update(
    conn: &Connection,
    project_id: &str,
) -> Result<(), UpdateRequirementError> {
    match crate::services::projects::ensure_project_writable(conn, project_id) {
        Ok(_) => Ok(()),
        Err(crate::services::projects::ProjectError::Archived(id)) => {
            Err(UpdateRequirementError::ProjectArchived(id))
        }
        Err(crate::services::projects::ProjectError::Sqlite(e)) => {
            Err(UpdateRequirementError::Sqlite(e))
        }
        Err(e) => Err(UpdateRequirementError::NotFound(e.to_string())),
    }
}

fn reject_archived_for_verb(conn: &Connection, project_id: &str) -> Result<(), VerbError> {
    match crate::services::projects::ensure_project_writable(conn, project_id) {
        Ok(_) => Ok(()),
        Err(crate::services::projects::ProjectError::Archived(id)) => {
            Err(VerbError::ProjectArchived(id))
        }
        Err(crate::services::projects::ProjectError::Sqlite(e)) => Err(VerbError::Sqlite(e)),
        Err(e) => Err(VerbError::NotFound(e.to_string())),
    }
}

/// Create a requirement. Status is always `todo` (verb: create). No free-form status.
pub fn create_requirement(
    conn: &Connection,
    input: CreateRequirementInput,
) -> Result<Requirement, CreateRequirementError> {
    if input.title.trim().is_empty() {
        return Err(CreateRequirementError::EmptyTitle);
    }

    let project_exists: bool = conn
        .query_row(
            "SELECT 1 FROM projects WHERE id = ?1",
            [&input.project_id],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if !project_exists {
        return Err(CreateRequirementError::ProjectNotFound(
            input.project_id.clone(),
        ));
    }
    reject_archived_for_create(conn, &input.project_id)?;

    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let now_s = now.to_rfc3339();
    let status = status_on_create();

    conn.execute(
        "INSERT INTO requirements (
            id, project_id, title, description, priority, status,
            scope_json, non_scope_json, acceptance_json, dependencies_json,
            claimed_by, progress_summary, blocked_reason, external_run_id,
            created_by, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            NULL, NULL, NULL, NULL,
            ?11, ?12, ?13
         )",
        rusqlite::params![
            id,
            input.project_id,
            input.title,
            input.description,
            input.priority,
            status.as_str(),
            input.scope_json,
            input.non_scope_json,
            input.acceptance_json,
            input.dependencies_json,
            input.created_by,
            now_s,
            now_s,
        ],
    )?;

    append_event(
        conn,
        &input.project_id,
        Some(&id),
        &input.created_by,
        "create",
        &format!("created requirement: {}", input.title),
        "{}",
    )?;

    Ok(Requirement {
        id,
        project_id: input.project_id,
        title: input.title,
        description: input.description,
        priority: input.priority,
        status,
        scope_json: input.scope_json,
        non_scope_json: input.non_scope_json,
        acceptance_json: input.acceptance_json,
        dependencies_json: input.dependencies_json,
        claimed_by: None,
        progress_summary: None,
        blocked_reason: None,
        external_run_id: None,
        created_by: input.created_by,
        created_at: now,
        updated_at: now,
    })
}

/// Atomically claim a task (`claim_task` verb).
///
/// Uses `BEGIN IMMEDIATE` so concurrent claimers serialize; only one wins.
pub fn claim_task(
    conn: &mut Connection,
    requirement_id: &str,
    actor: &str,
    role: Role,
) -> Result<Requirement, VerbError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let req = tx
        .query_row(
            &format!("{REQ_SELECT} WHERE id = ?1"),
            [requirement_id],
            row_to_requirement,
        )
        .optional()?
        .ok_or_else(|| VerbError::NotFound(requirement_id.to_string()))?;

    reject_archived_for_verb(&tx, &req.project_id)?;

    if !dependencies_satisfied(&tx, &req.dependencies_json)? {
        return Err(VerbError::DependenciesNotMet);
    }

    let ctx = TransitionContext {
        current: req.status,
        role,
        actor: actor.to_string(),
        claimed_by: req.claimed_by.clone(),
    };

    let result = apply_transition(&ctx, &Transition::ClaimTask)?;

    // Defensive double-check in SQL for race safety even if pure check passed.
    let now = Utc::now().to_rfc3339();
    let updated = tx.execute(
        "UPDATE requirements
         SET status = ?1,
             claimed_by = ?2,
             updated_at = ?3
         WHERE id = ?4
           AND status = 'todo'
           AND claimed_by IS NULL",
        rusqlite::params![result.new_status.as_str(), actor, now, requirement_id],
    )?;

    if updated != 1 {
        return Err(VerbError::ConcurrentClaimLost);
    }

    append_event(
        &tx,
        &req.project_id,
        Some(requirement_id),
        actor,
        "claim_task",
        &format!("{actor} claimed requirement"),
        "{}",
    )?;

    tx.commit()?;

    let mut out = req;
    out.status = result.new_status;
    out.claimed_by = Some(actor.to_string());
    out.updated_at = parse_dt(&now);
    Ok(out)
}

/// Report progress without changing status (claimant or admin).
pub fn report_progress(
    conn: &mut Connection,
    requirement_id: &str,
    actor: &str,
    role: Role,
    summary: Option<&str>,
    blocked_reason: Option<&str>,
) -> Result<Requirement, VerbError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let req = load_for_update(&tx, requirement_id)?;
    reject_archived_for_verb(&tx, &req.project_id)?;
    let ctx = context_from(&req, actor, role);
    let result = apply_transition(&ctx, &Transition::ReportProgress)?;

    let now = Utc::now().to_rfc3339();
    let new_summary = summary
        .map(|s| s.to_string())
        .or_else(|| req.progress_summary.clone());
    // Explicit None clears blocked_reason only when caller passes Some/None carefully:
    // if blocked_reason is Some(""), clear; if None, leave previous.
    // For API we treat provided fields as set; optional body fields handle this.
    let new_blocked = match blocked_reason {
        Some(s) if s.is_empty() => None,
        Some(s) => Some(s.to_string()),
        None => req.blocked_reason.clone(),
    };

    tx.execute(
        "UPDATE requirements
         SET progress_summary = ?1,
             blocked_reason = ?2,
             updated_at = ?3
         WHERE id = ?4",
        rusqlite::params![new_summary, new_blocked, now, requirement_id],
    )?;

    let payload = serde_json::json!({
        "summary": summary,
        "blocked_reason": blocked_reason,
    })
    .to_string();

    append_event(
        &tx,
        &req.project_id,
        Some(requirement_id),
        actor,
        "report_progress",
        summary.unwrap_or("progress reported"),
        &payload,
    )?;

    tx.commit()?;

    let mut out = req;
    out.status = result.new_status;
    out.progress_summary = new_summary;
    out.blocked_reason = new_blocked;
    out.updated_at = parse_dt(&now);
    Ok(out)
}

/// in_progress → review (claimant or admin).
pub fn submit_for_review(
    conn: &mut Connection,
    requirement_id: &str,
    actor: &str,
    role: Role,
) -> Result<Requirement, VerbError> {
    apply_status_verb(
        conn,
        requirement_id,
        actor,
        role,
        Transition::SubmitForReview,
        "submit_for_review",
        &format!("{actor} submitted for review"),
        "{}",
    )
}

/// review → done (pass) or todo (fail). Planner/admin in domain; HTTP may restrict further.
pub fn complete_review(
    conn: &mut Connection,
    requirement_id: &str,
    actor: &str,
    role: Role,
    pass: bool,
    reason: Option<&str>,
) -> Result<Requirement, VerbError> {
    if !pass {
        let reason = reason.map(str::trim).unwrap_or("");
        if reason.is_empty() {
            return Err(VerbError::RejectReasonRequired);
        }
    }
    let kind = if pass {
        "complete_review_pass"
    } else {
        "complete_review_fail"
    };
    let message = if pass {
        format!("{actor} approved review")
    } else {
        format!("{actor} rejected review")
    };
    let payload = serde_json::json!({
        "pass": pass,
        "reason": reason,
    })
    .to_string();

    apply_status_verb(
        conn,
        requirement_id,
        actor,
        role,
        Transition::CompleteReview { pass },
        kind,
        &message,
        &payload,
    )
}

/// in_progress → todo; clear claim (claimant or admin).
pub fn release_task(
    conn: &mut Connection,
    requirement_id: &str,
    actor: &str,
    role: Role,
) -> Result<Requirement, VerbError> {
    apply_status_verb(
        conn,
        requirement_id,
        actor,
        role,
        Transition::ReleaseTask,
        "release_task",
        &format!("{actor} released requirement"),
        "{}",
    )
}

/// Soft-cancel (role-scoped in domain).
pub fn cancel_requirement(
    conn: &mut Connection,
    requirement_id: &str,
    actor: &str,
    role: Role,
) -> Result<Requirement, VerbError> {
    apply_status_verb(
        conn,
        requirement_id,
        actor,
        role,
        Transition::Cancel,
        "cancel",
        &format!("{actor} cancelled requirement"),
        "{}",
    )
}

fn load_for_update(
    conn: &Connection,
    requirement_id: &str,
) -> Result<Requirement, VerbError> {
    conn.query_row(
        &format!("{REQ_SELECT} WHERE id = ?1"),
        [requirement_id],
        row_to_requirement,
    )
    .optional()?
    .ok_or_else(|| VerbError::NotFound(requirement_id.to_string()))
}

fn context_from(req: &Requirement, actor: &str, role: Role) -> TransitionContext {
    TransitionContext {
        current: req.status,
        role,
        actor: actor.to_string(),
        claimed_by: req.claimed_by.clone(),
    }
}

/// Generic transactional verb that updates status / claimed_by from `apply_transition`.
fn apply_status_verb(
    conn: &mut Connection,
    requirement_id: &str,
    actor: &str,
    role: Role,
    transition: Transition,
    event_kind: &str,
    event_message: &str,
    event_payload: &str,
) -> Result<Requirement, VerbError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let req = load_for_update(&tx, requirement_id)?;
    reject_archived_for_verb(&tx, &req.project_id)?;
    let ctx = context_from(&req, actor, role);
    let result = apply_transition(&ctx, &transition)?;

    let now = Utc::now().to_rfc3339();
    let new_claimed = match &result.claimed_by_update {
        Some(Some(v)) => Some(v.clone()),
        Some(None) => None,
        None => req.claimed_by.clone(),
    };

    match &result.claimed_by_update {
        Some(_) => {
            tx.execute(
                "UPDATE requirements
                 SET status = ?1,
                     claimed_by = ?2,
                     updated_at = ?3
                 WHERE id = ?4",
                rusqlite::params![
                    result.new_status.as_str(),
                    new_claimed,
                    now,
                    requirement_id
                ],
            )?;
        }
        None => {
            tx.execute(
                "UPDATE requirements
                 SET status = ?1,
                     updated_at = ?2
                 WHERE id = ?3",
                rusqlite::params![result.new_status.as_str(), now, requirement_id],
            )?;
        }
    }

    append_event(
        &tx,
        &req.project_id,
        Some(requirement_id),
        actor,
        event_kind,
        event_message,
        event_payload,
    )?;

    tx.commit()?;

    let mut out = req;
    out.status = result.new_status;
    out.claimed_by = new_claimed;
    out.updated_at = parse_dt(&now);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::services::projects::create_project;

    fn seed_todo(conn: &Connection) -> Requirement {
        let p = create_project(conn, "Demo", "#000", "", "").unwrap();
        create_requirement(
            conn,
            CreateRequirementInput {
                project_id: p.id,
                title: "Ship MVP".into(),
                description: "Task A".into(),
                priority: "high".into(),
                scope_json: "[]".into(),
                non_scope_json: "[]".into(),
                acceptance_json: "[]".into(),
                dependencies_json: "[]".into(),
                created_by: "planner".into(),
            },
        )
        .unwrap()
    }

    #[test]
    fn create_sets_status_todo() {
        let conn = open_in_memory().unwrap();
        let req = seed_todo(&conn);
        assert_eq!(req.status, Status::Todo);
        assert!(req.claimed_by.is_none());
    }

    #[test]
    fn claim_success() {
        let mut conn = open_in_memory().unwrap();
        let req = seed_todo(&conn);
        let claimed = claim_task(&mut conn, &req.id, "alice", Role::Foreman).unwrap();
        assert_eq!(claimed.status, Status::InProgress);
        assert_eq!(claimed.claimed_by.as_deref(), Some("alice"));
    }

    #[test]
    fn claim_twice_only_one_winner() {
        let mut conn = open_in_memory().unwrap();
        let req = seed_todo(&conn);

        let first = claim_task(&mut conn, &req.id, "alice", Role::Foreman);
        assert!(first.is_ok(), "first claim should win: {first:?}");

        let second = claim_task(&mut conn, &req.id, "bob", Role::Foreman);
        assert!(second.is_err(), "second claim must fail, got: {second:?}");
        match second.unwrap_err() {
            VerbError::Transition(TransitionError::AlreadyClaimed { claimed_by }) => {
                assert_eq!(claimed_by, "alice");
            }
            // After a successful claim the row is in_progress, so pure check fails as NotClaimable.
            VerbError::Transition(TransitionError::NotClaimable { status }) => {
                assert_eq!(status, Status::InProgress);
            }
            VerbError::ConcurrentClaimLost => {}
            other => panic!("unexpected error: {other:?}"),
        }

        let stored = get_requirement(&conn, &req.id).unwrap().unwrap();
        assert_eq!(stored.claimed_by.as_deref(), Some("alice"));
        assert_eq!(stored.status, Status::InProgress);
    }

    #[test]
    fn claim_missing_requirement() {
        let mut conn = open_in_memory().unwrap();
        let err = claim_task(&mut conn, "no-such-id", "alice", Role::Foreman).unwrap_err();
        assert!(matches!(err, VerbError::NotFound(_)));
    }

    #[test]
    fn full_happy_path_create_claim_submit_complete_pass() {
        let mut conn = open_in_memory().unwrap();
        let req = seed_todo(&conn);

        let claimed = claim_task(&mut conn, &req.id, "foreman", Role::Foreman).unwrap();
        assert_eq!(claimed.status, Status::InProgress);

        report_progress(
            &mut conn,
            &req.id,
            "foreman",
            Role::Foreman,
            Some("halfway"),
            None,
        )
        .unwrap();

        let in_review = submit_for_review(&mut conn, &req.id, "foreman", Role::Foreman).unwrap();
        assert_eq!(in_review.status, Status::Review);

        let done = complete_review(
            &mut conn,
            &req.id,
            "admin",
            Role::Admin,
            true,
            Some("looks good"),
        )
        .unwrap();
        assert_eq!(done.status, Status::Done);

        let events = list_events_for_requirement(&conn, &req.id).unwrap();
        let kinds: Vec<_> = events.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"create"));
        assert!(kinds.contains(&"claim_task"));
        assert!(kinds.contains(&"report_progress"));
        assert!(kinds.contains(&"submit_for_review"));
        assert!(kinds.contains(&"complete_review_pass"));
    }

    #[test]
    fn release_clears_claim() {
        let mut conn = open_in_memory().unwrap();
        let req = seed_todo(&conn);
        claim_task(&mut conn, &req.id, "foreman", Role::Foreman).unwrap();
        let released = release_task(&mut conn, &req.id, "foreman", Role::Foreman).unwrap();
        assert_eq!(released.status, Status::Todo);
        assert!(released.claimed_by.is_none());
    }

    #[test]
    fn cancel_by_planner_on_todo() {
        let mut conn = open_in_memory().unwrap();
        let req = seed_todo(&conn);
        let cancelled = cancel_requirement(&mut conn, &req.id, "planner", Role::Planner).unwrap();
        assert_eq!(cancelled.status, Status::Cancelled);
    }

    #[test]
    fn update_only_when_todo() {
        let conn = open_in_memory().unwrap();
        let req = seed_todo(&conn);
        let updated = update_requirement(
            &conn,
            &req.id,
            "planner",
            UpdateRequirementInput {
                title: Some("New title".into()),
                description: Some("d".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updated.title, "New title");
        assert_eq!(updated.description, "d");

        let mut conn2 = open_in_memory().unwrap();
        let req2 = seed_todo(&conn2);
        claim_task(&mut conn2, &req2.id, "foreman", Role::Foreman).unwrap();
        let err = update_requirement(
            &conn2,
            &req2.id,
            "planner",
            UpdateRequirementInput {
                title: Some("x".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(err, UpdateRequirementError::NotTodo(_)));
    }

    #[test]
    fn archived_project_rejects_create_and_verbs() {
        use crate::services::projects::archive_project;
        let mut conn = open_in_memory().unwrap();
        let req = seed_todo(&conn);
        archive_project(&conn, &req.project_id).unwrap();

        let err = create_requirement(
            &conn,
            CreateRequirementInput {
                project_id: req.project_id.clone(),
                title: "after archive".into(),
                description: "".into(),
                priority: "medium".into(),
                scope_json: "[]".into(),
                non_scope_json: "[]".into(),
                acceptance_json: "[]".into(),
                dependencies_json: "[]".into(),
                created_by: "planner".into(),
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, CreateRequirementError::ProjectArchived(_)),
            "got {err:?}"
        );

        let err = claim_task(&mut conn, &req.id, "alice", Role::Foreman).unwrap_err();
        assert!(matches!(err, VerbError::ProjectArchived(_)), "got {err:?}");
    }

    #[test]
    fn list_ready_tasks_respects_dependencies() {
        let conn = open_in_memory().unwrap();
        let p = create_project(&conn, "Demo", "#000", "", "").unwrap();
        let dep = create_requirement(
            &conn,
            CreateRequirementInput {
                project_id: p.id.clone(),
                title: "Dep".into(),
                description: "".into(),
                priority: "high".into(),
                scope_json: "[]".into(),
                non_scope_json: "[]".into(),
                acceptance_json: "[]".into(),
                dependencies_json: "[]".into(),
                created_by: "planner".into(),
            },
        )
        .unwrap();
        let blocked = create_requirement(
            &conn,
            CreateRequirementInput {
                project_id: p.id.clone(),
                title: "Blocked".into(),
                description: "".into(),
                priority: "high".into(),
                scope_json: "[]".into(),
                non_scope_json: "[]".into(),
                acceptance_json: "[]".into(),
                dependencies_json: serde_json::json!([dep.id]).to_string(),
                created_by: "planner".into(),
            },
        )
        .unwrap();
        let free = create_requirement(
            &conn,
            CreateRequirementInput {
                project_id: p.id.clone(),
                title: "Free".into(),
                description: "".into(),
                priority: "high".into(),
                scope_json: "[]".into(),
                non_scope_json: "[]".into(),
                acceptance_json: "[]".into(),
                dependencies_json: "[]".into(),
                created_by: "planner".into(),
            },
        )
        .unwrap();

        let ready = list_ready_tasks(&conn, Some(&p.id)).unwrap();
        let ids: Vec<_> = ready.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&dep.id.as_str()));
        assert!(ids.contains(&free.id.as_str()));
        assert!(!ids.contains(&blocked.id.as_str()));

        // mark dep done via SQL for readiness (skip full lifecycle)
        conn.execute(
            "UPDATE requirements SET status = 'done' WHERE id = ?1",
            [&dep.id],
        )
        .unwrap();
        let ready2 = list_ready_tasks(&conn, Some(&p.id)).unwrap();
        let ids2: Vec<_> = ready2.iter().map(|r| r.id.as_str()).collect();
        assert!(ids2.contains(&blocked.id.as_str()));
    }

    #[test]
    fn unknown_status_is_not_treated_as_todo() {
        let mut conn = open_in_memory().unwrap();
        let req = seed_todo(&conn);
        conn.execute(
            "UPDATE requirements SET status = 'BOGUS' WHERE id = ?1",
            [&req.id],
        )
        .unwrap();

        let loaded = get_requirement(&conn, &req.id);
        assert!(loaded.is_err(), "corrupt status must not load as todo: {loaded:?}");

        let err = claim_task(&mut conn, &req.id, "alice", Role::Foreman).unwrap_err();
        assert!(
            !matches!(err, VerbError::ConcurrentClaimLost),
            "must not masquerade as race: {err}"
        );
        let err2 = cancel_requirement(&mut conn, &req.id, "planner", Role::Planner).unwrap_err();
        assert!(
            !matches!(err2, VerbError::Transition(_)),
            "must not apply todo-cancel to corrupt row: {err2}"
        );
    }

    #[test]
    fn claim_rejects_unsatisfied_dependencies() {
        let mut conn = open_in_memory().unwrap();
        let p = create_project(&conn, "Demo", "#000", "", "").unwrap();
        let dep = create_requirement(
            &conn,
            CreateRequirementInput {
                project_id: p.id.clone(),
                title: "Dep".into(),
                description: "".into(),
                priority: "high".into(),
                scope_json: "[]".into(),
                non_scope_json: "[]".into(),
                acceptance_json: "[]".into(),
                dependencies_json: "[]".into(),
                created_by: "planner".into(),
            },
        )
        .unwrap();
        let blocked = create_requirement(
            &conn,
            CreateRequirementInput {
                project_id: p.id.clone(),
                title: "Blocked".into(),
                description: "".into(),
                priority: "high".into(),
                scope_json: "[]".into(),
                non_scope_json: "[]".into(),
                acceptance_json: "[]".into(),
                dependencies_json: serde_json::json!([dep.id]).to_string(),
                created_by: "planner".into(),
            },
        )
        .unwrap();

        let err = claim_task(&mut conn, &blocked.id, "alice", Role::Foreman).unwrap_err();
        assert!(matches!(err, VerbError::DependenciesNotMet), "got {err:?}");
        let stored = get_requirement(&conn, &blocked.id).unwrap().unwrap();
        assert_eq!(stored.status, Status::Todo);
        assert!(stored.claimed_by.is_none());

        conn.execute(
            "UPDATE requirements SET status = 'done' WHERE id = ?1",
            [&dep.id],
        )
        .unwrap();
        let claimed = claim_task(&mut conn, &blocked.id, "alice", Role::Foreman).unwrap();
        assert_eq!(claimed.status, Status::InProgress);
    }

}
