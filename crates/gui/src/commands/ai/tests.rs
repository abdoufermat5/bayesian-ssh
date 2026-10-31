use super::hub::{AiHub, MAX_CLOSED_RUNS, MAX_GRANT_MINUTES, RunHello, build_grant};
use super::sink::{HubSink, notification_body};
use super::socket::Discovery;
use super::types::{RunSummary, StoredEvent};
use bayesian_ssh::services::ai::ipc::IPC_VERSION;
use bayesian_ssh::services::ai::ipc::ServerMsg;
use bayesian_ssh::services::ai::types::{
    AiEvent, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Decision, RiskTag, ToolName,
};
use bayesian_ssh::services::ai::types::{Analysis, ToolCall};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::mpsc::UnboundedSender;
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
    let mut disc = Discovery::default();
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
    let mut disc = Discovery::default();
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
    let mut disc = Discovery::default();
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
    let mut disc = Discovery::default();
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
    let mut disc = Discovery::default();
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
    let mut disc = Discovery::default();
    disc.scan(&hub, &runs).await;
    assert!(hub.list_runs().is_empty());
    disc.scan(&hub, &runs).await;
    assert!(hub.list_runs().is_empty());
}

#[tokio::test]
async fn protocol_version_mismatch_is_reported_and_not_attached() {
    let (_dir, runs, rec, hub) = setup();
    let _run = fake_run(&runs.join("run-9.sock"), "run-9", IPC_VERSION + 1);
    let mut disc = Discovery::default();
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
