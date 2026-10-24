//! Local control socket: `bssh ai mcp` (server) <-> desktop app, `bssh ai approve`,
//! `bssh ai stop` (clients). Unix socket `<ai_dir>/control.sock`, mode 0600,
//! newline-delimited JSON, one `ClientMsg` or `ServerMsg` per line.
//!
//! Several `bssh ai mcp` processes may run at once (one per agent): each binds
//! `<ai_dir>/runs/<run_id>.sock`, and `control.sock` is not used. Clients list
//! `<ai_dir>/runs/*.sock` and connect to each.

use super::types::{AiEvent, ApprovalOutcome, ApprovalRequest, Grant};
use serde::{Deserialize, Serialize};

/// Bumped on any breaking change to the messages below.
pub const IPC_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello {
        version: u32,
        client: String,
    },
    /// Start receiving `ServerMsg::Event` for every new event.
    Subscribe,
    ListPending,
    /// Active grants of this run (answered with `ServerMsg::Grants`).
    ListGrants,
    Decide {
        request_id: String,
        outcome: ApprovalOutcome,
    },
    RevokeGrant {
        grant_id: String,
    },
    Stop {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Hello {
        version: u32,
        run_id: String,
        client: String,
        policy: String,
        started_at: String,
        /// Absent from older servers.
        #[serde(default)]
        on_behalf_of: String,
        #[serde(default)]
        hosts: Vec<String>,
    },
    Pending {
        requests: Vec<ApprovalRequest>,
    },
    /// A new approval is waiting (also delivered without Subscribe).
    NewRequest {
        request: ApprovalRequest,
    },
    Event {
        event: AiEvent,
    },
    Grants {
        grants: Vec<Grant>,
    },
    Ok,
    Error {
        message: String,
    },
}
