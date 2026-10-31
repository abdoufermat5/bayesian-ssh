//! Organization types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrgRole {
    /// Manages billing and owners, creates teams in the organization.
    Owner,
    /// Belongs to at least one of its teams.
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgOwner {
    pub account_id: Uuid,
    pub email: String,
}

/// `GET /v1/orgs` returns `Vec<OrgInfo>`: the organizations the caller owns or
/// has a team in. `GET /v1/orgs/{org_id}` one.
/// `POST /v1/orgs` (no body) creates an empty one owned by the caller; refused
/// with `PaymentRequired` while they own another without a subscription (one
/// free organization per account).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgInfo {
    pub org_id: Uuid,
    pub my_role: OrgRole,
    pub owners: Vec<OrgOwner>,
    /// Owners see every team; members only theirs.
    pub team_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// `POST /v1/orgs/{org_id}/owners` (owner): the account must be an active
/// member of one of the organization's teams. Returns the updated `OrgInfo`.
/// `DELETE /v1/orgs/{org_id}/owners/{account_id}` (owner) returns 204; the
/// last owner can't be removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddOwnerRequest {
    pub email: String,
}
