//! One method per `/v1` endpoint.

use super::audit::audit_query_pairs;
use super::error::{org_route, ClientResult};
use super::http::{TeamsClient, MAX_POLL_WAIT_SECS};
use bssh_teams_proto::api::{
    AcceptInviteRequest, AddOwnerRequest, AgentInfo, AuditListQuery, AuditRecord, BillingInfo,
    BillingRedirect, ChallengeRequest, ChallengeResponse, CheckoutRequest, CreateAccountRequest,
    CreateAccountResponse, CreateAgentRequest, CreateInviteRequest, CreateTeamRequest,
    EmailCodeRequest, EmailCodeResponse, IngestAuditRequest, IngestAuditResponse, Invite, Member,
    OrgInfo, PendingDevices, PullResponse, PushRequest, PushResponse, PutKeysRequest, RelayAck,
    RelayKind, RelayMessage, RelayPoll, TeamInfo, TokenRequest, TokenResponse, WrappedKey,
};
use reqwest::Method;
use uuid::Uuid;

impl TeamsClient {
    /// Mail a one-time code that proves `email` (unauthenticated). The answer
    /// is the same whether or not a code was sent, so it says nothing about
    /// which addresses exist; see [`CreateAccountRequest::email_code`].
    pub async fn request_email_code(
        &self,
        req: &EmailCodeRequest,
    ) -> ClientResult<EmailCodeResponse> {
        self.post_plain("/accounts/email-code", req).await
    }

    pub async fn create_account(
        &self,
        req: &CreateAccountRequest,
    ) -> ClientResult<CreateAccountResponse> {
        self.post_plain("/accounts", req).await
    }

    pub async fn request_challenge(&self) -> ClientResult<ChallengeResponse> {
        let req = ChallengeRequest {
            device_id: self.device_id,
        };
        self.post_plain("/auth/challenge", &req).await
    }

    pub async fn request_token(&self, req: &TokenRequest) -> ClientResult<TokenResponse> {
        self.post_plain("/auth/token", req).await
    }

    // -- teams, members, invites ---------------------------------------------

    /// The response type of `POST /teams` isn't specified in the contract; we
    /// assume [`TeamInfo`] (the caller needs the new `team_id`).
    pub async fn create_team(&self, req: &CreateTeamRequest) -> ClientResult<TeamInfo> {
        self.post("/teams", req).await
    }

    pub async fn list_teams(&self) -> ClientResult<Vec<TeamInfo>> {
        self.get("/teams", &[]).await
    }

    pub async fn team(&self, team_id: Uuid) -> ClientResult<TeamInfo> {
        self.get(&format!("/teams/{team_id}"), &[]).await
    }

    pub async fn list_members(&self, team_id: Uuid) -> ClientResult<Vec<Member>> {
        self.get(&format!("/teams/{team_id}/members"), &[]).await
    }

    pub async fn remove_member(&self, team_id: Uuid, member_id: Uuid) -> ClientResult<()> {
        self.call(
            Method::DELETE,
            &format!("/teams/{team_id}/members/{member_id}"),
            &[],
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn create_invite(
        &self,
        team_id: Uuid,
        req: &CreateInviteRequest,
    ) -> ClientResult<Invite> {
        self.post(&format!("/teams/{team_id}/invites"), req).await
    }

    /// Pending, unexpired invites (admin); tokens are never returned here.
    pub async fn list_invites(&self, team_id: Uuid) -> ClientResult<Vec<Invite>> {
        self.get(&format!("/teams/{team_id}/invites"), &[]).await
    }

    pub async fn revoke_invite(&self, team_id: Uuid, invite_id: Uuid) -> ClientResult<()> {
        self.call(
            Method::DELETE,
            &format!("/teams/{team_id}/invites/{invite_id}"),
            &[],
            None,
        )
        .await?;
        Ok(())
    }

    /// Join the team the invite is for; returns that team.
    pub async fn accept_invite(&self, req: &AcceptInviteRequest) -> ClientResult<TeamInfo> {
        self.post("/invites/accept", req).await
    }

    // -- agents and the approval relay (bssh AI) -----------------------------

    /// Register an agent identity (admin). The agent takes a seat.
    pub async fn create_agent(
        &self,
        team_id: Uuid,
        req: &CreateAgentRequest,
    ) -> ClientResult<AgentInfo> {
        self.post(&format!("/teams/{team_id}/agents"), req).await
    }

    pub async fn list_agents(&self, team_id: Uuid) -> ClientResult<Vec<AgentInfo>> {
        self.get(&format!("/teams/{team_id}/agents"), &[]).await
    }

    /// Queue a relay message for its recipients.
    pub async fn post_relay(&self, team_id: Uuid, msg: &RelayMessage) -> ClientResult<RelayAck> {
        self.post(&format!("/teams/{team_id}/relay"), msg).await
    }

    /// Messages addressed to this device after `after`. Waits up to `wait_secs`
    /// (the server caps it at 30, this client's request timeout is 30 too, so
    /// callers ask for at most [`MAX_POLL_WAIT_SECS`]) for one to arrive.
    pub async fn poll_relay(
        &self,
        team_id: Uuid,
        kind: Option<RelayKind>,
        request_id: Option<Uuid>,
        after: u64,
        wait_secs: u64,
    ) -> ClientResult<RelayPoll> {
        let mut query = vec![
            ("after", after.to_string()),
            ("wait", wait_secs.min(MAX_POLL_WAIT_SECS).to_string()),
        ];
        match kind {
            Some(RelayKind::Request) => query.push(("kind", "request".into())),
            Some(RelayKind::Decision) => query.push(("kind", "decision".into())),
            None => {}
        }
        if let Some(id) = request_id {
            query.push(("request_id", id.to_string()));
        }
        self.get(&format!("/teams/{team_id}/relay"), &query).await
    }

    // -- organizations -------------------------------------------------------

    /// Organizations this account owns or has a team in.
    pub async fn list_orgs(&self) -> ClientResult<Vec<OrgInfo>> {
        self.get("/orgs", &[]).await.map_err(org_route)
    }

    /// A new empty organization owned by this account. Refused with
    /// `PaymentRequired` while the account owns another without a subscription.
    pub async fn create_org(&self) -> ClientResult<OrgInfo> {
        self.post("/orgs", &serde_json::json!({}))
            .await
            .map_err(org_route)
    }

    pub async fn org(&self, org_id: Uuid) -> ClientResult<OrgInfo> {
        self.get(&format!("/orgs/{org_id}"), &[])
            .await
            .map_err(org_route)
    }

    pub async fn add_org_owner(&self, org_id: Uuid, email: &str) -> ClientResult<OrgInfo> {
        self.post(
            &format!("/orgs/{org_id}/owners"),
            &AddOwnerRequest {
                email: email.to_string(),
            },
        )
        .await
        .map_err(org_route)
    }

    pub async fn remove_org_owner(&self, org_id: Uuid, account_id: Uuid) -> ClientResult<()> {
        self.call(
            Method::DELETE,
            &format!("/orgs/{org_id}/owners/{account_id}"),
            &[],
            None,
        )
        .await
        .map_err(org_route)?;
        Ok(())
    }

    pub async fn org_billing(&self, org_id: Uuid) -> ClientResult<BillingInfo> {
        self.get(&format!("/orgs/{org_id}/billing"), &[])
            .await
            .map_err(org_route)
    }

    pub async fn org_billing_checkout(
        &self,
        org_id: Uuid,
        req: &CheckoutRequest,
    ) -> ClientResult<BillingRedirect> {
        self.post(&format!("/orgs/{org_id}/billing/checkout"), req)
            .await
            .map_err(org_route)
    }

    pub async fn org_billing_portal(&self, org_id: Uuid) -> ClientResult<BillingRedirect> {
        self.post(
            &format!("/orgs/{org_id}/billing/portal"),
            &serde_json::json!({}),
        )
        .await
        .map_err(org_route)
    }

    // -- billing -------------------------------------------------------------

    pub async fn billing(&self, team_id: Uuid) -> ClientResult<BillingInfo> {
        self.get(&format!("/teams/{team_id}/billing"), &[]).await
    }

    pub async fn billing_checkout(
        &self,
        team_id: Uuid,
        req: &CheckoutRequest,
    ) -> ClientResult<BillingRedirect> {
        self.post(&format!("/teams/{team_id}/billing/checkout"), req)
            .await
    }

    pub async fn billing_portal(&self, team_id: Uuid) -> ClientResult<BillingRedirect> {
        self.post(
            &format!("/teams/{team_id}/billing/portal"),
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn pending_devices(&self, team_id: Uuid) -> ClientResult<PendingDevices> {
        self.get(&format!("/teams/{team_id}/devices/pending"), &[])
            .await
    }

    // -- keys ----------------------------------------------------------------

    /// The response body isn't specified; it is ignored.
    pub async fn put_keys(&self, team_id: Uuid, req: &PutKeysRequest) -> ClientResult<()> {
        self.post_unit(&format!("/teams/{team_id}/keys"), req).await
    }

    /// Every key version addressed to this device, oldest first.
    pub async fn get_keys(&self, team_id: Uuid) -> ClientResult<Vec<WrappedKey>> {
        self.get(&format!("/teams/{team_id}/keys"), &[]).await
    }

    // -- vault, audit --------------------------------------------------------

    pub async fn pull_vault(
        &self,
        team_id: Uuid,
        since: u64,
        limit: u32,
    ) -> ClientResult<PullResponse> {
        self.get(
            &format!("/teams/{team_id}/vault"),
            &[("since", since.to_string()), ("limit", limit.to_string())],
        )
        .await
    }

    pub async fn push_vault(&self, team_id: Uuid, req: &PushRequest) -> ClientResult<PushResponse> {
        self.post(&format!("/teams/{team_id}/vault"), req).await
    }

    pub async fn ingest_audit_events(
        &self,
        team_id: Uuid,
        req: &IngestAuditRequest,
    ) -> ClientResult<IngestAuditResponse> {
        self.post(&format!("/teams/{team_id}/audit"), req).await
    }

    /// One page of the team's audit log (admins only), as `query` describes:
    /// filters, order, page size and cursor. [`walk_audit_pages`](super::walk_audit_pages) reads every
    /// page.
    pub async fn list_audit(
        &self,
        team_id: Uuid,
        query: &AuditListQuery,
    ) -> ClientResult<Vec<AuditRecord>> {
        let (keys, values): (Vec<String>, Vec<String>) =
            audit_query_pairs(query)?.into_iter().unzip();
        let pairs: Vec<(&str, String)> = keys.iter().map(String::as_str).zip(values).collect();
        self.get(&format!("/teams/{team_id}/audit"), &pairs).await
    }

    /// Revoke a device of this account, or of a member of a team the caller
    /// administers. The server answers 404 for a device the caller can't see,
    /// whether or not it exists.
    pub async fn revoke_device(&self, device_id: Uuid) -> ClientResult<()> {
        self.call(Method::DELETE, &format!("/devices/{device_id}"), &[], None)
            .await?;
        Ok(())
    }
}
