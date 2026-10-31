//! Test data shared by the approval-module tests.

use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::control_client::RunHello;
use crate::services::ai::types::{
    Analyzer, ApprovalRequest, ApprovalSubject, ChangePlan, Step, StepAction, Tier, ToolCall,
    ToolName,
};
use chrono::{DateTime, Utc};

pub(super) fn hello() -> RunHello {
    RunHello {
        on_behalf_of: "ana".into(),
        hosts: vec!["web-1".into()],
        run_id: "dddbee19-f151-4ee0-8473-93924f35e1d4".into(),
        client: "opencode".into(),
        policy: "default".into(),
        started_at: "2026-10-24T17:23:20Z".into(),
    }
}

pub(super) fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-24T17:24:50Z")
        .unwrap()
        .with_timezone(&Utc)
}

pub(super) fn base(subject: ApprovalSubject) -> ApprovalRequest {
    ApprovalRequest {
        request_id: "440947ca-0000-0000-0000-000000000000".into(),
        run_id: "dddbee19-f151-4ee0-8473-93924f35e1d4".into(),
        client: "opencode 2.1.2".into(),
        tier: Tier::OutOfBand,
        subject,
        created_at: "2026-10-24T17:24:31Z".into(),
        expires_at: "2026-10-24T17:29:31Z".into(),
        reason: String::new(),
    }
}

pub(super) fn call(tool: ToolName, command: Option<&str>) -> ToolCall {
    ToolCall {
        run_id: "run1".into(),
        call_id: "c1".into(),
        tool,
        host: Some("db-1".into()),
        command: command.map(String::from),
        path: None,
        args: serde_json::Value::Null,
    }
}

pub(super) fn run_step(id: &str, command: &str) -> Step {
    Step {
        id: id.into(),
        host: Some("web-1".into()),
        action: StepAction::Run {
            command: command.into(),
        },
    }
}

pub(super) fn demo_plan_request() -> ApprovalRequest {
    let script = "A=/var/tmp/var-www-cleanup-2026-10-24\n\
        install -d -m 0750 \"$A/releases\" \"$A/cache/assets\"\n\
        mv /var/www/releases/2026-08-14 /var/www/releases/2026-09-02 \
        /var/www/releases/2026-09-21 \"$A/releases/\"\n\
        echo 'archive complete'";
    let plan = ChangePlan {
        change_id: "change-696de2f2".into(),
        run_id: "dddbee19".into(),
        title: "Free 228 MB on web-1 /var/www (archive 3 old releases + stale bundle)".into(),
        reason: "/var/www is a 310 MB tmpfs at 93%. I will keep the newest release, move \
            the three older ones and the stale bundle into a dated folder under /var/tmp, \
            and check that every moved file arrived before anything else happens. Nothing \
            is deleted, so the move can be put back with the undo step."
            .into(),
        steps: vec![run_step("step-1", script)],
        verify: vec![
            run_step("step-2", "df -h /var/www"),
            run_step("step-3", "test -f /var/tmp/a/releases/2026-08-14"),
            run_step("step-4", "test -f /var/tmp/a/releases/2026-09-02"),
            run_step("step-5", "test -f /var/tmp/a/releases/2026-09-21"),
            run_step("step-6", "test -f /var/tmp/a/cache/assets/bundle-old.js"),
        ],
        rollback: vec![run_step(
            "step-7",
            "mv /var/tmp/a/releases/2026-08-14 /var/www/releases/",
        )],
    };
    let analyses = vec![Some(ShellAnalyzer.analyze(script))];
    let mut r = base(ApprovalSubject::Plan {
        plan_hash: "486c2f5a".repeat(8),
        plan,
        analyses,
    });
    r.client = "opencode 2.1.2".into();
    r
}

pub(super) fn service_request(tier: Tier) -> ApprovalRequest {
    let mut c = call(
        ToolName::Service,
        Some("sudo -n systemctl restart -- nginx"),
    );
    c.args = serde_json::json!({"unit": "nginx", "action": "restart"});
    let mut r = base(ApprovalSubject::Call {
        call: c,
        analysis: Some(ShellAnalyzer.analyze("sudo -n systemctl restart -- nginx")),
    });
    r.tier = tier;
    r
}
