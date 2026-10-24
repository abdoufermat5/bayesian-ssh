//! Local signed hash-chained AI audit log (AuditSink) and Teams forwarding hook.
//!
//! Owner: W6. Stub from the scaffold; replace freely.

use super::types::{AiEvent, AuditSink};
use crate::config::AppConfig;

/// Append-only, hash-chained, Ed25519-signed JSONL log in `<ai_dir>/audit/`.
pub struct LocalAudit {
    _private: (),
}

impl LocalAudit {
    pub fn open(config: &AppConfig) -> anyhow::Result<Self> {
        let _ = config;
        anyhow::bail!("LocalAudit::open: not implemented yet")
    }
}

impl AuditSink for LocalAudit {
    fn record(&self, event: &AiEvent) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }
}
