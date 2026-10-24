//! `bssh ai serve`: the gateway for unattended Teams agents.

use crate::cli::parser_ai::AiArgsServe;
use crate::config::AppConfig;
use crate::services::ai::gateway::{self, ServeOptions};
use crate::services::ai::mcp::RmcpBackend;
use anyhow::Result;

pub async fn execute(args: AiArgsServe, config: AppConfig) -> Result<()> {
    // The gateway authenticates and syncs the signed policy; the MCP server core is
    // mounted through `gateway::McpBackend`.
    gateway::serve(
        config,
        ServeOptions {
            listen: args.listen,
            agent: args.agent,
            allow_remote: args.allow_remote,
        },
        &RmcpBackend,
    )
    .await
}
