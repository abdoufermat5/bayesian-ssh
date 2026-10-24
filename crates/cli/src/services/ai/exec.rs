//! Executor for AI calls: timeouts, output caps, non-interactive auth, jailed and certified modes.
//!
//! Owner: W1. Stub from the scaffold; replace freely.

use super::types::{ExecLimits, ExecMode, ExecResult, Executor};
use crate::config::AppConfig;
use crate::models::Connection;
use async_trait::async_trait;

/// The executor every AI tool uses. Non-interactive: never prompts on a TTY.
pub struct AiExecutor {
    config: AppConfig,
}

impl AiExecutor {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

#[async_trait]
impl Executor for AiExecutor {
    async fn exec(
        &self,
        conn: &Connection,
        command: &str,
        mode: ExecMode,
        limits: ExecLimits,
    ) -> anyhow::Result<ExecResult> {
        let _ = (&self.config, conn, command, mode, limits);
        anyhow::bail!("AiExecutor::exec: not implemented yet")
    }
}
