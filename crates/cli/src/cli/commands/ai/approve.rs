//! `bssh ai approve`. Owner: W3.
//!
//! Connects to every live run socket, lists the approval requests waiting for a human, shows
//! the full subject (command, plan steps, diffs, analyzer tags, "the agent says") and sends
//! the decision. This is the trusted channel: the agent's process can't reach it.

use crate::cli::parser_ai::AiArgsApprove;
use crate::config::AppConfig;
use crate::services::ai::ai_dir;
use crate::services::ai::control_client::{connect_all, ControlClient, RunHello};
use crate::services::ai::types::{
    Analysis, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Grant, StepAction, Tier, ToolName,
};
use anyhow::{bail, Result};
use std::fmt::Write as _;
use std::io::{BufRead, Write};

pub async fn execute(args: AiArgsApprove, config: AppConfig) -> Result<()> {
    let dir = ai_dir(&config);
    let mut clients = connect_all(&dir, "bssh-cli").await;
    if clients.is_empty() {
        println!("No active AI runs. Start an agent with `bssh ai mcp` first.");
        return Ok(());
    }

    // (client index, request)
    let mut all: Vec<(usize, ApprovalRequest)> = Vec::new();
    for (i, c) in clients.iter_mut().enumerate() {
        match c.pending().await {
            Ok(reqs) => all.extend(reqs.into_iter().map(|r| (i, r))),
            Err(e) => eprintln!("run {}: {e}", c.hello.run_id),
        }
    }
    if all.is_empty() {
        println!("No pending approvals.");
        return Ok(());
    }

    if args.list {
        for (i, r) in &all {
            println!("{}", summary_line(&clients[*i].hello, r));
        }
        return Ok(());
    }

    let idx = match &args.request_id {
        Some(id) => find_request(&all, id)?,
        None if all.len() == 1 => 0,
        None => pick(&clients, &all)?,
    };
    let (ci, req) = all[idx].clone();
    println!("{}", render_request(&clients[ci].hello, &req));

    let who = format!("{}@cli", whoami::username());
    let approve = if args.deny {
        false
    } else {
        match prompt_choice()? {
            Choice::Approve => true,
            Choice::Deny => false,
            Choice::Skip => {
                println!("Left pending.");
                return Ok(());
            }
        }
    };

    let outcome = if approve {
        let grant = match args.grant {
            Some(mins) => {
                let g = build_grant(&req, mins);
                if g.is_none() {
                    println!("Note: grants only apply to single calls; approving without one.");
                }
                g
            }
            None => None,
        };
        ApprovalOutcome::Approved {
            by: who,
            channel: "cli".into(),
            grant,
        }
    } else {
        ApprovalOutcome::Denied {
            by: who,
            reason: args.reason.clone(),
        }
    };
    let granted = matches!(&outcome, ApprovalOutcome::Approved { grant: Some(_), .. });
    clients[ci].decide(&req.request_id, outcome).await?;
    if approve {
        println!(
            "Approved {}.{}",
            req.request_id,
            if granted { " Grant stored." } else { "" }
        );
        if req.tier == Tier::TwoPerson {
            println!("This request needs a second, different approver.");
        }
    } else {
        println!("Denied {}.", req.request_id);
    }
    Ok(())
}

/// Find a request by exact id or unique prefix.
fn find_request(all: &[(usize, ApprovalRequest)], id: &str) -> Result<usize> {
    if let Some(i) = all.iter().position(|(_, r)| r.request_id == id) {
        return Ok(i);
    }
    let hits: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, (_, r))| r.request_id.starts_with(id))
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => bail!("no pending request '{id}'"),
        _ => bail!("'{id}' matches several requests; give more characters"),
    }
}

fn pick(clients: &[ControlClient], all: &[(usize, ApprovalRequest)]) -> Result<usize> {
    for (n, (ci, r)) in all.iter().enumerate() {
        println!("{:>2}) {}", n + 1, summary_line(&clients[*ci].hello, r));
    }
    print!("Which one? [1-{}] ", all.len());
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    match line.trim().parse::<usize>() {
        Ok(n) if (1..=all.len()).contains(&n) => Ok(n - 1),
        _ => bail!("not a valid choice"),
    }
}

enum Choice {
    Approve,
    Deny,
    Skip,
}

fn prompt_choice() -> Result<Choice> {
    print!("[a]pprove, [d]eny or [s]kip? ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(match line.trim().to_lowercase().as_str() {
        "a" | "approve" | "y" | "yes" => Choice::Approve,
        "d" | "deny" | "n" | "no" => Choice::Deny,
        _ => Choice::Skip,
    })
}

fn summary_line(run: &RunHello, r: &ApprovalRequest) -> String {
    let what = match &r.subject {
        ApprovalSubject::Call { call, .. } => call
            .command
            .clone()
            .or_else(|| call.path.clone())
            .unwrap_or_else(|| format!("{:?}", call.tool)),
        ApprovalSubject::Plan { plan, .. } => format!("plan: {}", plan.title),
        ApprovalSubject::Access { reason, .. } => format!("access: {reason}"),
    };
    format!(
        "{}  {}  [{}]  {}",
        short_id(&r.request_id),
        run.client,
        tier_name(r.tier),
        what
    )
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn tier_name(t: Tier) -> &'static str {
    match t {
        Tier::InClient => "in-client",
        Tier::OutOfBand => "out-of-band",
        Tier::TwoPerson => "two-person",
    }
}

fn tags_line(a: &Analysis) -> String {
    let mut s = a
        .tags
        .iter()
        .map(|t| format!("{t:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(r) = &a.unknown_reason {
        let _ = write!(s, " (could not analyze: {r})");
    }
    s
}

fn describe_action(a: &StepAction) -> String {
    match a {
        StepAction::Run { command } => format!("run: {command}"),
        StepAction::EditFile { path, patch } => {
            let diff: String = patch.lines().map(|l| format!("\n        {l}")).collect();
            format!("edit {path}:{diff}")
        }
        StepAction::Service { unit, action } => {
            format!("service {unit}: {action:?}").to_lowercase()
        }
        StepAction::Undo { step } => format!("undo {step}"),
    }
}

/// Everything the approver needs to decide, as plain text.
pub(crate) fn render_request(run: &RunHello, r: &ApprovalRequest) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "Request {}", r.request_id);
    let _ = writeln!(
        o,
        "  from:    {} (self-reported), run {}, policy '{}'",
        r.client, run.run_id, run.policy
    );
    let _ = writeln!(o, "  tier:    {}", tier_name(r.tier));
    let _ = writeln!(o, "  expires: {}", r.expires_at);
    match &r.subject {
        ApprovalSubject::Call { call, analysis } => {
            let _ = writeln!(o, "  tool:    {:?}", call.tool);
            if let Some(h) = &call.host {
                let _ = writeln!(o, "  host:    {h}");
            }
            if let Some(c) = &call.command {
                let _ = writeln!(o, "  command: {c}");
            }
            if let Some(p) = &call.path {
                let _ = writeln!(o, "  path:    {p}");
            }
            if let Some(a) = analysis {
                let _ = writeln!(o, "  analyzer: {}", tags_line(a));
                for c in &a.commands {
                    if let Some(rule) = &c.rule {
                        let _ = writeln!(o, "    {} -> rule {rule}", c.program);
                    }
                }
            }
        }
        ApprovalSubject::Plan {
            plan,
            plan_hash,
            analyses,
        } => {
            let _ = writeln!(o, "  change:  {} ({})", plan.title, plan.change_id);
            let _ = writeln!(o, "  the agent says: {}", plan.reason);
            let _ = writeln!(o, "  plan hash: {plan_hash}");
            for (title, steps) in [
                ("steps", &plan.steps),
                ("verify", &plan.verify),
                ("rollback", &plan.rollback),
            ] {
                if steps.is_empty() {
                    continue;
                }
                let _ = writeln!(o, "  {title}:");
                for (i, s) in steps.iter().enumerate() {
                    let host = s.host.as_deref().unwrap_or("-");
                    let _ = writeln!(o, "    {} [{host}] {}", s.id, describe_action(&s.action));
                    if title == "steps" {
                        if let Some(Some(a)) = analyses.get(i) {
                            let _ = writeln!(o, "        analyzer: {}", tags_line(a));
                        }
                    }
                }
            }
        }
        ApprovalSubject::Access {
            scope,
            reason,
            duration,
        } => {
            let _ = writeln!(o, "  wider access for {duration}");
            let _ = writeln!(o, "  the agent says: {reason}");
            let _ = writeln!(o, "  tags:  {}", scope.tags.join(", "));
            let _ = writeln!(o, "  hosts: {}", scope.hosts.join(", "));
        }
    }
    o
}

/// A grant for the same kind of call, valid `minutes` from now. Only single calls qualify.
pub(crate) fn build_grant(r: &ApprovalRequest, minutes: u32) -> Option<Grant> {
    let ApprovalSubject::Call { call, .. } = &r.subject else {
        return None;
    };
    let target = match call.tool {
        ToolName::EditFile => call.path.clone(),
        ToolName::Service => call
            .args
            .get("unit")
            .and_then(|v| v.as_str())
            .map(String::from),
        _ => call.command.clone(),
    };
    Some(Grant {
        grant_id: uuid::Uuid::new_v4().to_string(),
        run_id: Some(r.run_id.clone()),
        tool: call.tool,
        hosts: call.host.clone().unwrap_or_else(|| "*".into()),
        target,
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(i64::from(minutes)))
            .to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::{
        ChangePlan, RiskTag, Scope, ServiceAction, SimpleCommand, Step, ToolCall,
    };
    use std::collections::BTreeSet;

    fn hello() -> RunHello {
        RunHello {
            run_id: "run1".into(),
            client: "claude-code".into(),
            policy: "default".into(),
            started_at: "2026-10-24T10:00:00Z".into(),
        }
    }

    fn base(subject: ApprovalSubject) -> ApprovalRequest {
        ApprovalRequest {
            request_id: "0123456789abcdef".into(),
            run_id: "run1".into(),
            client: "claude-code 1.0".into(),
            tier: Tier::OutOfBand,
            subject,
            created_at: "2026-10-24T10:00:00Z".into(),
            expires_at: "2026-10-24T10:05:00Z".into(),
        }
    }

    fn call(tool: ToolName) -> ToolCall {
        ToolCall {
            run_id: "run1".into(),
            call_id: "c1".into(),
            tool,
            host: Some("web-1".into()),
            command: Some("systemctl restart nginx".into()),
            path: None,
            args: serde_json::Value::Null,
        }
    }

    fn analysis() -> Analysis {
        let tags: BTreeSet<RiskTag> = [RiskTag::ServiceChange].into_iter().collect();
        Analysis {
            commands: vec![SimpleCommand {
                program: "systemctl".into(),
                args: vec!["restart".into(), "nginx".into()],
                wrappers: vec![],
                writes_to: vec![],
                tags: tags.clone(),
                rule: Some("systemctl-restart".into()),
            }],
            tags,
            unknown_reason: None,
        }
    }

    #[test]
    fn renders_a_call_with_analyzer_tags() {
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run),
            analysis: Some(analysis()),
        });
        let text = render_request(&hello(), &r);
        assert!(text.contains("command: systemctl restart nginx"));
        assert!(text.contains("host:    web-1"));
        assert!(text.contains("servicechange"));
        assert!(text.contains("rule systemctl-restart"));
        assert!(text.contains("out-of-band"));
    }

    #[test]
    fn renders_a_plan_with_steps_diff_and_agent_reason() {
        let plan = ChangePlan {
            change_id: "chg-1".into(),
            run_id: "run1".into(),
            title: "Raise worker count".into(),
            reason: "traffic doubled".into(),
            steps: vec![
                Step {
                    id: "step-1".into(),
                    host: Some("web-1".into()),
                    action: StepAction::EditFile {
                        path: "/etc/nginx/nginx.conf".into(),
                        patch: "-worker_processes 2;\n+worker_processes 4;".into(),
                    },
                },
                Step {
                    id: "step-2".into(),
                    host: Some("web-1".into()),
                    action: StepAction::Service {
                        unit: "nginx".into(),
                        action: ServiceAction::Reload,
                    },
                },
            ],
            verify: vec![Step {
                id: "step-3".into(),
                host: Some("web-1".into()),
                action: StepAction::Run {
                    command: "nginx -t".into(),
                },
            }],
            rollback: vec![],
        };
        let hash = plan.hash();
        let r = base(ApprovalSubject::Plan {
            plan,
            plan_hash: hash.clone(),
            analyses: vec![None, Some(analysis())],
        });
        let text = render_request(&hello(), &r);
        assert!(text.contains("the agent says: traffic doubled"));
        assert!(text.contains("+worker_processes 4;"));
        assert!(text.contains("-worker_processes 2;"));
        assert!(text.contains("step-2 [web-1] service nginx: reload"));
        assert!(text.contains("verify:"));
        assert!(!text.contains("rollback:"));
        assert!(text.contains(&hash));
        assert!(text.contains("analyzer: servicechange"));
    }

    #[test]
    fn renders_access_requests() {
        let r = base(ApprovalSubject::Access {
            scope: Scope {
                tags: vec!["prod".into()],
                hosts: vec!["db-1".into()],
            },
            reason: "debug outage".into(),
            duration: "30m".into(),
        });
        let text = render_request(&hello(), &r);
        assert!(text.contains("the agent says: debug outage"));
        assert!(text.contains("30m"));
        assert!(text.contains("db-1"));
    }

    #[test]
    fn unknown_analysis_is_flagged() {
        let a = Analysis {
            commands: vec![],
            tags: [RiskTag::Unknown].into_iter().collect(),
            unknown_reason: Some("eval".into()),
        };
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run),
            analysis: Some(a),
        });
        assert!(render_request(&hello(), &r).contains("could not analyze: eval"));
    }

    #[test]
    fn grant_for_a_call_targets_the_command_path_or_unit() {
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run),
            analysis: None,
        });
        let g = build_grant(&r, 30).unwrap();
        assert_eq!(g.tool, ToolName::Run);
        assert_eq!(g.hosts, "web-1");
        assert_eq!(g.target.as_deref(), Some("systemctl restart nginx"));
        assert_eq!(g.run_id.as_deref(), Some("run1"));

        let mut c = call(ToolName::EditFile);
        c.path = Some("/etc/nginx/a.conf".into());
        let g = build_grant(
            &base(ApprovalSubject::Call {
                call: c,
                analysis: None,
            }),
            5,
        )
        .unwrap();
        assert_eq!(g.target.as_deref(), Some("/etc/nginx/a.conf"));

        let mut c = call(ToolName::Service);
        c.args = serde_json::json!({"unit": "nginx"});
        let g = build_grant(
            &base(ApprovalSubject::Call {
                call: c,
                analysis: None,
            }),
            5,
        )
        .unwrap();
        assert_eq!(g.target.as_deref(), Some("nginx"));
    }

    #[test]
    fn grant_expires_in_the_future_and_is_refused_for_plans() {
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run),
            analysis: None,
        });
        let g = build_grant(&r, 30).unwrap();
        let exp = chrono::DateTime::parse_from_rfc3339(&g.expires_at).unwrap();
        let delta = exp.signed_duration_since(chrono::Utc::now());
        assert!(delta.num_minutes() >= 29 && delta.num_minutes() <= 30);

        let r = base(ApprovalSubject::Access {
            scope: Scope::default(),
            reason: "x".into(),
            duration: "1h".into(),
        });
        assert!(build_grant(&r, 30).is_none());
    }

    #[test]
    fn find_request_by_id_and_prefix() {
        let mut a = base(ApprovalSubject::Access {
            scope: Scope::default(),
            reason: "x".into(),
            duration: "1h".into(),
        });
        a.request_id = "abc111".into();
        let mut b = a.clone();
        b.request_id = "abd222".into();
        let all = vec![(0, a), (0, b)];
        assert_eq!(find_request(&all, "abc111").unwrap(), 0);
        assert_eq!(find_request(&all, "abd").unwrap(), 1);
        assert!(find_request(&all, "ab").is_err());
        assert!(find_request(&all, "zzz").is_err());
    }

    #[test]
    fn summary_line_is_one_line() {
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run),
            analysis: None,
        });
        let s = summary_line(&hello(), &r);
        assert!(!s.contains('\n'));
        assert!(s.starts_with("01234567"));
        assert!(s.contains("systemctl restart nginx"));
    }
}
