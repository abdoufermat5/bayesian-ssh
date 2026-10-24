//! bssh AI: a safe proxy between AI agents and servers.
//!

pub mod ipc;
pub mod types;

// Execution
pub mod exec;
// Command analysis and redaction
pub mod analyzer;
pub mod effects;
pub mod redact;
// Policy, approvals, runs
pub mod approval;
pub mod control_client;
pub mod policy;
pub mod store;
// MCP server and tools
pub mod mcp;
pub mod observe;
pub mod plan;
// Host enforcement
pub mod cert;
pub mod hostsetup;
pub mod jail;
// Audit, reports, client install
pub mod audit;
pub mod install;
pub mod report;
// The look shared by every AI surface
pub mod ui;
// Teams agents and gateway
pub mod gateway;
pub mod teams;

use std::path::PathBuf;

/// `<env dir>/ai/`: runs DB, socket, audit chain, backups, local CA.
pub fn ai_dir(config: &crate::config::AppConfig) -> PathBuf {
    config
        .database_path
        .parent()
        .map(|p| p.join("ai"))
        .unwrap_or_else(|| PathBuf::from("ai"))
}
