//! bssh AI: the desktop side of the run sockets.
//!
//! Every `bssh ai mcp` process binds `<ai_dir>/runs/<run_id>.sock` (see
//! `services::ai::ipc`). A background task lists that directory, connects to
//! each socket (Hello, Subscribe, ListPending) and keeps a live picture of the
//! runs in [`AiHub`]. The frontend gets three Tauri events:
//!
//! * `ai://runs`: the full list of [`RunSummary`], after any change;
//! * `ai://event`: one [`StoredEvent`] of a run (`{ run_id, seq, at, event }`);
//! * `ai://request`: a new approval request (`{ run_id, request }`), which also
//!   raises a native notification and brings the main window forward.
//!
//! The approval dialog is the trusted channel (design 6.3, tier 2): the agent's
//! process can't reach it, and it shows the exact command, not a summary.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::services::ai::ipc::{ClientMsg, ServerMsg};
use bayesian_ssh::services::ai::types::{
    AiEvent, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Decision, Grant, Policy, RiskTag,
    ToolName,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

/// Approval channel recorded in the audit.
const CHANNEL: &str = "desktop";
/// Events kept per run for the timeline.
const MAX_EVENTS_PER_RUN: usize = 1000;
/// Runs that are no longer connected, newest first, kept in the list.
const MAX_CLOSED_RUNS: usize = 20;
/// Longest grant a click can create.
const MAX_GRANT_MINUTES: u32 = 8 * 60;
const MAX_DENY_REASON_CHARS: usize = 500;

// ---------------------------------------------------------------- payloads

/// One event of a run with the order it arrived in and when.
#[derive(Debug, Clone, Serialize)]
pub struct StoredEvent {
    /// 1-based, per run. The frontend drops anything it has already seen.
    pub seq: u64,
    /// RFC 3339, when the desktop received it.
    pub at: String,
    pub event: AiEvent,
}

/// What the AI view shows for one run.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    /// MCP client name and version, self-reported by the agent.
    pub client: String,
    pub policy: String,
    pub started_at: String,
    pub on_behalf_of: Option<String>,
    /// Tool calls seen since the desktop connected.
    pub calls: u32,
    pub denied: u32,
    pub pending: Vec<ApprovalRequest>,
    pub grants: Vec<Grant>,
    /// The run's socket is open: Stop and Decide work.
    pub connected: bool,
    /// Set by the run's `RunEnded` event.
    pub ended: Option<String>,
}

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

/// `ai_settings`: the read-only part of `config.ai`.
#[derive(Debug, Serialize)]
pub struct AiSettingsView {
    pub policies: BTreeMap<String, Policy>,
    pub record_output: bool,
    pub approval_timeout_secs: u64,
}

// ---------------------------------------------------------------- sink

/// Where the hub reports changes. Tauri in the app, a recorder in tests.
pub trait HubSink: Send + Sync + 'static {
    fn runs_changed(&self, runs: &[RunSummary]);
    fn event(&self, run_id: &str, event: &StoredEvent);
    /// A request the user has not seen yet.
    fn request(&self, run_id: &str, client: &str, request: &ApprovalRequest);
    fn error(&self, run_id: &str, message: &str);
}

struct TauriSink(AppHandle);

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
fn notification_body(client: &str, request: &ApprovalRequest) -> String {
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

// ---------------------------------------------------------------- hub

struct Conn {
    id: u64,
    tx: UnboundedSender<ClientMsg>,
}

struct RunState {
    client: String,
    policy: String,
    started_at: String,
    on_behalf_of: Option<String>,
    conn: Option<Conn>,
    pending: Vec<ApprovalRequest>,
    /// Requests already announced, so a reconnect doesn't notify twice.
    seen: HashSet<String>,
    events: VecDeque<StoredEvent>,
    next_seq: u64,
    grants: Vec<Grant>,
    calls: u32,
    denied: u32,
    ended: Option<String>,
    /// Ordering for pruning closed runs.
    touched: u64,
}

impl RunState {
    fn summary(&self, run_id: &str) -> RunSummary {
        RunSummary {
            run_id: run_id.to_string(),
            client: self.client.clone(),
            policy: self.policy.clone(),
            started_at: self.started_at.clone(),
            on_behalf_of: self.on_behalf_of.clone(),
            calls: self.calls,
            denied: self.denied,
            pending: self.pending.clone(),
            grants: self.grants.clone(),
            connected: self.conn.is_some(),
            ended: self.ended.clone(),
        }
    }
}

#[derive(Default)]
struct HubState {
    runs: BTreeMap<String, RunState>,
    next_conn: u64,
    tick: u64,
}

/// Everything to tell the sink once the lock is released.
#[derive(Default)]
struct Effects {
    changed: bool,
    events: Vec<(String, StoredEvent)>,
    requests: Vec<(String, String, ApprovalRequest)>,
    errors: Vec<(String, String)>,
}

pub struct AiHub {
    state: Mutex<HubState>,
    sink: Arc<dyn HubSink>,
}

impl AiHub {
    pub fn new(sink: Arc<dyn HubSink>) -> Self {
        Self {
            state: Mutex::new(HubState::default()),
            sink,
        }
    }

    /// A panic while the lock was held must not take the GUI down.
    fn lock(&self) -> MutexGuard<'_, HubState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn apply(&self, f: impl FnOnce(&mut HubState, &mut Effects)) {
        let mut fx = Effects::default();
        let runs = {
            let mut st = self.lock();
            f(&mut st, &mut fx);
            prune_closed(&mut st);
            fx.changed.then(|| summaries(&st))
        };
        for (run_id, event) in &fx.events {
            self.sink.event(run_id, event);
        }
        for (run_id, client, request) in &fx.requests {
            self.sink.request(run_id, client, request);
        }
        for (run_id, message) in &fx.errors {
            self.sink.error(run_id, message);
        }
        if let Some(runs) = runs {
            self.sink.runs_changed(&runs);
        }
    }

    pub fn list_runs(&self) -> Vec<RunSummary> {
        summaries(&self.lock())
    }

    pub fn run_events(&self, run_id: &str) -> Vec<StoredEvent> {
        self.lock()
            .runs
            .get(run_id)
            .map(|r| r.events.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Registers (or re-registers after a reconnect) a run. Returns the
    /// connection id to pass to [`AiHub::detach`].
    fn attach(&self, hello: RunHello, tx: UnboundedSender<ClientMsg>) -> u64 {
        let mut conn_id = 0;
        self.apply(|st, fx| {
            st.next_conn += 1;
            conn_id = st.next_conn;
            st.tick += 1;
            let tick = st.tick;
            let run = st
                .runs
                .entry(hello.run_id.clone())
                .or_insert_with(|| RunState {
                    client: String::new(),
                    policy: String::new(),
                    started_at: String::new(),
                    on_behalf_of: None,
                    conn: None,
                    pending: Vec::new(),
                    seen: HashSet::new(),
                    events: VecDeque::new(),
                    next_seq: 1,
                    grants: Vec::new(),
                    calls: 0,
                    denied: 0,
                    ended: None,
                    touched: tick,
                });
            run.client = hello.client;
            run.policy = hello.policy;
            run.started_at = hello.started_at;
            run.touched = tick;
            run.conn = Some(Conn { id: conn_id, tx });
            fx.changed = true;
        });
        conn_id
    }

    /// The run's socket closed. A newer connection to the same run is kept.
    fn detach(&self, run_id: &str, conn_id: u64) {
        self.apply(|st, fx| {
            st.tick += 1;
            let tick = st.tick;
            if let Some(run) = st.runs.get_mut(run_id) {
                if run.conn.as_ref().is_some_and(|c| c.id == conn_id) {
                    run.conn = None;
                    // Nobody can answer these any more.
                    run.pending.clear();
                    run.touched = tick;
                    fx.changed = true;
                }
            }
        });
    }

    fn on_message(&self, run_id: &str, msg: ServerMsg) {
        self.apply(|st, fx| {
            let Some(run) = st.runs.get_mut(run_id) else {
                return;
            };
            match msg {
                ServerMsg::Hello { .. } | ServerMsg::Ok => {}
                ServerMsg::Pending { requests } => {
                    run.pending = requests;
                    for req in &run.pending {
                        if run.seen.insert(req.request_id.clone()) {
                            fx.requests
                                .push((run_id.to_string(), run.client.clone(), req.clone()));
                        }
                    }
                    fx.changed = true;
                }
                ServerMsg::NewRequest { request } => {
                    if !run
                        .pending
                        .iter()
                        .any(|r| r.request_id == request.request_id)
                    {
                        run.pending.push(request.clone());
                    }
                    if run.seen.insert(request.request_id.clone()) {
                        fx.requests
                            .push((run_id.to_string(), run.client.clone(), request));
                    }
                    fx.changed = true;
                }
                ServerMsg::Event { event } => {
                    match &event {
                        AiEvent::RunStarted {
                            client,
                            on_behalf_of,
                            policy,
                            ..
                        } => {
                            run.client = client.clone();
                            run.policy = policy.clone();
                            run.on_behalf_of = Some(on_behalf_of.clone());
                        }
                        AiEvent::Call { decision, .. } => {
                            run.calls += 1;
                            if matches!(decision, Decision::Deny { .. }) {
                                run.denied += 1;
                            }
                        }
                        AiEvent::Approval { request_id, .. } => {
                            run.pending.retain(|r| &r.request_id != request_id);
                        }
                        AiEvent::RunEnded { reason, .. } => {
                            run.ended = Some(reason.clone());
                            run.pending.clear();
                        }
                        _ => {}
                    }
                    let stored = StoredEvent {
                        seq: run.next_seq,
                        at: chrono::Utc::now().to_rfc3339(),
                        event,
                    };
                    run.next_seq += 1;
                    run.events.push_back(stored.clone());
                    while run.events.len() > MAX_EVENTS_PER_RUN {
                        run.events.pop_front();
                    }
                    fx.events.push((run_id.to_string(), stored));
                    fx.changed = true;
                }
                ServerMsg::Grants { grants } => {
                    run.grants = grants;
                    fx.changed = true;
                }
                ServerMsg::Error { message } => {
                    fx.errors.push((run_id.to_string(), message));
                }
            }
        });
    }

    fn sender(&self, run_id: &str) -> Result<UnboundedSender<ClientMsg>, String> {
        self.lock()
            .runs
            .get(run_id)
            .and_then(|r| r.conn.as_ref().map(|c| c.tx.clone()))
            .ok_or_else(|| "The run is not connected any more.".to_string())
    }

    pub fn decide(
        &self,
        run_id: &str,
        request_id: &str,
        approve: bool,
        reason: Option<String>,
        grant_minutes: Option<u32>,
    ) -> Result<(), String> {
        let (tx, request) = {
            let st = self.lock();
            let run = st.runs.get(run_id).ok_or("Unknown run.")?;
            let tx = run
                .conn
                .as_ref()
                .map(|c| c.tx.clone())
                .ok_or("The run is not connected any more.")?;
            let request = run
                .pending
                .iter()
                .find(|r| r.request_id == request_id)
                .cloned()
                .ok_or("This request is no longer pending.")?;
            (tx, request)
        };
        let by = local_user();
        let outcome = if approve {
            let grant = match grant_minutes.filter(|m| *m > 0) {
                Some(minutes) => Some(build_grant(run_id, &request, minutes, chrono::Utc::now())?),
                None => None,
            };
            ApprovalOutcome::Approved {
                by,
                channel: CHANNEL.to_string(),
                grant,
            }
        } else {
            let reason = reason
                .map(|r| {
                    r.trim()
                        .chars()
                        .take(MAX_DENY_REASON_CHARS)
                        .collect::<String>()
                })
                .filter(|r| !r.is_empty());
            ApprovalOutcome::Denied { by, reason }
        };
        tx.send(ClientMsg::Decide {
            request_id: request_id.to_string(),
            outcome,
        })
        .map_err(|_| "The run is not connected any more.".to_string())?;
        // The server answers with an Approval event; this refreshes the list
        // in case that event got lost.
        let _ = tx.send(ClientMsg::ListPending);
        Ok(())
    }

    pub fn stop(&self, run_id: &str) -> Result<(), String> {
        self.sender(run_id)?
            .send(ClientMsg::Stop {
                reason: "stopped from the desktop app".to_string(),
            })
            .map_err(|_| "The run is not connected any more.".to_string())
    }

    pub fn revoke_grant(&self, run_id: &str, grant_id: &str) -> Result<(), String> {
        self.sender(run_id)?
            .send(ClientMsg::RevokeGrant {
                grant_id: grant_id.to_string(),
            })
            .map_err(|_| "The run is not connected any more.".to_string())
    }
}

fn summaries(st: &HubState) -> Vec<RunSummary> {
    let mut runs: Vec<(&String, &RunState)> = st.runs.iter().collect();
    // Open runs first, then the most recently started.
    runs.sort_by(|(_, a), (_, b)| {
        b.conn
            .is_some()
            .cmp(&a.conn.is_some())
            .then_with(|| b.started_at.cmp(&a.started_at))
    });
    runs.into_iter().map(|(id, r)| r.summary(id)).collect()
}

/// Keeps the newest `MAX_CLOSED_RUNS` runs that have no connection.
fn prune_closed(st: &mut HubState) {
    let mut closed: Vec<(u64, String)> = st
        .runs
        .iter()
        .filter(|(_, r)| r.conn.is_none())
        .map(|(id, r)| (r.touched, id.clone()))
        .collect();
    if closed.len() <= MAX_CLOSED_RUNS {
        return;
    }
    closed.sort_unstable_by_key(|(touched, _)| std::cmp::Reverse(*touched));
    for (_, id) in closed.into_iter().skip(MAX_CLOSED_RUNS) {
        st.runs.remove(&id);
    }
}

/// `by` of an approval: the local account.
fn local_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "desktop-user".to_string())
}

/// "Approve and allow for N minutes": a grant for this run, this tool and this
/// host only. Refused where a repeat prompt is the point: destructive,
/// privileged, exfiltrating or unanalysed commands, plans and access requests.
fn build_grant(
    run_id: &str,
    request: &ApprovalRequest,
    minutes: u32,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Grant, String> {
    let ApprovalSubject::Call { call, analysis } = &request.subject else {
        return Err("A grant covers a single call, not a plan or an access request.".into());
    };
    let host = call
        .host
        .as_deref()
        .filter(|h| !h.is_empty() && !h.contains(['*', '?', '[', ']']))
        .ok_or("A grant needs one named host.")?;
    let blocked = |tag: &RiskTag| {
        matches!(
            tag,
            RiskTag::Destructive | RiskTag::Privilege | RiskTag::ExfilRisk | RiskTag::Unknown
        )
    };
    if matches!(call.tool, ToolName::Run | ToolName::RunMany) && analysis.is_none() {
        return Err("This command was not analysed, so it can't be granted.".into());
    }
    if analysis
        .as_ref()
        .is_some_and(|a| a.unknown_reason.is_some() || a.tags.iter().any(blocked))
    {
        return Err("Destructive, privileged or unclear commands can't be granted.".into());
    }
    let minutes = minutes.min(MAX_GRANT_MINUTES);
    let target = if call.tool == ToolName::EditFile {
        call.path.clone()
    } else {
        None
    };
    Ok(Grant {
        grant_id: Uuid::new_v4().to_string(),
        run_id: Some(run_id.to_string()),
        tool: call.tool,
        hosts: host.to_string(),
        target,
        expires_at: (now + chrono::Duration::minutes(i64::from(minutes))).to_rfc3339(),
    })
}

struct RunHello {
    run_id: String,
    client: String,
    policy: String,
    started_at: String,
}

// ---------------------------------------------------------------- sockets

#[cfg(unix)]
mod sock {
    use super::*;
    use bayesian_ssh::services::ai::ai_dir;
    use bayesian_ssh::services::ai::ipc::IPC_VERSION;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use tokio::sync::mpsc::unbounded_channel;
    use tokio::task::JoinHandle;

    /// How the desktop introduces itself in `ClientMsg::Hello`.
    const CLIENT_NAME: &str = "bssh-desktop";
    /// How often `<ai_dir>/runs` is listed.
    const SCAN_INTERVAL: Duration = Duration::from_secs(2);
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

    enum Slot {
        /// A session task owns the socket.
        Live(JoinHandle<()>),
        /// Connecting failed: skipped until the socket file changes.
        Dead(Option<SystemTime>),
    }

    /// Tracks which `*.sock` files already have a session.
    #[derive(Default)]
    pub struct Discovery {
        slots: HashMap<PathBuf, Slot>,
    }

    impl Discovery {
        /// One pass over `<ai_dir>/runs`: connects to sockets without a session.
        pub async fn scan(&mut self, hub: &Arc<AiHub>, runs_dir: &Path) {
            let mut found = HashSet::new();
            if let Ok(entries) = std::fs::read_dir(runs_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "sock") {
                        found.insert(path);
                    }
                }
            }
            self.slots.retain(|path, slot| match slot {
                Slot::Live(handle) => !handle.is_finished() || found.contains(path),
                Slot::Dead(_) => found.contains(path),
            });
            for path in found {
                match self.slots.get(&path) {
                    Some(Slot::Live(handle)) if !handle.is_finished() => continue,
                    Some(Slot::Dead(seen)) if *seen == mtime(&path) => continue,
                    _ => {}
                }
                let stream =
                    tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(&path)).await;
                match stream {
                    Ok(Ok(stream)) => {
                        let hub = Arc::clone(hub);
                        let handle = tokio::spawn(session(hub, stream));
                        self.slots.insert(path, Slot::Live(handle));
                    }
                    other => {
                        // Left behind by a process that died, or not ours.
                        tracing::debug!(path = %path.display(), "ai socket not reachable: {other:?}");
                        let seen = mtime(&path);
                        self.slots.insert(path, Slot::Dead(seen));
                    }
                }
            }
        }
    }

    fn mtime(path: &Path) -> Option<SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    fn encode(msg: &ClientMsg) -> Option<Vec<u8>> {
        let mut line = serde_json::to_vec(msg).ok()?;
        line.push(b'\n');
        Some(line)
    }

    /// Speaks the protocol on one connection until it closes.
    async fn session(hub: Arc<AiHub>, stream: UnixStream) {
        let (read, mut write) = stream.into_split();
        let (tx, mut rx) = unbounded_channel::<ClientMsg>();
        let writer = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let Some(line) = encode(&msg) else { continue };
                if write.write_all(&line).await.is_err() || write.flush().await.is_err() {
                    break;
                }
            }
        });

        let _ = tx.send(ClientMsg::Hello {
            version: IPC_VERSION,
            client: CLIENT_NAME.to_string(),
        });
        let _ = tx.send(ClientMsg::Subscribe);
        let _ = tx.send(ClientMsg::ListPending);

        let mut lines = BufReader::new(read).lines();
        let mut attached: Option<(String, u64)> = None;
        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(e) => {
                    tracing::debug!("ai socket read failed: {e}");
                    break;
                }
            };
            let msg = match serde_json::from_str::<ServerMsg>(&line) {
                Ok(msg) => msg,
                Err(e) => {
                    // A newer server may send variants this build doesn't know.
                    tracing::warn!("ignoring unreadable ai socket message: {e}");
                    continue;
                }
            };
            match (&attached, msg) {
                (
                    None,
                    ServerMsg::Hello {
                        version,
                        run_id,
                        client,
                        policy,
                        started_at,
                        ..
                    },
                ) => {
                    if version != IPC_VERSION {
                        tracing::warn!(version, "ai run speaks another protocol version");
                        hub.apply(|_, fx| {
                            fx.errors.push((
                                run_id.clone(),
                                format!(
                                    "Run {run_id} uses protocol version {version}; this app uses {IPC_VERSION}. Update bssh."
                                ),
                            ));
                        });
                        break;
                    }
                    let conn_id = hub.attach(
                        RunHello {
                            run_id: run_id.clone(),
                            client,
                            policy,
                            started_at,
                        },
                        tx.clone(),
                    );
                    attached = Some((run_id, conn_id));
                }
                (Some((run_id, _)), msg) => hub.on_message(run_id, msg),
                (None, _) => tracing::debug!("ai socket message before Hello ignored"),
            }
        }
        if let Some((run_id, conn_id)) = attached {
            hub.detach(&run_id, conn_id);
        }
        writer.abort();
    }

    /// `<ai_dir>/runs` of the active environment. The config is only reloaded
    /// when the environment changes.
    #[derive(Default)]
    struct RunsDirResolver {
        env: Option<String>,
        dir: Option<PathBuf>,
    }

    impl RunsDirResolver {
        fn resolve(&mut self) -> Option<PathBuf> {
            let env = AppConfig::get_active_env();
            if self.env.as_deref() != Some(env.as_str()) || self.dir.is_none() {
                self.dir = AppConfig::load(None).ok().map(|config| {
                    bayesian_ssh::services::ai::control_client::runs_dir(&ai_dir(&config))
                });
                self.env = Some(env);
            }
            self.dir.clone()
        }
    }

    /// Lists the runs directory forever, connecting to new sockets.
    pub async fn discovery_loop(hub: Arc<AiHub>) {
        let mut resolver = RunsDirResolver::default();
        let mut discovery = Discovery::default();
        loop {
            if let Some(dir) = resolver.resolve() {
                discovery.scan(&hub, &dir).await;
            }
            tokio::time::sleep(SCAN_INTERVAL).await;
        }
    }
}

/// Starts the discovery task and manages the hub as Tauri state.
pub fn start(app: &AppHandle) {
    let hub = Arc::new(AiHub::new(Arc::new(TauriSink(app.clone()))));
    app.manage(Arc::clone(&hub));
    #[cfg(unix)]
    tauri::async_runtime::spawn(sock::discovery_loop(hub));
    // The run sockets are Unix sockets: nothing to discover elsewhere.
    #[cfg(not(unix))]
    let _ = hub;
}

// ---------------------------------------------------------------- commands

#[tauri::command]
pub fn ai_list_runs(hub: State<'_, Arc<AiHub>>) -> Vec<RunSummary> {
    hub.list_runs()
}

#[tauri::command]
pub fn ai_run_events(hub: State<'_, Arc<AiHub>>, run_id: String) -> Vec<StoredEvent> {
    hub.run_events(&run_id)
}

#[tauri::command]
pub fn ai_decide(
    hub: State<'_, Arc<AiHub>>,
    run_id: String,
    request_id: String,
    approve: bool,
    reason: Option<String>,
    grant_minutes: Option<u32>,
) -> Result<(), String> {
    hub.decide(&run_id, &request_id, approve, reason, grant_minutes)
}

#[tauri::command]
pub fn ai_stop(hub: State<'_, Arc<AiHub>>, run_id: String) -> Result<(), String> {
    hub.stop(&run_id)
}

#[tauri::command]
pub fn ai_revoke_grant(
    hub: State<'_, Arc<AiHub>>,
    run_id: String,
    grant_id: String,
) -> Result<(), String> {
    hub.revoke_grant(&run_id, &grant_id)
}

/// Read-only view of `config.ai` for the settings page.
#[tauri::command(async)]
pub fn ai_settings() -> Result<AiSettingsView, String> {
    let config = AppConfig::load(None).map_err(|e| e.to_string())?;
    Ok(AiSettingsView {
        policies: config.ai.policies,
        record_output: config.ai.record_output,
        approval_timeout_secs: config.ai.approval_timeout_secs,
    })
}

// ---------------------------------------------------------------- tests

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use bayesian_ssh::services::ai::ipc::IPC_VERSION;
    use bayesian_ssh::services::ai::types::{Analysis, ToolCall};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;
    use tokio::sync::mpsc::unbounded_channel;

    #[derive(Default)]
    struct Recorder {
        runs: Mutex<Vec<Vec<RunSummary>>>,
        events: Mutex<Vec<(String, u64)>>,
        requests: Mutex<Vec<String>>,
        errors: Mutex<Vec<String>>,
        notified: AtomicUsize,
    }

    impl HubSink for Recorder {
        fn runs_changed(&self, runs: &[RunSummary]) {
            self.runs.lock().unwrap().push(runs.to_vec());
        }
        fn event(&self, run_id: &str, event: &StoredEvent) {
            self.events.lock().unwrap().push((run_id.into(), event.seq));
        }
        fn request(&self, _run_id: &str, _client: &str, request: &ApprovalRequest) {
            self.notified.fetch_add(1, Ordering::SeqCst);
            self.requests
                .lock()
                .unwrap()
                .push(request.request_id.clone());
        }
        fn error(&self, _run_id: &str, message: &str) {
            self.errors.lock().unwrap().push(message.into());
        }
    }

    fn call(tool: ToolName, host: Option<&str>, command: Option<&str>) -> ToolCall {
        ToolCall {
            run_id: "run-1".into(),
            call_id: "call-1".into(),
            tool,
            host: host.map(Into::into),
            command: command.map(Into::into),
            path: None,
            args: serde_json::Value::Null,
        }
    }

    fn analysis(tags: &[RiskTag], unknown: Option<&str>) -> Analysis {
        Analysis {
            commands: Vec::new(),
            tags: tags.iter().copied().collect::<BTreeSet<_>>(),
            unknown_reason: unknown.map(Into::into),
        }
    }

    fn request(id: &str, subject: ApprovalSubject) -> ApprovalRequest {
        ApprovalRequest {
            request_id: id.into(),
            run_id: "run-1".into(),
            client: "Claude Code 2.0".into(),
            tier: bayesian_ssh::services::ai::types::Tier::OutOfBand,
            subject,
            created_at: "2026-10-24T10:00:00Z".into(),
            expires_at: "2026-10-24T10:05:00Z".into(),
            reason: String::new(),
        }
    }

    fn call_request(id: &str, tool: ToolName, host: &str, tags: &[RiskTag]) -> ApprovalRequest {
        request(
            id,
            ApprovalSubject::Call {
                call: call(tool, Some(host), Some("systemctl reload nginx")),
                analysis: Some(analysis(tags, None)),
            },
        )
    }

    // -- grants

    #[test]
    fn grant_is_scoped_to_run_tool_and_host() {
        let req = call_request("r1", ToolName::Service, "web-2", &[RiskTag::ServiceChange]);
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-24T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let g = build_grant("run-1", &req, 30, now).unwrap();
        assert_eq!(g.run_id.as_deref(), Some("run-1"));
        assert_eq!(g.tool, ToolName::Service);
        assert_eq!(g.hosts, "web-2");
        assert_eq!(g.target, None);
        assert_eq!(g.expires_at, "2026-10-24T10:30:00+00:00");
    }

    #[test]
    fn grant_for_edit_file_keeps_the_path() {
        let mut c = call(ToolName::EditFile, Some("web-2"), None);
        c.path = Some("/etc/nginx/nginx.conf".into());
        let req = request(
            "r1",
            ApprovalSubject::Call {
                call: c,
                analysis: None,
            },
        );
        let g = build_grant("run-1", &req, 10, chrono::Utc::now()).unwrap();
        assert_eq!(g.target.as_deref(), Some("/etc/nginx/nginx.conf"));
    }

    #[test]
    fn grant_is_capped() {
        let req = call_request("r1", ToolName::Service, "web-2", &[RiskTag::ServiceChange]);
        let now = chrono::Utc::now();
        let g = build_grant("run-1", &req, 100_000, now).unwrap();
        let expires = chrono::DateTime::parse_from_rfc3339(&g.expires_at).unwrap();
        assert_eq!(
            (expires.with_timezone(&chrono::Utc) - now).num_minutes(),
            i64::from(MAX_GRANT_MINUTES)
        );
    }

    #[test]
    fn grant_refused_for_risky_unknown_or_wide_requests() {
        let now = chrono::Utc::now();
        for tag in [
            RiskTag::Destructive,
            RiskTag::Privilege,
            RiskTag::ExfilRisk,
            RiskTag::Unknown,
        ] {
            let req = call_request("r1", ToolName::Run, "web-2", &[tag]);
            assert!(build_grant("run-1", &req, 5, now).is_err(), "{tag:?}");
        }
        // Unanalysed `run`.
        let req = request(
            "r1",
            ApprovalSubject::Call {
                call: call(ToolName::Run, Some("web-2"), Some("ls")),
                analysis: None,
            },
        );
        assert!(build_grant("run-1", &req, 5, now).is_err());
        // Analysis flagged unclear.
        let req = request(
            "r1",
            ApprovalSubject::Call {
                call: call(ToolName::Run, Some("web-2"), Some("eval $x")),
                analysis: Some(analysis(&[RiskTag::Write], Some("eval"))),
            },
        );
        assert!(build_grant("run-1", &req, 5, now).is_err());
        // No host, or a glob in the host name.
        for host in [None, Some("web-*"), Some("")] {
            let req = request(
                "r1",
                ApprovalSubject::Call {
                    call: call(ToolName::Service, host, None),
                    analysis: None,
                },
            );
            assert!(build_grant("run-1", &req, 5, now).is_err(), "{host:?}");
        }
        // Access requests.
        let req = request(
            "r1",
            ApprovalSubject::Access {
                scope: Default::default(),
                reason: "x".into(),
                duration: "1h".into(),
            },
        );
        assert!(build_grant("run-1", &req, 5, now).is_err());
    }

    #[test]
    fn notification_names_the_tool_and_host_but_not_the_reason() {
        let req = call_request("r1", ToolName::Service, "web-2", &[]);
        let body = notification_body("Claude Code 2.0", &req);
        assert_eq!(body, "Claude Code 2.0: Service on web-2");
    }

    // -- hub + socket, against a fake run server

    /// A run server that speaks just enough of `ipc.rs`.
    struct FakeRun {
        /// What the client sent, one JSON value per line.
        received: Arc<Mutex<Vec<serde_json::Value>>>,
        push: UnboundedSender<ServerMsg>,
        _task: tokio::task::JoinHandle<()>,
    }

    fn fake_run(sock: &Path, run_id: &str, version: u32) -> FakeRun {
        let listener = UnixListener::bind(sock).unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let (push, mut push_rx) = unbounded_channel::<ServerMsg>();
        let log = Arc::clone(&received);
        let run_id = run_id.to_string();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();
            loop {
                tokio::select! {
                    line = lines.next_line() => {
                        let Ok(Some(line)) = line else { break };
                        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
                        let ty = value["type"].as_str().unwrap_or("").to_string();
                        log.lock().unwrap().push(value);
                        let reply = match ty.as_str() {
                            "hello" => Some(ServerMsg::Hello {
                                version,
                                run_id: run_id.clone(),
                                client: "Claude Code 2.0".into(),
                                policy: "default".into(),
                                started_at: "2026-10-24T10:00:00Z".into(),
                                on_behalf_of: "ana".into(),
                                hosts: vec!["web-1".into()],
                            }),
                            "subscribe" => Some(ServerMsg::Ok),
                            "list_pending" => Some(ServerMsg::Pending { requests: vec![] }),
                            _ => None,
                        };
                        if let Some(reply) = reply {
                            let mut out = serde_json::to_vec(&reply).unwrap();
                            out.push(b'\n');
                            if write.write_all(&out).await.is_err() { break }
                        }
                    }
                    msg = push_rx.recv() => {
                        let Some(msg) = msg else { break };
                        let mut out = serde_json::to_vec(&msg).unwrap();
                        out.push(b'\n');
                        if write.write_all(&out).await.is_err() { break }
                    }
                }
            }
        });
        FakeRun {
            received,
            push,
            _task: task,
        }
    }

    async fn until(what: &str, mut cond: impl FnMut() -> bool) {
        for _ in 0..200 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {what}");
    }

    fn setup() -> (tempfile::TempDir, PathBuf, Arc<Recorder>, Arc<AiHub>) {
        let dir = tempfile::tempdir().unwrap();
        let runs = dir.path().join("runs");
        std::fs::create_dir_all(&runs).unwrap();
        let rec = Arc::new(Recorder::default());
        let hub = Arc::new(AiHub::new(rec.clone()));
        (dir, runs, rec, hub)
    }

    fn received_types(run: &FakeRun) -> Vec<String> {
        run.received
            .lock()
            .unwrap()
            .iter()
            .map(|v| v["type"].as_str().unwrap_or("").to_string())
            .collect()
    }

    #[tokio::test]
    async fn connects_subscribes_and_streams_events() {
        let (_dir, runs, rec, hub) = setup();
        let run = fake_run(&runs.join("run-1.sock"), "run-1", IPC_VERSION);
        let mut disc = sock::Discovery::default();
        disc.scan(&hub, &runs).await;

        until("run listed", || {
            hub.list_runs().first().is_some_and(|r| r.connected)
        })
        .await;
        let r = &hub.list_runs()[0];
        assert_eq!(r.run_id, "run-1");
        assert_eq!(r.client, "Claude Code 2.0");
        assert_eq!(r.policy, "default");
        until("handshake", || received_types(&run).len() >= 3).await;
        assert_eq!(received_types(&run), ["hello", "subscribe", "list_pending"]);
        assert_eq!(run.received.lock().unwrap()[0]["version"], IPC_VERSION);

        run.push
            .send(ServerMsg::Event {
                event: AiEvent::Call {
                    call: call(ToolName::RunReadonly, Some("web-2"), Some("uptime")),
                    decision: Decision::Deny {
                        reason: "out of scope".into(),
                    },
                },
            })
            .unwrap();
        run.push
            .send(ServerMsg::Event {
                event: AiEvent::Call {
                    call: call(ToolName::RunReadonly, Some("web-2"), Some("uptime")),
                    decision: Decision::Allow {
                        reason: "read".into(),
                    },
                },
            })
            .unwrap();
        until("events", || hub.run_events("run-1").len() == 2).await;
        let r = &hub.list_runs()[0];
        assert_eq!((r.calls, r.denied), (2, 1));
        let seqs: Vec<u64> = hub.run_events("run-1").iter().map(|e| e.seq).collect();
        assert_eq!(seqs, [1, 2]);
        assert_eq!(rec.events.lock().unwrap().len(), 2);
        // A second scan doesn't open a second session.
        disc.scan(&hub, &runs).await;
        assert_eq!(hub.list_runs().len(), 1);
    }

    #[tokio::test]
    async fn new_request_notifies_once_and_decision_reaches_the_run() {
        let (_dir, runs, rec, hub) = setup();
        let run = fake_run(&runs.join("run-1.sock"), "run-1", IPC_VERSION);
        let mut disc = sock::Discovery::default();
        disc.scan(&hub, &runs).await;
        until("connected", || {
            hub.list_runs().first().is_some_and(|r| r.connected)
        })
        .await;

        let req = call_request(
            "req-1",
            ToolName::Service,
            "web-2",
            &[RiskTag::ServiceChange],
        );
        run.push
            .send(ServerMsg::NewRequest {
                request: req.clone(),
            })
            .unwrap();
        // The same request again (Pending after a refresh) must not notify twice.
        run.push
            .send(ServerMsg::Pending {
                requests: vec![req.clone()],
            })
            .unwrap();
        until("pending", || {
            hub.list_runs()[0].pending.len() == 1 && rec.requests.lock().unwrap().len() == 1
        })
        .await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(rec.notified.load(Ordering::SeqCst), 1);

        hub.decide("run-1", "req-1", true, None, Some(15)).unwrap();
        until("decide sent", || {
            received_types(&run).contains(&"decide".to_string())
        })
        .await;
        let sent = run
            .received
            .lock()
            .unwrap()
            .iter()
            .find(|v| v["type"] == "decide")
            .cloned()
            .unwrap();
        assert_eq!(sent["request_id"], "req-1");
        assert_eq!(sent["outcome"]["outcome"], "approved");
        assert_eq!(sent["outcome"]["channel"], "desktop");
        assert_eq!(sent["outcome"]["grant"]["tool"], "service");
        assert_eq!(sent["outcome"]["grant"]["hosts"], "web-2");
        assert_eq!(sent["outcome"]["grant"]["run_id"], "run-1");

        // The server's Approval event clears the request.
        run.push
            .send(ServerMsg::Event {
                event: AiEvent::Approval {
                    request_id: "req-1".into(),
                    run_id: "run-1".into(),
                    outcome: ApprovalOutcome::Expired,
                },
            })
            .unwrap();
        until("pending cleared", || hub.list_runs()[0].pending.is_empty()).await;
    }

    #[tokio::test]
    async fn deny_carries_a_trimmed_reason_and_stop_is_sent() {
        let (_dir, runs, _rec, hub) = setup();
        let run = fake_run(&runs.join("run-1.sock"), "run-1", IPC_VERSION);
        let mut disc = sock::Discovery::default();
        disc.scan(&hub, &runs).await;
        until("connected", || {
            hub.list_runs().first().is_some_and(|r| r.connected)
        })
        .await;
        run.push
            .send(ServerMsg::NewRequest {
                request: call_request("req-1", ToolName::Service, "web-2", &[]),
            })
            .unwrap();
        until("pending", || hub.list_runs()[0].pending.len() == 1).await;

        hub.decide(
            "run-1",
            "req-1",
            false,
            Some("  not now \n".into()),
            Some(30),
        )
        .unwrap();
        hub.stop("run-1").unwrap();
        hub.revoke_grant("run-1", "g-1").unwrap();
        until("all sent", || {
            let t = received_types(&run);
            t.contains(&"decide".into())
                && t.contains(&"stop".into())
                && t.contains(&"revoke_grant".into())
        })
        .await;
        let received = run.received.lock().unwrap().clone();
        let decide = received.iter().find(|v| v["type"] == "decide").unwrap();
        assert_eq!(decide["outcome"]["outcome"], "denied");
        assert_eq!(decide["outcome"]["reason"], "not now");
        assert!(decide["outcome"].get("grant").is_none());
        let stop = received.iter().find(|v| v["type"] == "stop").unwrap();
        assert!(!stop["reason"].as_str().unwrap().is_empty());
        let revoke = received
            .iter()
            .find(|v| v["type"] == "revoke_grant")
            .unwrap();
        assert_eq!(revoke["grant_id"], "g-1");
    }

    #[tokio::test]
    async fn deciding_an_unknown_request_or_run_fails() {
        let (_dir, _runs, _rec, hub) = setup();
        assert!(hub.decide("nope", "r", true, None, None).is_err());
        assert!(hub.stop("nope").is_err());
        assert!(hub.revoke_grant("nope", "g").is_err());
    }

    #[tokio::test]
    async fn closed_socket_marks_the_run_disconnected_and_keeps_its_timeline() {
        let (_dir, runs, _rec, hub) = setup();
        let sock_path = runs.join("run-1.sock");
        let run = fake_run(&sock_path, "run-1", IPC_VERSION);
        let mut disc = sock::Discovery::default();
        disc.scan(&hub, &runs).await;
        until("connected", || {
            hub.list_runs().first().is_some_and(|r| r.connected)
        })
        .await;
        run.push
            .send(ServerMsg::Event {
                event: AiEvent::RunEnded {
                    run_id: "run-1".into(),
                    reason: "stopped".into(),
                },
            })
            .unwrap();
        until("ended", || hub.list_runs()[0].ended.is_some()).await;
        drop(run); // closes the connection
        until("disconnected", || !hub.list_runs()[0].connected).await;
        assert_eq!(hub.list_runs()[0].ended.as_deref(), Some("stopped"));
        assert_eq!(hub.run_events("run-1").len(), 1);
        assert!(hub.stop("run-1").is_err());
    }

    #[tokio::test]
    async fn reconnects_when_the_run_comes_back() {
        let (_dir, runs, _rec, hub) = setup();
        let sock_path = runs.join("run-1.sock");
        let first = fake_run(&sock_path, "run-1", IPC_VERSION);
        let mut disc = sock::Discovery::default();
        disc.scan(&hub, &runs).await;
        until("connected", || {
            hub.list_runs().first().is_some_and(|r| r.connected)
        })
        .await;
        drop(first);
        until("disconnected", || !hub.list_runs()[0].connected).await;

        std::fs::remove_file(&sock_path).unwrap();
        let second = fake_run(&sock_path, "run-1", IPC_VERSION);
        disc.scan(&hub, &runs).await;
        until("reconnected", || hub.list_runs()[0].connected).await;
        until("handshake", || received_types(&second).len() >= 3).await;
        assert_eq!(hub.list_runs().len(), 1);
    }

    #[tokio::test]
    async fn stale_socket_is_skipped_until_it_changes() {
        let (_dir, runs, _rec, hub) = setup();
        let sock_path = runs.join("old.sock");
        // Bind then drop the listener: the file stays, nobody listens.
        drop(std::os::unix::net::UnixListener::bind(&sock_path).unwrap());
        let mut disc = sock::Discovery::default();
        disc.scan(&hub, &runs).await;
        assert!(hub.list_runs().is_empty());
        disc.scan(&hub, &runs).await;
        assert!(hub.list_runs().is_empty());
    }

    #[tokio::test]
    async fn protocol_version_mismatch_is_reported_and_not_attached() {
        let (_dir, runs, rec, hub) = setup();
        let _run = fake_run(&runs.join("run-9.sock"), "run-9", IPC_VERSION + 1);
        let mut disc = sock::Discovery::default();
        disc.scan(&hub, &runs).await;
        until("error", || !rec.errors.lock().unwrap().is_empty()).await;
        assert!(rec.errors.lock().unwrap()[0].contains("protocol version"));
        assert!(hub.list_runs().is_empty());
    }

    #[test]
    fn closed_runs_are_pruned_to_the_newest() {
        let rec = Arc::new(Recorder::default());
        let hub = AiHub::new(rec);
        for n in 0..(MAX_CLOSED_RUNS + 5) {
            let (tx, _rx) = unbounded_channel();
            let id = format!("run-{n:02}");
            let conn = hub.attach(
                RunHello {
                    run_id: id.clone(),
                    client: "c".into(),
                    policy: "default".into(),
                    started_at: format!("2026-10-24T10:{n:02}:00Z"),
                },
                tx,
            );
            hub.detach(&id, conn);
        }
        let runs = hub.list_runs();
        assert_eq!(runs.len(), MAX_CLOSED_RUNS);
        assert!(runs.iter().all(|r| r.run_id.as_str() >= "run-05"));
    }

    #[test]
    fn older_connection_closing_does_not_detach_the_newer_one() {
        let hub = AiHub::new(Arc::new(Recorder::default()));
        let hello = || RunHello {
            run_id: "run-1".into(),
            client: "c".into(),
            policy: "default".into(),
            started_at: "2026-10-24T10:00:00Z".into(),
        };
        let (tx1, _rx1) = unbounded_channel();
        let (tx2, _rx2) = unbounded_channel();
        let old = hub.attach(hello(), tx1);
        let _new = hub.attach(hello(), tx2);
        hub.detach("run-1", old);
        assert!(hub.list_runs()[0].connected);
    }
}
