//! MCP server for bssh AI (stdio; transport-agnostic so W8 can mount it on streamable HTTP).
//!
//! # The seam for other transports (W8)
//!
//! ```ignore
//! // One run per session:
//! let deps = RunDeps {
//!     hosts, files,                          // inventory and SFTP (DbHosts / SftpFiles locally)
//!     executor: Arc::new(AiExecutor::new(&config)),
//!     broker: relay_broker,                  // your ApprovalBroker (RelayBroker)
//!     analyzer, redactor, engine, audit,     // ShellAnalyzer, SecretRedactor, DefaultPolicyEngine, sink
//!     scope: Arc::new(policy::host_in_scope),
//!     elicitor: None,                        // no in-client approvals over the gateway
//! };
//! let ctx = RunContext::new(config, policy_name, policy, deps);
//! let server = build_server(ctx.clone());    // BsshMcp: an rmcp ServerHandler
//! ```
//!
//! With rmcp's streamable HTTP service, pass a factory
//! `move || Ok(build_server(ctx_for_this_session))`. The context emits `RunStarted` when the
//! client initialises, and the caller ends the run with `ctx.end("...")` (or `ctx.stop(..)`
//! for the kill switch; `ctx.stopped().await` resolves then). Add the rmcp feature
//! `transport-streamable-http-server` to Cargo.toml for HTTP.
//!
//! Owner: W4.

pub mod context;
pub mod files;
pub mod server;
pub mod tools;
pub mod util;

#[cfg(test)]
pub(crate) mod testkit;
#[cfg(test)]
mod tests;

pub use context::{DbHosts, HostSource, RunContext, RunDeps, ScopeFn, StaticHosts, ToolError};
pub use files::{FileAccess, FileRead, SftpFiles};
pub use server::{build_server, BsshMcp, McpElicitor};

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
