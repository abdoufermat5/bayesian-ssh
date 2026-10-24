//! `bssh ai status`. Owner: W3.
//!
//! Active runs (from their control sockets): pending approvals and grants; plus the most
//! recent runs from the runs database.

use crate::cli::parser_ai::AiArgsStatus;
use crate::config::AppConfig;
use crate::services::ai::ai_dir;
use crate::services::ai::control_client::{connect_all, RunHello};
use crate::services::ai::store::{AiStore, RunRecord};
use crate::services::ai::types::{ApprovalRequest, Grant};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ActiveRun {
    #[serde(flatten)]
    pub run: RunHello,
    pub pending: Vec<ApprovalRequest>,
    pub grants: Vec<Grant>,
}

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub active: Vec<ActiveRun>,
    pub recent: Vec<RunRecord>,
}

pub async fn execute(args: AiArgsStatus, config: AppConfig) -> Result<()> {
    let mut active = Vec::new();
    for mut c in connect_all(&ai_dir(&config), "bssh-cli").await {
        let pending = c.pending().await.unwrap_or_default();
        let grants = c.grants().await.unwrap_or_default();
        active.push(ActiveRun {
            run: c.hello.clone(),
            pending,
            grants,
        });
    }
    // The database is optional context: status still works without it.
    let recent = match AiStore::open(&config).and_then(|s| s.list_runs(false, 10)) {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("status: no run history: {e}");
            Vec::new()
        }
    };
    let report = StatusReport { active, recent };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", render(&report));
    }
    Ok(())
}

pub(crate) fn render(r: &StatusReport) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    if r.active.is_empty() {
        let _ = writeln!(o, "No active AI runs.");
    } else {
        let _ = writeln!(o, "Active runs:");
        for a in &r.active {
            let _ = writeln!(
                o,
                "  {}  {}  policy '{}'  since {}",
                a.run.run_id, a.run.client, a.run.policy, a.run.started_at
            );
            let _ = writeln!(
                o,
                "    {} pending approval(s), {} active grant(s)",
                a.pending.len(),
                a.grants.len()
            );
            for g in &a.grants {
                let _ = writeln!(
                    o,
                    "    grant {}: {:?} on {}{} until {}",
                    g.grant_id,
                    g.tool,
                    g.hosts,
                    g.target
                        .as_ref()
                        .map(|t| format!(" ({t})"))
                        .unwrap_or_default(),
                    g.expires_at
                );
            }
        }
    }
    let ended: Vec<&RunRecord> = r.recent.iter().filter(|x| x.ended.is_some()).collect();
    if !ended.is_empty() {
        let _ = writeln!(o, "Recent runs:");
        for x in ended {
            let _ = writeln!(
                o,
                "  {}  {}  ended {}  {}",
                x.id,
                x.client,
                x.ended.as_deref().unwrap_or("-"),
                x.reason.as_deref().unwrap_or("")
            );
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::ToolName;

    #[test]
    fn empty_report() {
        let r = StatusReport {
            active: vec![],
            recent: vec![],
        };
        assert_eq!(render(&r), "No active AI runs.\n");
    }

    #[test]
    fn report_lists_runs_grants_and_history() {
        let r = StatusReport {
            active: vec![ActiveRun {
                run: RunHello {
                    run_id: "run1".into(),
                    client: "claude-code".into(),
                    policy: "default".into(),
                    started_at: "2026-10-24T10:00:00Z".into(),
                },
                pending: vec![],
                grants: vec![Grant {
                    grant_id: "g1".into(),
                    run_id: Some("run1".into()),
                    tool: ToolName::Service,
                    hosts: "web-*".into(),
                    target: Some("nginx".into()),
                    expires_at: "2026-10-24T11:00:00Z".into(),
                }],
            }],
            recent: vec![RunRecord {
                id: "run0".into(),
                client: "cursor".into(),
                on_behalf_of: "alice".into(),
                policy: "default".into(),
                started: "2026-10-24T08:00:00Z".into(),
                ended: Some("2026-10-24T09:00:00Z".into()),
                reason: Some("done".into()),
            }],
        };
        let text = render(&r);
        assert!(text.contains("run1  claude-code"));
        assert!(text.contains("0 pending approval(s), 1 active grant(s)"));
        assert!(text.contains("grant g1"));
        assert!(text.contains("(nginx)"));
        assert!(text.contains("Recent runs:"));
        assert!(text.contains("run0"));
    }

    #[test]
    fn json_flattens_the_run() {
        let r = StatusReport {
            active: vec![ActiveRun {
                run: RunHello {
                    run_id: "run1".into(),
                    client: "c".into(),
                    policy: "p".into(),
                    started_at: "t".into(),
                },
                pending: vec![],
                grants: vec![],
            }],
            recent: vec![],
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["active"][0]["run_id"], "run1");
    }
}
