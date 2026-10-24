//! `bssh ai mcp`. Owner: W4.
//!
//! stdout is the MCP channel: nothing here (or below) may print to it. Logging goes to stderr
//! (set up in main.rs), and errors are reported on stderr by the CLI wrapper.

use crate::config::AppConfig;
use crate::services::ai::mcp;
use anyhow::Result;

pub async fn execute(args: crate::cli::parser_ai::AiArgsMcp, config: AppConfig) -> Result<()> {
    mcp::serve_stdio(config, args.policy).await
}
