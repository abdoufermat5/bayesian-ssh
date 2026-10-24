//! Local user CA and approval-bound short-lived certificates (force-command).
//!
//! Owner: W5. Stub from the scaffold; replace freely.

use crate::config::AppConfig;
use std::time::Duration;
use zeroize::Zeroizing;

pub const AGENT_PRINCIPAL: &str = "bssh-agent";

/// An ephemeral key plus a certificate signed by the local CA.
pub struct IssuedCert {
    pub private_key_openssh: Zeroizing<String>,
    pub certificate_openssh: String,
    pub key_id: String,
}

/// Is a local CA initialised (`bssh ai ca init`)?
pub fn ca_available(config: &AppConfig) -> bool {
    let _ = config;
    false
}

/// Issue a certificate for `bssh-agent` with `force-command`, no PTY/forwarding.
pub fn issue(
    config: &AppConfig,
    force_command: &str,
    ttl: Duration,
    source_address: Option<&str>,
) -> anyhow::Result<IssuedCert> {
    let _ = (config, force_command, ttl, source_address);
    anyhow::bail!("cert::issue: not implemented yet")
}
