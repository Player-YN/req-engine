//! REST routes under `/v1`.

use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::domain::models::{AgentSeat, CreateRequirementInput};
use crate::domain::state::Role;
use crate::http::auth::AuthUser;
use crate::http::dto::{
    CompleteReviewBody, CreateProjectBody, CreateRequirementBody, EventDto, HealthResponse,
    OccupantDto, PatchProjectBody, ProgressBody, ProjectDto, ProjectPairCodesDto,
    RequirementDetailDto, RequirementDto, SeatPairDto,
};
use crate::http::error::ApiError;
use crate::http::AppState;
use crate::services::{
    ack_agent_seat, archive_project, cancel_requirement, claim_task, complete_review,
    create_project, create_project_with_id, create_requirement, ensure_project_pair_codes,
    get_project, get_requirement, list_events_for_requirement, list_projects_filtered,
    list_requirements_for_project, onboarding_prompt, release_task, report_progress,
    rotate_pair_code, submit_for_review, unarchive_project, update_project, OnboardingCtx,
    project_presence,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/projects", get(list_projects_handler).post(create_project_handler))
        .route("/v1/projects/{id}", patch(patch_project_handler))
        .route("/v1/projects/{id}/archive", post(archive_project_handler))
        .route(
            "/v1/projects/{id}/unarchive",
            post(unarchive_project_handler),
        )
        .route(
            "/v1/projects/{id}/agents/{seat}/ack",
            post(ack_agent_seat_handler),
        )
        .route(
            "/v1/projects/{id}/pair-codes",
            get(get_pair_codes_handler),
        )
        .route(
            "/v1/projects/{id}/pair-codes/{seat}/rotate",
            post(rotate_pair_code_handler),
        )
        .route(
            "/v1/projects/{id}/requirements",
            get(list_requirements_handler).post(create_requirement_handler),
        )
        .route("/v1/requirements/{id}", get(get_requirement_handler))
        .route("/v1/requirements/{id}/claim", post(claim_handler))
        .route("/v1/requirements/{id}/progress", post(progress_handler))
        .route(
            "/v1/requirements/{id}/submit-review",
            post(submit_review_handler),
        )
        .route(
            "/v1/requirements/{id}/complete-review",
            post(complete_review_handler),
        )
        .route("/v1/requirements/{id}/cancel", post(cancel_handler))
        .route("/v1/requirements/{id}/release", post(release_handler))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "req-engine",
    })
}

#[derive(Debug, Default, Deserialize)]
struct ListProjectsQuery {
    #[serde(default)]
    include_archived: bool,
}

async fn list_projects_handler(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(q): Query<ListProjectsQuery>,
) -> Result<Json<Vec<ProjectDto>>, ApiError> {
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let projects = list_projects_filtered(&conn, q.include_archived)?;
    Ok(Json(
        projects
            .into_iter()
            .map(|p| project_dto_with_presence(&conn, p))
            .collect(),
    ))
}

async fn create_project_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateProjectBody>,
) -> Result<(axum::http::StatusCode, Json<ProjectDto>), ApiError> {
    user.require_roles(&[Role::Admin])?;
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;

    let project = if let Some(ref id) = body.id {
        create_project_with_id(
            &conn,
            id,
            &body.name,
            &body.color,
            &body.blurb,
            &body.local_path,
        )?
    } else {
        create_project(
            &conn,
            &body.name,
            &body.color,
            &body.blurb,
            &body.local_path,
        )?
    };

    let _ = ensure_project_pair_codes(&conn, &state.home, &project.id);

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ProjectDto::from(project)),
    ))
}

async fn patch_project_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PatchProjectBody>,
) -> Result<Json<ProjectDto>, ApiError> {
    user.require_roles(&[Role::Admin])?;
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;

    let project = update_project(
        &conn,
        &id,
        body.name.as_deref(),
        body.color.as_deref(),
        body.blurb.as_deref(),
        body.local_path.as_deref(),
    )?;
    Ok(Json(ProjectDto::from(project)))
}

/// Soft-archive project (admin). Hidden from GET /projects; requirements retained.
async fn archive_project_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<ProjectDto>, ApiError> {
    user.require_roles(&[Role::Admin])?;
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let project = archive_project(&conn, &id)?;
    Ok(Json(ProjectDto::from(project)))
}

/// Restore a soft-archived project (admin).
async fn unarchive_project_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<ProjectDto>, ApiError> {
    user.require_roles(&[Role::Admin])?;
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let project = unarchive_project(&conn, &id)?;
    Ok(Json(ProjectDto::from(project)))
}

/// Mark agent seat configured after MCP setup.
/// - seat `discuss` → 讨论 Agent (requires planner or admin token)
/// - seat `build` → 实现 Agent (requires foreman or admin token)
async fn ack_agent_seat_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, seat)): Path<(String, String)>,
) -> Result<Json<ProjectDto>, ApiError> {
    let seat = AgentSeat::parse(&seat).ok_or_else(|| {
        ApiError::bad_request("seat must be `discuss` (讨论 Agent) or `build` (实现 Agent)")
    })?;
    match seat {
        AgentSeat::Discuss => user.require_roles(&[Role::Planner, Role::Admin])?,
        AgentSeat::Build => user.require_roles(&[Role::Foreman, Role::Admin])?,
    }
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let project = ack_agent_seat(&conn, &id, seat)?;
    Ok(Json(ProjectDto::from(project)))
}

fn project_dto_with_presence(
    conn: &rusqlite::Connection,
    project: crate::domain::models::Project,
) -> ProjectDto {
    let mut dto = ProjectDto::from(project);
    if let Ok(live) = project_presence(conn, &dto.id) {
        dto.discuss_seated = live.discuss.seated;
        dto.build_seated = live.build.seated;
        dto.discuss_occupant = occupant_dto(&live.discuss);
        dto.build_occupant = occupant_dto(&live.build);
    }
    dto
}

fn occupant_dto(live: &crate::services::SeatLive) -> Option<OccupantDto> {
    if !live.seated {
        return None;
    }
    let face = live.face.as_ref()?;
    Some(OccupantDto {
        raw_name: live.client_name.clone().unwrap_or_default(),
        label: face.label.clone(),
        key: face.key.clone(),
        initials: face.initials.clone(),
        hue: face.hue,
        known: face.known,
    })
}

fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "req-engine".into())
}

fn pair_codes_dto(
    project: &crate::domain::models::Project,
    codes: &crate::services::SeatPlaintexts,
    home: &std::path::Path,
    conn: &rusqlite::Connection,
) -> ProjectPairCodesDto {
    let home_s = home.display().to_string();
    let exe = exe_path();
    let live = project_presence(conn, &project.id).unwrap_or_default();
    let discuss_ctx = OnboardingCtx {
        exe: &exe,
        home: &home_s,
        project_id: &project.id,
        project_name: &project.name,
        pair_code: &codes.discuss,
        local_path: &project.local_path,
    };
    let build_ctx = OnboardingCtx {
        exe: &exe,
        home: &home_s,
        project_id: &project.id,
        project_name: &project.name,
        pair_code: &codes.build,
        local_path: &project.local_path,
    };
    ProjectPairCodesDto {
        discuss: SeatPairDto {
            code: codes.discuss.clone(),
            copy_text: onboarding_prompt(AgentSeat::Discuss, &discuss_ctx),
            seated: live.discuss.seated,
            last_seen_at: live.discuss.last_seen_at.map(|t| t.to_rfc3339()),
            occupant: occupant_dto(&live.discuss),
        },
        build: SeatPairDto {
            code: codes.build.clone(),
            copy_text: onboarding_prompt(AgentSeat::Build, &build_ctx),
            seated: live.build.seated,
            last_seen_at: live.build.last_seen_at.map(|t| t.to_rfc3339()),
            occupant: occupant_dto(&live.build),
        },
    }
}

async fn get_pair_codes_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<ProjectPairCodesDto>, ApiError> {
    user.require_roles(&[Role::Admin])?;
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let project = get_project(&conn, &id)?.ok_or_else(|| ApiError::not_found(format!("project not found: {id}")))?;
    let codes = ensure_project_pair_codes(&conn, &state.home, &id)?;
    Ok(Json(pair_codes_dto(&project, &codes, &state.home, &conn)))
}

async fn rotate_pair_code_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, seat)): Path<(String, String)>,
) -> Result<Json<ProjectPairCodesDto>, ApiError> {
    user.require_roles(&[Role::Admin])?;
    let seat = AgentSeat::parse(&seat).ok_or_else(|| {
        ApiError::bad_request("seat must be `discuss` or `build`")
    })?;
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let project = get_project(&conn, &id)?.ok_or_else(|| ApiError::not_found(format!("project not found: {id}")))?;
    // Ensure the other seat exists before rotating one.
    let _ = ensure_project_pair_codes(&conn, &state.home, &id);
    rotate_pair_code(&conn, &state.home, &id, seat)?;
    let codes = crate::services::read_plaintext_codes(&state.home, &id)?;
    Ok(Json(pair_codes_dto(&project, &codes, &state.home, &conn)))
}

async fn list_requirements_handler(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<RequirementDto>>, ApiError> {
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;

    if get_project(&conn, &project_id)?.is_none() {
        return Err(ApiError::not_found(format!(
            "project not found: {project_id}"
        )));
    }

    let reqs = list_requirements_for_project(&conn, &project_id)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(reqs.into_iter().map(RequirementDto::from).collect()))
}

async fn create_requirement_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(project_id): Path<String>,
    Json(body): Json<CreateRequirementBody>,
) -> Result<(axum::http::StatusCode, Json<RequirementDto>), ApiError> {
    user.require_roles(&[Role::Admin, Role::Planner])?;
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;

    let (scope, non_scope, acceptance, deps) = body.to_json_strings();
    let req = create_requirement(
        &conn,
        CreateRequirementInput {
            project_id,
            title: body.title,
            description: body.description,
            priority: body.priority,
            scope_json: scope,
            non_scope_json: non_scope,
            acceptance_json: acceptance,
            dependencies_json: deps,
            created_by: user.name.clone(),
        },
    )?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(RequirementDto::from(req)),
    ))
}

async fn get_requirement_handler(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RequirementDetailDto>, ApiError> {
    let conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;

    let req = get_requirement(&conn, &id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("requirement not found: {id}")))?;

    let events = list_events_for_requirement(&conn, &id)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(RequirementDetailDto {
        requirement: RequirementDto::from(req),
        events: events.into_iter().map(EventDto::from).collect(),
    }))
}

async fn claim_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RequirementDto>, ApiError> {
    // HTTP gate: foreman or admin (domain also allows planner).
    user.require_roles(&[Role::Foreman, Role::Admin])?;
    let mut conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let req = claim_task(&mut conn, &id, &user.name, user.role)?;
    Ok(Json(RequirementDto::from(req)))
}

async fn progress_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<ProgressBody>,
) -> Result<Json<RequirementDto>, ApiError> {
    user.require_roles(&[Role::Foreman, Role::Admin])?;
    let mut conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let req = report_progress(
        &mut conn,
        &id,
        &user.name,
        user.role,
        body.summary.as_deref(),
        body.blocked_reason.as_deref(),
    )?;
    Ok(Json(RequirementDto::from(req)))
}

async fn submit_review_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RequirementDto>, ApiError> {
    user.require_roles(&[Role::Foreman, Role::Admin])?;
    let mut conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let req = submit_for_review(&mut conn, &id, &user.name, user.role)?;
    Ok(Json(RequirementDto::from(req)))
}

async fn complete_review_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<CompleteReviewBody>,
) -> Result<Json<RequirementDto>, ApiError> {
    // MVP HTTP: admin only (domain allows planner too).
    user.require_roles(&[Role::Admin])?;
    let mut conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let req = complete_review(
        &mut conn,
        &id,
        &user.name,
        user.role,
        body.pass,
        body.reason.as_deref(),
    )?;
    Ok(Json(RequirementDto::from(req)))
}

async fn cancel_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RequirementDto>, ApiError> {
    user.require_roles(&[Role::Planner, Role::Admin])?;
    let mut conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let req = cancel_requirement(&mut conn, &id, &user.name, user.role)?;
    Ok(Json(RequirementDto::from(req)))
}

async fn release_handler(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<RequirementDto>, ApiError> {
    user.require_roles(&[Role::Foreman, Role::Admin])?;
    let mut conn = state
        .db
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let req = release_task(&mut conn, &id, &user.name, user.role)?;
    Ok(Json(RequirementDto::from(req)))
}
