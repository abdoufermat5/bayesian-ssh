//! Policy decisions, approvals, budgets, the circuit breaker and run state.

use super::support::{call, hosts, std_kit};
use crate::services::ai::mcp::context::ToolError;
use crate::services::ai::mcp::testkit::{allow_all, ask_writes, kit, policy, FakeEngine};
use crate::services::ai::mcp::tools;
use crate::services::ai::types::{
    AiEvent, ApprovalOutcome, ApprovalSubject, Decision, Profile, Tier,
};
use serde_json::json;

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
        matches!(&e, ToolError::Denied(m) if m.contains("alice") && m.contains("no")),
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
async fn denial_burst_pauses_the_run_and_resume_clears_it() {
    let mut p = policy();
    p.budgets.max_denials_per_min = 3;
    let k = kit(
        p,
        FakeEngine(Box::new(|_| Decision::Deny {
            reason: "policy says no".into(),
        })),
        hosts(),
    );
    for _ in 0..3 {
        assert!(matches!(
            call(&k, "list_hosts", json!({})).await,
            Err(ToolError::Denied(_))
        ));
    }
    assert!(k.ctx.is_paused());
    assert!(!k.ctx.is_stopped());
    // Status still explains the pause.
    let e = call(
        &k,
        "run_readonly",
        json!({ "host": "web-1", "command": "uptime" }),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&e, ToolError::Denied(m) if m.contains("paused by the circuit breaker")),
        "{e}"
    );
    let ev = k.audit.events();
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Paused { .. })));
    k.ctx.resume("tester").unwrap();
    assert!(!k.ctx.is_paused());
    let ev = k.audit.events();
    assert!(ev.iter().any(|e| matches!(e, AiEvent::Resumed { .. })));
}

#[tokio::test]
async fn resume_refuses_a_run_that_is_not_paused_or_is_stopped() {
    let k = std_kit();
    assert_eq!(
        k.ctx.resume("tester").unwrap_err().to_string(),
        "the run is not paused"
    );
    k.ctx.stop("test");
    assert_eq!(
        k.ctx.resume("tester").unwrap_err().to_string(),
        "the run is stopped and cannot be resumed; start a new agent session"
    );
}

#[tokio::test]
async fn unknown_hosts_do_not_trip_the_breaker() {
    let mut p = policy();
    p.budgets.max_denials_per_min = 3;
    let k = kit(p, allow_all(), hosts());
    for _ in 0..5 {
        let _ = call(&k, "host_info", json!({ "host": "vault-1" })).await;
        let _ = call(&k, "host_info", json!({ "host": "nope" })).await;
    }
    assert!(!k.ctx.is_paused());
    assert!(call(&k, "status", json!({})).await.is_ok());
}

#[tokio::test]
async fn human_decisions_and_expiry_do_not_trip_the_breaker() {
    let mut p = policy();
    p.budgets.max_denials_per_min = 2;
    let k = kit(p, ask_writes(), hosts());
    for _ in 0..3 {
        k.broker.push(ApprovalOutcome::Denied {
            by: "alice".into(),
            reason: Some("no".into()),
        });
        assert!(call(
            &k,
            "run",
            json!({ "host": "web-1", "command": "touch /tmp/a" })
        )
        .await
        .is_err());
    }
    k.broker.push(ApprovalOutcome::Expired);
    assert!(call(
        &k,
        "run",
        json!({ "host": "web-1", "command": "touch /tmp/b" })
    )
    .await
    .is_err());
    assert!(!k.ctx.is_paused());
    assert!(call(&k, "status", json!({})).await.is_ok());
}

#[tokio::test]
async fn call_budget_denials_do_not_trip_the_breaker() {
    let mut p = policy();
    p.budgets.calls_per_min = 1;
    p.budgets.max_denials_per_min = 2;
    let k = kit(p, allow_all(), hosts());
    call(&k, "list_hosts", json!({})).await.unwrap();
    for _ in 0..3 {
        let e = call(&k, "list_hosts", json!({})).await.unwrap_err();
        assert!(matches!(e, ToolError::Denied(m) if m.contains("budget")));
    }
    assert!(!k.ctx.is_paused());
}

#[tokio::test]
async fn change_budget_is_checked_before_asking() {
    let mut p = policy();
    p.budgets.changes_per_run = 1;
    let k = kit(p, ask_writes(), hosts());
    k.ctx.enable_async_approvals();
    k.broker.hold();
    let v = call(
        &k,
        "run",
        json!({ "host": "web-1", "command": "touch /tmp/a" }),
    )
    .await
    .unwrap();
    assert_eq!(v["status"], "pending");
    assert_eq!(k.broker.count(), 1);

    // The budget is gone, so the second call never reaches a human.
    let e = call(
        &k,
        "run",
        json!({ "host": "web-1", "command": "touch /tmp/b" }),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ToolError::Denied(m) if m.contains("change budget exhausted")));
    assert_eq!(
        k.broker.count(),
        1,
        "no approval for a call that cannot run"
    );
}

#[tokio::test]
async fn pending_then_wait_approval_executes_once_even_concurrently() {
    let k = kit(policy(), ask_writes(), hosts());
    k.ctx.enable_async_approvals();
    k.broker.hold();
    let v = call(
        &k,
        "run",
        json!({ "host": "web-1", "command": "touch /tmp/a" }),
    )
    .await
    .unwrap();
    assert_eq!(v["status"], "pending");
    assert_eq!(v["tool"], "run");
    let id = v["request_id"].as_str().unwrap().to_string();
    assert!(v["approve_with"]
        .as_str()
        .unwrap()
        .contains("bssh ai watch"));
    assert_eq!(k.broker.count(), 1);
    assert!(!k.exec.commands().iter().any(|c| c == "touch /tmp/a"));

    let mut waits = Vec::new();
    for _ in 0..2 {
        let ctx = k.ctx.clone();
        let id = id.clone();
        waits.push(tokio::spawn(async move {
            tools::call(&ctx, "wait_approval", json!({ "request_id": id })).await
        }));
    }
    k.broker.push(ApprovalOutcome::Approved {
        by: "alice".into(),
        channel: "test".into(),
        grant: None,
    });
    let mut results = Vec::new();
    for h in waits {
        results.push(h.await.unwrap().unwrap());
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(results[0]["status"], "done");
    assert_eq!(results[0]["tool"], "run");
    assert_eq!(results[0]["result"]["exit_code"], 0);
    assert_eq!(
        k.exec
            .commands()
            .iter()
            .filter(|c| *c == "touch /tmp/a")
            .count(),
        1,
        "the approved action runs exactly once"
    );

    // The result is kept until the run ends.
    let again = call(&k, "wait_approval", json!({ "request_id": id }))
        .await
        .unwrap();
    assert_eq!(again, results[0]);
    assert_eq!(
        k.exec
            .commands()
            .iter()
            .filter(|c| *c == "touch /tmp/a")
            .count(),
        1
    );
}

#[tokio::test]
async fn wait_approval_rejects_an_unknown_request() {
    let k = std_kit();
    let e = call(&k, "wait_approval", json!({ "request_id": "nope" }))
        .await
        .unwrap_err();
    assert!(matches!(e, ToolError::Invalid(_)), "{e}");
}

#[test]
fn observe_profile_advertises_no_change_tools() {
    use crate::services::ai::mcp::server::instructions::tools_for;

    let all = tools::definitions();
    let names: Vec<String> = tools_for(&Profile::Observe, &all)
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    for hidden in [
        "propose_change",
        "apply_change",
        "edit_file",
        "service",
        "run",
        "undo",
    ] {
        assert!(!names.iter().any(|n| n == hidden), "{hidden} advertised");
    }
    for kept in [
        "read_file",
        "run_readonly",
        "request_access",
        "wait_approval",
    ] {
        assert!(names.iter().any(|n| n == kept), "{kept} missing");
    }

    let names: Vec<String> = tools_for(&Profile::Operate, &all)
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.iter().any(|n| n == "run"));
}

#[tokio::test]
async fn run_ttl_ends_the_run() {
    let mut p = policy();
    p.budgets.run_ttl = "1s".into();
    let k = kit(p, allow_all(), hosts());
    call(&k, "status", json!({})).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let e = call(&k, "list_hosts", json!({})).await.unwrap_err();
    assert!(
        matches!(&e, ToolError::Denied(m) if m.contains("expired")),
        "{e}"
    );
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
