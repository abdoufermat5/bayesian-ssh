//! The workflows behind `bssh teams`: identity setup, teams, invites, device
//! approval, sync and key rotation, built on the client, codec, store and sync
//! engine.
//!
//! Trust rule enforced here: the server never decides which devices receive a
//! team key. An admin grants a key only to a device whose fingerprint they
//! were given out of band (`grant`), and a rotation wraps the new key only to
//! devices this machine approved. A server that lists an extra device makes
//! the rotation fail instead of leaking the key.

mod agents;
mod audit;
mod billing;
mod handle;
mod identity;
mod local;
mod removal;
mod teams;
mod types;
mod vault;

#[cfg(test)]
mod tests;

pub use audit::{AuditLine, AuditQuery, TeamAudit, DEFAULT_AUDIT_LIMIT};
pub use billing::explain;
pub use handle::Session;
pub use identity::{
    init, init_interactive, init_interactive_with, load_identity, request_email_code, unlock,
    EmailCodePrompt, Identity, InitParams,
};
pub use local::{local_team_names, local_teams, record_session_start, resolve_team};
pub use removal::{plan_removal, PreviewDevice, RemovePreview, TeamAttentionState};
pub use types::{AgentCreated, Edit, JoinWith, PendingDevice, TeamSync};

pub(crate) use local::{kind_from_str, kind_str, materialize_into, team_of_connection, SCHEMA_V9};
