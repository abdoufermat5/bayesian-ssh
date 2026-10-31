//! The hub: live picture of the runs, fed by the sockets.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use bayesian_ssh::services::ai::ipc::{ClientMsg, ServerMsg};
use bayesian_ssh::services::ai::types::{
    AiEvent, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Decision, Grant, RiskTag, ToolName,
};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use super::sink::HubSink;
use super::types::{RunSummary, StoredEvent};
use crate::commands::CommandError;

/// Approval channel recorded in the audit.
pub(super) const CHANNEL: &str = "desktop";
/// Events kept per run for the timeline.
pub(super) const MAX_EVENTS_PER_RUN: usize = 1000;
/// Runs that are no longer connected, newest first, kept in the list.
pub(super) const MAX_CLOSED_RUNS: usize = 20;
/// Longest grant a click can create.
pub(super) const MAX_GRANT_MINUTES: u32 = 8 * 60;
pub(super) const MAX_DENY_REASON_CHARS: usize = 500;

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
    paused: bool,
    pause_reason: Option<String>,
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
            paused: self.paused,
            pause_reason: self.pause_reason.clone(),
        }
    }
}

#[derive(Default)]
pub(super) struct HubState {
    runs: BTreeMap<String, RunState>,
    next_conn: u64,
    tick: u64,
}

/// Everything to tell the sink once the lock is released.
#[derive(Default)]
pub(super) struct Effects {
    pub(super) changed: bool,
    pub(super) events: Vec<(String, StoredEvent)>,
    pub(super) requests: Vec<(String, String, ApprovalRequest)>,
    pub(super) errors: Vec<(String, String)>,
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

    pub(super) fn apply(&self, f: impl FnOnce(&mut HubState, &mut Effects)) {
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
    pub(super) fn attach(&self, hello: RunHello, tx: UnboundedSender<ClientMsg>) -> u64 {
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
                    paused: false,
                    pause_reason: None,
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
    pub(super) fn detach(&self, run_id: &str, conn_id: u64) {
        self.apply(|st, fx| {
            st.tick += 1;
            let tick = st.tick;
            if let Some(run) = st.runs.get_mut(run_id)
                && run.conn.as_ref().is_some_and(|c| c.id == conn_id)
            {
                run.conn = None;
                // Nobody can answer these any more.
                run.pending.clear();
                run.touched = tick;
                fx.changed = true;
            }
        });
    }

    pub(super) fn on_message(&self, run_id: &str, msg: ServerMsg) {
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
                        AiEvent::Paused { reason, .. } => {
                            run.paused = true;
                            run.pause_reason = Some(reason.clone());
                        }
                        AiEvent::Resumed { .. } => {
                            run.paused = false;
                            run.pause_reason = None;
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

    fn sender(&self, run_id: &str) -> Result<UnboundedSender<ClientMsg>, CommandError> {
        self.lock()
            .runs
            .get(run_id)
            .and_then(|r| r.conn.as_ref().map(|c| c.tx.clone()))
            .ok_or_else(|| CommandError::from("The run is not connected any more."))
    }

    pub fn decide(
        &self,
        run_id: &str,
        request_id: &str,
        approve: bool,
        reason: Option<String>,
        grant_minutes: Option<u32>,
    ) -> Result<(), CommandError> {
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
        .map_err(|_| CommandError::from("The run is not connected any more."))?;
        // The server answers with an Approval event; this refreshes the list
        // in case that event got lost.
        let _ = tx.send(ClientMsg::ListPending);
        Ok(())
    }

    pub fn resume(&self, run_id: &str) -> Result<(), CommandError> {
        self.sender(run_id)?
            .send(ClientMsg::Resume { by: local_user() })
            .map_err(|_| CommandError::from("The run is not connected any more."))
    }

    pub fn stop(&self, run_id: &str) -> Result<(), CommandError> {
        self.sender(run_id)?
            .send(ClientMsg::Stop {
                reason: "stopped from the desktop app".to_string(),
            })
            .map_err(|_| CommandError::from("The run is not connected any more."))
    }

    pub fn revoke_grant(&self, run_id: &str, grant_id: &str) -> Result<(), CommandError> {
        self.sender(run_id)?
            .send(ClientMsg::RevokeGrant {
                grant_id: grant_id.to_string(),
            })
            .map_err(|_| CommandError::from("The run is not connected any more."))
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
pub(super) fn build_grant(
    run_id: &str,
    request: &ApprovalRequest,
    minutes: u32,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Grant, CommandError> {
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

pub(super) struct RunHello {
    pub(super) run_id: String,
    pub(super) client: String,
    pub(super) policy: String,
    pub(super) started_at: String,
}
