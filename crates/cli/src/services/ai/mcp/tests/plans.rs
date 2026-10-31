//! Change plans: propose, apply, undo, edit_file and service.

use super::support::{call, hosts, CONF, CONF_PATH, PATCH, PATCHED};
use crate::services::ai::mcp::context::ToolError;
use crate::services::ai::mcp::testkit::{ask_writes, kit, policy, FakeEngine, Kit};
use crate::services::ai::types::{AiEvent, ApprovalOutcome, ApprovalSubject, Decision};
use serde_json::{json, Value};

fn plan_input() -> Value {
    plan_with_hosts(false)
}

fn canary_plan_input() -> Value {
    plan_with_hosts(true)
}

fn plan_with_hosts(second_host: bool) -> Value {
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
    let p = call(&k, "propose_change", plan_input()).await.unwrap();
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
    let p = call(&k, "propose_change", plan_input()).await.unwrap();
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
    assert!(call(&k, "propose_change", plan_input()).await.is_err());
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
    let e = call(&k, "propose_change", plan_input()).await.unwrap_err();
    assert!(matches!(e, ToolError::Denied(m) if m.contains("step-2")));
    assert_eq!(k.broker.count(), 0);
}

#[tokio::test]
async fn propose_validates_scope_paths_and_shape() {
    let mut p = policy();
    p.deny_paths = vec!["/etc/nginx/conf.d".into()];
    let k = kit(p, ask_writes(), hosts());
    assert!(matches!(
        call(&k, "propose_change", plan_input()).await,
        Err(ToolError::Denied(_))
    ));

    let k = plan_kit();
    let mut bad = plan_input();
    bad["steps"][1]["host"] = json!("vault-1");
    assert!(matches!(
        call(&k, "propose_change", bad).await,
        Err(ToolError::Invalid(_))
    ));
    let mut bad = plan_input();
    bad["steps"] = json!([]);
    assert!(call(&k, "propose_change", bad).await.is_err());
    assert_eq!(k.broker.count(), 0);
}

#[tokio::test]
async fn apply_stops_at_the_first_failure_and_undo_restores() {
    let k = plan_kit();
    k.exec.on("nginx -t", "nginx: configuration test failed", 1);
    let p = call(&k, "propose_change", plan_input()).await.unwrap();
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
    let p = call(&k, "propose_change", plan_input()).await.unwrap();
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
    let p = call(&k, "propose_change", canary_plan_input())
        .await
        .unwrap();
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
    let p = call(&k, "propose_change", canary_plan_input())
        .await
        .unwrap();
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
    let p = call(&k, "propose_change", plan_input()).await.unwrap();
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
