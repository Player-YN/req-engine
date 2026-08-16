//! Foreman MCP surface: claim / progress / review / release. No complete_review.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::service::ServiceExt;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;
use rmcp::transport::stdio;
use rmcp::ServerHandler;

use crate::mcp::dto::{
    ListReadyTasksArgs, ReleaseTaskArgs, ReportProgressArgs, RequirementIdArgs,
};
use crate::mcp::util::{
    assert_req_in_scope, bound_project_id, json_ok, lock_db, req_view, reqs_view, tool_err,
    ToolResult,
};
use crate::mcp::McpState;
use crate::services::{
    claim_task, get_requirement, list_ready_tasks, release_task, report_progress, submit_for_review,
};
use rusqlite::Connection;

fn require_in_scope(state: &McpState, conn: &Connection, id: &str) -> Result<(), String> {
    match get_requirement(conn, id) {
        Ok(Some(r)) => assert_req_in_scope(state, &r),
        Ok(None) => Err(format!("requirement not found: {id}")),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Clone)]
pub struct ForemanServer {
    state: McpState,
}

#[tool_router]
impl ForemanServer {
    pub fn new(state: McpState) -> Self {
        Self { state }
    }

    #[tool(
        name = "list_ready_tasks",
        description = "List todo requirements whose dependencies are all done (bound project)"
    )]
    async fn list_ready_tasks_tool(
        &self,
        Parameters(args): Parameters<ListReadyTasksArgs>,
    ) -> ToolResult {
        let conn = lock_db(&self.state)?;
        let filter = bound_project_id(&self.state)
            .map(|s| s.to_string())
            .or(args.project_id);
        match list_ready_tasks(&conn, filter.as_deref()) {
            Ok(rs) => json_ok(&reqs_view(rs)),
            Err(e) => tool_err(e.to_string()),
        }
    }

    #[tool(
        name = "get_requirement",
        description = "Get a single requirement by id"
    )]
    async fn get_requirement_tool(
        &self,
        Parameters(args): Parameters<RequirementIdArgs>,
    ) -> ToolResult {
        let conn = lock_db(&self.state)?;
        match get_requirement(&conn, &args.id) {
            Ok(Some(r)) => match assert_req_in_scope(&self.state, &r) {
                Ok(()) => json_ok(&req_view(r)),
                Err(e) => tool_err(e),
            },
            Ok(None) => tool_err(format!("requirement not found: {}", args.id)),
            Err(e) => tool_err(e.to_string()),
        }
    }

    #[tool(
        name = "claim_task",
        description = "Claim a todo requirement (todo → in_progress). Atomic; one winner."
    )]
    async fn claim_task_tool(
        &self,
        Parameters(args): Parameters<RequirementIdArgs>,
    ) -> ToolResult {
        let mut conn = lock_db(&self.state)?;
        if let Err(e) = require_in_scope(&self.state, &conn, &args.id) {
            return tool_err(e);
        }
        match claim_task(
            &mut conn,
            &args.id,
            &self.state.auth.name,
            self.state.auth.role,
        ) {
            Ok(r) => json_ok(&req_view(r)),
            Err(e) => tool_err(e.to_string()),
        }
    }

    #[tool(
        name = "report_progress",
        description = "Report progress on a claimed (in_progress/review) task; status unchanged"
    )]
    async fn report_progress_tool(
        &self,
        Parameters(args): Parameters<ReportProgressArgs>,
    ) -> ToolResult {
        let mut conn = lock_db(&self.state)?;
        if let Err(e) = require_in_scope(&self.state, &conn, &args.id) {
            return tool_err(e);
        }
        match report_progress(
            &mut conn,
            &args.id,
            &self.state.auth.name,
            self.state.auth.role,
            Some(&args.summary),
            args.blocked_reason.as_deref(),
        ) {
            Ok(r) => json_ok(&req_view(r)),
            Err(e) => tool_err(e.to_string()),
        }
    }

    #[tool(
        name = "submit_for_review",
        description = "Submit claimed task for review (in_progress → review)"
    )]
    async fn submit_for_review_tool(
        &self,
        Parameters(args): Parameters<RequirementIdArgs>,
    ) -> ToolResult {
        let mut conn = lock_db(&self.state)?;
        if let Err(e) = require_in_scope(&self.state, &conn, &args.id) {
            return tool_err(e);
        }
        match submit_for_review(
            &mut conn,
            &args.id,
            &self.state.auth.name,
            self.state.auth.role,
        ) {
            Ok(r) => json_ok(&req_view(r)),
            Err(e) => tool_err(e.to_string()),
        }
    }

    #[tool(
        name = "release_task",
        description = "Release claimed task back to todo (in_progress → todo). Optional reason."
    )]
    async fn release_task_tool(
        &self,
        Parameters(args): Parameters<ReleaseTaskArgs>,
    ) -> ToolResult {
        let mut conn = lock_db(&self.state)?;
        if let Err(e) = require_in_scope(&self.state, &conn, &args.id) {
            return tool_err(e);
        }
        match release_task(
            &mut conn,
            &args.id,
            &self.state.auth.name,
            self.state.auth.role,
        ) {
            Ok(r) => {
                let mut view = serde_json::to_value(req_view(r))
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
                if let Some(reason) = args.reason {
                    if let Some(obj) = view.as_object_mut() {
                        obj.insert("release_reason".into(), serde_json::Value::String(reason));
                    }
                }
                json_ok(&view)
            }
            Err(e) => tool_err(e.to_string()),
        }
    }
}

#[tool_handler]
impl ServerHandler for ForemanServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::new(
            "req-engine-foreman",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Requirements Engine implement (foreman) MCP. Bound to one project when started \
             with --pair. Tools: list_ready_tasks, get_requirement, claim_task, \
             report_progress, submit_for_review, release_task. \
             No complete_review (admin/HTTP only), no create_requirement, no hard delete.",
        )
    }
}

pub async fn serve(state: McpState) -> Result<(), Box<dyn std::error::Error>> {
    let server = ForemanServer::new(state.clone());
    let service = server.serve(stdio()).await?;
    if let Some(peer) = service.peer_info() {
        crate::mcp::record_peer_client(
            &state,
            &peer.client_info.name,
            peer.client_info.title.as_deref(),
            Some(peer.client_info.version.as_str()),
        );
    }
    service.waiting().await?;
    Ok(())
}
