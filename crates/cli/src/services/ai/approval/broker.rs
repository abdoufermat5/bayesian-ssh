//! The local broker: approval by tier, grants and desktop notifications.

use crate::services::ai::store::AiStore;
use crate::services::ai::types::{
    ApprovalBroker, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Grant, Tier,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, oneshot};

/// Asks the human through the agent's own client (MCP elicitation, tier T1).
/// Implemented by the MCP layer; `Ok(None)` = the client can't elicit.
#[expect(
    clippy::double_must_use,
    reason = "`#[async_trait]` expands the async method into a must_use boxed future that also carries its own must_use; the duplicate comes from the macro"
)]
#[async_trait]
pub trait Elicitor: Send + Sync {
    async fn confirm(&self, req: &ApprovalRequest) -> anyhow::Result<Option<bool>>;

    /// Whether `confirm` can reach a human right now. The MCP client only becomes
    /// reachable after `initialize`.
    fn can_elicit(&self) -> bool {
        true
    }
}

/// Static facts about the run, sent in `ServerMsg::Hello`.
#[derive(Debug, Clone)]
pub struct RunInfo {
    pub run_id: String,
    pub client: String,
    pub policy: String,
    pub started_at: String,
    pub on_behalf_of: String,
    /// Hosts in the run's scope when it started.
    pub hosts: Vec<String>,
}

/// A request waiting for a human.
pub(super) struct Pending {
    req: ApprovalRequest,
    /// Local approvers so far. A two-person request needs the Teams relay for its second
    /// approver, so it never finishes from this list.
    approvers: Vec<String>,
    first_grant: Option<Grant>,
    tx: Option<oneshot::Sender<ApprovalOutcome>>,
}

/// Broker for one run: T1 via the elicitor when allowed, T2 via the control socket.
pub struct LocalBroker {
    store: Arc<AiStore>,
    run: RunInfo,
    elicitor: Option<Arc<dyn Elicitor>>,
    timeout_secs: u64,
    pending: Mutex<HashMap<String, Pending>>,
    new_requests: broadcast::Sender<ApprovalRequest>,
    notifications: bool,
    /// MCP client name: only known after `initialize`, which comes after the socket is up.
    client: Mutex<String>,
}

impl LocalBroker {
    pub fn new(
        store: Arc<AiStore>,
        run: RunInfo,
        elicitor: Option<Arc<dyn Elicitor>>,
        timeout_secs: u64,
    ) -> Self {
        let (new_requests, _) = broadcast::channel(64);
        let client = Mutex::new(run.client.clone());
        Self {
            client,
            store,
            run,
            elicitor,
            timeout_secs,
            pending: Mutex::new(HashMap::new()),
            new_requests,
            notifications: true,
        }
    }

    /// Turn the best-effort desktop notification off (on by default).
    pub fn without_notifications(mut self) -> Self {
        self.notifications = false;
        self
    }

    pub fn store(&self) -> &Arc<AiStore> {
        &self.store
    }

    pub fn run_info(&self) -> &RunInfo {
        &self.run
    }

    /// Record the client name reported in MCP `initialize`.
    pub fn set_client(&self, client: &str) {
        if let Ok(mut c) = self.client.lock() {
            *c = client.to_string();
        }
    }

    /// The client name as currently known (`run_info().client` until `set_client`).
    pub fn client(&self) -> String {
        self.client
            .lock()
            .map(|c| c.clone())
            .unwrap_or_else(|_| self.run.client.clone())
    }

    /// Stream of requests as they start waiting (the control server forwards these).
    pub fn subscribe_requests(&self) -> broadcast::Receiver<ApprovalRequest> {
        self.new_requests.subscribe()
    }

    /// Requests waiting for a decision right now, oldest first.
    pub fn pending_requests(&self) -> Vec<ApprovalRequest> {
        let mut v: Vec<ApprovalRequest> = match self.pending.lock() {
            Ok(m) => m.values().map(|p| p.req.clone()).collect(),
            Err(_) => Vec::new(),
        };
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        v
    }

    /// Apply a human decision (from the control socket). Errors when the request is unknown,
    /// already decided or expired, or when the same person approves a two-person request twice.
    ///
    /// The outcome is checked here, not trusted: a grant is narrowed to the request (see
    /// `checked_grant`), and a two-person request never completes from local approvals —
    /// every local approval comes from the same machine, so at most one is recorded and the
    /// request keeps waiting for the Teams relay.
    pub fn decide(&self, request_id: &str, outcome: ApprovalOutcome) -> anyhow::Result<()> {
        let mut map = self
            .pending
            .lock()
            .map_err(|_| anyhow::anyhow!("approval state lock poisoned"))?;
        let entry = map
            .get_mut(request_id)
            .ok_or_else(|| anyhow::anyhow!("no pending request '{request_id}'"))?;

        let final_outcome = match outcome {
            ApprovalOutcome::Expired => {
                anyhow::bail!("a client can't set a request to expired")
            }
            denied @ ApprovalOutcome::Denied { .. } => denied,
            ApprovalOutcome::Approved { by, channel, grant } => {
                if entry.approvers.contains(&by) {
                    anyhow::bail!(
                        "'{by}' already approved this request; a second person is needed"
                    );
                }
                if entry.req.tier == Tier::TwoPerson && !entry.approvers.is_empty() {
                    anyhow::bail!(
                        "this request needs two different people, and a local approval can't \
                         be the second one; a second person approves it on the Teams relay"
                    );
                }
                let grant = match grant {
                    Some(g) => Some(checked_grant(&entry.req, &self.run, g)?),
                    None => None,
                };
                entry.approvers.push(by);
                if entry.first_grant.is_none() {
                    entry.first_grant = grant;
                }
                if entry.req.tier == Tier::TwoPerson {
                    // Recorded as one approving person, still pending: it expires as
                    // "needs Teams (two approvers)" unless the relay supplies the second.
                    return Ok(());
                }
                ApprovalOutcome::Approved {
                    by: entry.approvers.join(" + "),
                    channel,
                    grant: entry.first_grant.take(),
                }
            }
        };

        let Some(mut done) = map.remove(request_id) else {
            anyhow::bail!("no pending request '{request_id}'");
        };
        drop(map);
        let final_outcome = self.finish(request_id, final_outcome);
        if let Some(tx) = done.tx.take() {
            let _ = tx.send(final_outcome);
        }
        Ok(())
    }

    /// Persist the outcome and store the grant of an approval (already checked by `decide`).
    fn finish(&self, request_id: &str, outcome: ApprovalOutcome) -> ApprovalOutcome {
        if let ApprovalOutcome::Approved { grant: Some(g), .. } = &outcome
            && let Err(e) = self.store.add_grant(g)
        {
            tracing::warn!("could not store grant: {e}");
        }
        if let Err(e) = self.store.record_outcome(request_id, &outcome) {
            tracing::warn!("could not record approval outcome: {e}");
        }
        outcome
    }

    /// The request timed out: remove it and record why.
    fn expire(&self, request_id: &str) -> ApprovalOutcome {
        let removed = self
            .pending
            .lock()
            .ok()
            .and_then(|mut m| m.remove(request_id));
        let outcome = match removed {
            Some(p) if p.req.tier == Tier::TwoPerson => ApprovalOutcome::Denied {
                by: "bssh".into(),
                reason: Some(format!(
                    "needs Teams (two approvers); got {} of 2 local approvals",
                    p.approvers.len()
                )),
            },
            _ => ApprovalOutcome::Expired,
        };
        if let Err(e) = self.store.record_outcome(request_id, &outcome) {
            tracing::warn!("could not record approval expiry: {e}");
        }
        outcome
    }

    /// Time left for a request: the configured timeout, narrowed by its own `expires_at`.
    ///
    /// The out-of-band request and any in-client attempt share this one budget, so a client
    /// that cannot elicit does not add a second timeout on top of the first.
    fn wait_budget(&self, req: &ApprovalRequest) -> Duration {
        let cap = Duration::from_secs(self.timeout_secs);
        let until = chrono::DateTime::parse_from_rfc3339(&req.expires_at)
            .ok()
            .and_then(|t| {
                (t.with_timezone(&chrono::Utc) - chrono::Utc::now())
                    .to_std()
                    .ok()
            });
        until.map_or(cap, |d| d.min(cap))
    }

    async fn out_of_band(
        &self,
        req: ApprovalRequest,
        wait: Duration,
    ) -> anyhow::Result<ApprovalOutcome> {
        let id = req.request_id.clone();
        self.store.insert_request(&req)?;
        let (tx, mut rx) = oneshot::channel();
        {
            let mut map = self
                .pending
                .lock()
                .map_err(|_| anyhow::anyhow!("approval state lock poisoned"))?;
            map.insert(
                id.clone(),
                Pending {
                    req: req.clone(),
                    approvers: Vec::new(),
                    first_grant: None,
                    tx: Some(tx),
                },
            );
        }
        // No receiver is fine: the request stays in `pending` for the next client to list.
        let _ = self.new_requests.send(req.clone());
        if self.notifications {
            desktop_notify(
                "bssh AI: approval needed",
                &format!("{} asks to {}", req.client, short_subject(&req)),
            );
        }

        match tokio::time::timeout(wait, &mut rx).await {
            Ok(Ok(outcome)) => Ok(outcome),
            Ok(Err(_)) => Ok(self.expire(&id)),
            Err(_) => {
                let outcome = self.expire(&id);
                // A decision may have landed just before the timeout fired.
                match rx.try_recv() {
                    Ok(decided) => Ok(decided),
                    Err(_) => Ok(outcome),
                }
            }
        }
    }
}

/// Longest grant a decision may create, whatever the client asked for.
pub(super) const MAX_GRANT_HOURS: i64 = 8;

/// Rewrite a grant supplied by a control-socket client so it covers only the request being
/// approved: server-generated `grant_id`, this run's id, the request's own tool and exact
/// host, and an expiry within [`MAX_GRANT_HOURS`].
///
/// The client (`bssh ai approve`, the desktop app) chooses whether there is a grant and for
/// how long, but it can only narrow a decision, never widen it beyond what was asked.
fn checked_grant(req: &ApprovalRequest, run: &RunInfo, mut g: Grant) -> anyhow::Result<Grant> {
    let ApprovalSubject::Call { call, .. } = &req.subject else {
        anyhow::bail!("grants only cover a single call, not a plan or an access request");
    };
    if g.tool != call.tool {
        anyhow::bail!(
            "grant for {} does not match the request's {}",
            tool_name(g.tool),
            tool_name(call.tool)
        );
    }
    let Some(host) = call.host.clone() else {
        anyhow::bail!("a grant needs the requested call's host");
    };
    let now = chrono::Utc::now();
    let expires = chrono::DateTime::parse_from_rfc3339(&g.expires_at)
        .map_err(|_| anyhow::anyhow!("grant expiry '{}' is not RFC 3339", g.expires_at))?
        .with_timezone(&chrono::Utc);
    if expires <= now {
        anyhow::bail!("grant expiry '{}' is in the past", g.expires_at);
    }
    let cap = now + chrono::Duration::hours(MAX_GRANT_HOURS);
    g.grant_id = uuid::Uuid::new_v4().to_string();
    g.run_id = Some(run.run_id.clone());
    g.hosts = host;
    g.expires_at = expires.min(cap).to_rfc3339();
    Ok(g)
}

#[async_trait]
impl ApprovalBroker for LocalBroker {
    async fn request(&self, mut req: ApprovalRequest) -> anyhow::Result<ApprovalOutcome> {
        let budget = self.wait_budget(&req);
        let started = std::time::Instant::now();
        if req.tier == Tier::InClient {
            if let Some(e) = &self.elicitor {
                let wait = budget;
                match tokio::time::timeout(wait, e.confirm(&req)).await {
                    Ok(Ok(Some(true))) => {
                        return Ok(ApprovalOutcome::Approved {
                            by: "user".into(),
                            channel: "confirmed-in-client".into(),
                            grant: None,
                        })
                    }
                    Ok(Ok(Some(false))) => {
                        return Ok(ApprovalOutcome::Denied {
                            by: "user".into(),
                            reason: None,
                        })
                    }
                    Ok(Ok(None)) => tracing::debug!("client can't elicit; asking out of band"),
                    Ok(Err(e)) => tracing::warn!("elicitation failed ({e}); asking out of band"),
                    Err(_) => tracing::warn!("elicitation timed out; asking out of band"),
                }
            }
            req.tier = Tier::OutOfBand;
        }
        let left = budget.saturating_sub(started.elapsed());
        self.out_of_band(req, left).await
    }
}

/// Wire name of a tool (`run_readonly`, `service`, ...), as the agent sees it.
fn tool_name(t: crate::services::ai::types::ToolName) -> &'static str {
    crate::services::ai::mcp::tools::name_str(t)
}

/// One line for notifications.
pub(super) fn short_subject(req: &ApprovalRequest) -> String {
    use crate::services::ai::types::ApprovalSubject::{Access, Call, Plan};
    match &req.subject {
        Call { call, .. } => match (&call.command, &call.path) {
            (Some(c), _) => format!("run `{}`", truncate(c, 80)),
            (None, Some(p)) => format!("change {}", truncate(p, 80)),
            _ => format!("use {}", tool_name(call.tool)),
        },
        Plan { plan, .. } => format!("apply the change '{}'", truncate(&plan.title, 80)),
        Access { reason, .. } => format!("widen its access: {}", truncate(reason, 80)),
    }
}

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}...")
    }
}

/// Best-effort desktop notification (notify-send on Linux, osascript on macOS).
/// Mirrors `crates/gui/src/tray.rs`; never fails, never blocks, does nothing in tests.
pub fn desktop_notify(title: &str, body: &str) {
    if cfg!(test) {
        return;
    }
    // Control characters could break the AppleScript / argument wrappers.
    let title: String = title.chars().filter(|c| !c.is_control()).collect();
    let body: String = body.chars().filter(|c| !c.is_control()).collect();
    let child = {
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("notify-send")
                .arg("-a")
                .arg("Bayesian SSH")
                .arg(&title)
                .arg(&body)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
        }
        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                body.replace('\\', "\\\\").replace('"', "\\\""),
                title.replace('\\', "\\\\").replace('"', "\\\"")
            );
            std::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .spawn()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (&title, &body);
            Err::<std::process::Child, std::io::Error>(std::io::Error::other("unsupported"))
        }
    };
    match child {
        Ok(mut c) => {
            // Reap the child so it doesn't linger as a zombie.
            std::thread::spawn(move || {
                let _ = c.wait();
            });
        }
        Err(e) => tracing::debug!("desktop notification not sent: {e}"),
    }
}
