//! Run reports, bssh ai log / replay.
//!
//! Owner: W6. Stub from the scaffold; replace freely.

use super::types::ExecResult;
use crate::config::AppConfig;

/// Store the output of one call as an asciicast recording when `ai.record_output` is on.
pub fn record_output(
    config: &AppConfig,
    run_id: &str,
    call_id: &str,
    command: &str,
    result: &ExecResult,
) -> anyhow::Result<()> {
    let _ = (config, run_id, call_id, command, result);
    Ok(())
}
