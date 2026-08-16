//! Bearer token auth → role from `api_tokens`.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::domain::state::Role;
use crate::http::error::ApiError;
use crate::http::AppState;
use crate::services::tokens::lookup_token;

/// Authenticated actor extracted from `Authorization: Bearer <token>`.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub role: Role,
    /// Token name (used as actor id for claims / events).
    pub name: String,
}

impl AuthUser {
    pub fn require_roles(&self, allowed: &[Role]) -> Result<(), ApiError> {
        if allowed.contains(&self.role) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!(
                "role `{}` is not allowed for this endpoint",
                self.role
            )))
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::unauthorized("missing Authorization header"))?;

        let token = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::unauthorized("expected Bearer token"))?;

        let conn = state
            .db
            .lock()
            .map_err(|_| ApiError::internal("database lock poisoned"))?;

        let row = lookup_token(&conn, token)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .ok_or_else(|| ApiError::unauthorized("invalid token"))?;

        Ok(AuthUser {
            role: row.role,
            name: row.name,
        })
    }
}
