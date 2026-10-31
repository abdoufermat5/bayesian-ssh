//! Read tools: hosts, observe, files, logs, read-only commands.

use super::support::{call, hosts, std_kit};
use crate::services::ai::mcp::context::ToolError;
use crate::services::ai::mcp::testkit::{FakeEngine, allow_all, kit, policy};
use crate::services::ai::types::{
    AiEvent, ApprovalOutcome, ApprovalSubject, Decision, Enforcement, ExecMode, HostFacts,
    JailKind, ToolCall, ToolName,
};
use serde_json::json;

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
    assert_eq!(v["hosts"][0]["via_bastion"], false);
    assert!(v["hosts"][0]["bastion"].is_null());
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
    // The jail trials and the agent-user check run in one script; a zero exit keeps the
    // probe cacheable.
    k.exec.on("id -u bssh-agent", "", 0);
    let v = call(&k, "host_info", json!({ "host": "web-1" }))
        .await
        .unwrap();
    assert_eq!(v["enforcement"]["jail"], "none");
    assert_eq!(v["enforcement"]["certificates"], false);
    assert_eq!(v["untrusted_system"]["kernel"], "Linux 6.1.0");
    assert_eq!(v["via_bastion"], false);
    assert!(v["probe_error"].is_null());
    call(&k, "host_info", json!({ "host": "web-1" }))
        .await
        .unwrap();
    let probes = k
        .exec
        .raw()
        .iter()
        .filter(|(c, _)| c.contains("id -u bssh-agent"))
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
    assert!(
        call(
            &k,
            "observe",
            json!({ "host": "web-1", "what": "nonsense" })
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn observe_unparsable_output_is_an_error_not_a_guess() {
    let k = std_kit();
    k.exec.on("free -b", "free: command not found", 127);
    let e = call(&k, "observe", json!({ "host": "web-1", "what": "memory" }))
        .await
        .unwrap_err();
    assert!(matches!(e, ToolError::FailedAfterRun(_)), "{e}");
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
    // A read resolves the path on the host first.
    k.exec
        .on("readlink -f -- '/etc/app.env'", "/etc/app.env\n", 0);
    k.exec.on("readlink -f -- '/bin/blob'", "/bin/blob\n", 0);

    let v = call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "/etc/app.env" }),
    )
    .await
    .unwrap();
    assert!(
        v["untrusted_content"]
            .as_str()
            .unwrap()
            .contains("«redacted:vault-secret»")
    );
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
    assert!(
        call(
            &k,
            "read_file",
            json!({ "host": "web-1", "path": "/bin/blob" })
        )
        .await
        .is_err()
    );
    assert!(
        call(
            &k,
            "read_file",
            json!({ "host": "web-1", "path": "relative" })
        )
        .await
        .is_err()
    );
    // Refused calls are in the audit as denials.
    assert!(k.audit.calls().iter().any(
        |(c, d)| c.path.as_deref() == Some("/etc/shadow") && matches!(d, Decision::Deny { .. })
    ));
}

#[tokio::test]
async fn read_file_denies_a_symlink_into_a_deny_path() {
    let mut p = policy();
    p.read_paths = vec!["/var/log/**".into()];
    p.deny_paths = vec!["/root/.ssh".into()];
    let k = kit(p, allow_all(), hosts());
    k.files.put("web-1", "/var/log/app.log", "public log");
    k.files.put("web-1", "/root/.ssh/id_ed25519", "PRIVATE KEY");
    // A link inside the readable paths points at a deny path.
    k.exec.on(
        "readlink -f -- '/var/log/app.log'",
        "/root/.ssh/id_ed25519\n",
        0,
    );

    let e = call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "/var/log/app.log" }),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ToolError::Denied(_)), "{e}");
    assert!(e.to_string().contains("deny rule"), "{e}");
    // The refusal is audited against the target that was actually resolved.
    assert!(
        k.audit
            .calls()
            .iter()
            .any(|(c, d)| matches!(d, Decision::Deny { .. })
                && c.path.as_deref() == Some("/root/.ssh/id_ed25519"))
    );
}

#[tokio::test]
async fn read_file_reads_the_resolved_target() {
    let k = std_kit();
    k.files.put("web-1", "/srv/link", "the link itself");
    k.files.put("web-1", "/srv/real.txt", "the real target");
    k.exec
        .on("readlink -f -- '/srv/link'", "/srv/real.txt\n", 0);

    let v = call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "/srv/link" }),
    )
    .await
    .unwrap();
    assert_eq!(v["untrusted_content"], "the real target");
    assert_eq!(v["path"], "/srv/real.txt");
    let (c, _) = k
        .audit
        .calls()
        .into_iter()
        .find(|(c, _)| c.tool == ToolName::ReadFile)
        .expect("read call audited");
    assert_eq!(c.path.as_deref(), Some("/srv/real.txt"));
    assert_eq!(c.args["requested_path"], "/srv/link");
}

#[tokio::test]
async fn read_file_denies_when_the_host_cannot_resolve_the_path() {
    let k = std_kit();
    k.files.put("web-1", "/srv/file", "secret");
    // The fake host answers with nothing (and no readlink rule was scripted).
    let e = call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "/srv/file" }),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ToolError::Denied(_)), "{e}");

    let k = std_kit();
    k.exec.on("readlink", "", 1);
    let e = call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "/srv/file" }),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ToolError::Denied(_)), "{e}");
}

#[tokio::test]
async fn read_file_re_decides_the_policy_on_the_resolved_path() {
    // The engine stands in for the real one: only read_paths are allowed.
    let engine = FakeEngine(Box::new(|c: &ToolCall| {
        if c.path
            .as_deref()
            .is_some_and(|p| p.starts_with("/var/log/"))
        {
            Decision::Allow {
                reason: "read_paths".into(),
            }
        } else {
            Decision::Deny {
                reason: "outside read_paths".into(),
            }
        }
    }));
    let k = kit(policy(), engine, hosts());
    k.files.put("web-1", "/var/log/app.log", "public log");
    k.files.put("web-1", "/srv/secret", "secret");
    k.exec
        .on("readlink -f -- '/var/log/app.log'", "/srv/secret\n", 0);

    let e = call(
        &k,
        "read_file",
        json!({ "host": "web-1", "path": "/var/log/app.log" }),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ToolError::Denied(_)), "{e}");
    assert!(e.to_string().contains("outside read_paths"), "{e}");
}

#[test]
fn read_mode_uses_the_agent_principal_when_the_ca_is_trusted() {
    use crate::services::ai::mcp::context::RunContext;
    let facts = |jail: JailKind, certificates: bool| HostFacts {
        name: "web-1".into(),
        tags: vec![],
        tags_trusted: true,
        tag_source: None,
        enforcement: Enforcement {
            jail,
            certificates,
            agent_user: certificates,
        },
        probe_error: None,
    };
    assert_eq!(
        RunContext::read_mode(&facts(JailKind::None, true)),
        ExecMode::Plain,
        "no jail, nothing to wrap"
    );
    assert_eq!(
        RunContext::read_mode(&facts(JailKind::Landlock, false)),
        ExecMode::Jailed(JailKind::Landlock),
        "no CA: the jail runs as the login user"
    );
    assert_eq!(
        RunContext::read_mode(&facts(JailKind::Landlock, true)),
        ExecMode::JailedAsAgent(JailKind::Landlock)
    );
    assert_eq!(
        RunContext::read_mode(&facts(JailKind::Bubblewrap, true)),
        ExecMode::JailedAsAgent(JailKind::Bubblewrap)
    );
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

    // Not provably read-only on a host with no jail: a human approves the exact call.
    k.broker.push(ApprovalOutcome::Denied {
        by: "alice".into(),
        reason: Some("no".into()),
    });
    let e = call(
        &k,
        "run_readonly",
        json!({ "host": "web-1", "command": "rm -rf /tmp/x" }),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ToolError::Denied(_)), "{e}");
    assert!(!k.exec.commands().iter().any(|c| c.contains("rm -rf")));
    assert_eq!(k.broker.count(), 1);
    let asked = k.broker.requests.lock().unwrap()[0].clone();
    assert!(matches!(
        &asked.subject,
        ApprovalSubject::Call { call, .. }
            if call.host.as_deref() == Some("web-1")
                && call.command.as_deref() == Some("rm -rf /tmp/x")
    ));

    // Approved, it runs.
    call(
        &k,
        "run_readonly",
        json!({ "host": "web-1", "command": "rm -rf /tmp/x" }),
    )
    .await
    .unwrap();
    assert!(k.exec.commands().iter().any(|c| c == "rm -rf /tmp/x"));
}

#[tokio::test]
async fn huge_commands_are_rejected_before_analysis() {
    let k = std_kit();
    // 1 MiB of glob characters: an analyzer/glob matcher must never see this.
    let giant = format!("cat {}", "*".repeat(1024 * 1024));
    let start = std::time::Instant::now();
    for tool in ["run_readonly", "run", "run_many"] {
        let args = if tool == "run_many" {
            json!({ "tag": "web", "command": giant.clone() })
        } else {
            json!({ "host": "web-1", "command": giant.clone() })
        };
        let e = call(&k, tool, args).await.unwrap_err();
        assert!(
            matches!(&e, ToolError::Invalid(m) if m.contains("longer than")),
            "{tool}: {e}"
        );
    }
    assert!(start.elapsed().as_secs() < 2, "{:?}", start.elapsed());
    assert!(k.exec.commands().is_empty(), "nothing may run");
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
    let expect = crate::services::ai::mcp::util::sha256_hex(
        "x «redacted:vault-secret» y\n--stderr--\n".as_bytes(),
    );
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
    assert!(
        call(
            &k,
            "run_many",
            json!({ "tag": "hidden", "command": "uptime" })
        )
        .await
        .is_err()
    );
    // A non-read-only command asks once for every host; a denial runs nothing.
    k.broker.push(ApprovalOutcome::Denied {
        by: "alice".into(),
        reason: Some("not now".into()),
    });
    let e = call(&k, "run_many", json!({ "tag": "web", "command": "reboot" }))
        .await
        .unwrap_err();
    assert!(matches!(e, ToolError::Denied(_)), "{e}");
    assert!(!k.exec.commands().iter().any(|c| c == "reboot"));
    assert_eq!(k.broker.count(), 1, "one request for the whole call");
    let asked = k.broker.requests.lock().unwrap()[0].clone();
    let hosts: Vec<&str> = match &asked.subject {
        ApprovalSubject::Call { call, .. } => call.args["hosts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(hosts, ["web-1", "web-2"]);

    // Approved once, it runs on both hosts.
    let v = call(&k, "run_many", json!({ "tag": "web", "command": "reboot" }))
        .await
        .unwrap();
    assert_eq!(v["results"][0]["ok"], true);
    assert_eq!(
        k.exec.commands().iter().filter(|c| *c == "reboot").count(),
        2
    );
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
    assert!(
        raw.iter()
            .filter(|(c, _)| c == "make report" || c.contains("free -b"))
            .all(|(_, m)| *m == ExecMode::JailedAsAgent(JailKind::Landlock))
    );

    call(
        &k,
        "run",
        json!({ "host": "web-1", "command": "touch /tmp/x" }),
    )
    .await
    .unwrap();
    assert!(
        k.exec
            .raw()
            .iter()
            .any(|(c, m)| c == "touch /tmp/x" && *m == ExecMode::Certified)
    );
    // Another host stays plain.
    call(
        &k,
        "run",
        json!({ "host": "web-2", "command": "touch /tmp/y" }),
    )
    .await
    .unwrap();
    assert!(
        k.exec
            .raw()
            .iter()
            .any(|(c, m)| c == "touch /tmp/y" && *m == ExecMode::Plain)
    );
}
