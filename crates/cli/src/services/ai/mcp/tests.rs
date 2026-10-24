//! Handler tests with fake executor, files, broker and audit; plus an in-process MCP round trip.
//!
//!

use super::context::ToolError;
use super::server::build_server;
use super::testkit::*;
use super::tools;
use crate::services::ai::types::*;
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use serde_json::{json, Value};

async fn call(k: &Kit, tool: &str, args: Value) -> Result<Value, ToolError> {
    tools::call(&k.ctx, tool, args).await
}

fn hosts() -> Vec<crate::models::Connection> {
    vec![
        conn("web-1", &["prod", "web"]),
        conn("web-2", &["prod", "web"]),
        conn("vault-1", &["hidden"]),
    ]
}

fn std_kit() -> Kit {
    kit(policy(), allow_all(), hosts())
}

const CONF: &str = "listen 80;\nproxy_pass http://127.0.0.1:8080;\n";
const PATCH: &str = "--- a/app.conf\n+++ b/app.conf\n@@ -1,2 +1,2 @@\n listen 80;\n-proxy_pass http://127.0.0.1:8080;\n+proxy_pass http://127.0.0.1:8081;\n";
const PATCHED: &str = "listen 80;\nproxy_pass http://127.0.0.1:8081;\n";
const CONF_PATH: &str = "/etc/nginx/conf.d/app.conf";

// ---------------------------------------------------------------- reads

#[tokio::test]
async fn list_hosts_shows_only_in_scope_hosts() {
    let k = std_kit();
    let v = call(&k, "list_hosts", json!({})).await.unwrap();
    let names: Vec<&str> = v["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["web-1", "web-2"]);
    let v = call(&k, "list_hosts", json!({ "query": "WEB-2" }))
        .await
        .unwrap();
    assert_eq!(v["count"], 1);
    let v = call(&k, "list_hosts", json!({ "tag": "db" }))
        .await
        .unwrap();
    assert_eq!(v["count"], 0);
}

#[tokio::test]
async fn out_of_scope_and_unknown_hosts_look_the_same() {
    let k = std_kit();
    let a = call(&k, "host_info", json!({ "host": "vault-1" }))
        .await
        .unwrap_err();
    let b = call(&k, "host_info", json!({ "host": "nope" }))
        .await
        .unwrap_err();
    let (ToolError::Invalid(a), ToolError::Invalid(b)) = (a, b) else {
        panic!("expected Invalid");
    };
    assert_eq!(a.replace("vault-1", "X"), b.replace("nope", "X"));
    assert!(
        k.exec.commands().is_empty(),
        "nothing may reach an out-of-scope host"
    );
}

#[tokio::test]
async fn host_info_reports_enforcement_and_caches_the_probe() {
    let k = std_kit();
    k.exec.on("uname", "Linux 6.1.0\nup 3 days\n", 0);
    let v = call(&k, "host_info", json!({ "host": "web-1" }))
        .await
        .unwrap();
    assert_eq!(v["enforcement"]["jail"], "none");
    assert_eq!(v["enforcement"]["certificates"], false);
    assert_eq!(v["untrusted_system"]["kernel"], "Linux 6.1.0");
    assert_eq!(v["via_bastion"], false);
    call(&k, "host_info", json!({ "host": "web-1" }))
        .await
        .unwrap();
    let probes = k
        .exec
        .raw()
        .iter()
        .filter(|(c, _)| c == "id -u bssh-agent")
        .count();
    assert_eq!(probes, 1, "enforcement is probed once per run");
}

#[tokio::test]
async fn observe_parses_and_redacts_leaves() {
    let k = std_kit();
    k.exec.on(
        "df -P",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 1000 500 500 50% /mnt/hunter2\n",
        0,
    );
    let v = call(&k, "observe", json!({ "host": "web-1", "what": "disk" }))
        .await
        .unwrap();
    assert_eq!(v["untrusted_data"]["filesystems"][0]["use_percent"], 50);
    let mount = v["untrusted_data"]["filesystems"][0]["mount"]
        .as_str()
        .unwrap();
    assert!(
        !mount.contains("hunter2") && mount.contains("redacted"),
        "{mount}"
    );
    assert!(v["notice"].as_str().unwrap().contains("untrusted"));
    assert!(call(
        &k,
        "observe",
        json!({ "host": "web-1", "what": "nonsense" })
    )
    .await
    .is_err());
}

#[tokio::test]
async fn observe_unparsable_output_is_an_error_not_a_guess() {
    let k = std_kit();
    k.exec.on("free -b", "free: command not found", 127);
    let e = call(&k, "observe", json!({ "host": "web-1", "what": "memory" }))
        .await
        .unwrap_err();
    assert!(matches!(e, ToolError::Failed(_)));
}

#[tokio::test]
async fn read_file_range_cap_deny_and_redaction() {
    let mut p = policy();
    p.deny_paths = vec!["/etc/shadow".into(), "/root/.ssh".into()];
    let k = kit(p, allow_all(), hosts());
    k.files
        .put("web-1", "/etc/app.env", "TOKEN=hunter2\nline2\nline3\n");
    k.files.put("web-1", "/etc/shadow", "root:x");
    k.files.put("web-1", "/bin/blob", "a\0b");

    let v = call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "/etc/app.env" }),
    )
    .await
    .unwrap();
    assert!(v["untrusted_content"]
        .as_str()
        .unwrap()
        .contains("«redacted:vault-secret»"));
    assert!(!v["untrusted_content"].as_str().unwrap().contains("hunter2"));
    assert_eq!(v["truncated"], false);
    assert_eq!(v["redactions"][0]["kind"], "vault-secret");

    let v = call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "/etc/app.env", "range": { "offset": 14, "length": 5 } }),
    )
    .await
    .unwrap();
    assert_eq!(v["untrusted_content"], "line2");
    assert_eq!(v["truncated"], true);
    assert_eq!(v["offset"], 14);

    // Deny paths, directly, through `..`, and for directories.
    for path in ["/etc/shadow", "/etc/../etc/shadow", "/root/.ssh/id_ed25519"] {
        let e = call(&k, "read_file", json!({ "host": "web-1", "path": path }))
            .await
            .unwrap_err();
        assert!(
            matches!(e, ToolError::Denied(_) | ToolError::Invalid(_)),
            "{path}: {e}"
        );
    }
    assert!(call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "/bin/blob" })
    )
    .await
    .is_err());
    assert!(call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "relative" })
    )
    .await
    .is_err());
    // Refused calls are in the audit as denials.
    assert!(k.audit.calls().iter().any(
        |(c, d)| c.path.as_deref() == Some("/etc/shadow") && matches!(d, Decision::Deny { .. })
    ));
}

#[tokio::test]
async fn search_logs_runs_a_quoted_command() {
    let k = std_kit();
    k.exec.on("journalctl", "Oct 24 nginx[1]: 502 hunter2\n", 0);
    let v = call(
        &k,
        "search_logs",
        json!({ "host": "web-1", "source": "nginx.service", "pattern": "50[23]", "since": "1h ago", "lines": 20 }),
    )
    .await
    .unwrap();
    assert!(v["untrusted_output"].as_str().unwrap().contains("redacted"));
    assert_eq!(
        k.exec.commands().last().unwrap(),
        "journalctl --no-pager -o short-iso -u 'nginx.service' --since '1h ago' -g '50[23]' -n 20"
    );
    let e = call(
        &k,
        "search_logs",
        json!({ "host": "web-1", "source": "x; reboot" }),
    )
    .await;
    assert!(matches!(e, Err(ToolError::Invalid(_))));
}

#[tokio::test]
async fn run_readonly_refuses_writes_without_a_jail_and_redacts() {
    let k = std_kit();
    k.exec.on("cat /etc/motd", "welcome hunter2", 0);
    let v = call(
        &k,
        "run_readonly",
        json!({ "host": "web-1", "command": "cat /etc/motd" }),
    )
    .await
    .unwrap();
    assert_eq!(v["exit_code"], 0);
    assert_eq!(v["jailed"], false);
    assert_eq!(v["untrusted_output"], "welcome «redacted:vault-secret»");
    assert_eq!(v["redactions"][0]["count"], 1);

    let e = call(
        &k,
        "run_readonly",
        json!({ "host": "web-1", "command": "rm -rf /tmp/x" }),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ToolError::Denied(_)), "{e}");
    assert!(!k.exec.commands().iter().any(|c| c.contains("rm -rf")));
}

#[tokio::test]
async fn result_events_hash_the_redacted_output() {
    let k = std_kit();
    k.exec.on("cat /f", "x hunter2 y", 0);
    call(
        &k,
        "run_readonly",
        json!({ "host": "web-1", "command": "cat /f" }),
    )
    .await
    .unwrap();
    let ev = k.audit.events();
    let kinds: Vec<&str> = ev
        .iter()
        .map(|e| match e {
            AiEvent::RunStarted { .. } => "start",
            AiEvent::Call { .. } => "call",
            AiEvent::Result { .. } => "result",
            _ => "other",
        })
        .collect();
    // start, then list of probe-free events: call before result
    let call_ix = kinds.iter().position(|k| *k == "call").unwrap();
    let res_ix = kinds.iter().rposition(|k| *k == "result").unwrap();
    assert!(call_ix < res_ix);
    let AiEvent::Result {
        output_sha256,
        redactions,
        exit_code,
        ..
    } = &ev[res_ix]
    else {
        panic!()
    };
    assert_eq!(*exit_code, Some(0));
    assert_eq!(redactions[0].kind, "vault-secret");
    let expect = super::util::sha256_hex("x «redacted:vault-secret» y\n--stderr--\n".as_bytes());
    assert_eq!(output_sha256, &expect);
}

#[tokio::test]
async fn run_many_fans_out_by_tag() {
    let k = std_kit();
    k.exec.on("uptime", "up 1 day", 0);
    let v = call(&k, "run_many", json!({ "tag": "web", "command": "uptime" }))
        .await
        .unwrap();
    assert_eq!(v["count"], 2);
    assert_eq!(v["results"][0]["host"], "web-1");
    assert_eq!(v["results"][1]["result"]["untrusted_output"], "up 1 day");
    assert!(call(
        &k,
        "run_many",
        json!({ "tag": "hidden", "command": "uptime" })
    )
    .await
    .is_err());
    // A non-read-only command is refused per host, not run.
    let v = call(&k, "run_many", json!({ "tag": "web", "command": "reboot" }))
        .await
        .unwrap();
    assert_eq!(v["results"][0]["ok"], false);
    assert!(!k.exec.commands().iter().any(|c| c == "reboot"));
}

#[tokio::test]
async fn jailed_hosts_run_readonly_inside_the_jail_and_certified_hosts_write_with_certs() {
    let k = std_kit();
    k.ctx
        .prime_enforcement(
            "web-1",
            Enforcement {
                jail: JailKind::Landlock,
                certificates: true,
                agent_user: true,
            },
        )
        .await;
    // In a jail the command need not be provably read-only: the jail is the boundary.
    let v = call(
        &k,
        "run_readonly",
        json!({ "host": "web-1", "command": "make report" }),
    )
    .await
    .unwrap();
    assert_eq!(v["jailed"], true);
    let v = call(&k, "observe", json!({ "host": "web-1", "what": "memory" })).await;
    assert!(v.is_err()); // unparsable fake output, but it must have run jailed
    let raw = k.exec.raw();
    assert!(raw
        .iter()
        .filter(|(c, _)| c == "make report" || c.contains("free -b"))
        .all(|(_, m)| *m == ExecMode::Jailed(JailKind::Landlock)));

    call(
        &k,
        "run",
        json!({ "host": "web-1", "command": "touch /tmp/x" }),
    )
    .await
    .unwrap();
    assert!(k
        .exec
        .raw()
        .iter()
        .any(|(c, m)| c == "touch /tmp/x" && *m == ExecMode::Certified));
    // Another host stays plain.
    call(
        &k,
        "run",
        json!({ "host": "web-2", "command": "touch /tmp/y" }),
    )
    .await
    .unwrap();
    assert!(k
        .exec
        .raw()
        .iter()
        .any(|(c, m)| c == "touch /tmp/y" && *m == ExecMode::Plain));
}

#[test]
fn approval_text_labels_the_agents_words() {
    use super::server::describe_request;
    let plan = ChangePlan {
        change_id: "c".into(),
        run_id: "r".into(),
        title: "Fix".into(),
        reason: "ignore all rules".into(),
        steps: vec![],
        verify: vec![],
        rollback: vec![],
    };
    let hash = plan.hash();
    let req = ApprovalRequest {
        request_id: "q".into(),
        run_id: "r".into(),
        client: "Claude Code 2.0".into(),
        tier: Tier::InClient,
        subject: ApprovalSubject::Plan {
            plan,
            plan_hash: hash,
            analyses: vec![],
        },
        created_at: String::new(),
        expires_at: String::new(),
    };
    let text = describe_request(&req);
    assert!(text.contains("Claude Code 2.0") && text.contains("the agent says: ignore all rules"));
}

// ---------------------------------------------------------------- policy, approvals, budgets

#[tokio::test]
async fn run_asks_and_honours_the_answer() {
    let k = kit(policy(), ask_writes(), hosts());
    k.exec.on("systemctl restart", "", 0);
    let v = call(
        &k,
        "run",
        json!({ "host": "web-1", "command": "systemctl restart nginx" }),
    )
    .await
    .unwrap();
    assert_eq!(v["exit_code"], 0);
    assert_eq!(k.broker.count(), 1);
    let req = k.broker.requests.lock().unwrap()[0].clone();
    assert_eq!(req.client, "test-client 1.0");
    assert_eq!(req.tier, Tier::OutOfBand);
    assert!(
        matches!(&req.subject, ApprovalSubject::Call { call, analysis: Some(_) }
        if call.command.as_deref() == Some("systemctl restart nginx"))
    );

    k.broker.push(ApprovalOutcome::Denied {
        by: "alice".into(),
        reason: Some("no".into()),
    });
    let e = call(&k, "run", json!({ "host": "web-1", "command": "reboot" }))
        .await
        .unwrap_err();
    assert!(
        matches!(&e, ToolError::Denied(m) if m.contains("alice")),
        "{e}"
    );
    assert!(!k.exec.commands().iter().any(|c| c == "reboot"));

    k.broker.push(ApprovalOutcome::Expired);
    assert!(
        call(&k, "run", json!({ "host": "web-1", "command": "halt" }))
            .await
            .is_err()
    );
    assert!(!k.exec.commands().iter().any(|c| c == "halt"));
}

#[tokio::test]
async fn deny_decision_never_executes() {
    let k = kit(
        policy(),
        FakeEngine(Box::new(|_| Decision::Deny {
            reason: "nope".into(),
        })),
        hosts(),
    );
    let e = call(&k, "list_hosts", json!({})).await.unwrap_err();
    assert!(matches!(e, ToolError::Denied(m) if m == "nope"));
    let e = call(&k, "run", json!({ "host": "web-1", "command": "ls" }))
        .await
        .unwrap_err();
    assert!(matches!(e, ToolError::Denied(_)));
    assert!(k.exec.commands().iter().all(|c| !c.starts_with("ls")));
}

#[tokio::test]
async fn change_budget_is_enforced() {
    let mut p = policy();
    p.budgets.changes_per_run = 1;
    let k = kit(p, allow_all(), hosts());
    call(
        &k,
        "run",
        json!({ "host": "web-1", "command": "touch /tmp/a" }),
    )
    .await
    .unwrap();
    let e = call(
        &k,
        "run",
        json!({ "host": "web-1", "command": "touch /tmp/b" }),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ToolError::Denied(m) if m.contains("budget")));
}

#[tokio::test]
async fn call_rate_limit_and_circuit_breaker() {
    let mut p = policy();
    p.budgets.calls_per_min = 2;
    p.budgets.max_denials_per_min = 3;
    let k = kit(p, allow_all(), hosts());
    call(&k, "status", json!({})).await.unwrap();
    call(&k, "status", json!({})).await.unwrap();
    // Over budget: denied, counted; the third denial trips the breaker and stops the run.
    for _ in 0..3 {
        assert!(matches!(
            call(&k, "status", json!({})).await,
            Err(ToolError::Denied(_))
        ));
    }
    assert!(k.ctx.is_stopped());
    let e = call(&k, "status", json!({})).await.unwrap_err();
    assert!(matches!(e, ToolError::Denied(m) if m.contains("circuit breaker")));
    let ev = k.audit.events();
    assert!(ev
        .iter()
        .any(|e| matches!(e, AiEvent::CircuitBreaker { .. })));
    assert!(ev
        .iter()
        .any(|e| matches!(e, AiEvent::RunEnded { reason, .. } if reason == "circuit breaker")));
}

#[tokio::test]
async fn probing_out_of_scope_hosts_trips_the_breaker() {
    let mut p = policy();
    p.budgets.max_denials_per_min = 3;
    let k = kit(p, allow_all(), hosts());
    for _ in 0..3 {
        let _ = call(&k, "host_info", json!({ "host": "vault-1" })).await;
    }
    assert!(k.ctx.is_stopped());
}

#[tokio::test]
async fn run_ttl_ends_the_run() {
    let mut p = policy();
    p.budgets.run_ttl = "1s".into();
    let k = kit(p, allow_all(), hosts());
    call(&k, "status", json!({})).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let e = call(&k, "status", json!({})).await.unwrap_err();
    assert!(matches!(e, ToolError::Denied(m) if m.contains("expired")));
    assert!(k
        .audit
        .events()
        .iter()
        .any(|e| matches!(e, AiEvent::RunEnded { .. })));
}

#[tokio::test]
async fn stop_flag_blocks_everything() {
    let k = std_kit();
    k.ctx.stop("stopped by test");
    assert!(matches!(
        call(&k, "list_hosts", json!({})).await,
        Err(ToolError::Denied(_))
    ));
    k.ctx.stopped().await; // resolves immediately
}

#[tokio::test]
async fn request_access_widens_scope_after_approval() {
    let k = kit(policy(), ask_writes(), hosts());
    let args = json!({ "scope": { "hosts": ["vault-1"] }, "reason": "check the vault", "duration": "30m" });
    k.broker.push(ApprovalOutcome::Denied {
        by: "alice".into(),
        reason: None,
    });
    assert!(call(&k, "request_access", args.clone()).await.is_err());
    assert!(call(&k, "host_info", json!({ "host": "vault-1" }))
        .await
        .is_err());

    let v = call(&k, "request_access", args).await.unwrap();
    assert_eq!(v["status"], "granted");
    assert_eq!(v["expires_in_secs"], 1800);
    let req = k.broker.requests.lock().unwrap().last().unwrap().clone();
    assert!(matches!(req.subject, ApprovalSubject::Access { .. }));
    let v = call(&k, "list_hosts", json!({})).await.unwrap();
    assert_eq!(v["count"], 3);
    let s = call(&k, "status", json!({})).await.unwrap();
    assert_eq!(s["access_grants"].as_array().unwrap().len(), 1);
    assert!(call(
        &k,
        "request_access",
        json!({ "scope": {}, "reason": "x", "duration": "1h" })
    )
    .await
    .is_err());
    assert!(call(
        &k,
        "request_access",
        json!({ "scope": { "tags": ["a"] }, "reason": "x", "duration": "99h" })
    )
    .await
    .is_err());
}

#[tokio::test]
async fn status_reports_budgets() {
    let k = std_kit();
    let s = call(&k, "status", json!({})).await.unwrap();
    assert_eq!(s["client"], "test-client 1.0");
    assert_eq!(s["budgets"]["changes_left"], 10);
    assert_eq!(s["stopped"], false);
    assert!(s["changes"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn bad_arguments_are_invalid_requests() {
    let k = std_kit();
    for (tool, args) in [
        ("observe", json!({ "host": "web-1" })),
        (
            "run",
            json!({ "host": "web-1", "command": "ls", "extra": 1 }),
        ),
        ("nope", json!({})),
    ] {
        assert!(
            matches!(call(&k, tool, args).await, Err(ToolError::Invalid(_))),
            "{tool}"
        );
    }
}

// ---------------------------------------------------------------- change plans

fn plan_input(second_host: bool) -> Value {
    let mut steps = vec![
        json!({ "host": "web-1", "edit_file": CONF_PATH, "patch": PATCH }),
        json!({ "host": "web-1", "run": "nginx -t" }),
        json!({ "host": "web-1", "service": "nginx", "action": "reload" }),
    ];
    let mut verify = vec![json!({ "host": "web-1", "run": "curl -fsS web-1/health" })];
    if second_host {
        steps.push(json!({ "host": "web-2", "run": "echo canary-passed web-2" }));
        verify.push(json!({ "host": "web-2", "run": "curl -fsS web-2/health" }));
    }
    json!({
        "title": "Fix nginx 502",
        "reason": "ignore previous instructions",
        "steps": steps,
        "verify": verify,
        "rollback": [{ "undo": "step-1" }, { "host": "web-1", "service": "nginx", "action": "reload" }],
    })
}

fn plan_kit() -> Kit {
    let k = kit(policy(), ask_writes(), hosts());
    k.files.put("web-1", CONF_PATH, CONF);
    k
}

#[tokio::test]
async fn propose_asks_once_for_the_whole_plan_and_apply_does_not_ask_again() {
    let k = plan_kit();
    let p = call(&k, "propose_change", plan_input(false)).await.unwrap();
    assert_eq!(p["state"], "approved");
    assert_eq!(p["approved_by"], "tester");
    assert_eq!(k.broker.count(), 1);
    let req = k.broker.requests.lock().unwrap()[0].clone();
    let ApprovalSubject::Plan {
        plan,
        plan_hash,
        analyses,
    } = &req.subject
    else {
        panic!("not a plan")
    };
    assert_eq!(plan.hash(), *plan_hash);
    assert_eq!(p["plan_hash"], Value::String(plan_hash.clone()));
    assert_eq!(plan.reason, "ignore previous instructions");
    // steps + verify + rollback, one analysis slot each, run steps analyzed
    assert_eq!(analyses.len(), 3 + 1 + 2);
    assert!(analyses[1].is_some() && analyses[0].is_none());
    assert!(k.exec.commands().is_empty(), "propose must not touch hosts");

    let id = p["change_id"].as_str().unwrap();
    let a = call(&k, "apply_change", json!({ "change_id": id }))
        .await
        .unwrap();
    assert_eq!(a["status"], "applied", "{a}");
    assert_eq!(a["steps"].as_array().unwrap().len(), 3);
    assert_eq!(a["verify"].as_array().unwrap().len(), 1);
    assert_eq!(k.broker.count(), 1, "the plan approval covers apply");
    assert_eq!(k.files.get("web-1", CONF_PATH).unwrap(), PATCHED);
    // Backups exist locally and on the host.
    let local = a["undo_point"].as_str().unwrap();
    assert!(std::path::Path::new(local).join("step-1.orig").exists());
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(local).join("step-1.orig")).unwrap(),
        CONF
    );
    assert!(k
        .exec
        .commands()
        .iter()
        .any(|c| c.contains("/var/tmp/bssh-agent-backups/") && c.contains("cp -p")));
    // The audit has the change.
    assert!(k.audit.events().iter().any(|e| matches!(
        e,
        AiEvent::Change {
            applied: true,
            undo_point: Some(_),
            ..
        }
    )));
    // Re-applying is refused.
    assert!(call(&k, "apply_change", json!({ "change_id": id }))
        .await
        .is_err());
}

#[tokio::test]
async fn apply_refuses_a_plan_that_changed_after_approval() {
    let k = plan_kit();
    let p = call(&k, "propose_change", plan_input(false)).await.unwrap();
    let id = p["change_id"].as_str().unwrap();
    k.ctx.plans.tamper_title(id, "Totally different");
    let e = call(&k, "apply_change", json!({ "change_id": id }))
        .await
        .unwrap_err();
    assert!(
        matches!(&e, ToolError::Denied(m) if m.contains("changed")),
        "{e}"
    );
    assert!(k.exec.commands().is_empty());
    assert_eq!(k.files.get("web-1", CONF_PATH).unwrap(), CONF);
    assert!(matches!(
        call(&k, "apply_change", json!({ "change_id": "change-zzz" })).await,
        Err(ToolError::Invalid(_))
    ));
}

#[tokio::test]
async fn denied_approval_or_denied_step_stores_no_plan() {
    let k = plan_kit();
    k.broker.push(ApprovalOutcome::Denied {
        by: "alice".into(),
        reason: None,
    });
    assert!(call(&k, "propose_change", plan_input(false)).await.is_err());
    assert!(call(&k, "status", json!({})).await.unwrap()["changes"]
        .as_array()
        .unwrap()
        .is_empty());

    // The engine denies one step: the whole plan is refused without asking.
    let k = kit(
        policy(),
        FakeEngine(Box::new(|c| {
            if c.command.as_deref().is_some_and(|x| x.contains("nginx -t")) {
                Decision::Deny {
                    reason: "no nginx -t today".into(),
                }
            } else {
                Decision::Allow { reason: "t".into() }
            }
        })),
        hosts(),
    );
    k.files.put("web-1", CONF_PATH, CONF);
    let e = call(&k, "propose_change", plan_input(false))
        .await
        .unwrap_err();
    assert!(matches!(e, ToolError::Denied(m) if m.contains("step-2")));
    assert_eq!(k.broker.count(), 0);
}

#[tokio::test]
async fn propose_validates_scope_paths_and_shape() {
    let mut p = policy();
    p.deny_paths = vec!["/etc/nginx/conf.d".into()];
    let k = kit(p, ask_writes(), hosts());
    assert!(matches!(
        call(&k, "propose_change", plan_input(false)).await,
        Err(ToolError::Denied(_))
    ));

    let k = plan_kit();
    let mut bad = plan_input(false);
    bad["steps"][1]["host"] = json!("vault-1");
    assert!(matches!(
        call(&k, "propose_change", bad).await,
        Err(ToolError::Invalid(_))
    ));
    let mut bad = plan_input(false);
    bad["steps"] = json!([]);
    assert!(call(&k, "propose_change", bad).await.is_err());
    assert_eq!(k.broker.count(), 0);
}

#[tokio::test]
async fn apply_stops_at_the_first_failure_and_undo_restores() {
    let k = plan_kit();
    k.exec.on("nginx -t", "nginx: configuration test failed", 1);
    let p = call(&k, "propose_change", plan_input(false)).await.unwrap();
    let id = p["change_id"].as_str().unwrap().to_string();
    let a = call(&k, "apply_change", json!({ "change_id": id }))
        .await
        .unwrap();
    assert_eq!(a["status"], "failed");
    assert_eq!(a["failed_step"], "step-2");
    assert_eq!(
        a["steps"].as_array().unwrap().len(),
        2,
        "step-3 must not run"
    );
    assert!(!k
        .exec
        .commands()
        .iter()
        .any(|c| c.contains("systemctl reload")));
    assert!(a["steps"][1]["untrusted_output"]
        .as_str()
        .unwrap()
        .contains("configuration test failed"));
    assert!(a["verify"].as_array().unwrap().is_empty());
    assert_eq!(k.files.get("web-1", CONF_PATH).unwrap(), PATCHED);

    // undo: needs approval, restores the file, runs the remaining rollback step.
    let before = k.broker.count();
    let u = call(&k, "undo", json!({ "change_id": id })).await.unwrap();
    assert_eq!(u["status"], "undone", "{u}");
    assert_eq!(k.broker.count(), before + 1);
    assert_eq!(k.files.get("web-1", CONF_PATH).unwrap(), CONF);
    assert!(k
        .exec
        .commands()
        .iter()
        .any(|c| c.contains("systemctl reload")));
    assert!(
        call(&k, "undo", json!({ "change_id": id })).await.is_err(),
        "already undone"
    );
}

#[tokio::test]
async fn failed_verify_is_reported_and_undoable() {
    let k = plan_kit();
    k.exec.on("curl -fsS web-1/health", "503", 22);
    let p = call(&k, "propose_change", plan_input(false)).await.unwrap();
    let a = call(&k, "apply_change", json!({ "change_id": p["change_id"] }))
        .await
        .unwrap();
    assert_eq!(a["status"], "applied_verify_failed");
    assert_eq!(a["failed_step"], "verify-1");
    assert_eq!(a["verify"][0]["ok"], false);
}

#[tokio::test]
async fn multi_host_plans_canary_the_first_host() {
    let k = plan_kit();
    k.exec.on("curl -fsS web-1/health", "503", 22);
    let p = call(&k, "propose_change", plan_input(true)).await.unwrap();
    assert_eq!(p["hosts"], json!(["web-1", "web-2"]));
    let a = call(&k, "apply_change", json!({ "change_id": p["change_id"] }))
        .await
        .unwrap();
    assert_eq!(a["status"], "applied_verify_failed");
    assert!(
        !k.exec
            .commands()
            .iter()
            .any(|c| c.contains("canary-passed web-2")),
        "the second host must not be touched when the canary's verify fails"
    );

    // And the happy path touches both, in order.
    let k = plan_kit();
    let p = call(&k, "propose_change", plan_input(true)).await.unwrap();
    let a = call(&k, "apply_change", json!({ "change_id": p["change_id"] }))
        .await
        .unwrap();
    assert_eq!(a["status"], "applied", "{a}");
    let cmds = k.exec.commands();
    let ix = |needle: &str| cmds.iter().position(|c| c.contains(needle)).unwrap();
    assert!(ix("curl -fsS web-1/health") < ix("canary-passed web-2"));
    assert!(ix("canary-passed web-2") < ix("curl -fsS web-2/health"));
}

#[tokio::test]
async fn unhealthy_service_fails_the_step() {
    let k = plan_kit();
    k.exec.on("systemctl is-active", "inactive", 3);
    let p = call(&k, "propose_change", plan_input(false)).await.unwrap();
    let a = call(&k, "apply_change", json!({ "change_id": p["change_id"] }))
        .await
        .unwrap();
    assert_eq!(a["failed_step"], "step-3");
    assert!(a["steps"][2]["error"]
        .as_str()
        .unwrap()
        .contains("not active"));
}

#[tokio::test]
async fn edit_file_backs_up_writes_and_undoes() {
    let k = plan_kit();
    let v = call(
        &k,
        "edit_file",
        json!({ "host": "web-1", "path": CONF_PATH, "patch": PATCH }),
    )
    .await
    .unwrap();
    assert_eq!(v["status"], "applied");
    assert_eq!(k.files.get("web-1", CONF_PATH).unwrap(), PATCHED);
    assert_eq!(k.broker.count(), 1);
    let req = k.broker.requests.lock().unwrap()[0].clone();
    // The approver sees the diff in the call arguments.
    assert!(
        matches!(&req.subject, ApprovalSubject::Call { call, .. } if call.args["patch"] == PATCH)
    );
    let backup = std::fs::read_to_string(
        std::path::Path::new(v["undo_point"].as_str().unwrap()).join("step-1.orig"),
    )
    .unwrap();
    assert_eq!(backup, CONF);

    let u = call(&k, "undo", json!({ "change_id": v["change_id"] }))
        .await
        .unwrap();
    assert_eq!(u["status"], "undone");
    assert_eq!(k.files.get("web-1", CONF_PATH).unwrap(), CONF);
}

#[tokio::test]
async fn edit_file_rejects_unclean_patches_without_writing() {
    let k = plan_kit();
    k.files.put(
        "web-1",
        CONF_PATH,
        "listen 81;\nproxy_pass http://127.0.0.1:9090;\n",
    );
    let e = call(
        &k,
        "edit_file",
        json!({ "host": "web-1", "path": CONF_PATH, "patch": PATCH }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&e, ToolError::Invalid(m) if m.contains("cleanly")),
        "{e}"
    );
    assert_eq!(
        k.files.get("web-1", CONF_PATH).unwrap(),
        "listen 81;\nproxy_pass http://127.0.0.1:9090;\n"
    );
    assert!(
        !k.exec.commands().iter().any(|c| c.contains("cp -p")),
        "no backup, no write"
    );
    assert!(call(
        &k,
        "edit_file",
        json!({ "host": "web-1", "path": CONF_PATH, "patch": "garbage" })
    )
    .await
    .is_err());
    assert!(call(
        &k,
        "edit_file",
        json!({ "host": "web-1", "path": "/etc/missing.conf", "patch": PATCH })
    )
    .await
    .is_err());
}

#[tokio::test]
async fn edit_file_stops_if_the_remote_backup_fails() {
    let k = plan_kit();
    k.exec.on("cp -p", "", 1);
    let e = call(
        &k,
        "edit_file",
        json!({ "host": "web-1", "path": CONF_PATH, "patch": PATCH }),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ToolError::Failed(m) if m.contains("back up")));
    assert_eq!(k.files.get("web-1", CONF_PATH).unwrap(), CONF);
}

#[tokio::test]
async fn service_tool_runs_and_checks_health() {
    let k = plan_kit();
    let v = call(
        &k,
        "service",
        json!({ "host": "web-1", "unit": "nginx.service", "action": "restart" }),
    )
    .await
    .unwrap();
    assert_eq!(v["status"], "ok");
    assert_eq!(v["healthy"], true);
    let cmds = k.exec.commands();
    assert!(cmds
        .iter()
        .any(|c| c == "sudo -n systemctl restart -- 'nginx.service'"));
    assert!(cmds.iter().any(|c| c.contains("is-active")));
    assert_eq!(k.broker.count(), 1);
    assert!(call(
        &k,
        "service",
        json!({ "host": "web-1", "unit": "a b", "action": "stop" })
    )
    .await
    .is_err());
    k.exec.on("is-active", "", 3);
    let v = call(
        &k,
        "service",
        json!({ "host": "web-1", "unit": "nginx", "action": "start" }),
    )
    .await
    .unwrap();
    assert_eq!(v["status"], "failed");
}

// ---------------------------------------------------------------- MCP round trip

#[tokio::test]
async fn mcp_round_trip_over_a_duplex_pipe() {
    let k = std_kit();
    k.exec.on("cat /etc/hostname", "web-1\n", 0);
    let server = build_server(k.ctx.clone());
    let (st, ct) = tokio::io::duplex(1 << 20);
    let srv = tokio::spawn(async move {
        let s = server.serve(st).await.expect("server init");
        s.waiting().await.expect("server run");
    });
    let mut client = ().serve(ct).await.expect("client init");

    // The client's name reaches the run.
    let info = client.peer_info().expect("server info");
    assert_eq!(
        info.server_info.as_ref().map(|i| i.name.as_str()),
        Some("bssh-ai")
    );
    assert!(info.instructions.as_deref().unwrap().contains("untrusted_"));
    assert!(k
        .audit
        .events()
        .iter()
        .any(|e| matches!(e, AiEvent::RunStarted { client, .. } if !client.is_empty())));

    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 15);
    assert!(tools
        .iter()
        .all(|t| t.output_schema.is_some() && t.annotations.is_some()));

    let r = client
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(false));
    let s = r.structured_content.expect("structured");
    assert_eq!(s["count"], 2);

    let args = json!({ "host": "web-1", "command": "cat /etc/hostname" });
    let r = client
        .call_tool(
            CallToolRequestParams::new("run_readonly")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let s = r.structured_content.expect("structured");
    assert_eq!(s["untrusted_output"], "web-1\n");
    assert_eq!(s["exit_code"], 0);
    assert!(r.content[0]
        .as_text()
        .unwrap()
        .text
        .contains("untrusted_output"));

    // Errors come back as tool errors the model can read.
    let r = client
        .call_tool(
            CallToolRequestParams::new("host_info")
                .with_arguments(json!({ "host": "vault-1" }).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    assert!(r.content[0]
        .as_text()
        .unwrap()
        .text
        .contains("unknown host"));

    client.close().await.unwrap();
    srv.await.unwrap();
}
