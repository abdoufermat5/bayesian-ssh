//! [`TeamsTransport`] over the HTTP client.

use super::error::ClientResult;
use super::http::TeamsClient;
use crate::services::teams::sync::TeamsTransport;
use async_trait::async_trait;
use bssh_teams_proto::api::{
    IngestAuditRequest, IngestAuditResponse, PullResponse, PushRequest, PushResponse,
};
use uuid::Uuid;

#[async_trait(?Send)]
impl TeamsTransport for TeamsClient {
    async fn pull(&self, team_id: Uuid, since: u64, limit: u32) -> ClientResult<PullResponse> {
        self.pull_vault(team_id, since, limit).await
    }

    async fn push(&self, team_id: Uuid, req: PushRequest) -> ClientResult<PushResponse> {
        self.push_vault(team_id, &req).await
    }

    async fn ingest_audit(
        &self,
        team_id: Uuid,
        req: IngestAuditRequest,
    ) -> ClientResult<IngestAuditResponse> {
        self.ingest_audit_events(team_id, &req).await
    }
}
