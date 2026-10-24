//! `bssh ai status`.
//!
//! Active runs (from their control sockets): pending approvals and grants; plus the most
//! recent runs from the runs database.

use crate::cli::parser_ai::AiArgsStatus;
use crate::config::AppConfig;
use crate::services::ai::ai_dir;
use crate::services::ai::control_client::{connect_all, RunHello};
use crate::services::ai::store::{AiStore, RunRecord};
use crate::services::ai::types::{AiEvent, ApprovalRequest, Grant};
use crate::services::ai::ui::{self, Glyph, Style};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct ActiveRun {
    #[serde(flatten)]
    pub run: RunHello,
    pub pending: Vec<ApprovalRequest>,
    pub grants: Vec<Grant>,
    #[serde(skip_serializing, default)]
    pub on_behalf_of: Option<String>,
    #[serde(skip_serializing, default)]
    pub hosts: Vec<String>,
    #[serde(skip_serializing, default)]
    pub calls: usize,
}

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub active: Vec<ActiveRun>,
    pub recent: Vec<RunRecord>,
}

pub async fn execute(args: AiArgsStatus, config: AppConfig) -> Result<()> {
    let mut active = Vec::new();
    let entries = crate::services::ai::audit::read_entries(&config).unwrap_or_default();
    let store = AiStore::open(&config).ok();
    for mut c in connect_all(&ai_dir(&config), "bssh-cli").await {
        let pending = c.pending().await.unwrap_or_default();
        let grants = c.grants().await.unwrap_or_default();
        let run_id = &c.hello.run_id;
        let mut on_behalf_of = Some(c.hello.on_behalf_of.clone()).filter(|o| !o.is_empty());
        let mut hosts = Vec::new();
        let mut calls = 0;
        for e in entries.iter().filter(|e| event_run_id(&e.event) == run_id) {
            match &e.event {
                AiEvent::RunStarted {
                    on_behalf_of: obo, ..
                } => {
                    on_behalf_of.get_or_insert_with(|| obo.clone());
                }
                AiEvent::Call { call, .. } => {
                    calls += 1;
                    if let Some(h) = &call.host {
                        if !hosts.contains(h) {
                            hosts.push(h.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        if on_behalf_of.is_none() {
            if let Some(s) = &store {
                if let Ok(Some(r)) = s.get_run(run_id) {
                    on_behalf_of = Some(r.on_behalf_of);
                }
            }
        }
        // Hosts it has touched; before its first call, the hosts in its scope.
        if hosts.is_empty() {
            hosts = c.hello.hosts.clone();
        }
        active.push(ActiveRun {
            run: c.hello.clone(),
            pending,
            grants,
            on_behalf_of,
            hosts,
            calls,
        });
    }
    // The database is optional context: status still works without it.
    let recent = store
        .as_ref()
        .and_then(|s| s.list_runs(false, 10).ok())
        .unwrap_or_default();
    let report = StatusReport { active, recent };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        let style = Style::terminal();
        print!("{}", render_status(&report, &style, Utc::now()));
    }
    Ok(())
}

fn event_run_id(e: &AiEvent) -> &str {
    match e {
        AiEvent::RunStarted { run_id, .. }
        | AiEvent::Approval { run_id, .. }
        | AiEvent::Result { run_id, .. }
        | AiEvent::Change { run_id, .. }
        | AiEvent::CircuitBreaker { run_id, .. }
        | AiEvent::RunEnded { run_id, .. } => run_id,
        AiEvent::Call { call, .. } => &call.run_id,
    }
}

pub fn render_status(r: &StatusReport, style: &Style, _now: DateTime<Utc>) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    if r.active.is_empty() {
        return "No agents are running.\n".to_string();
    }
    for (i, a) in r.active.iter().enumerate() {
        if i > 0 {
            o.push('\n');
        }
        let mark = style.mark(Glyph::Running);
        let client = &a.run.client;
        let run = format!("run {}", ui::short_id(&a.run.run_id));
        let policy = format!("policy {}", a.run.policy);
        let obo = format!("on behalf of {}", a.on_behalf_of.as_deref().unwrap_or("-"));
        let hosts = format!(
            "hosts {}",
            if a.hosts.is_empty() {
                "-".to_string()
            } else {
                a.hosts.join(", ")
            }
        );
        let since = format!("since {}", ui::clock(&a.run.started_at));

        let _ = writeln!(
            o,
            "{mark} {client}   {run}   {policy}   {obo}   {hosts}   {since}"
        );

        let pending_str = if a.pending.len() == 1 {
            "1 pending request".to_string()
        } else {
            format!("{} pending requests", a.pending.len())
        };
        let calls_str = if a.calls == 1 {
            "1 call so far".to_string()
        } else {
            format!("{} calls so far", a.calls)
        };
        let mut facts_parts = vec![pending_str.as_str(), calls_str.as_str()];
        let grants_str;
        if !a.grants.is_empty() {
            grants_str = if a.grants.len() == 1 {
                "1 active grant".to_string()
            } else {
                format!("{} active grants", a.grants.len())
            };
            facts_parts.push(&grants_str);
        }
        let _ = writeln!(o, "  {}", style.facts(&facts_parts));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_report() {
        let r = StatusReport {
            active: vec![],
            recent: vec![],
        };
        assert_eq!(
            render_status(&r, &Style::plain(), Utc::now()),
            "No agents are running.\n"
        );
    }

    #[test]
    fn report_renders_active_run_block() {
        let now = DateTime::parse_from_rfc3339("2026-10-24T17:25:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let r = StatusReport {
            active: vec![ActiveRun {
                run: RunHello {
                    on_behalf_of: "ana".into(),
                    hosts: vec!["web-1".into()],
                    run_id: "dddbee19-f151-4ee0-8473-93924f35e1d4".into(),
                    client: "opencode 2.1.2".into(),
                    policy: "default".into(),
                    started_at: "2026-10-24T17:23:20Z".into(),
                },
                pending: vec![],
                grants: vec![],
                on_behalf_of: Some("ana".into()),
                hosts: vec!["web-1".into()],
                calls: 38,
            }],
            recent: vec![],
        };
        let text = render_status(&r, &Style::plain(), now);
        let since = ui::clock("2026-10-24T17:23:20Z");
        assert!(text.contains(&format!(
            "● opencode 2.1.2   run dddbee19   policy default   on behalf of ana   hosts web-1   since {since}"
        )));
        assert!(text.contains("0 pending requests · 38 calls so far"));
    }

    #[test]
    fn json_flattens_the_run() {
        let r = StatusReport {
            active: vec![ActiveRun {
                run: RunHello {
                    on_behalf_of: "ana".into(),
                    hosts: vec!["web-1".into()],
                    run_id: "run1".into(),
                    client: "c".into(),
                    policy: "p".into(),
                    started_at: "t".into(),
                },
                pending: vec![],
                grants: vec![],
                on_behalf_of: None,
                hosts: vec![],
                calls: 0,
            }],
            recent: vec![],
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["active"][0]["run_id"], "run1");
        assert!(v["active"][0].get("on_behalf_of").is_none());
        assert!(v["active"][0].get("hosts").is_none());
        assert!(v["active"][0].get("calls").is_none());
    }
}
