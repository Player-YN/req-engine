//! Shared helpers for MCP tool handlers.

use std::sync::{MutexGuard, PoisonError};

use rusqlite::Connection;
use serde::Serialize;

use crate::domain::models::Requirement;
use crate::mcp::dto::RequirementView;
use crate::mcp::McpState;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::ErrorData as RmcpError;

pub type ToolResult = Result<CallToolResult, RmcpError>;

pub fn lock_db(state: &McpState) -> Result<MutexGuard<'_, Connection>, RmcpError> {
    state
        .db
        .lock()
        .map_err(|e: PoisonError<_>| {
            RmcpError::internal_error(format!("db lock poisoned: {e}"), None)
        })
}

pub fn json_ok<T: Serialize>(value: &T) -> ToolResult {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| RmcpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

pub fn tool_err(msg: impl Into<String>) -> ToolResult {
    Ok(CallToolResult::error(vec![ContentBlock::text(msg.into())]))
}

pub fn req_view(r: Requirement) -> RequirementView {
    RequirementView::from(r)
}

pub fn reqs_view(rs: Vec<Requirement>) -> Vec<RequirementView> {
    rs.into_iter().map(RequirementView::from).collect()
}

pub fn bound_project_id(state: &McpState) -> Option<&str> {
    state.binding.as_ref().map(|b| b.project_id.as_str())
}

pub fn effective_project_id(
    state: &McpState,
    requested: Option<&str>,
) -> Result<String, String> {
    if let Some(id) = bound_project_id(state) {
        return Ok(id.to_string());
    }
    requested
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| "project_id required".into())
}

pub fn assert_req_in_scope(state: &McpState, r: &Requirement) -> Result<(), String> {
    if let Some(id) = bound_project_id(state) {
        if r.project_id != id {
            return Err(format!(
                "requirement {} is not in bound project {id}",
                r.id
            ));
        }
    }
    Ok(())
}
