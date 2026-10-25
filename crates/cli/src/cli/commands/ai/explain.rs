//! `bssh ai explain`.

use crate::cli::parser_ai::AiArgsExplain;
use crate::config::AppConfig;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::effects::{effects, Verb};
use crate::services::ai::policy::{self, DefaultPolicyEngine};
use crate::services::ai::store::AiStore;
use crate::services::ai::types::{
    Analysis, Analyzer, Decision, Enforcement, HostFacts, PolicyEngine, RiskTag, SimpleCommand,
    Tier, ToolCall, ToolName,
};
use crate::services::ai::ui::{self, Glyph, Style, Tone, Verdict};
use crate::services::SshService;
use anyhow::{bail, Result};
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
        &Style::terminal(),
        &analyzer,
        &command,
        &args.policy,
        args.host.as_deref(),
        &facts.tags,
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
    style: &Style,
    analyzer: &dyn Analyzer,
    command: &str,
    policy_name: &str,
    host: Option<&str>,
    host_tags: &[String],
    decide: impl FnOnce(&Analysis) -> Result<Decision>,
) -> (Analysis, String) {
    let analysis = analyzer.analyze(command);
    let text = match decide(&analysis) {
        Ok(decision) => render_explain(
            style,
            command,
            &analysis,
            &PolicySection::Resolved {
                name: policy_name,
                host,
                host_tags,
                decision: &decision,
            },
        ),
        Err(error) => {
            let error = error.to_string();
            render_explain(
                style,
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
        host_tags: &'a [String],
        decision: &'a Decision,
    },
    Unavailable {
        error: &'a str,
    },
}

/// Human-readable explain output: verdict line, policy reason, and parsed commands.
pub(crate) fn render_explain(
    style: &Style,
    _command: &str,
    analysis: &Analysis,
    policy: &PolicySection<'_>,
) -> String {
    let mut out = String::new();

    let (verdict_str, plain_sentence) = match policy {
        PolicySection::Resolved { decision, .. } => match decision {
            Decision::Allow { .. } => (style.verdict(Verdict::Allow), "runs without asking"),
            Decision::Deny { .. } => (style.verdict(Verdict::Deny), "nobody is asked"),
            Decision::Ask { tier, .. } => {
                let sentence = match tier {
                    Tier::InClient => "a person approves it, in the client",
                    Tier::OutOfBand => "a person approves it, outside the agent",
                    Tier::TwoPerson => "two people approve it, outside the agent",
                };
                (style.verdict(Verdict::Ask), sentence)
            }
        },
        PolicySection::Unavailable { .. } => (
            if style.color {
                style.paint(
                    Tone::Bad,
                    &format!("{} unavailable", style.glyph(Glyph::Bad)),
                )
            } else {
                "[unavailable]".to_string()
            },
            "policy unavailable",
        ),
    };
    let left = format!("{verdict_str}  {plain_sentence}");

    let right = match policy {
        PolicySection::Resolved {
            name,
            host,
            host_tags,
            ..
        } => {
            let host_part = match host {
                Some(h) if !h.is_empty() => {
                    if host_tags.is_empty() {
                        h.to_string()
                    } else {
                        format!("{h} ({})", host_tags.join(", "))
                    }
                }
                _ => String::new(),
            };
            let policy_part = format!("policy {name}");
            if host_part.is_empty() {
                policy_part
            } else {
                style.facts(&[&host_part, &policy_part])
            }
        }
        PolicySection::Unavailable { .. } => String::new(),
    };

    let term_width = ui::width();
    let left_w = ui::display_width(&left);
    let right_w = ui::display_width(&right);
    if right_w > 0 && left_w + 2 + right_w <= term_width {
        let spaces = term_width - left_w - right_w;
        out.push_str(&format!("{left}{}{right}\n", " ".repeat(spaces)));
    } else if right_w > 0 {
        out.push_str(&format!("{left}  {right}\n"));
    } else {
        out.push_str(&format!("{left}\n"));
    }

    match policy {
        PolicySection::Resolved { decision, .. } => {
            let reason = decision_reason(decision);
            if !reason.is_empty() {
                out.push_str(&format!("       {}\n", style.paint(Tone::Dim, reason)));
            }
        }
        PolicySection::Unavailable { error } => {
            out.push_str(&format!("       {}\n", style.paint(Tone::Dim, error)));
        }
    }

    if analysis.commands.is_empty() {
        out.push('\n');
        if let Some(reason) = &analysis.unknown_reason {
            out.push_str(&format!(
                "  The analyzer could not parse the command: {reason}\n"
            ));
        } else {
            out.push_str("  The analyzer could not parse the command\n");
        }
        return out;
    }

    out.push('\n');

    struct CmdRow {
        idx: String,
        cmd: String,
        verb: &'static str,
        risks: String,
        rule: Option<String>,
    }

    let max_idx_w = analysis.commands.len().to_string().len().max(1);
    let mut rows = Vec::with_capacity(analysis.commands.len());
    for (i, simple) in analysis.commands.iter().enumerate() {
        let idx = format!("  {:>width$}  ", i + 1, width = max_idx_w);
        let cmd = simple_command_string(simple);
        let verb = command_verb(simple);
        let risks = style.risks(&simple.tags);
        let rule = simple.rule.clone();
        rows.push(CmdRow {
            idx,
            cmd,
            verb,
            risks,
            rule,
        });
    }

    let idx_col_w = max_idx_w + 4;
    let max_verb_w = rows
        .iter()
        .map(|r| ui::display_width(r.verb))
        .max()
        .unwrap_or(0);
    let max_risks_w = rows
        .iter()
        .map(|r| ui::display_width(&r.risks))
        .max()
        .unwrap_or(0);
    let has_rules = rows.iter().any(|r| r.rule.is_some());
    let max_rule_w = if has_rules {
        rows.iter()
            .filter_map(|r| r.rule.as_ref())
            .map(|rule| ui::display_width(&format!("rule {rule}")))
            .max()
            .unwrap_or(0)
    } else {
        0
    };

    let fixed_overhead = idx_col_w
        + 3
        + max_verb_w
        + (if max_risks_w > 0 { 3 + max_risks_w } else { 0 })
        + (if has_rules { 3 + max_rule_w } else { 0 });
    let max_cmd_len = term_width.saturating_sub(fixed_overhead).max(20);

    let max_cmd_w = rows
        .iter()
        .map(|r| ui::display_width(&ui::truncate(&r.cmd, max_cmd_len)))
        .max()
        .unwrap_or(0);

    for r in &rows {
        let cmd_truncated = ui::truncate(&r.cmd, max_cmd_len);
        let cmd_padded = ui::pad(&cmd_truncated, max_cmd_w);
        let verb_padded = ui::pad(r.verb, max_verb_w);

        if has_rules {
            let risks_padded = ui::pad(&r.risks, max_risks_w);
            if let Some(rule) = &r.rule {
                let rule_str = style.paint(Tone::Dim, &format!("rule {rule}"));
                out.push_str(&format!(
                    "{}{cmd_padded}   {verb_padded}   {risks_padded}   {rule_str}\n",
                    r.idx
                ));
            } else {
                out.push_str(&format!(
                    "{}{cmd_padded}   {verb_padded}   {risks_padded}\n",
                    r.idx
                ));
            }
        } else if max_risks_w > 0 {
            out.push_str(&format!(
                "{}{cmd_padded}   {verb_padded}   {}\n",
                r.idx, r.risks
            ));
        } else {
            out.push_str(&format!("{}{cmd_padded}   {verb_padded}\n", r.idx));
        }
    }

    if let Some(reason) = &analysis.unknown_reason {
        out.push_str(&format!(
            "\n  The analyzer could not parse the full command: {reason}\n"
        ));
    }

    out
}

fn simple_command_string(simple: &SimpleCommand) -> String {
    let mut parts = Vec::new();
    for w in &simple.wrappers {
        parts.push(w.as_str());
    }
    parts.push(&simple.program);
    for a in &simple.args {
        parts.push(a.as_str());
    }
    parts.join(" ")
}

fn command_verb(simple: &SimpleCommand) -> &'static str {
    let single_analysis = Analysis {
        commands: vec![simple.clone()],
        tags: simple.tags.clone(),
        unknown_reason: None,
    };
    let eff = effects(&single_analysis);
    if let Some(item) = eff.items.first() {
        item.verb.word()
    } else if simple.tags.contains(&RiskTag::Read) && !simple.tags.contains(&RiskTag::Write) {
        "reads"
    } else {
        Verb::Runs.word()
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
    let source = crate::services::ai::source_name(config, &connection);
    Ok(policy::connection_facts(
        &connection,
        source.as_deref(),
        &config.ai.trusted_tag_sources,
        Enforcement::default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::SimpleCommand;
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    fn simple(program: &str, args: &[&str]) -> SimpleCommand {
        SimpleCommand {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            wrappers: Vec::new(),
            writes_to: Vec::new(),
            reads_from: Vec::new(),
            glob_args: Vec::new(),
            cwd: None,
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
    fn render_minimal_analysis() {
        let text = render_explain(
            &Style::plain(),
            "ls",
            &empty_analysis(),
            &PolicySection::Unavailable {
                error: "policy::resolve: not implemented yet",
            },
        );
        assert!(text.contains("[unavailable]  policy unavailable"));
        assert!(text.contains("policy::resolve: not implemented yet"));
        assert!(text.contains("The analyzer could not parse the command"));
    }

    #[test]
    fn render_unknown_reason_line() {
        let mut analysis = empty_analysis();
        analysis.tags = tags(&[RiskTag::Unknown]);
        analysis.unknown_reason = Some("contains eval".to_string());
        let text = render_explain(
            &Style::plain(),
            "eval $x",
            &analysis,
            &PolicySection::Unavailable { error: "e" },
        );
        assert!(text.contains("The analyzer could not parse the command: contains eval"));
    }

    #[test]
    fn render_full_report_aligns_columns() {
        let mut rm = simple("rm", &["-rf", "/var/www/releases/2026-08-14"]);
        rm.wrappers = vec!["sudo".to_string()];
        rm.tags = tags(&[RiskTag::Destructive, RiskTag::Privilege]);
        rm.rule = Some("rm".to_string());

        let mut reload = simple("systemctl", &["reload", "nginx"]);
        reload.tags = tags(&[RiskTag::ServiceChange]);
        reload.rule = Some("systemctl".to_string());

        let analysis = Analysis {
            commands: vec![rm, reload],
            tags: tags(&[
                RiskTag::Destructive,
                RiskTag::Privilege,
                RiskTag::ServiceChange,
            ]),
            unknown_reason: None,
        };
        let decision = Decision::Ask {
            tier: Tier::OutOfBand,
            reason: "Changes the host, and the default policy asks for every change.".to_string(),
        };
        let host_tags = vec!["staging".to_string()];
        let text = render_explain(
            &Style::plain(),
            "sudo rm -rf ...",
            &analysis,
            &PolicySection::Resolved {
                name: "default",
                host: Some("web-1"),
                host_tags: &host_tags,
                decision: &decision,
            },
        );
        assert!(text.contains("[ask]  a person approves it, outside the agent"));
        assert!(text.contains("web-1 (staging) · policy default"));
        assert!(text.contains("Changes the host, and the default policy asks for every change."));
        assert!(text.contains("1  sudo rm -rf /var/www/"));
        assert!(text.contains("deletes"));
        assert!(text.contains("destructive · privilege"));
        assert!(text.contains("rule rm"));
        assert!(text.contains("2  systemctl reload nginx"));
        assert!(text.contains("reloads"));
        assert!(text.contains("service"));
        assert!(text.contains("rule systemctl"));
    }

    #[test]
    fn render_colored_has_ansi_escapes() {
        let mut rm = simple("rm", &["-rf", "/var/www"]);
        rm.wrappers = vec!["sudo".to_string()];
        rm.tags = tags(&[RiskTag::Destructive]);
        let analysis = Analysis {
            commands: vec![rm],
            tags: tags(&[RiskTag::Destructive]),
            unknown_reason: None,
        };
        let decision = Decision::Ask {
            tier: Tier::OutOfBand,
            reason: "changes host".to_string(),
        };
        let style = Style {
            color: true,
            unicode: true,
        };
        let text = render_explain(
            &style,
            "sudo rm -rf /var/www",
            &analysis,
            &PolicySection::Resolved {
                name: "default",
                host: Some("web-1"),
                host_tags: &[],
                decision: &decision,
            },
        );
        assert!(text.contains("\x1b[33m◆ ask\x1b[0m"));
        assert!(text.contains("\x1b[90mchanges host\x1b[0m"));
    }

    #[test]
    fn render_verdict_sentences_for_each_case() {
        let style = Style::plain();
        let analysis = empty_analysis();

        // Allow
        let allow_dec = Decision::Allow {
            reason: "read".into(),
        };
        let text = render_explain(
            &style,
            "ls",
            &analysis,
            &PolicySection::Resolved {
                name: "default",
                host: None,
                host_tags: &[],
                decision: &allow_dec,
            },
        );
        assert!(text.contains("[allow]  runs without asking"));

        // Deny
        let deny_dec = Decision::Deny {
            reason: "forbidden".into(),
        };
        let text = render_explain(
            &style,
            "ls",
            &analysis,
            &PolicySection::Resolved {
                name: "default",
                host: None,
                host_tags: &[],
                decision: &deny_dec,
            },
        );
        assert!(text.contains("[deny]  nobody is asked"));

        // Ask in-client
        let ask_client = Decision::Ask {
            tier: Tier::InClient,
            reason: "r".into(),
        };
        let text = render_explain(
            &style,
            "ls",
            &analysis,
            &PolicySection::Resolved {
                name: "default",
                host: None,
                host_tags: &[],
                decision: &ask_client,
            },
        );
        assert!(text.contains("[ask]  a person approves it, in the client"));

        // Ask two-person
        let ask_2p = Decision::Ask {
            tier: Tier::TwoPerson,
            reason: "r".into(),
        };
        let text = render_explain(
            &style,
            "ls",
            &analysis,
            &PolicySection::Resolved {
                name: "default",
                host: None,
                host_tags: &[],
                decision: &ask_2p,
            },
        );
        assert!(text.contains("[ask]  two people approve it, outside the agent"));
    }

    #[test]
    fn render_omits_rule_when_none() {
        let mut analysis = empty_analysis();
        analysis.commands.push(simple("ls", &[]));
        let text = render_explain(
            &Style::plain(),
            "ls",
            &analysis,
            &PolicySection::Unavailable { error: "e" },
        );
        assert!(text.contains("1  ls"));
        assert!(!text.contains("rule"));
    }

    #[test]
    fn render_host_without_tags() {
        let decision = Decision::Allow {
            reason: "read-only".to_string(),
        };
        let text = render_explain(
            &Style::plain(),
            "ls",
            &empty_analysis(),
            &PolicySection::Resolved {
                name: "default",
                host: Some("web-1"),
                host_tags: &[],
                decision: &decision,
            },
        );
        assert!(text.contains("web-1 · policy default"));
        assert!(text.contains("[allow]  runs without asking"));
    }

    #[test]
    fn render_numbers_every_command() {
        let mut analysis = empty_analysis();
        for program in ["a", "b", "c"] {
            analysis.commands.push(simple(program, &[]));
        }
        let text = render_explain(
            &Style::plain(),
            "x",
            &analysis,
            &PolicySection::Unavailable { error: "e" },
        );
        assert!(text.contains("1  a"));
        assert!(text.contains("2  b"));
        assert!(text.contains("3  c"));
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
        let (got, text) = explain_report(
            &Style::plain(),
            &analyzer,
            "ls -la",
            "default",
            Some("db-1"),
            &[],
            |_| {
                Ok(Decision::Allow {
                    reason: "read".to_string(),
                })
            },
        );
        assert_eq!(got.commands[0].program, "ls");
        assert!(text.contains("[allow]  runs without asking"));
        assert!(text.contains("db-1 · policy default"));
        assert!(text.contains("1  ls -la"));
        assert_eq!(analyzer.seen.lock().unwrap().as_slice(), ["ls -la"]);
    }

    #[test]
    fn explain_says_when_a_scope_tag_was_ignored() {
        use crate::models::Connection;
        use crate::services::ai::types::{Policy, Profile};

        let dir = tempfile::tempdir().unwrap();
        let mut config = AppConfig {
            database_path: dir.path().join("history.db"),
            ..AppConfig::default()
        };
        let mut p = Policy {
            profile: Profile::Operate,
            ..Policy::default()
        };
        p.scope.tags = vec!["prod".into()];
        config.ai.policies.insert("ops".into(), p.clone());

        let mut conn = Connection::new(
            "db-1".into(),
            "h".into(),
            "u".into(),
            22,
            None,
            None,
            false,
            None,
        );
        conn.tags = vec!["prod".into()];
        conn.source_id = Some(uuid::Uuid::new_v4());
        let facts = policy::connection_facts(
            &conn,
            Some("aws-prod"),
            &config.ai.trusted_tag_sources,
            Enforcement::default(),
        );
        assert!(!facts.tags_trusted);

        let (_, text) = explain_report(
            &Style::plain(),
            &ShellAnalyzer::new(),
            "ls",
            "ops",
            Some("db-1"),
            &facts.tags,
            |analysis| {
                let engine = DefaultPolicyEngine::new(Arc::new(AiStore::open(&config)?));
                let call = tool_call("ls", &facts, analysis);
                Ok(engine.decide(&p, &call, Some(analysis), &facts))
            },
        );
        assert!(text.contains("nobody is asked"), "{text}");
        assert!(text.contains("ai.trusted_tag_sources"), "{text}");
        assert!(text.contains("aws-prod"), "{text}");
    }

    #[test]
    fn explain_report_turns_decision_errors_into_unavailable() {
        let (_, text) = explain_report(
            &Style::plain(),
            &ShellAnalyzer::new(),
            "ls",
            "default",
            None,
            &[],
            |_| bail!("store locked"),
        );
        assert!(text.contains("[unavailable]  policy unavailable"));
        assert!(text.contains("store locked"));
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
