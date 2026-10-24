//! Fakes for the traits in `types.rs`, so tests never touch a network or a real host.
//!
//! Owner: W4. Test-only.

use super::context::{RunContext, RunDeps, StaticHosts};
use super::files::{FileAccess, FileRead};
use super::server::McpElicitor;
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::ai::types::*;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Scripted executor: first rule whose needle occurs in the command wins.
pub struct FakeExecutor {
    pub log: Mutex<Vec<(String, String, ExecMode)>>,
    rules: Mutex<Vec<(String, ExecResult)>>,
}

/// Jail and agent-user probes (internal to bssh, not agent calls).
fn is_probe(command: &str) -> bool {
    [
        "landlock",
        "bwrap",
        "bubblewrap",
        "setpriv",
        "command -v",
        "which ",
    ]
    .iter()
    .any(|p| command.contains(p))
        || command == "id -u bssh-agent"
}

pub fn exec_result(stdout: &str, code: i32) -> ExecResult {
    ExecResult {
        stdout: stdout.into(),
        stderr: String::new(),
        exit_code: Some(code),
        truncated: false,
        timed_out: false,
        duration_ms: 1,
    }
}

impl FakeExecutor {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            log: Mutex::new(Vec::new()),
            rules: Mutex::new(Vec::new()),
        })
    }

    pub fn on(&self, needle: &str, stdout: &str, code: i32) {
        self.rules
            .lock()
            .unwrap()
            .push((needle.into(), exec_result(stdout, code)));
    }

    /// Commands run so far, without capability probes.
    pub fn commands(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, c, _)| !is_probe(c))
            .map(|(_, c, _)| c.clone())
            .collect()
    }

    /// All commands with their mode, probes included.
    pub fn raw(&self) -> Vec<(String, ExecMode)> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(_, c, m)| (c.clone(), m.clone()))
            .collect()
    }
}

#[async_trait]
impl Executor for FakeExecutor {
    async fn exec(
        &self,
        conn: &Connection,
        command: &str,
        mode: ExecMode,
        _limits: ExecLimits,
    ) -> Result<ExecResult> {
        self.log
            .lock()
            .unwrap()
            .push((conn.name.clone(), command.to_string(), mode));
        let rules = self.rules.lock().unwrap();
        for (needle, res) in rules.iter() {
            if command.contains(needle.as_str()) {
                return Ok(res.clone());
            }
        }
        // Capability probes (jail, agent user) fail unless a test scripts them, so tests stay
        // independent of how the jail module probes.
        if is_probe(command) {
            return Ok(exec_result("", 127));
        }
        Ok(exec_result("", 0))
    }
}

/// In-memory files keyed by (host, path).
#[derive(Default)]
pub struct FakeFiles {
    pub files: Mutex<HashMap<(String, String), Vec<u8>>>,
}

impl FakeFiles {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn put(&self, host: &str, path: &str, data: &str) {
        self.files
            .lock()
            .unwrap()
            .insert((host.into(), path.into()), data.as_bytes().to_vec());
    }

    pub fn get(&self, host: &str, path: &str) -> Option<String> {
        self.files
            .lock()
            .unwrap()
            .get(&(host.to_string(), path.to_string()))
            .map(|d| String::from_utf8_lossy(d).into_owned())
    }
}

#[async_trait]
impl FileAccess for FakeFiles {
    async fn read(
        &self,
        conn: &Connection,
        path: &str,
        offset: u64,
        max_bytes: usize,
    ) -> Result<FileRead> {
        let files = self.files.lock().unwrap();
        let data = files
            .get(&(conn.name.clone(), path.to_string()))
            .ok_or_else(|| anyhow!("no such file {path}"))?;
        let start = (offset as usize).min(data.len());
        let end = (start + max_bytes).min(data.len());
        Ok(FileRead {
            data: data[start..end].to_vec(),
            size: data.len() as u64,
            mode: Some(0o644),
        })
    }

    async fn write(
        &self,
        conn: &Connection,
        path: &str,
        data: &[u8],
        _mode: Option<u32>,
    ) -> Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert((conn.name.clone(), path.to_string()), data.to_vec());
        Ok(())
    }
}

/// Broker that answers from a queue (default: approved by "tester") and remembers requests.
pub struct FakeBroker {
    pub requests: Mutex<Vec<ApprovalRequest>>,
    outcomes: Mutex<VecDeque<ApprovalOutcome>>,
}

impl FakeBroker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            outcomes: Mutex::new(VecDeque::new()),
        })
    }

    pub fn push(&self, o: ApprovalOutcome) {
        self.outcomes.lock().unwrap().push_back(o);
    }

    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

#[async_trait]
impl ApprovalBroker for FakeBroker {
    async fn request(&self, req: ApprovalRequest) -> Result<ApprovalOutcome> {
        self.requests.lock().unwrap().push(req);
        Ok(self
            .outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(ApprovalOutcome::Approved {
                by: "tester".into(),
                channel: "fake".into(),
                grant: None,
            }))
    }
}

#[derive(Default)]
pub struct FakeAudit {
    pub events: Mutex<Vec<AiEvent>>,
}

impl FakeAudit {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn events(&self) -> Vec<AiEvent> {
        self.events.lock().unwrap().clone()
    }

    pub fn calls(&self) -> Vec<(ToolCall, Decision)> {
        self.events()
            .into_iter()
            .filter_map(|e| match e {
                AiEvent::Call { call, decision } => Some((call, decision)),
                _ => None,
            })
            .collect()
    }
}

impl AuditSink for FakeAudit {
    fn record(&self, event: &AiEvent) -> Result<()> {
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }
}

type DecideFn = dyn Fn(&ToolCall) -> Decision + Send + Sync;

pub struct FakeEngine(pub Box<DecideFn>);

impl PolicyEngine for FakeEngine {
    fn decide(
        &self,
        _p: &Policy,
        call: &ToolCall,
        _a: Option<&Analysis>,
        _h: &HostFacts,
    ) -> Decision {
        (self.0)(call)
    }
}

pub fn allow_all() -> FakeEngine {
    FakeEngine(Box::new(|_| Decision::Allow {
        reason: "test".into(),
    }))
}

/// Reads allowed, writes ask (out-of-band), like the observe/operate profiles.
pub fn ask_writes() -> FakeEngine {
    FakeEngine(Box::new(|c| {
        if c.tool.is_write() {
            Decision::Ask {
                tier: Tier::OutOfBand,
                reason: "write".into(),
            }
        } else {
            Decision::Allow {
                reason: "read".into(),
            }
        }
    }))
}

/// Read-only when the first word is a known reader.
pub struct SimpleAnalyzer;

impl Analyzer for SimpleAnalyzer {
    fn analyze(&self, command: &str) -> Analysis {
        let first = command.split_whitespace().next().unwrap_or("");
        let read = matches!(
            first,
            "cat"
                | "ls"
                | "df"
                | "free"
                | "ps"
                | "uptime"
                | "id"
                | "journalctl"
                | "tail"
                | "head"
                | "whoami"
        ) && !command.contains(';')
            && !command.contains('>');
        let tag = if read { RiskTag::Read } else { RiskTag::Write };
        Analysis {
            commands: Vec::new(),
            tags: [tag].into_iter().collect(),
            unknown_reason: None,
        }
    }
}

/// Replaces the literal `hunter2`.
pub struct MaskRedactor;

impl Redactor for MaskRedactor {
    fn redact(&self, text: &str) -> (String, Vec<Redaction>) {
        let n = text.matches("hunter2").count();
        if n == 0 {
            return (text.to_string(), Vec::new());
        }
        (
            text.replace("hunter2", "«redacted:vault-secret»"),
            vec![Redaction {
                kind: "vault-secret".into(),
                count: n,
            }],
        )
    }
}

pub fn conn(name: &str, tags: &[&str]) -> Connection {
    let mut c = Connection::new(
        name.into(),
        format!("{name}.example.test"),
        "deploy".into(),
        22,
        None,
        None,
        false,
        None,
    );
    c.tags = tags.iter().map(|t| t.to_string()).collect();
    c
}

/// A policy with generous budgets, so tests control limits themselves.
pub fn policy() -> Policy {
    Policy {
        scope: Scope {
            tags: vec!["prod".into()],
            hosts: vec![],
        },
        ..Policy::default()
    }
}

pub struct Kit {
    pub ctx: Arc<RunContext>,
    pub exec: Arc<FakeExecutor>,
    pub files: Arc<FakeFiles>,
    pub broker: Arc<FakeBroker>,
    pub audit: Arc<FakeAudit>,
    /// Keeps the temp dir (ai_dir) alive for the test.
    pub _tmp: tempfile::TempDir,
}

/// Scope: every host except those tagged `hidden`.
pub fn kit(policy: Policy, engine: FakeEngine, hosts: Vec<Connection>) -> Kit {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig {
        database_path: tmp.path().join("bssh.db"),
        ..AppConfig::default()
    };
    config.ai.approval_timeout_secs = 5;
    let exec = FakeExecutor::new();
    let files = FakeFiles::new();
    let broker = FakeBroker::new();
    let audit = FakeAudit::new();
    let deps = RunDeps {
        hosts: Arc::new(StaticHosts(hosts)),
        files: files.clone(),
        executor: exec.clone(),
        broker: broker.clone(),
        analyzer: Arc::new(SimpleAnalyzer),
        redactor: Arc::new(MaskRedactor),
        engine: Arc::new(engine),
        audit: audit.clone(),
        scope: Arc::new(|_p, c| !c.tags.iter().any(|t| t == "hidden")),
        elicitor: Some(Arc::new(McpElicitor::default())),
    };
    let ctx = RunContext::new(config, "default".into(), policy, deps);
    ctx.start("test-client 1.0");
    Kit {
        ctx,
        exec,
        files,
        broker,
        audit,
        _tmp: tmp,
    }
}
