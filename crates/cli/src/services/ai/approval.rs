//! Approval broker: in-client (elicitation hook), out-of-band over the control socket, expiry.
//!
//! Owner: W3. Stub from the scaffold; replace freely.

use super::store::AiStore;
use super::types::{AiEvent, ApprovalBroker, ApprovalOutcome, ApprovalRequest};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Asks the human through the agent's own client (MCP elicitation, tier T1).
/// Implemented by the MCP layer (W4); `Ok(None)` = the client can't elicit.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Elicitor: Send + Sync {
    async fn confirm(&self, req: &ApprovalRequest) -> anyhow::Result<Option<bool>>;
}

/// Static facts about the run, sent in `ServerMsg::Hello`.
#[derive(Debug, Clone)]
pub struct RunInfo {
    pub run_id: String,
    pub client: String,
    pub policy: String,
    pub started_at: String,
}

/// Broker for one run: T1 via the elicitor when allowed, T2 via the control socket.
pub struct LocalBroker {
    store: Arc<AiStore>,
    run: RunInfo,
    elicitor: Option<Arc<dyn Elicitor>>,
    timeout_secs: u64,
}

impl LocalBroker {
    pub fn new(
        store: Arc<AiStore>,
        run: RunInfo,
        elicitor: Option<Arc<dyn Elicitor>>,
        timeout_secs: u64,
    ) -> Self {
        Self {
            store,
            run,
            elicitor,
            timeout_secs,
        }
    }
}

#[async_trait]
impl ApprovalBroker for LocalBroker {
    async fn request(&self, req: ApprovalRequest) -> anyhow::Result<ApprovalOutcome> {
        let _ = (
            &self.store,
            &self.run,
            &self.elicitor,
            self.timeout_secs,
            req,
        );
        anyhow::bail!("LocalBroker::request: not implemented yet")
    }
}

/// Serve `<ai_dir>/runs/<run_id>.sock` (protocol in `ipc.rs`) until `stop` fires.
/// Returns when a client sends `Stop` too; the caller ends the run.
pub async fn serve_control(
    socket: PathBuf,
    run: RunInfo,
    broker: Arc<LocalBroker>,
    events: broadcast::Sender<AiEvent>,
    stop: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let _ = (socket, run, broker, events, stop);
    anyhow::bail!("serve_control: not implemented yet")
}
