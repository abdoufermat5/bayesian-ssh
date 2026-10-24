//! `bssh ai serve`: the gateway for unattended Teams agents. Owner: W8.

use crate::cli::parser_ai::AiArgsServe;
use crate::config::AppConfig;
use crate::services::ai::gateway::{self, ServeOptions, UnwiredMcp};
use anyhow::Result;

pub async fn execute(args: AiArgsServe, config: AppConfig) -> Result<()> {
    // The MCP core is mounted through `gateway::McpBackend`; the integrator
    // swaps `UnwiredMcp` for the W4-backed implementation.
    gateway::serve(
        config,
        ServeOptions {
            listen: args.listen,
            agent: args.agent,
            allow_remote: args.allow_remote,
        },
        &UnwiredMcp,
    )
    .await
}
