//! Where the hub reports changes: Tauri events in the app.

use bayesian_ssh::services::ai::types::{ApprovalRequest, ApprovalSubject};
use tauri::{AppHandle, Emitter};

use serde::Serialize;

use super::types::{RunSummary, StoredEvent};

#[derive(Serialize, Clone)]
struct EventPayload<'a> {
    run_id: &'a str,
    #[serde(flatten)]
    stored: &'a StoredEvent,
}

#[derive(Serialize, Clone)]
struct RequestPayload<'a> {
    run_id: &'a str,
    request: &'a ApprovalRequest,
}

#[derive(Serialize, Clone)]
struct ErrorPayload<'a> {
    run_id: &'a str,
    message: &'a str,
}

/// Where the hub reports changes. Tauri in the app, a recorder in tests.
pub trait HubSink: Send + Sync + 'static {
    fn runs_changed(&self, runs: &[RunSummary]);
    fn event(&self, run_id: &str, event: &StoredEvent);
    /// A request the user has not seen yet.
    fn request(&self, run_id: &str, client: &str, request: &ApprovalRequest);
    fn error(&self, run_id: &str, message: &str);
}

pub(super) struct TauriSink(pub(super) AppHandle);

impl HubSink for TauriSink {
    fn runs_changed(&self, runs: &[RunSummary]) {
        let _ = self.0.emit("ai://runs", runs);
    }

    fn event(&self, run_id: &str, event: &StoredEvent) {
        let _ = self.0.emit(
            "ai://event",
            EventPayload {
                run_id,
                stored: event,
            },
        );
    }

    fn request(&self, run_id: &str, client: &str, request: &ApprovalRequest) {
        let _ = self
            .0
            .emit("ai://request", RequestPayload { run_id, request });
        crate::tray::show_main_window(&self.0);
        // Only what the policy engine produced goes into the notification:
        // never the agent's reason, which may come from an injection.
        let _ = crate::tray::send_desktop_notification(
            "Agent needs approval".to_string(),
            notification_body(client, request),
        );
    }

    fn error(&self, run_id: &str, message: &str) {
        let _ = self.0.emit("ai://error", ErrorPayload { run_id, message });
    }
}

/// `<client>: <what> on <hosts>`, short and without agent-written text.
pub(super) fn notification_body(client: &str, request: &ApprovalRequest) -> String {
    let what = match &request.subject {
        ApprovalSubject::Call { call, .. } => {
            let host = call.host.as_deref().unwrap_or("-");
            format!("{:?} on {host}", call.tool)
        }
        ApprovalSubject::Plan { plan, .. } => {
            let mut hosts: Vec<&str> = plan
                .steps
                .iter()
                .filter_map(|s| s.host.as_deref())
                .collect();
            hosts.sort_unstable();
            hosts.dedup();
            format!(
                "change plan, {} step(s) on {}",
                plan.steps.len(),
                hosts.join(", ")
            )
        }
        ApprovalSubject::Access { .. } => "wider access".to_string(),
    };
    let body = format!("{client}: {what}");
    body.chars().take(160).collect()
}
