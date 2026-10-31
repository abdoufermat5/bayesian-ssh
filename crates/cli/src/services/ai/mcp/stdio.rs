//! The stdio transport behind `bssh ai mcp`.

use super::context::RunContext;
use super::server::build_server;
use crate::config::AppConfig;
use anyhow::{anyhow, Result};
use rmcp::ServiceExt;
use tracing::info;

/// Run the MCP server on stdio for policy `policy` until the client disconnects.
///
/// stdout carries the protocol: everything else (logs, errors) must go to stderr.
pub async fn serve_stdio(config: AppConfig, policy: String) -> Result<()> {
    let ctx = RunContext::from_config(config, &policy).await?;
    let server = build_server(ctx.clone());
    let running = server
        .serve(rmcp::transport::stdio_pipes())
        .await
        .map_err(|e| anyhow!("MCP initialization failed: {e}"))?;
    info!("bssh ai mcp: run {} (policy {policy})", ctx.run_id);

    // Kill switch, circuit breaker or TTL: close the transport.
    let token = running.cancellation_token();
    let watcher = ctx.clone();
    tokio::spawn(async move {
        watcher.stopped().await;
        token.cancel();
    });

    let quit = running.waiting().await;
    ctx.end("client disconnected");
    if ctx.is_stopped() {
        // A blocked stdin read must not keep a stopped run alive.
        std::process::exit(0);
    }
    quit.map_err(|e| anyhow!("MCP session ended with an error: {e}"))?;
    Ok(())
}
