//! `McpElicitor`: asks the human through the agent's own client (tier T1).

use super::describe::describe_request;
use crate::services::ai::approval::Elicitor;
use crate::services::ai::types::ApprovalRequest;
use async_trait::async_trait;
use rmcp::model::{ProgressNotificationParam, ProgressToken};
use rmcp::service::{ElicitationError, ElicitationMode, Peer};
use rmcp::RoleServer;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Mutex;
use std::time::Duration;
use tracing::warn;

/// What the elicitation form asks the human.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Confirm {
    /// Approve this request?
    pub approve: bool,
}
rmcp::elicit_safe!(Confirm);

/// Asks the human through the agent's own client (tier T1). The server fills in the client's
/// peer when the MCP session initialises.
#[derive(Default)]
pub struct McpElicitor {
    peer: Mutex<Option<Peer<RoleServer>>>,
}

impl McpElicitor {
    pub fn set_peer(&self, peer: Peer<RoleServer>) {
        if let Ok(mut p) = self.peer.lock() {
            *p = Some(peer);
        }
    }

    /// Send `notifications/progress` to the client for a long-running call.
    pub async fn progress(
        &self,
        token: ProgressToken,
        progress: f64,
        message: String,
    ) -> Result<(), rmcp::service::ServiceError> {
        let peer = self.peer.lock().ok().and_then(|p| p.clone());
        let Some(peer) = peer else {
            return Ok(());
        };
        let params = ProgressNotificationParam::new(token, progress).with_message(message);
        peer.notify_progress(params).await
    }
}

#[async_trait]
impl Elicitor for McpElicitor {
    fn can_elicit(&self) -> bool {
        self.peer
            .lock()
            .ok()
            .and_then(|p| p.clone())
            .is_some_and(|p| {
                p.supported_elicitation_modes()
                    .contains(&ElicitationMode::Form)
            })
    }

    async fn confirm(&self, req: &ApprovalRequest) -> anyhow::Result<Option<bool>> {
        let peer = self.peer.lock().ok().and_then(|p| p.clone());
        let Some(peer) = peer else {
            return Ok(None);
        };
        if !peer
            .supported_elicitation_modes()
            .contains(&ElicitationMode::Form)
        {
            return Ok(None);
        }
        match peer
            .elicit_with_timeout::<Confirm>(describe_request(req), Some(Duration::from_secs(300)))
            .await
        {
            Ok(Some(c)) => Ok(Some(c.approve)),
            Ok(None) => Ok(Some(false)),
            Err(ElicitationError::UserDeclined | ElicitationError::UserCancelled) => {
                Ok(Some(false))
            }
            Err(e) => {
                warn!("elicitation failed, falling back to out-of-band approval: {e}");
                Ok(None)
            }
        }
    }
}
