use super::*;
use crate::services::ai::types::{ServiceAction, Step, StepAction};

fn run(cmd: &str) -> StepInput {
    StepInput {
        host: Some("web-1".into()),
        run: Some(cmd.into()),
        ..Default::default()
    }
}

const PATCH: &str = "--- a/app.conf\n+++ b/app.conf\n@@ -1,2 +1,2 @@\n listen 80;\n-proxy_pass http://127.0.0.1:8080;\n+proxy_pass http://127.0.0.1:8081;\n";

fn input() -> ProposeInput {
    ProposeInput {
        title: "Fix nginx 502".into(),
        reason: "port changed".into(),
        steps: vec![
            StepInput {
                host: Some("web-1".into()),
                edit_file: Some("/etc/nginx//conf.d/app.conf".into()),
                patch: Some(PATCH.into()),
                ..Default::default()
            },
            run("nginx -t"),
            StepInput {
                host: Some("web-1".into()),
                service: Some("nginx".into()),
                action: Some(ServiceActionArg::Reload),
                ..Default::default()
            },
        ],
        verify: vec![run("curl -fsS localhost/health")],
        rollback: vec![StepInput {
            undo: Some("step-1".into()),
            ..Default::default()
        }],
    }
}

#[test]
fn builds_ids_and_normalizes_paths() {
    let p = build_plan("run", "change-1", input()).unwrap();
    assert_eq!(
        p.steps.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["step-1", "step-2", "step-3"]
    );
    assert_eq!(p.verify[0].id, "verify-1");
    assert_eq!(p.rollback[0].id, "rollback-1");
    match &p.steps[0].action {
        StepAction::EditFile { path, .. } => assert_eq!(path, "/etc/nginx/conf.d/app.conf"),
        _ => panic!(),
    }
    assert_eq!(plan_hosts(&p), ["web-1"]);
}

#[test]
fn hash_covers_steps_and_changes_with_them() {
    let a = build_plan("run", "c", input()).unwrap();
    let mut b = a.clone();
    assert_eq!(a.hash(), b.hash());
    b.steps.push(Step {
        id: "step-4".into(),
        host: Some("web-1".into()),
        action: StepAction::Run {
            command: "rm -rf /".into(),
        },
    });
    assert_ne!(a.hash(), b.hash());
    // run id, reason and change id are not part of the hash
    let mut c = a.clone();
    c.reason = "different".into();
    c.change_id = "other".into();
    assert_eq!(a.hash(), c.hash());
}

#[test]
fn rejects_bad_steps() {
    let mut i = input();
    i.steps.clear();
    assert!(build_plan("r", "c", i).is_err());

    let mut i = input();
    i.steps.push(StepInput {
        host: Some("h".into()),
        run: Some("ls".into()),
        service: Some("x".into()),
        action: Some(ServiceActionArg::Stop),
        ..Default::default()
    });
    assert!(
        build_plan("r", "c", i)
            .unwrap_err()
            .to_string()
            .contains("exactly one")
    );

    let mut i = input();
    i.steps[1].host = None;
    assert!(
        build_plan("r", "c", i)
            .unwrap_err()
            .to_string()
            .contains("host is required")
    );

    let mut i = input();
    i.rollback[0].undo = Some("step-2".into());
    assert!(
        build_plan("r", "c", i)
            .unwrap_err()
            .to_string()
            .contains("not an edit step")
    );

    let mut i = input();
    i.steps[2].service = Some("nginx; reboot".into());
    assert!(
        build_plan("r", "c", i)
            .unwrap_err()
            .to_string()
            .contains("invalid unit")
    );

    let mut i = input();
    i.steps[0].patch = Some("not a patch".into());
    assert!(build_plan("r", "c", i).is_err());

    let mut i = input();
    i.steps[0].edit_file = Some("/etc/../shadow".into());
    assert!(build_plan("r", "c", i).is_err());
}

#[test]
fn patches_apply_cleanly_or_not_at_all() {
    let orig = "listen 80;\nproxy_pass http://127.0.0.1:8080;\n";
    let out = apply_unified_diff(orig, PATCH).unwrap();
    assert_eq!(out, "listen 80;\nproxy_pass http://127.0.0.1:8081;\n");
    let drifted = "listen 81;\nproxy_pass http://127.0.0.1:9090;\n";
    assert!(apply_unified_diff(drifted, PATCH).is_err());
    assert!(apply_unified_diff(orig, "garbage").is_err());
}

#[test]
fn service_commands_are_quoted() {
    assert_eq!(
        service_command("nginx.service", ServiceAction::Restart, true),
        "sudo -n systemctl restart -- 'nginx.service'"
    );
    assert_eq!(
        service_command("a", ServiceAction::Stop, false),
        "systemctl stop -- 'a'"
    );
}

#[test]
fn multi_host_canary_order_is_first_use() {
    let mut i = input();
    i.steps.push(StepInput {
        host: Some("web-2".into()),
        run: Some("nginx -t".into()),
        ..Default::default()
    });
    let p = build_plan("r", "c", i).unwrap();
    assert_eq!(plan_hosts(&p), ["web-1", "web-2"]);
}
