//! Approval broker: in-client (elicitation hook), out-of-band over the control socket, expiry.
//!
//!
//! `LocalBroker::request` by tier:
//! - `InClient`: ask the elicitor; if there is none, or it can't answer, fall back to
//!   `OutOfBand`.
//! - `OutOfBand`: persist the request, push `ServerMsg::NewRequest` to every control client,
//!   wait for a `Decide` or for `approval_timeout_secs` (then `Expired`).
//! - `TwoPerson`: like out-of-band, but it needs two decisions from distinct `by`. Locally that
//!   is rarely possible, so on timeout with fewer approvals it is denied with
//!   "needs Teams (two approvers)" (Teams' RelayBroker is the real T3 path).

use super::ipc::{ClientMsg, ServerMsg, IPC_VERSION};
use super::store::AiStore;
use super::types::{AiEvent, ApprovalBroker, ApprovalOutcome, ApprovalRequest, Tier};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, oneshot};

/// Asks the human through the agent's own client (MCP elicitation, tier T1).
/// Implemented by the MCP layer; `Ok(None)` = the client can't elicit.
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
    pub on_behalf_of: String,
    /// Hosts in the run's scope when it started.
    pub hosts: Vec<String>,
}

/// A request waiting for a human.
struct Pending {
    req: ApprovalRequest,
    /// Distinct approvers so far (two-person needs two).
    approvers: Vec<String>,
    first_grant: Option<super::types::Grant>,
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

    /// Turn the best-effort desktop notification on or off (on by default).
    pub fn with_notifications(mut self, on: bool) -> Self {
        self.notifications = on;
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
                entry.approvers.push(by);
                if entry.first_grant.is_none() {
                    entry.first_grant = grant;
                }
                let needed = if entry.req.tier == Tier::TwoPerson {
                    2
                } else {
                    1
                };
                if entry.approvers.len() < needed {
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

    /// Persist the outcome and store the grant of an approval.
    fn finish(&self, request_id: &str, outcome: ApprovalOutcome) -> ApprovalOutcome {
        let outcome = match outcome {
            ApprovalOutcome::Approved {
                by,
                channel,
                grant: Some(mut g),
            } => {
                if g.grant_id.is_empty() {
                    g.grant_id = uuid::Uuid::new_v4().to_string();
                }
                if let Err(e) = self.store.add_grant(&g) {
                    tracing::warn!("could not store grant: {e}");
                }
                ApprovalOutcome::Approved {
                    by,
                    channel,
                    grant: Some(g),
                }
            }
            other => other,
        };
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

    async fn out_of_band(&self, req: ApprovalRequest) -> anyhow::Result<ApprovalOutcome> {
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

        match tokio::time::timeout(Duration::from_secs(self.timeout_secs), &mut rx).await {
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

#[async_trait]
impl ApprovalBroker for LocalBroker {
    async fn request(&self, mut req: ApprovalRequest) -> anyhow::Result<ApprovalOutcome> {
        if req.tier == Tier::InClient {
            if let Some(e) = &self.elicitor {
                let wait = Duration::from_secs(self.timeout_secs);
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
        self.out_of_band(req).await
    }
}

/// One line for notifications.
fn short_subject(req: &ApprovalRequest) -> String {
    use super::types::ApprovalSubject::*;
    match &req.subject {
        Call { call, .. } => match (&call.command, &call.path) {
            (Some(c), _) => format!("run `{}`", truncate(c, 80)),
            (None, Some(p)) => format!("change {}", truncate(p, 80)),
            _ => format!("use {:?}", call.tool),
        },
        Plan { plan, .. } => format!("apply the change '{}'", truncate(&plan.title, 80)),
        Access { reason, .. } => format!("widen its access: {}", truncate(reason, 80)),
    }
}

fn truncate(s: &str, max: usize) -> String {
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

// ---------------------------------------------------------------- control socket

/// Serve `<ai_dir>/runs/<run_id>.sock` (protocol in `ipc.rs`) until `stop` fires or a client
/// sends `Stop`.
///
/// Returns `Some(reason)` when a client asked to stop (the caller ends the run), `None` when
/// the `stop` watch fired. The socket file is removed on return.
#[cfg(unix)]
pub async fn serve_control(
    socket: PathBuf,
    run: RunInfo,
    broker: Arc<LocalBroker>,
    events: broadcast::Sender<AiEvent>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<Option<String>> {
    use anyhow::Context;
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixListener;

    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        crate::config::enforce_secure_dir(dir);
    }
    // A leftover file from a crashed run would make bind fail.
    if socket.exists() {
        let _ = std::fs::remove_file(&socket);
    }
    let listener =
        UnixListener::bind(&socket).with_context(|| format!("bind {}", socket.display()))?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;

    let (client_stop_tx, mut client_stop_rx) = tokio::sync::watch::channel(None::<String>);
    let client_stop_tx = Arc::new(client_stop_tx);
    let (done_tx, done_rx) = tokio::sync::watch::channel(false);

    let result = loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        tokio::spawn(handle_conn(
                            stream,
                            run.clone(),
                            broker.clone(),
                            events.subscribe(),
                            client_stop_tx.clone(),
                            done_rx.clone(),
                        ));
                    }
                    Err(e) => tracing::warn!("control socket accept failed: {e}"),
                }
            }
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break None;
                }
            }
            changed = client_stop_rx.changed() => {
                if changed.is_ok() {
                    break client_stop_rx.borrow().clone();
                }
            }
        }
    };
    let _ = done_tx.send(true);
    drop(listener);
    let _ = std::fs::remove_file(&socket);
    Ok(result)
}

#[cfg(not(unix))]
pub async fn serve_control(
    socket: PathBuf,
    run: RunInfo,
    broker: Arc<LocalBroker>,
    events: broadcast::Sender<AiEvent>,
    stop: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<Option<String>> {
    let _ = (socket, run, broker, events, stop);
    anyhow::bail!("the AI control socket needs a Unix platform")
}

#[cfg(unix)]
async fn send_msg(
    wr: &mut tokio::net::unix::OwnedWriteHalf,
    msg: &ServerMsg,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut line = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    line.push(b'\n');
    wr.write_all(&line).await
}

#[cfg(unix)]
async fn handle_conn(
    stream: tokio::net::UnixStream,
    run: RunInfo,
    broker: Arc<LocalBroker>,
    mut events: broadcast::Receiver<AiEvent>,
    client_stop: Arc<tokio::sync::watch::Sender<Option<String>>>,
    mut done: tokio::sync::watch::Receiver<bool>,
) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let (rd, mut wr) = stream.into_split();
    let mut lines = BufReader::new(rd).lines();

    // Handshake: the first message must be a Hello with our version.
    let first = tokio::time::timeout(Duration::from_secs(10), lines.next_line()).await;
    let hello = match first {
        Ok(Ok(Some(l))) => serde_json::from_str::<ClientMsg>(&l).ok(),
        _ => None,
    };
    match hello {
        Some(ClientMsg::Hello { version, client }) if version == IPC_VERSION => {
            tracing::debug!("control client connected: {client}");
            let reply = ServerMsg::Hello {
                version: IPC_VERSION,
                run_id: run.run_id.clone(),
                client: broker.client(),
                policy: run.policy.clone(),
                started_at: run.started_at.clone(),
                on_behalf_of: run.on_behalf_of.clone(),
                hosts: run.hosts.clone(),
            };
            if send_msg(&mut wr, &reply).await.is_err() {
                return;
            }
        }
        Some(ClientMsg::Hello { version, .. }) => {
            let _ = send_msg(
                &mut wr,
                &ServerMsg::Error {
                    message: format!(
                        "protocol version {version} not supported (server speaks {IPC_VERSION})"
                    ),
                },
            )
            .await;
            return;
        }
        _ => {
            let _ = send_msg(
                &mut wr,
                &ServerMsg::Error {
                    message: "expected a hello message first".into(),
                },
            )
            .await;
            return;
        }
    }

    let mut subscribed = false;
    let mut new_requests = broker.subscribe_requests();
    loop {
        let reply: Option<ServerMsg> = tokio::select! {
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(l)) => l,
                    _ => break,
                };
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<ClientMsg>(&line) {
                    Ok(msg) => handle_msg(msg, &broker, &client_stop, &mut subscribed),
                    Err(e) => Some(ServerMsg::Error { message: format!("bad message: {e}") }),
                }
            }
            ev = events.recv(), if subscribed => match ev {
                Ok(event) => Some(ServerMsg::Event { event }),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!("control client missed {n} events");
                    None
                }
                Err(broadcast::error::RecvError::Closed) => None,
            },
            req = new_requests.recv() => match req {
                Ok(request) => Some(ServerMsg::NewRequest { request }),
                Err(_) => None,
            },
            _ = done.changed() => break,
        };
        if let Some(r) = reply {
            if send_msg(&mut wr, &r).await.is_err() {
                break;
            }
        }
    }
}

#[cfg(unix)]
fn handle_msg(
    msg: ClientMsg,
    broker: &LocalBroker,
    client_stop: &tokio::sync::watch::Sender<Option<String>>,
    subscribed: &mut bool,
) -> Option<ServerMsg> {
    Some(match msg {
        ClientMsg::Hello { .. } => ServerMsg::Error {
            message: "already greeted".into(),
        },
        ClientMsg::Subscribe => {
            *subscribed = true;
            ServerMsg::Ok
        }
        ClientMsg::ListPending => ServerMsg::Pending {
            requests: broker.pending_requests(),
        },
        ClientMsg::ListGrants => {
            match broker
                .store()
                .active_grants(Some(&broker.run_info().run_id))
            {
                Ok(grants) => ServerMsg::Grants { grants },
                Err(e) => ServerMsg::Error {
                    message: format!("could not read grants: {e}"),
                },
            }
        }
        ClientMsg::Decide {
            request_id,
            outcome,
        } => match broker.decide(&request_id, outcome) {
            Ok(()) => ServerMsg::Ok,
            Err(e) => ServerMsg::Error {
                message: e.to_string(),
            },
        },
        ClientMsg::RevokeGrant { grant_id } => match broker.store().revoke_grant(&grant_id) {
            Ok(true) => ServerMsg::Ok,
            Ok(false) => ServerMsg::Error {
                message: format!("no active grant '{grant_id}'"),
            },
            Err(e) => ServerMsg::Error {
                message: e.to_string(),
            },
        },
        ClientMsg::Stop { reason } => {
            let _ = client_stop.send(Some(reason));
            ServerMsg::Ok
        }
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::services::ai::types::{ApprovalSubject, Grant, Scope, ToolName};

    pub(crate) fn run_info() -> RunInfo {
        RunInfo {
            on_behalf_of: "ana".into(),
            hosts: vec!["web-1".into()],
            run_id: "run1".into(),
            client: "test-client".into(),
            policy: "default".into(),
            started_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub(crate) fn request(id: &str, tier: Tier) -> ApprovalRequest {
        ApprovalRequest {
            request_id: id.into(),
            run_id: "run1".into(),
            client: "test-client".into(),
            tier,
            subject: ApprovalSubject::Access {
                scope: Scope::default(),
                reason: "need logs".into(),
                duration: "30m".into(),
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            expires_at: (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
            reason: String::new(),
        }
    }

    pub(crate) fn broker(
        timeout: u64,
        el: Option<Arc<dyn Elicitor>>,
    ) -> (tempfile::TempDir, Arc<LocalBroker>) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
        let b = LocalBroker::new(store, run_info(), el, timeout).with_notifications(false);
        (tmp, Arc::new(b))
    }

    fn approve(by: &str) -> ApprovalOutcome {
        ApprovalOutcome::Approved {
            by: by.into(),
            channel: "cli".into(),
            grant: None,
        }
    }

    /// Wait until the broker has `n` pending requests.
    pub(crate) async fn wait_pending(b: &LocalBroker, n: usize) {
        for _ in 0..200 {
            if b.pending_requests().len() >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("request never became pending");
    }

    struct FixedElicitor(Option<bool>);
    #[async_trait]
    impl Elicitor for FixedElicitor {
        async fn confirm(&self, _: &ApprovalRequest) -> anyhow::Result<Option<bool>> {
            Ok(self.0)
        }
    }
    struct FailingElicitor;
    #[async_trait]
    impl Elicitor for FailingElicitor {
        async fn confirm(&self, _: &ApprovalRequest) -> anyhow::Result<Option<bool>> {
            anyhow::bail!("client went away")
        }
    }

    #[tokio::test]
    async fn timeout_expires_the_request() {
        let (_t, b) = broker(0, None);
        let out = b.request(request("q1", Tier::OutOfBand)).await.unwrap();
        assert_eq!(out, ApprovalOutcome::Expired);
        assert_eq!(
            b.store().request_status("q1").unwrap().as_deref(),
            Some("expired")
        );
        assert!(b.pending_requests().is_empty());
        // a late decision is refused
        assert!(b.decide("q1", approve("alice")).is_err());
    }

    #[tokio::test]
    async fn out_of_band_approval() {
        let (_t, b) = broker(30, None);
        let b2 = b.clone();
        let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
        wait_pending(&b, 1).await;
        assert_eq!(
            b.store().request_status("q1").unwrap().as_deref(),
            Some("pending")
        );
        b.decide("q1", approve("alice")).unwrap();
        let out = h.await.unwrap().unwrap();
        assert_eq!(out, approve("alice"));
        assert_eq!(
            b.store().request_status("q1").unwrap().as_deref(),
            Some("approved")
        );
        assert!(b.pending_requests().is_empty());
    }

    #[tokio::test]
    async fn out_of_band_denial_keeps_reason() {
        let (_t, b) = broker(30, None);
        let b2 = b.clone();
        let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
        wait_pending(&b, 1).await;
        let denied = ApprovalOutcome::Denied {
            by: "alice".into(),
            reason: Some("not now".into()),
        };
        b.decide("q1", denied.clone()).unwrap();
        assert_eq!(h.await.unwrap().unwrap(), denied);
        assert_eq!(b.store().get_outcome("q1").unwrap(), Some(denied));
    }

    #[tokio::test]
    async fn approval_with_grant_stores_the_grant() {
        let (_t, b) = broker(30, None);
        let b2 = b.clone();
        let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
        wait_pending(&b, 1).await;
        let grant = Grant {
            grant_id: String::new(),
            run_id: Some("run1".into()),
            tool: ToolName::Service,
            hosts: "web-*".into(),
            target: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339(),
        };
        b.decide(
            "q1",
            ApprovalOutcome::Approved {
                by: "alice".into(),
                channel: "cli".into(),
                grant: Some(grant),
            },
        )
        .unwrap();
        match h.await.unwrap().unwrap() {
            ApprovalOutcome::Approved { grant: Some(g), .. } => assert!(!g.grant_id.is_empty()),
            other => panic!("{other:?}"),
        }
        let stored = b.store().active_grants(Some("run1")).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].hosts, "web-*");
    }

    #[tokio::test]
    async fn decide_unknown_or_expired_outcome_errors() {
        let (_t, b) = broker(30, None);
        assert!(b.decide("nope", approve("a")).is_err());
        let b2 = b.clone();
        let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
        wait_pending(&b, 1).await;
        assert!(b.decide("q1", ApprovalOutcome::Expired).is_err());
        b.decide("q1", approve("a")).unwrap();
        h.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn two_person_needs_two_distinct_approvers() {
        let (_t, b) = broker(30, None);
        let b2 = b.clone();
        let h = tokio::spawn(async move { b2.request(request("q1", Tier::TwoPerson)).await });
        wait_pending(&b, 1).await;
        b.decide("q1", approve("alice")).unwrap();
        assert_eq!(
            b.pending_requests().len(),
            1,
            "still waiting for a second person"
        );
        assert!(
            b.decide("q1", approve("alice")).is_err(),
            "same person twice"
        );
        b.decide("q1", approve("bob")).unwrap();
        match h.await.unwrap().unwrap() {
            ApprovalOutcome::Approved { by, .. } => assert_eq!(by, "alice + bob"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn two_person_denial_by_one_is_final() {
        let (_t, b) = broker(30, None);
        let b2 = b.clone();
        let h = tokio::spawn(async move { b2.request(request("q1", Tier::TwoPerson)).await });
        wait_pending(&b, 1).await;
        b.decide("q1", approve("alice")).unwrap();
        b.decide(
            "q1",
            ApprovalOutcome::Denied {
                by: "bob".into(),
                reason: None,
            },
        )
        .unwrap();
        assert!(matches!(
            h.await.unwrap().unwrap(),
            ApprovalOutcome::Denied { .. }
        ));
    }

    #[tokio::test]
    async fn two_person_alone_is_denied_needs_teams() {
        let (_t, b) = broker(0, None);
        let out = b.request(request("q1", Tier::TwoPerson)).await.unwrap();
        match out {
            ApprovalOutcome::Denied {
                reason: Some(r), ..
            } => {
                assert!(r.contains("needs Teams (two approvers)"), "{r}")
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn in_client_uses_the_elicitor() {
        let (_t, b) = broker(30, Some(Arc::new(FixedElicitor(Some(true)))));
        let out = b.request(request("q1", Tier::InClient)).await.unwrap();
        match out {
            ApprovalOutcome::Approved { channel, .. } => {
                assert_eq!(channel, "confirmed-in-client")
            }
            other => panic!("{other:?}"),
        }
        let (_t, b) = broker(30, Some(Arc::new(FixedElicitor(Some(false)))));
        let out = b.request(request("q2", Tier::InClient)).await.unwrap();
        assert!(matches!(out, ApprovalOutcome::Denied { .. }));
    }

    #[tokio::test]
    async fn in_client_falls_back_to_out_of_band() {
        for el in [
            None,
            Some(Arc::new(FixedElicitor(None)) as Arc<dyn Elicitor>),
            Some(Arc::new(FailingElicitor) as Arc<dyn Elicitor>),
        ] {
            let (_t, b) = broker(0, el);
            let out = b.request(request("q1", Tier::InClient)).await.unwrap();
            // Nobody answers the out-of-band request within 0s.
            assert_eq!(out, ApprovalOutcome::Expired);
            assert!(
                b.store().request_status("q1").unwrap().is_some(),
                "was persisted"
            );
        }
    }

    #[tokio::test]
    async fn new_requests_are_broadcast() {
        let (_t, b) = broker(30, None);
        let mut rx = b.subscribe_requests();
        let b2 = b.clone();
        let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
        let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.request_id, "q1");
        b.decide("q1", approve("a")).unwrap();
        h.await.unwrap().unwrap();
    }

    #[test]
    fn short_subject_is_one_line() {
        let r = request("q", Tier::OutOfBand);
        assert!(short_subject(&r).contains("need logs"));
        assert_eq!(truncate("abcdef", 3), "abc...");
        assert_eq!(truncate("ab", 3), "ab");
    }
}
