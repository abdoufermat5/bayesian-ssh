//! Audit log types.

use super::teams::MemberKind;
use crate::wire::{Bytes, Hash32, Nonce12, Signature64};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    SessionStart,
    SessionEnd,
    VaultRead,
    VaultWrite,
    /// bssh AI: a tool call and its policy decision. Detail is encrypted.
    AiCall,
    /// bssh AI: an approval request and its outcome.
    AiApproval,
    /// bssh AI: a change plan applied or undone.
    AiChange,
    /// bssh AI: run started or ended, circuit breaker tripped.
    AiRun,
}

impl AuditKind {
    /// Byte used in [`crate::layout::audit_signing_bytes`]. Never reuse a tag.
    pub fn tag(self) -> u8 {
        match self {
            AuditKind::SessionStart => 1,
            AuditKind::SessionEnd => 2,
            AuditKind::VaultRead => 3,
            AuditKind::VaultWrite => 4,
            AuditKind::AiCall => 5,
            AuditKind::AiApproval => 6,
            AuditKind::AiChange => 7,
            AuditKind::AiRun => 8,
        }
    }

    /// Kinds 1 to 4 predate bssh AI and are signed with the v1 layout (when
    /// they carry no actor or detail).
    pub fn is_ai(self) -> bool {
        self.tag() >= 5
    }

    /// Wire name, as used in `kind` query filters and server storage.
    pub fn as_str(self) -> &'static str {
        match self {
            AuditKind::SessionStart => "session_start",
            AuditKind::SessionEnd => "session_end",
            AuditKind::VaultRead => "vault_read",
            AuditKind::VaultWrite => "vault_write",
            AuditKind::AiCall => "ai_call",
            AuditKind::AiApproval => "ai_approval",
            AuditKind::AiChange => "ai_change",
            AuditKind::AiRun => "ai_run",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "session_start" => AuditKind::SessionStart,
            "session_end" => AuditKind::SessionEnd,
            "vault_read" => AuditKind::VaultRead,
            "vault_write" => AuditKind::VaultWrite,
            "ai_call" => AuditKind::AiCall,
            "ai_approval" => AuditKind::AiApproval,
            "ai_change" => AuditKind::AiChange,
            "ai_run" => AuditKind::AiRun,
            _ => return None,
        })
    }
}

/// Who acted. Signed with the event (v2 layout), visible to the server so
/// "which agent did something this week" can be queried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditActor {
    pub kind: MemberKind,
    /// Agent name, or the member's email for a human.
    pub name: String,
    /// The human an interactive agent acts for ("Claude Code on behalf of alice").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
}

/// Encrypted detail of an AI event (commands, hosts, outcome). AES-256-GCM
/// under the team's audit key, see `PROTOCOL.md`. Opaque to the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditDetail {
    pub key_version: u32,
    pub nonce: Nonce12,
    pub ciphertext: Bytes,
}

/// A client-signed audit event. Events per device form a hash chain
/// (`seq`, `prev_hash`), see [`crate::layout::audit_event_hash`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Client-generated, makes uploads idempotent.
    pub event_id: Uuid,
    pub team_id: Uuid,
    pub device_id: Uuid,
    /// Starts at 0 per device, no gaps.
    pub seq: u64,
    pub kind: AuditKind,
    /// The vault entry involved, if any (opaque to the server).
    pub entry_id: Option<Uuid>,
    /// Millisecond precision is all that's signed.
    pub occurred_at: DateTime<Utc>,
    pub prev_hash: Hash32,
    pub signature: Signature64,
    /// Who acted; set on AI events. Absent on kinds 1 to 4 from older clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<AuditActor>,
    /// Encrypted detail; set on AI events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<AuditDetail>,
}

/// `POST /v1/teams/{team_id}/audit`: events from the calling device, in
/// `seq` order. Already-stored `event_id`s are reported as duplicates, not
/// errors, so retries are safe. A batch that doesn't continue the stored
/// chain fails with `AuditChainBroken` and stores nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestAuditRequest {
    pub events: Vec<AuditEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestAuditResponse {
    pub accepted: Vec<Uuid>,
    pub duplicates: Vec<Uuid>,
}

/// Sort order of `GET /v1/teams/{team_id}/audit`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditOrder {
    /// `received_at` descending, then `device_id` descending, then `seq`
    /// descending: the newest events first. The default.
    #[default]
    Newest,
    /// `device_id` ascending, then `seq` ascending: each device's hash chain
    /// in order, one device after the other. For verification and export.
    Chain,
}

/// Query string of `GET /v1/teams/{team_id}/audit` (admin). The response is
/// `Vec<AuditRecord>` in the requested [`AuditOrder`].
///
/// Filters: `device_id`, `entry_id`, `kind` (an [`AuditKind::as_str`] name),
/// `actor` (an exact actor name), `since` (`received_at >= since`: the time the
/// server observed the event, not the signed `occurred_at`). `limit` is clamped
/// by the server to 1 to 1000 (not rejected), default 100.
///
/// Paging is keyset-based: ask for a page, then ask again with the cursor of
/// the last record ([`AuditListQuery::after`]) until a page holds fewer records
/// than the effective (clamped) `limit`, which is the last. The cursor is stable
/// while events keep arriving: new events never shift or repeat the pages
/// already read.
///
/// - `order=newest` (default): cursor `before_received_at`, `before_device_id`
///   and `before_seq`, all three or none. Returns the events strictly after
///   that position in the newest-first order. All filters apply.
/// - `order=chain`: cursor `after_device_id` and `after_seq`, both or none.
///   Returns the events strictly after that position in the chain order. Only
///   `device_id` and `limit` are allowed besides the cursor: a filter would
///   drop events from the middle of a chain and break the contiguity that
///   verification relies on.
///
/// Anything else is `InvalidRequest`: a partial cursor, a cursor of the other
/// order, or a filter with `order=chain`.
///
/// A server older than paging ignores the cursor and the order and answers
/// the first page again. A client that may talk to one must stop as soon as a
/// page does not advance past its cursor (the last record is the same as the
/// previous page's, or the page is not past the cursor in the requested
/// order), instead of looping.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<AuditKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Absent means [`AuditOrder::Newest`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<AuditOrder>,
    /// Newest cursor, with `before_device_id` and `before_seq`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_received_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_device_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_seq: Option<u64>,
    /// Chain cursor, with `after_seq`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_device_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<u64>,
}

impl AuditListQuery {
    /// The query for the page after the one that ended with `last`: the
    /// cursor of the query's order is set from `last` (the other order's
    /// cursor is cleared), every other field is kept.
    pub fn after(&self, last: &AuditRecord) -> AuditListQuery {
        let mut next = self.clone();
        match self.order.unwrap_or_default() {
            AuditOrder::Newest => {
                next.before_received_at = Some(last.received_at);
                next.before_device_id = Some(last.event.device_id);
                next.before_seq = Some(last.event.seq);
                next.after_device_id = None;
                next.after_seq = None;
            }
            AuditOrder::Chain => {
                next.after_device_id = Some(last.event.device_id);
                next.after_seq = Some(last.event.seq);
                next.before_received_at = None;
                next.before_device_id = None;
                next.before_seq = None;
            }
        }
        next
    }
}

/// One stored audit event as `GET …/audit` returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub event: AuditEvent,
    /// Observed by the server, not signed.
    pub received_at: DateTime<Utc>,
    pub client_ip: Option<String>,
}
