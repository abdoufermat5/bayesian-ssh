//! MCP server for bssh AI: stdio for `bssh ai mcp`, streamable HTTP for the `bssh ai serve` gateway.
//!
//! # Mounting the server on another transport
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

pub mod context;
pub mod files;
pub mod http;
pub mod server;
mod stdio;
pub mod tools;
pub mod util;

#[cfg(test)]
pub(crate) mod testkit;
#[cfg(test)]
mod tests;

pub use context::{DbHosts, HostSource, RunContext, RunDeps, ScopeFn, StaticHosts, ToolError};
pub use files::{FileAccess, FileRead, SftpFiles};
pub use http::RmcpBackend;
pub use server::{build_server, BsshMcp, McpElicitor};
pub use stdio::serve_stdio;
