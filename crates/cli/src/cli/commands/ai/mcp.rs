//! `bssh ai mcp`.
//!
//! stdout is the MCP channel: nothing here (or below) may print to it. Logging goes to stderr
//! (set up in main.rs), and errors are reported on stderr by the CLI wrapper.

use crate::config::AppConfig;
use crate::services::ai::{mcp, policy};
use anyhow::Result;

pub async fn execute(args: crate::cli::parser_ai::AiArgsMcp, config: AppConfig) -> Result<()> {
    // A policy typo would otherwise be dropped silently by serde; say so once, on stderr.
    for warning in policy::validation_warnings(&config, &args.policy) {
        eprintln!("[warn] {warning}");
    }
    mcp::serve_stdio(config, args.policy).await
}
