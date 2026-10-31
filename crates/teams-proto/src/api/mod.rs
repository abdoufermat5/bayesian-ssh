//! Request and response bodies, one section per endpoint group. Paths are
//! listed on each type; all are under [`crate::API_PREFIX`] and, except
//! account creation and auth, need `Authorization: Bearer <access_token>`.

mod accounts;
mod audit;
mod billing;
mod errors;
mod keys;
mod orgs;
mod relay;
mod teams;
mod vault;

pub use accounts::{
    AddDeviceRequest, AddDeviceResponse, ChallengeRequest, ChallengeResponse, CreateAccountRequest,
    CreateAccountResponse, DeviceInfo, EmailCodeRequest, EmailCodeResponse, NewDevice,
    TokenRequest, TokenResponse,
};
pub use audit::{
    AuditActor, AuditDetail, AuditEvent, AuditKind, AuditListQuery, AuditOrder, AuditRecord,
    IngestAuditRequest, IngestAuditResponse,
};
pub use billing::{BillingInfo, BillingInterval, BillingRedirect, CheckoutRequest, Plan};
pub use errors::{ApiError, ErrorCode};
pub use keys::{PutKeysRequest, WrappedKey};
pub use orgs::{AddOwnerRequest, OrgInfo, OrgOwner, OrgRole};
pub use relay::{RelayAck, RelayEnvelope, RelayKind, RelayMessage, RelayPoll, RelayRecipient};
pub use teams::{
    AcceptInviteRequest, AgentInfo, CreateAgentRequest, CreateInviteRequest, CreateTeamRequest,
    Invite, Member, MemberKind, PendingDevices, Role, TeamInfo,
};
pub use vault::{
    Accepted, ConnectionRecord, EncryptedEntry, PullResponse, PushConflict, PushRequest,
    PushResponse, SignedAgentPolicy, VaultPayload,
};

#[cfg(test)]
mod compat_tests;
#[cfg(test)]
mod tests;
