//! Run lifecycle: events, start/end, stop, circuit-breaker pause and resume.

use super::error::{ResumeError, ToolError};
use super::run_context::RunContext;
use crate::services::ai::mcp::util::sha256_hex;
use crate::services::ai::types::AiEvent;
use anyhow::Result;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tokio::sync::broadcast;
use tracing::warn;

impl RunContext {
    pub fn subscribe(&self) -> broadcast::Receiver<AiEvent> {
        self.events.subscribe()
    }

    pub fn events(&self) -> broadcast::Sender<AiEvent> {
        self.events.clone()
    }

    pub fn client(&self) -> String {
        self.client.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Record the self-reported client and emit `RunStarted` (once).
    pub fn start(&self, client: &str) {
        if let Ok(mut c) = self.client.lock() {
            *c = client.to_string();
        }
        if let Some(b) = self.local_broker.get() {
            b.set_client(client);
        }
        if self.start_emitted.swap(true, Ordering::SeqCst) {
            return;
        }
        let policy_json = serde_json::to_string(&self.policy).unwrap_or_default();
        let ev = AiEvent::RunStarted {
            run_id: self.run_id.clone(),
            client: client.to_string(),
            on_behalf_of: whoami::username(),
            policy: self.policy_name.clone(),
            policy_hash: sha256_hex(policy_json.as_bytes()),
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit RunStarted: {e:#}");
        }
    }

    /// Emit `RunEnded` once.
    pub fn end(&self, reason: &str) {
        if self.end_emitted.swap(true, Ordering::SeqCst) {
            return;
        }
        let ev = AiEvent::RunEnded {
            run_id: self.run_id.clone(),
            reason: reason.to_string(),
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit RunEnded: {e:#}");
        }
    }

    /// Kill switch: no more calls succeed, the transport shuts down.
    pub fn stop(&self, reason: &str) {
        self.stopped.store(true, Ordering::SeqCst);
        self.stop_tx.send_replace(true);
        self.end(reason);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Resolves when the run is stopped (kill switch, breaker, TTL).
    pub async fn stopped(&self) {
        let mut rx = self.stop_tx.subscribe();
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    pub fn emit(&self, ev: &AiEvent) -> Result<()> {
        let res = self.deps.audit.record(ev);
        // No subscribers is normal.
        let _ = self.events.send(ev.clone());
        res
    }

    /// Run still allowed to make calls?
    pub fn check_alive(&self) -> Result<(), ToolError> {
        if self.is_stopped() {
            return Err(ToolError::Denied(
                "this run was stopped; ask the user to start a new agent session".into(),
            ));
        }
        if self.is_paused() {
            let reason = self
                .pause_reason
                .lock()
                .ok()
                .and_then(|r| r.clone())
                .unwrap_or_else(|| "too many denials in a minute".into());
            return Err(ToolError::Denied(format!(
                "{reason}; the user can resume it with `bssh ai resume {}` or in the desktop app",
                self.short_run_id()
            )));
        }
        if Instant::now() >= self.deadline {
            self.stop("run TTL reached");
            return Err(ToolError::Denied(
                "this run has expired (TTL) and stopped; ask the user to restart the agent".into(),
            ));
        }
        Ok(())
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// First 8 characters of the run id, as `bssh ai status` shows it.
    pub fn short_run_id(&self) -> &str {
        self.run_id.get(..8).unwrap_or(&self.run_id)
    }

    /// Pause the run after the circuit breaker tripped. Calls are refused (except `status`
    /// and `wait_approval`) until [`resume`](Self::resume); the run is not stopped.
    pub(super) fn pause(&self, reason: String) {
        if self.paused.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut r) = self.pause_reason.lock() {
            *r = Some(reason.clone());
        }
        let ev = AiEvent::Paused {
            run_id: self.run_id.clone(),
            reason,
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit Paused: {e:#}");
        }
    }

    /// Clear a circuit-breaker pause and the denial window. Stop is final, so a stopped run
    /// cannot be resumed.
    ///
    /// # Errors
    ///
    /// [`ResumeError::Stopped`] when the run was stopped, [`ResumeError::NotPaused`] when no
    /// pause is in effect.
    pub fn resume(&self, by: &str) -> Result<(), ResumeError> {
        if self.is_stopped() {
            return Err(ResumeError::Stopped);
        }
        if !self.paused.swap(false, Ordering::SeqCst) {
            return Err(ResumeError::NotPaused);
        }
        if let Ok(mut r) = self.pause_reason.lock() {
            *r = None;
        }
        // A resume starts a fresh window: the denials that tripped it don't count again.
        if let Ok(mut d) = self.denials.lock() {
            d.clear();
        }
        let ev = AiEvent::Resumed {
            run_id: self.run_id.clone(),
            by: by.to_string(),
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit Resumed: {e:#}");
        }
        Ok(())
    }
}
