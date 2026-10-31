//! The in-flight tool call shared by the agent's `wait_approval` and the task that runs it.

use super::error::ToolResult;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// What a tool call that is kept alive for an approval looks like from the outside.
#[derive(Debug, Clone)]
pub enum CallPhase {
    /// Still running in the foreground: no approval was needed yet.
    Running,
    /// The call is waiting for a human; `wait_approval` collects the result.
    Pending {
        request_id: String,
        expires_in_secs: u64,
        /// When the approval window closes.
        expires_at: Instant,
    },
    /// Finished (approved and executed, denied, or failed). Kept until the run ends.
    Done(ToolResult),
}

/// One in-flight tool call, shared by the agent's `wait_approval` and the task that runs it.
///
/// The handler runs exactly once, in a spawned task; approval only pauses it. That is what
/// makes repeated or concurrent `wait_approval` calls safe: they all observe the same result.
pub struct CallSlot {
    tool: String,
    tx: watch::Sender<CallPhase>,
}

impl CallSlot {
    pub(crate) fn new(tool: &str) -> Arc<Self> {
        let (tx, _) = watch::channel(CallPhase::Running);
        Arc::new(Self {
            tool: tool.to_string(),
            tx,
        })
    }

    pub fn tool(&self) -> &str {
        &self.tool
    }

    fn phase(&self) -> CallPhase {
        self.tx.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<CallPhase> {
        self.tx.subscribe()
    }

    pub(crate) fn phase_now(&self) -> CallPhase {
        self.phase()
    }

    /// Mark the call pending unless it already is (or finished).
    pub(super) fn mark_pending(&self, request_id: &str, expires_in_secs: u64) {
        self.tx.send_if_modified(|p| {
            if matches!(p, CallPhase::Running) {
                *p = CallPhase::Pending {
                    request_id: request_id.to_string(),
                    expires_in_secs,
                    expires_at: Instant::now() + Duration::from_secs(expires_in_secs),
                };
                true
            } else {
                false
            }
        });
    }

    pub(crate) fn finish(&self, result: ToolResult) {
        self.tx.send_replace(CallPhase::Done(result));
    }

    /// Wait for the call to finish, or for the deadline to pass.
    pub async fn wait_done(&self, until: Instant) -> Option<ToolResult> {
        let mut rx = self.tx.subscribe();
        loop {
            let phase = rx.borrow().clone();
            match phase {
                CallPhase::Done(r) => return Some(r),
                _ => {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return None;
                    }
                    if tokio::time::timeout(left, rx.changed()).await.is_err() {
                        return None;
                    }
                }
            }
        }
    }

    pub fn pending(&self) -> Option<(String, u64, Instant)> {
        match self.phase() {
            CallPhase::Pending {
                request_id,
                expires_in_secs,
                expires_at,
            } => Some((request_id, expires_in_secs, expires_at)),
            _ => None,
        }
    }
}

tokio::task_local! {
    /// The slot of the tool call being served, so an approval can mark it pending.
    static CALL_SLOT: Arc<CallSlot>;
}

/// Serve `fut` as the handler of `slot`'s call.
pub(crate) async fn with_call_slot<F: Future>(slot: Arc<CallSlot>, fut: F) -> F::Output {
    CALL_SLOT.scope(slot, fut).await
}

pub(super) fn current_call_slot() -> Option<Arc<CallSlot>> {
    CALL_SLOT.try_with(|s| s.clone()).ok()
}
