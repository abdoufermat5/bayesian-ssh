//! bssh AI: a safe proxy between AI agents and servers.
//! Design: bssh-cloud/docs/design/ai-agent-access.md.
//!
//! Module owners (workstreams) are listed in `docs/ai/WORKSTREAMS.md`.

pub mod ipc;
pub mod types;

// W1 groundwork
pub mod exec;
// W2 analyzer and redaction
pub mod analyzer;
pub mod redact;
// W3 policy, approvals, runs
pub mod approval;
pub mod policy;
pub mod store;
// W4 MCP server and tools
pub mod mcp;
pub mod observe;
pub mod plan;
// W5 host enforcement
pub mod cert;
pub mod hostsetup;
pub mod jail;
// W6 audit, reports, install
pub mod audit;
pub mod install;
pub mod report;
// W8 Teams agents and gateway
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
