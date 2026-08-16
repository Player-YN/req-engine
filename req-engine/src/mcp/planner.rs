//! Discuss (planner) MCP surface: list/get/create/update/cancel todo in one project.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::service::ServiceExt;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;
use rmcp::transport::stdio;
use rmcp::ServerHandler;

use crate::domain::models::CreateRequirementInput;
use crate::domain::state::Status;
use crate::mcp::dto::{
    CreateRequirementArgs, ListRequirementsArgs, RequirementIdArgs, UpdateRequirementArgs,
    value_to_json_array_string,
};
use crate::mcp::util::{
    assert_req_in_scope, effective_project_id, json_ok, lock_db, req_view, reqs_view, tool_err,
    ToolResult,
};
use crate::mcp::McpState;
use crate::services::{
    cancel_requirement, create_requirement, get_requirement,
    list_requirements_for_project_filtered, update_requirement, UpdateRequirementInput,
};

#[derive(Clone)]
pub struct PlannerServer {
    state: McpState,
}

#[tool_router]
impl PlannerServer {
    pub fn new(state: McpState) -> Self {
        Self { state }
    }

    #[tool(
        name = "create_requirement",
        description = "Create a requirement (always starts as todo) in the bound project."
    )]
    async fn create_requirement_tool(
        &self,
        Parameters(args): Parameters<CreateRequirementArgs>,
    ) -> ToolResult {
        let project_id = match effective_project_id(&self.state, args.project_id.as_deref()) {
            Ok(id) => id,
            Err(e) => return tool_err(e),
        };
        let conn = lock_db(&self.state)?;
        let input = CreateRequirementInput {
            project_id,
            title: args.title,
            description: args.description.unwrap_or_default(),
            priority: args.priority.unwrap_or_else(|| "medium".into()),
            scope_json: value_to_json_array_string(args.scope),
            non_scope_json: "[]".into(),
            acceptance_json: value_to_json_array_string(args.acceptance_criteria),
            dependencies_json: value_to_json_array_string(args.dependencies),
            created_by: self.state.auth.name.clone(),
        };
        match create_requirement(&conn, input) {
            Ok(r) => json_ok(&req_view(r)),
            Err(e) => tool_err(e.to_string()),
        }
    }

    #[tool(
        name = "list_requirements",
        description = "List requirements in the bound project; optional status filter"
    )]
    async fn list_requirements(
        &self,
        Parameters(args): Parameters<ListRequirementsArgs>,
    ) -> ToolResult {
        let project_id = match effective_project_id(&self.state, args.project_id.as_deref()) {
            Ok(id) => id,
            Err(e) => return tool_err(e),
        };
        let status = match args.status.as_deref() {
            None => None,
            Some(s) => match Status::parse(s) {
                Some(st) => Some(st),
                None => {
                    return tool_err(format!(
                        "invalid status '{s}' (use todo|in_progress|review|done|cancelled)"
                    ));
                }
            },
        };
        let conn = lock_db(&self.state)?;
        match list_requirements_for_project_filtered(&conn, &project_id, status) {
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
        name = "update_requirement",
        description = "Update requirement fields (only allowed while status is todo)"
    )]
    async fn update_requirement_tool(
        &self,
        Parameters(args): Parameters<UpdateRequirementArgs>,
    ) -> ToolResult {
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
        let conn = lock_db(&self.state)?;
        match get_requirement(&conn, &args.id) {
            Ok(Some(r)) => {
                if let Err(e) = assert_req_in_scope(&self.state, &r) {
                    return tool_err(e);
                }
            }
            Ok(None) => return tool_err(format!("requirement not found: {}", args.id)),
            Err(e) => return tool_err(e.to_string()),
        }
        match update_requirement(&conn, &args.id, &self.state.auth.name, input) {
            Ok(r) => json_ok(&req_view(r)),
            Err(e) => tool_err(e.to_string()),
        }
    }

    #[tool(
        name = "cancel_requirement",
        description = "Soft-cancel a requirement. Planner: todo only. Admin: any non-terminal."
    )]
    async fn cancel_requirement_tool(
        &self,
        Parameters(args): Parameters<RequirementIdArgs>,
    ) -> ToolResult {
        let mut conn = lock_db(&self.state)?;
        match get_requirement(&conn, &args.id) {
            Ok(Some(r)) => {
                if let Err(e) = assert_req_in_scope(&self.state, &r) {
                    return tool_err(e);
                }
            }
            Ok(None) => return tool_err(format!("requirement not found: {}", args.id)),
            Err(e) => return tool_err(e.to_string()),
        }
        match cancel_requirement(
            &mut conn,
            &args.id,
            &self.state.auth.name,
            self.state.auth.role,
        ) {
            Ok(r) => json_ok(&req_view(r)),
            Err(e) => tool_err(e.to_string()),
        }
    }
}

#[tool_handler]
impl ServerHandler for PlannerServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::new(
            "req-engine-planner",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Requirements Engine discuss (planner) MCP. Bound to one project when started \
             with --pair. Tools: list_requirements, get_requirement, create_requirement, \
             update_requirement (todo only), cancel_requirement (todo only). \
             Do not implement code. No claim, no complete_review, no hard delete.",
        )
    }
}

pub async fn serve(state: McpState) -> Result<(), Box<dyn std::error::Error>> {
    let server = PlannerServer::new(state.clone());
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
