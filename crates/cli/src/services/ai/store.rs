//! Runs, approvals and grants in SQLite (<ai_dir>/runs.db).
//!
//! Owner: W3. Stub from the scaffold; replace freely.

use crate::config::AppConfig;

/// SQLite store for runs, approval requests, grants and budget counters.
pub struct AiStore {
    _private: (),
}

impl AiStore {
    pub fn open(config: &AppConfig) -> anyhow::Result<Self> {
        let _ = config;
        anyhow::bail!("AiStore::open: not implemented yet")
    }
}
