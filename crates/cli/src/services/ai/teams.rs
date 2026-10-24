//! Teams side: agent identities, signed policies, relayed approvals.
//!
//! Owner: W8. Stub from the scaffold; replace freely.

use super::types::AiEvent;
use crate::config::AppConfig;

/// Forward an event already appended to the local audit chain to the team's audit log
/// (encrypted to the team audit key). No-op outside Teams. Called by `audit::LocalAudit` (W6).
pub fn forward_event(config: &AppConfig, event: &AiEvent) -> anyhow::Result<()> {
    let _ = (config, event);
    Ok(())
}
