//! HTTP error mapping.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::domain::state::TransitionError;
use crate::services::pair_codes::PairError;
use crate::services::projects::ProjectError;
use crate::services::requirements::{CreateRequirementError, VerbError};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", msg)
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", msg)
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", msg)
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", msg)
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", msg)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "code": self.code,
                "message": self.message,
            }
        });
        (self.status, Json(body)).into_response()
    }
}

impl From<VerbError> for ApiError {
    fn from(e: VerbError) -> Self {
        match e {
            VerbError::NotFound(id) => ApiError::not_found(format!("requirement not found: {id}")),
            VerbError::ConcurrentClaimLost => {
                ApiError::conflict("concurrent claim lost (already claimed or status changed)")
            }
            VerbError::ProjectArchived(id) => ApiError::new(
                StatusCode::CONFLICT,
                "project_archived",
                format!("project is archived: {id}"),
            ),
            VerbError::DependenciesNotMet => ApiError::new(
                StatusCode::CONFLICT,
                "dependencies_not_met",
                "cannot claim: one or more dependencies are not done",
            ),
            VerbError::RejectReasonRequired => ApiError::new(
                StatusCode::BAD_REQUEST,
                "reject_reason_required",
                "rejecting a review requires a non-empty reason",
            ),
            VerbError::Transition(t) => transition_to_api(t),
            VerbError::Sqlite(e) => ApiError::internal(e.to_string()),
        }
    }
}

impl From<CreateRequirementError> for ApiError {
    fn from(e: CreateRequirementError) -> Self {
        match e {
            CreateRequirementError::ProjectNotFound(id) => {
                ApiError::not_found(format!("project not found: {id}"))
            }
            CreateRequirementError::EmptyTitle => ApiError::bad_request("title must not be empty"),
            CreateRequirementError::ProjectArchived(id) => ApiError::new(
                StatusCode::CONFLICT,
                "project_archived",
                format!("project is archived: {id}"),
            ),
            CreateRequirementError::Sqlite(e) => ApiError::internal(e.to_string()),
        }
    }
}

impl From<ProjectError> for ApiError {
    fn from(e: ProjectError) -> Self {
        match e {
            ProjectError::NotFound(id) => ApiError::not_found(format!("project not found: {id}")),
            ProjectError::EmptyName => ApiError::bad_request("name must not be empty"),
            ProjectError::AlreadyExists(id) => {
                ApiError::conflict(format!("project already exists: {id}"))
            }
            ProjectError::InvalidLocalPath(msg) => {
                ApiError::bad_request(format!("invalid local_path: {msg}"))
            }
            ProjectError::Archived(id) => ApiError::new(
                StatusCode::CONFLICT,
                "project_archived",
                format!("project is archived: {id}"),
            ),
            ProjectError::InvalidId(msg) => ApiError::bad_request(format!("invalid project id: {msg}")),
            ProjectError::Sqlite(e) => ApiError::internal(e.to_string()),
        }
    }
}

impl From<PairError> for ApiError {
    fn from(e: PairError) -> Self {
        match e {
            PairError::NotFound(id) => ApiError::not_found(format!("project not found: {id}")),
            PairError::InvalidCode => ApiError::unauthorized("invalid pair code"),
            PairError::PlaintextLost(id) => ApiError::conflict(format!(
                "plaintext pair codes missing for project {id}; rotate to issue new ones"
            )),
            PairError::File(msg) => ApiError::internal(msg),
            PairError::Io(err) => ApiError::internal(err.to_string()),
            PairError::Sqlite(err) => ApiError::internal(err.to_string()),
        }
    }
}

fn transition_to_api(e: TransitionError) -> ApiError {
    match e {
        TransitionError::ForbiddenRole { role, verb } => {
            ApiError::forbidden(format!("role `{role}` cannot perform `{verb}`"))
        }
        TransitionError::NotClaimant { actor, claimed_by } => ApiError::forbidden(format!(
            "actor `{actor}` is not the claimant (claimed_by={claimed_by:?})"
        )),
        TransitionError::AlreadyClaimed { claimed_by } => {
            ApiError::conflict(format!("already claimed by `{claimed_by}`"))
        }
        TransitionError::NotClaimable { status } => {
            ApiError::conflict(format!("not claimable in status `{status}`"))
        }
        TransitionError::IllegalFromStatus { verb, from } => {
            ApiError::conflict(format!("`{verb}` not allowed from status `{from}`"))
        }
        TransitionError::Terminal { status } => {
            ApiError::conflict(format!("terminal status `{status}` rejects further transitions"))
        }
    }
}
