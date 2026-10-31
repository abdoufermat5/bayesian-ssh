//! Plain data exchanged by the session workflows.

use crate::services::inventory::sync as inventory;
use crate::services::teams::sync::SyncReport;
use bssh_teams_proto::api::{ConnectionRecord, DeviceInfo, SignedAgentPolicy};
use uuid::Uuid;

/// A change to a vault, applied inside a sync.
#[derive(Debug, Clone)]
pub enum Edit {
    /// The team's display name; written once, when the team is created.
    Profile(String),
    Connection {
        entry_id: Option<Uuid>,
        record: ConnectionRecord,
    },
    Delete(Uuid),
    /// An admin-signed agent policy (bssh AI); `entry_id` re-signs an existing one.
    AgentPolicy {
        entry_id: Option<Uuid>,
        policy: SignedAgentPolicy,
    },
}

/// What one `sync_team` did.
#[derive(Debug, Default)]
pub struct TeamSync {
    pub team_id: Uuid,
    pub name: Option<String>,
    /// No team key has been granted to this device yet.
    pub waiting_for_key: bool,
    pub report: SyncReport,
    pub hosts: Option<inventory::SyncReport>,
    pub rotated_to: Option<u32>,
    pub unreadable: usize,
    /// Entry ids of the edits passed in, in order.
    pub entry_ids: Vec<Uuid>,
}

/// How to join: with a fresh invite token, or a team already accepted on the
/// server whose admin fingerprint didn't match the first time.
#[derive(Debug, Clone, Copy)]
pub enum JoinWith<'a> {
    Token(&'a str),
    Team(Uuid),
}

/// A device waiting for a team key, for `bssh teams grant` to show.
#[derive(Debug, Clone)]
pub struct PendingDevice {
    pub email: String,
    pub device: DeviceInfo,
    pub fingerprint: String,
}

/// What `bssh teams agent add` produced.
#[derive(Debug)]
pub struct AgentCreated {
    pub info: bssh_teams_proto::api::AgentInfo,
    pub credential_path: std::path::PathBuf,
    pub fingerprint: String,
    pub policy: String,
}
