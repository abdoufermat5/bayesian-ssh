//! `bssh ai explain`. Owner: W2.

use crate::cli::parser_ai::AiArgsExplain;
use crate::config::AppConfig;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::policy::{self, DefaultPolicyEngine};
use crate::services::ai::store::AiStore;
use crate::services::ai::types::{
    Analysis, Analyzer, Decision, Enforcement, HostFacts, PolicyEngine, RiskTag, SimpleCommand,
    Tier, ToolCall, ToolName,
};
use crate::services::SshService;
use anyhow::{bail, Result};
use std::collections::BTreeSet;
use std::sync::Arc;

pub async fn execute(args: AiArgsExplain, config: AppConfig) -> Result<()> {
    let command = command_string(&args.command);
    let analyzer = ShellAnalyzer::new();

    if args.json {
        let analysis = analyzer.analyze(&command);
        println!("{}", serde_json::to_string_pretty(&analysis)?);
        return Ok(());
    }

    let facts = host_facts(&config, args.host.as_deref()).await?;
    let (_, report) = explain_report(
        &analyzer,
        &command,
        &args.policy,
        args.host.as_deref(),
        |analysis| decide(&config, &args.policy, &facts, &command, analysis),
    );
    print!("{report}");
    Ok(())
}

/// The command as the analyzer sees it: one argument stays verbatim (it was
/// quoted on the command line), several are joined with single spaces.
pub(crate) fn command_string(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        _ => parts.join(" "),
    }
}

/// Render the analysis and the decision. Any decision failure is shown as an
/// unavailable policy instead of failing the command.
pub(crate) fn explain_report(
    analyzer: &dyn Analyzer,
    command: &str,
    policy_name: &str,
    host: Option<&str>,
    decide: impl FnOnce(&Analysis) -> Result<Decision>,
) -> (Analysis, String) {
    let analysis = analyzer.analyze(command);
    let text = match decide(&analysis) {
        Ok(decision) => render_explain(
            command,
            &analysis,
            &PolicySection::Resolved {
                name: policy_name,
                host,
                decision: &decision,
            },
        ),
        Err(error) => {
            let error = error.to_string();
            render_explain(
                command,
                &analysis,
                &PolicySection::Unavailable { error: &error },
            )
        }
    };
    (analysis, text)
}

/// The decision part of the report.
pub(crate) enum PolicySection<'a> {
    Resolved {
        name: &'a str,
        host: Option<&'a str>,
        decision: &'a Decision,
    },
    Unavailable {
        error: &'a str,
    },
}

/// Human-readable explain output, one short line per fact.
pub(crate) fn render_explain(
    command: &str,
    analysis: &Analysis,
    policy: &PolicySection<'_>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("command: {command}\n"));
    out.push_str(&format!("tags:    {}\n", tags_label(&analysis.tags)));
    if let Some(reason) = &analysis.unknown_reason {
        out.push_str(&format!("unknown: {reason}\n"));
    }
    out.push_str("commands:\n");
    for (index, simple) in analysis.commands.iter().enumerate() {
        out.push_str(&format!("  {}. {}\n", index + 1, simple_line(simple)));
        out.push_str(&format!(
            "     {:<9} {}\n",
            "tags:",
            tags_label(&simple.tags)
        ));
        if !simple.wrappers.is_empty() {
            out.push_str(&format!(
                "     {:<9} {}\n",
                "wrappers:",
                simple.wrappers.join(", ")
            ));
        }
        if !simple.writes_to.is_empty() {
            out.push_str(&format!(
                "     {:<9} {}\n",
                "writes:",
                simple.writes_to.join(", ")
            ));
        }
        if let Some(rule) = &simple.rule {
            out.push_str(&format!("     {:<9} {rule}\n", "rule:"));
        }
    }
    match policy {
        PolicySection::Resolved {
            name,
            host,
            decision,
        } => {
            out.push_str(&format!(
                "policy:  {}  host: {}\n",
                name,
                host.unwrap_or("-")
            ));
            out.push_str(&format!(
                "decision: {} - {}\n",
                decision_label(decision),
                decision_reason(decision)
            ));
        }
        PolicySection::Unavailable { error } => {
            out.push_str(&format!("policy:  unavailable ({error})\n"));
        }
    }
    out
}

fn simple_line(simple: &SimpleCommand) -> String {
    if simple.args.is_empty() {
        simple.program.clone()
    } else {
        format!("{} {}", simple.program, simple.args.join(" "))
    }
}

fn tags_label(tags: &BTreeSet<RiskTag>) -> String {
    if tags.is_empty() {
        return "none".to_string();
    }
    tags.iter()
        .map(|tag| tag_label(*tag))
        .collect::<Vec<_>>()
        .join(", ")
}

fn tag_label(tag: RiskTag) -> &'static str {
    match tag {
        RiskTag::Read => "read",
        RiskTag::Write => "write",
        RiskTag::ServiceChange => "service-change",
        RiskTag::Destructive => "destructive",
        RiskTag::Privilege => "privilege",
        RiskTag::Network => "network",
        RiskTag::ExfilRisk => "exfil-risk",
        RiskTag::Unknown => "unknown",
    }
}

fn tier_label(tier: Tier) -> &'static str {
    match tier {
        Tier::InClient => "in-client",
        Tier::OutOfBand => "out-of-band",
        Tier::TwoPerson => "two-person",
    }
}

/// `allow`, `ask (tier)` or `deny`.
pub(crate) fn decision_label(decision: &Decision) -> String {
    match decision {
        Decision::Allow { .. } => "allow".to_string(),
        Decision::Ask { tier, .. } => format!("ask ({})", tier_label(*tier)),
        Decision::Deny { .. } => "deny".to_string(),
    }
}

pub(crate) fn decision_reason(decision: &Decision) -> &str {
    match decision {
        Decision::Allow { reason } | Decision::Ask { reason, .. } | Decision::Deny { reason } => {
            reason
        }
    }
}

pub(crate) fn tool_call(command: &str, host: &HostFacts, analysis: &Analysis) -> ToolCall {
    ToolCall {
        run_id: "explain".to_string(),
        call_id: "explain-1".to_string(),
        tool: if analysis.is_read_only() {
            ToolName::RunReadonly
        } else {
            ToolName::Run
        },
        host: if host.name.is_empty() {
            None
        } else {
            Some(host.name.clone())
        },
        command: Some(command.to_string()),
        path: None,
        args: serde_json::Value::Null,
    }
}

fn decide(
    config: &AppConfig,
    policy_name: &str,
    host: &HostFacts,
    command: &str,
    analysis: &Analysis,
) -> Result<Decision> {
    let policy = policy::resolve(config, policy_name)?;
    let store = AiStore::open(config)?;
    let engine = DefaultPolicyEngine::new(Arc::new(store));
    let call = tool_call(command, host, analysis);
    Ok(engine.decide(&policy, &call, Some(analysis), host))
}

async fn host_facts(config: &AppConfig, name: Option<&str>) -> Result<HostFacts> {
    let Some(name) = name else {
        return Ok(HostFacts::default());
    };
    let service = SshService::new(config.clone())?;
    let Some(connection) = service.get_connection(name).await? else {
        bail!("host '{name}' not found");
    };
    Ok(HostFacts {
        name: connection.name,
        tags: connection.tags,
        enforcement: Enforcement::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::SimpleCommand;
    use std::sync::Mutex;

    fn simple(program: &str, args: &[&str]) -> SimpleCommand {
        SimpleCommand {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            wrappers: Vec::new(),
            writes_to: Vec::new(),
            tags: BTreeSet::new(),
            rule: None,
        }
    }

    fn tags(items: &[RiskTag]) -> BTreeSet<RiskTag> {
        items.iter().copied().collect()
    }

    fn empty_analysis() -> Analysis {
        Analysis {
            commands: Vec::new(),
            tags: BTreeSet::new(),
            unknown_reason: None,
        }
    }

    fn label(decision: &Decision) -> String {
        decision_label(decision)
    }

    #[test]
    fn command_string_uses_one_argument_verbatim() {
        let parts = vec!["echo 'a  b'".to_string()];
        assert_eq!(command_string(&parts), "echo 'a  b'");
    }

    #[test]
    fn command_string_joins_several_arguments() {
        let parts = vec!["ls".to_string(), "-la".to_string(), "/tmp".to_string()];
        assert_eq!(command_string(&parts), "ls -la /tmp");
    }

    #[test]
    fn command_string_handles_an_empty_list() {
        assert_eq!(command_string(&[]), "");
    }

    #[test]
    fn tags_label_is_none_when_empty() {
        assert_eq!(tags_label(&BTreeSet::new()), "none");
    }

    #[test]
    fn tags_label_follows_set_order() {
        assert_eq!(
            tags_label(&tags(&[RiskTag::Network, RiskTag::Read, RiskTag::Write])),
            "read, write, network"
        );
    }

    #[test]
    fn every_tag_has_a_label() {
        assert_eq!(tag_label(RiskTag::Read), "read");
        assert_eq!(tag_label(RiskTag::Write), "write");
        assert_eq!(tag_label(RiskTag::ServiceChange), "service-change");
        assert_eq!(tag_label(RiskTag::Destructive), "destructive");
        assert_eq!(tag_label(RiskTag::Privilege), "privilege");
        assert_eq!(tag_label(RiskTag::Network), "network");
        assert_eq!(tag_label(RiskTag::ExfilRisk), "exfil-risk");
        assert_eq!(tag_label(RiskTag::Unknown), "unknown");
    }

    #[test]
    fn allow_label_has_no_tier() {
        assert_eq!(
            label(&Decision::Allow {
                reason: "read".into()
            }),
            "allow"
        );
    }

    #[test]
    fn ask_label_names_each_tier() {
        assert_eq!(
            label(&Decision::Ask {
                tier: Tier::InClient,
                reason: "r".into()
            }),
            "ask (in-client)"
        );
        assert_eq!(
            label(&Decision::Ask {
                tier: Tier::OutOfBand,
                reason: "r".into()
            }),
            "ask (out-of-band)"
        );
        assert_eq!(
            label(&Decision::Ask {
                tier: Tier::TwoPerson,
                reason: "r".into()
            }),
            "ask (two-person)"
        );
    }

    #[test]
    fn deny_label_is_plain() {
        assert_eq!(
            label(&Decision::Deny {
                reason: "nope".into()
            }),
            "deny"
        );
    }

    #[test]
    fn decision_reason_is_exposed_for_every_variant() {
        assert_eq!(
            decision_reason(&Decision::Allow { reason: "a".into() }),
            "a"
        );
        assert_eq!(
            decision_reason(&Decision::Ask {
                tier: Tier::OutOfBand,
                reason: "b".into()
            }),
            "b"
        );
        assert_eq!(decision_reason(&Decision::Deny { reason: "c".into() }), "c");
    }

    #[test]
    fn render_minimal_analysis() {
        let text = render_explain(
            "ls",
            &empty_analysis(),
            &PolicySection::Unavailable {
                error: "policy::resolve: not implemented yet",
            },
        );
        assert_eq!(
            text,
            "command: ls\n\
             tags:    none\n\
             commands:\n\
             policy:  unavailable (policy::resolve: not implemented yet)\n"
        );
    }

    #[test]
    fn render_unknown_reason_line() {
        let mut analysis = empty_analysis();
        analysis.tags = tags(&[RiskTag::Unknown]);
        analysis.unknown_reason = Some("contains eval".to_string());
        let text = render_explain(
            "eval $x",
            &analysis,
            &PolicySection::Unavailable { error: "e" },
        );
        assert!(text.contains("unknown: contains eval\n"));
        assert!(text.contains("tags:    unknown\n"));
    }

    #[test]
    fn render_full_report_aligns_columns() {
        let mut restart = simple("systemctl", &["restart", "nginx"]);
        restart.wrappers = vec!["sudo".to_string(), "timeout".to_string()];
        restart.writes_to = vec!["/etc/nginx/nginx.conf".to_string()];
        restart.tags = tags(&[RiskTag::Write, RiskTag::Privilege]);
        restart.rule = Some("systemctl-restart".to_string());

        let mut cat = simple("cat", &["/etc/hosts"]);
        cat.tags = tags(&[RiskTag::Read]);

        let analysis = Analysis {
            commands: vec![restart, cat],
            tags: tags(&[RiskTag::Read, RiskTag::Network]),
            unknown_reason: None,
        };
        let decision = Decision::Ask {
            tier: Tier::OutOfBand,
            reason: "write outside read paths".to_string(),
        };
        let text = render_explain(
            "sudo systemctl restart nginx",
            &analysis,
            &PolicySection::Resolved {
                name: "default",
                host: Some("web-1"),
                decision: &decision,
            },
        );
        let expected = concat!(
            "command: sudo systemctl restart nginx\n",
            "tags:    read, network\n",
            "commands:\n",
            "  1. systemctl restart nginx\n",
            "     tags:     write, privilege\n",
            "     wrappers: sudo, timeout\n",
            "     writes:   /etc/nginx/nginx.conf\n",
            "     rule:     systemctl-restart\n",
            "  2. cat /etc/hosts\n",
            "     tags:     read\n",
            "policy:  default  host: web-1\n",
            "decision: ask (out-of-band) - write outside read paths\n",
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn render_omits_empty_optional_command_fields() {
        let mut analysis = empty_analysis();
        analysis.commands.push(simple("ls", &[]));
        let text = render_explain("ls", &analysis, &PolicySection::Unavailable { error: "e" });
        assert!(text.contains("  1. ls\n     tags:     none\n"));
        assert!(!text.contains("wrappers:"));
        assert!(!text.contains("writes:"));
        assert!(!text.contains("rule:"));
    }

    #[test]
    fn render_writes_only() {
        let mut command = simple("tee", &["out.txt"]);
        command.writes_to = vec!["out.txt".to_string(), "log.txt".to_string()];
        let mut analysis = empty_analysis();
        analysis.commands.push(command);
        let text = render_explain("tee", &analysis, &PolicySection::Unavailable { error: "e" });
        assert!(text.contains("     writes:   out.txt, log.txt\n"));
    }

    #[test]
    fn render_host_dash_when_no_host() {
        let decision = Decision::Allow {
            reason: "read-only".to_string(),
        };
        let text = render_explain(
            "ls",
            &empty_analysis(),
            &PolicySection::Resolved {
                name: "default",
                host: None,
                decision: &decision,
            },
        );
        assert!(text.contains("policy:  default  host: -\n"));
        assert!(text.contains("decision: allow - read-only\n"));
    }

    #[test]
    fn render_numbers_every_command() {
        let mut analysis = empty_analysis();
        for program in ["a", "b", "c"] {
            analysis.commands.push(simple(program, &[]));
        }
        let text = render_explain("x", &analysis, &PolicySection::Unavailable { error: "e" });
        assert!(text.contains("  1. a\n"));
        assert!(text.contains("  2. b\n"));
        assert!(text.contains("  3. c\n"));
    }

    #[test]
    fn explain_report_uses_the_injected_analyzer() {
        let analysis = Analysis {
            commands: vec![SimpleCommand {
                tags: tags(&[RiskTag::Read]),
                ..simple("ls", &["-la"])
            }],
            tags: tags(&[RiskTag::Read]),
            unknown_reason: None,
        };
        let analyzer = RecordingAnalyzer {
            seen: Mutex::new(Vec::new()),
            analysis,
        };
        let (got, text) = explain_report(&analyzer, "ls -la", "default", Some("db-1"), |_| {
            Ok(Decision::Allow {
                reason: "read".to_string(),
            })
        });
        assert_eq!(got.commands[0].program, "ls");
        let expected = concat!(
            "command: ls -la\n",
            "tags:    read\n",
            "commands:\n",
            "  1. ls -la\n",
            "     tags:     read\n",
            "policy:  default  host: db-1\n",
            "decision: allow - read\n",
        );
        assert_eq!(text, expected);
        assert_eq!(analyzer.seen.lock().unwrap().as_slice(), ["ls -la"]);
    }

    #[test]
    fn explain_report_turns_decision_errors_into_unavailable() {
        let (_, text) = explain_report(&ShellAnalyzer::new(), "ls", "default", None, |_| {
            bail!("store locked")
        });
        assert!(text.contains("policy:  unavailable (store locked)\n"));
        assert!(!text.contains("decision:"));
    }

    #[test]
    fn tool_call_marks_read_only_commands() {
        let analysis = Analysis {
            commands: vec![simple("cat", &["/etc/hosts"])],
            tags: tags(&[RiskTag::Read]),
            unknown_reason: None,
        };
        let call = tool_call("cat /etc/hosts", &HostFacts::default(), &analysis);
        assert_eq!(call.tool, ToolName::RunReadonly);
        assert_eq!(call.run_id, "explain");
        assert_eq!(call.call_id, "explain-1");
        assert_eq!(call.host, None);
        assert_eq!(call.command.as_deref(), Some("cat /etc/hosts"));
        assert_eq!(call.path, None);
        assert_eq!(call.args, serde_json::Value::Null);
    }

    #[test]
    fn tool_call_marks_changes_as_run() {
        let analysis = Analysis {
            commands: vec![simple("rm", &["-rf", "/tmp/x"])],
            tags: tags(&[RiskTag::Destructive]),
            unknown_reason: None,
        };
        let host = HostFacts {
            name: "web-1".to_string(),
            ..HostFacts::default()
        };
        let call = tool_call("rm -rf /tmp/x", &host, &analysis);
        assert_eq!(call.tool, ToolName::Run);
        assert_eq!(call.host.as_deref(), Some("web-1"));
    }

    #[test]
    fn tool_call_treats_unknown_analysis_as_run() {
        let analysis = Analysis {
            commands: Vec::new(),
            tags: tags(&[RiskTag::Unknown]),
            unknown_reason: Some("eval".to_string()),
        };
        let call = tool_call("eval $x", &HostFacts::default(), &analysis);
        assert_eq!(call.tool, ToolName::Run);
    }

    #[test]
    fn json_output_round_trips() {
        let analysis = Analysis {
            commands: vec![simple("ls", &["-la"])],
            tags: tags(&[RiskTag::Read]),
            unknown_reason: Some("test".to_string()),
        };
        let json = serde_json::to_string_pretty(&analysis).unwrap();
        let back: Analysis = serde_json::from_str(&json).unwrap();
        assert_eq!(back, analysis);
    }

    struct RecordingAnalyzer {
        seen: Mutex<Vec<String>>,
        analysis: Analysis,
    }

    impl Analyzer for RecordingAnalyzer {
        fn analyze(&self, command: &str) -> Analysis {
            self.seen.lock().unwrap().push(command.to_string());
            self.analysis.clone()
        }
    }
}
