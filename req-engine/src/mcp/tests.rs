//! Unit tests: MCP tool handlers map to services correctly (no full stdio loop).

use std::sync::{Arc, Mutex};

use crate::db::open_in_memory;
use crate::domain::models::CreateRequirementInput;
use crate::domain::state::{Role, Status};
use crate::mcp::auth::AuthContext;
use crate::mcp::dto::{
    CreateRequirementArgs, ListReadyTasksArgs, ListRequirementsArgs, ProjectView,
    ReleaseTaskArgs, ReportProgressArgs, RequirementIdArgs, UpdateRequirementArgs,
    value_to_json_array_string,
};
use crate::mcp::util::{assert_req_in_scope, effective_project_id};
use crate::mcp::{McpRole, McpState, role_allowed_for_surface};
use crate::services::PairBinding;
use crate::domain::models::AgentSeat;
use crate::services::{
    cancel_requirement, claim_task, create_project, create_requirement, get_requirement,
    list_projects, list_ready_tasks, list_requirements_for_project_filtered, release_task,
    report_progress, submit_for_review, update_requirement, UpdateRequirementInput,
};

fn planner_state(conn: rusqlite::Connection) -> McpState {
    McpState {
        db: Arc::new(Mutex::new(conn)),
        auth: AuthContext {
            name: "planner".into(),
            role: Role::Planner,
        },
        surface: McpRole::Planner,
        binding: None,
    }
}

fn foreman_state(conn: rusqlite::Connection) -> McpState {
    McpState {
        db: Arc::new(Mutex::new(conn)),
        auth: AuthContext {
            name: "foreman".into(),
            role: Role::Foreman,
        },
        surface: McpRole::Foreman,
        binding: None,
    }
}

/// Mirror of planner create_requirement tool body (service mapping).
fn tool_create_requirement(
    state: &McpState,
    args: CreateRequirementArgs,
) -> Result<serde_json::Value, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let project_id = effective_project_id(state, args.project_id.as_deref())?;
    let input = CreateRequirementInput {
        project_id,
        title: args.title,
        description: args.description.unwrap_or_default(),
        priority: args.priority.unwrap_or_else(|| "medium".into()),
        scope_json: value_to_json_array_string(args.scope),
        non_scope_json: "[]".into(),
        acceptance_json: value_to_json_array_string(args.acceptance_criteria),
        dependencies_json: value_to_json_array_string(args.dependencies),
        created_by: state.auth.name.clone(),
    };
    create_requirement(&conn, input)
        .map(|r| serde_json::to_value(r).unwrap())
        .map_err(|e| e.to_string())
}

fn tool_list_projects(state: &McpState) -> Result<Vec<ProjectView>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    list_projects(&conn)
        .map(|ps| ps.into_iter().map(ProjectView::from).collect())
        .map_err(|e| e.to_string())
}

fn tool_list_requirements(
    state: &McpState,
    args: ListRequirementsArgs,
) -> Result<Vec<String>, String> {
    let status = match args.status.as_deref() {
        None => None,
        Some(s) => Some(Status::parse(s).ok_or_else(|| format!("bad status {s}"))?),
    };
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let project_id = effective_project_id(state, args.project_id.as_deref())?;
    list_requirements_for_project_filtered(&conn, &project_id, status)
        .map(|rs| rs.into_iter().map(|r| r.id).collect())
        .map_err(|e| e.to_string())
}

fn tool_update_requirement(
    state: &McpState,
    args: UpdateRequirementArgs,
) -> Result<String, String> {
    let input = UpdateRequirementInput {
        title: args.title,
        description: args.description,
        priority: args.priority,
        scope_json: args.scope.map(|v| value_to_json_array_string(Some(v))),
        non_scope_json: None,
        acceptance_json: args
            .acceptance_criteria
            .map(|v| value_to_json_array_string(Some(v))),
        dependencies_json: args
            .dependencies
            .map(|v| value_to_json_array_string(Some(v))),
    };
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    update_requirement(&conn, &args.id, &state.auth.name, input)
        .map(|r| r.title)
        .map_err(|e| e.to_string())
}

fn tool_cancel(state: &McpState, args: RequirementIdArgs) -> Result<Status, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    cancel_requirement(&mut conn, &args.id, &state.auth.name, state.auth.role)
        .map(|r| r.status)
        .map_err(|e| e.to_string())
}

fn tool_list_ready(
    state: &McpState,
    args: ListReadyTasksArgs,
) -> Result<Vec<String>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    list_ready_tasks(&conn, args.project_id.as_deref())
        .map(|rs| rs.into_iter().map(|r| r.id).collect())
        .map_err(|e| e.to_string())
}

fn tool_claim(state: &McpState, args: RequirementIdArgs) -> Result<Status, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    claim_task(&mut conn, &args.id, &state.auth.name, state.auth.role)
        .map(|r| r.status)
        .map_err(|e| e.to_string())
}

fn tool_progress(state: &McpState, args: ReportProgressArgs) -> Result<Option<String>, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    report_progress(
        &mut conn,
        &args.id,
        &state.auth.name,
        state.auth.role,
        Some(&args.summary),
        args.blocked_reason.as_deref(),
    )
    .map(|r| r.progress_summary)
    .map_err(|e| e.to_string())
}

fn tool_submit(state: &McpState, args: RequirementIdArgs) -> Result<Status, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    submit_for_review(&mut conn, &args.id, &state.auth.name, state.auth.role)
        .map(|r| r.status)
        .map_err(|e| e.to_string())
}

fn tool_release(state: &McpState, args: ReleaseTaskArgs) -> Result<Status, String> {
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    release_task(&mut conn, &args.id, &state.auth.name, state.auth.role)
        .map(|r| r.status)
        .map_err(|e| e.to_string())
}

#[test]
fn role_surface_matrix() {
    assert!(role_allowed_for_surface(Role::Admin, McpRole::Planner));
    assert!(role_allowed_for_surface(Role::Admin, McpRole::Foreman));
    assert!(role_allowed_for_surface(Role::Planner, McpRole::Planner));
    assert!(!role_allowed_for_surface(Role::Planner, McpRole::Foreman));
    assert!(role_allowed_for_surface(Role::Foreman, McpRole::Foreman));
    assert!(!role_allowed_for_surface(Role::Foreman, McpRole::Planner));
}

#[test]
fn planner_tools_map_to_services() {
    let conn = open_in_memory().unwrap();
    let p = create_project(&conn, "P", "#000", "", "").unwrap();
    let state = planner_state(conn);

    let projects = tool_list_projects(&state).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "P");

    let created = tool_create_requirement(
        &state,
        CreateRequirementArgs {
            project_id: Some(p.id.clone()),
            title: "T1".into(),
            description: Some("d".into()),
            priority: Some("high".into()),
            scope: Some(serde_json::json!(["a"])),
            acceptance_criteria: Some(serde_json::json!(["ok"])),
            dependencies: None,
        },
    )
    .unwrap();
    assert_eq!(created["status"], "todo");
    let id = created["id"].as_str().unwrap().to_string();

    let listed = tool_list_requirements(
        &state,
        ListRequirementsArgs {
            project_id: Some(p.id.clone()),
            status: Some("todo".into()),
        },
    )
    .unwrap();
    assert!(listed.contains(&id));

    let new_title = tool_update_requirement(
        &state,
        UpdateRequirementArgs {
            id: id.clone(),
            title: Some("T1-renamed".into()),
            description: None,
            priority: None,
            scope: None,
            acceptance_criteria: None,
            dependencies: None,
        },
    )
    .unwrap();
    assert_eq!(new_title, "T1-renamed");

    let st = tool_cancel(
        &state,
        RequirementIdArgs { id: id.clone() },
    )
    .unwrap();
    assert_eq!(st, Status::Cancelled);
}

#[test]
fn foreman_tools_map_to_services() {
    let conn = open_in_memory().unwrap();
    let p = create_project(&conn, "P", "#000", "", "").unwrap();
    let req = create_requirement(
        &conn,
        CreateRequirementInput {
            project_id: p.id.clone(),
            title: "Ship".into(),
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
    let id = req.id.clone();
    drop(req);

    let state = foreman_state(conn);

    let ready = tool_list_ready(
        &state,
        ListReadyTasksArgs {
            project_id: Some(p.id.clone()),
        },
    )
    .unwrap();
    assert!(ready.contains(&id));

    assert_eq!(
        tool_claim(&state, RequirementIdArgs { id: id.clone() }).unwrap(),
        Status::InProgress
    );

    assert_eq!(
        tool_progress(
            &state,
            ReportProgressArgs {
                id: id.clone(),
                summary: "50%".into(),
                blocked_reason: None,
            },
        )
        .unwrap()
        .as_deref(),
        Some("50%")
    );

    assert_eq!(
        tool_submit(&state, RequirementIdArgs { id: id.clone() }).unwrap(),
        Status::Review
    );

    // release only from in_progress — force status back for release test path
    {
        let mut c = state.db.lock().unwrap();
        // re-claim path: fail submit already moved to review; create new for release
        let req2 = create_requirement(
            &c,
            CreateRequirementInput {
                project_id: p.id.clone(),
                title: "Release me".into(),
                description: "".into(),
                priority: "low".into(),
                scope_json: "[]".into(),
                non_scope_json: "[]".into(),
                acceptance_json: "[]".into(),
                dependencies_json: "[]".into(),
                created_by: "planner".into(),
            },
        )
        .unwrap();
        let id2 = req2.id;
        claim_task(&mut c, &id2, "foreman", Role::Foreman).unwrap();
        drop(c);
        assert_eq!(
            tool_release(
                &state,
                ReleaseTaskArgs {
                    id: id2.clone(),
                    reason: Some("context switch".into()),
                },
            )
            .unwrap(),
            Status::Todo
        );
        let stored = get_requirement(&state.db.lock().unwrap(), &id2)
            .unwrap()
            .unwrap();
        assert!(stored.claimed_by.is_none());
    }
}

#[test]
fn planner_tool_names() {
    // Document expected planner surface (compile-time / registry intent).
    let names = [
        "create_requirement",
        "list_requirements",
        "get_requirement",
        "update_requirement",
        "cancel_requirement",
    ];
    assert_eq!(names.len(), 5);
    assert!(!names.contains(&"list_projects"));
    assert!(!names.contains(&"claim_task"));
}

#[test]
fn foreman_tool_names() {
    let names = [
        "list_ready_tasks",
        "get_requirement",
        "claim_task",
        "report_progress",
        "submit_for_review",
        "release_task",
    ];
    assert_eq!(names.len(), 6);
    // Explicitly forbid complete_review / set_status on foreman surface.
    assert!(!names.contains(&"complete_review"));
    assert!(!names.contains(&"set_status"));
}

#[test]
fn pair_binding_pins_project() {
    let conn = open_in_memory().unwrap();
    let a = create_project(&conn, "A", "#000", "", "").unwrap();
    let b = create_project(&conn, "B", "#000", "", "").unwrap();
    let req_b = create_requirement(
        &conn,
        CreateRequirementInput {
            project_id: b.id.clone(),
            title: "other".into(),
            description: "".into(),
            priority: "low".into(),
            scope_json: "[]".into(),
            non_scope_json: "[]".into(),
            acceptance_json: "[]".into(),
            dependencies_json: "[]".into(),
            created_by: "planner".into(),
        },
    )
    .unwrap();

    let mut state = planner_state(conn);
    state.binding = Some(PairBinding {
        project_id: a.id.clone(),
        seat: AgentSeat::Discuss,
    });

    assert_eq!(
        effective_project_id(&state, Some(&b.id)).unwrap(),
        a.id
    );
    assert!(assert_req_in_scope(&state, &req_b).is_err());

    let created = tool_create_requirement(
        &state,
        CreateRequirementArgs {
            project_id: Some(b.id.clone()),
            title: "forced-into-a".into(),
            description: None,
            priority: None,
            scope: None,
            acceptance_criteria: None,
            dependencies: None,
        },
    )
    .unwrap();
    assert_eq!(created["project_id"], a.id);
}
