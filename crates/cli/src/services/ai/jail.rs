//! Read-only jail: Landlock (setpriv / bssh-jail helper) or bubblewrap wrappers.
//!
//! Owner: W5. Stub from the scaffold; replace freely.

use super::types::{Executor, JailKind};
use crate::models::Connection;

/// Wrap `command` so it runs read-only, without network, reads limited to `read_paths`.
/// Errors for `JailKind::None`.
pub fn wrap(kind: JailKind, read_paths: &[String], command: &str) -> anyhow::Result<String> {
    let _ = (kind, read_paths, command);
    anyhow::bail!("jail::wrap: not implemented yet")
}

/// Find the best jail the host supports (runs a few probe commands via `exec`, Plain mode).
pub async fn probe(conn: &Connection, exec: &dyn Executor) -> anyhow::Result<JailKind> {
    let _ = (conn, exec);
    Ok(JailKind::None)
}
