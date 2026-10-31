//! `BsshMcp`: the rmcp `ServerHandler` for one run.

use super::instructions::{instructions, tools_for};
use super::progress::with_progress_token;
use super::render::to_result;
use crate::services::ai::mcp::context::RunContext;
use crate::services::ai::mcp::tools;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, DiscoverResult, Implementation,
    InitializeRequestParams, InitializeResult, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use std::sync::Arc;
use tracing::debug;

/// The MCP server for one run.
#[derive(Clone)]
pub struct BsshMcp {
    ctx: Arc<RunContext>,
    tools: Arc<Vec<rmcp::model::Tool>>,
}

/// Build the MCP server for a run. Transport-agnostic: serve it on stdio
/// (`ServiceExt::serve(server, rmcp::transport::stdio())`, see `serve_stdio`) or mount it on
/// streamable HTTP. For one run per HTTP session, call `RunContext::new` per session with the
/// gateway's own `ApprovalBroker` and `Executor` in `RunDeps`, then `build_server(ctx)`.
pub fn build_server(ctx: Arc<RunContext>) -> BsshMcp {
    BsshMcp {
        ctx,
        tools: Arc::new(tools::definitions()),
    }
}

impl BsshMcp {
    pub fn context(&self) -> &Arc<RunContext> {
        &self.ctx
    }
}

/// `name version`, with the client's display title when it sends one (`Claude Code 2.1.310`).
fn client_label(info: &Implementation) -> String {
    let name = info
        .title
        .as_deref()
        .filter(|t| !t.is_empty())
        .unwrap_or(&info.name);
    if info.version.is_empty() {
        name.to_string()
    } else {
        format!("{name} {}", info.version)
    }
}

impl BsshMcp {
    /// MCP 2026-07-28 has no `initialize`: the client names itself in every request's
    /// `_meta`. The first request that does starts the run, as `initialize` would.
    fn note_client(&self, context: &RequestContext<RoleServer>) {
        if self.ctx.client() != "unknown" {
            return;
        }
        let Some(info) = context.meta.client_info() else {
            return;
        };
        let client = client_label(&info);
        self.ctx.start(&client);
        if let Some(e) = &self.ctx.deps.elicitor {
            e.set_peer(context.peer.clone());
        }
        debug!("MCP client: {client}");
    }
}

impl ServerHandler for BsshMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("bssh-ai", env!("CARGO_PKG_VERSION")))
            .with_instructions(instructions(&self.ctx))
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        context.peer.set_peer_info(request.clone());
        let client = client_label(&request.client_info);
        // Self-reported by the client: shown as such everywhere.
        self.ctx.start(&client);
        if let Some(e) = &self.ctx.deps.elicitor {
            e.set_peer(context.peer.clone());
        }
        debug!("MCP client: {client}");
        self.negotiate_initialize(&request)
    }

    async fn discover(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<DiscoverResult, McpError> {
        self.note_client(&context);
        Ok(DiscoverResult::from_server_info(
            self.supported_protocol_versions().into_owned(),
            self.get_info(),
        ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        self.note_client(&context);
        // Calling a hidden change tool anyway still returns the profile's denial (it is not
        // counted by the breaker).
        let tools = tools_for(&self.ctx.policy.profile, &self.tools);
        Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        self.note_client(&context);
        let args = serde_json::Value::Object(request.arguments.unwrap_or_default());
        let token = context.meta.get_progress_token();
        let out = with_progress_token(token, tools::call(&self.ctx, &request.name, args)).await;
        Ok(to_result(&request.name, out).into())
    }
}
