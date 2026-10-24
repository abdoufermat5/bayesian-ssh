//! `bssh ai policy`.

use crate::cli::parser_ai::{AiArgsPolicy, AiPolicyCommands};
use crate::config::AppConfig;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::policy::{self, DefaultPolicyEngine};
use crate::services::ai::store::AiStore;
use crate::services::ai::types::{
    Analysis, Analyzer, Decision, Enforcement, HostFacts, Policy, PolicyEngine, Profile, Scope,
    Tier, ToolCall, ToolName,
};
use crate::services::ai::ui::{self, Glyph, Style, Tone};
use crate::services::SshService;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub async fn execute(args: AiArgsPolicy, config: AppConfig) -> Result<()> {
    match args.command {
        AiPolicyCommands::List => {
            print!("{}", render_list(&Style::terminal(), &config.ai.policies));
            Ok(())
        }
        AiPolicyCommands::Show { name } => {
            let policy = policy::resolve(&config, &name)
                .map_err(|e| anyhow::anyhow!("policy '{name}': {e}"))?;
            print!("{}", render_show(&Style::terminal(), &name, &policy));
            Ok(())
        }
        AiPolicyCommands::Test { file, policy, host } => {
            run_test(&Style::terminal(), &config, &file, &policy, host.as_deref()).await
        }
    }
}

/// `bssh ai policy list`: one policy per line, plus the built-in default.
pub(crate) fn render_list(style: &Style, policies: &BTreeMap<String, Policy>) -> String {
    let mut names: Vec<&str> = policies.keys().map(|s| s.as_str()).collect();
    if !policies.contains_key("default") {
        names.push("default");
    }
    let width = names
        .iter()
        .map(|name| name.chars().count())
        .max()
        .unwrap_or(7)
        .max(7);
    let mut out = String::new();
    for (name, policy) in policies {
        out.push_str(&format!(
            "{}  {}  {}\n",
            ui::pad(name, width),
            ui::pad(profile_label(&policy.profile), 8),
            scope_summary(style, &policy.scope)
        ));
    }
    if !policies.contains_key("default") {
        out.push_str(&format!(
            "{}  {}  (built-in)\n",
            ui::pad("default", width),
            ui::pad("observe", 8)
        ));
    }
    out
}

/// `bssh ai policy show`: human-readable policy details.
pub(crate) fn render_show(style: &Style, name: &str, policy: &Policy) -> String {
    let mut out = String::new();
    let header = style.facts(&[
        &format!("policy {name}"),
        &format!("profile {}", profile_label(&policy.profile)),
    ]);
    out.push_str(&format!("{header}\n\n"));

    let label_w = 10;
    let mut row = |label: &str, val: &str| {
        let l = style.paint(Tone::Dim, label);
        out.push_str(&format!("  {}  {val}\n", ui::pad(&l, label_w)));
    };

    row("scope", &scope_summary(style, &policy.scope));

    let default_tier = match policy.approval.default {
        Tier::InClient => "in-client",
        Tier::OutOfBand => "out-of-band",
        Tier::TwoPerson => "two-person",
    };
    let mut app_parts = vec![format!("default: {default_tier}")];
    if !policy.approval.in_client.is_empty() {
        app_parts.push(format!(
            "in-client on tags: {}",
            policy.approval.in_client.join(", ")
        ));
    }
    if !policy.approval.two_person.is_empty() {
        app_parts.push(format!(
            "two-person on tags: {}",
            policy.approval.two_person.join(", ")
        ));
    }
    row(
        "approval",
        &style.facts(&app_parts.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
    );

    let reads = if policy.read_paths.is_empty() {
        "any in scope".to_string()
    } else {
        policy.read_paths.join(", ")
    };
    row("reads", &reads);

    let writes = if policy.write_paths.is_empty() {
        "none".to_string()
    } else {
        policy.write_paths.join(", ")
    };
    row("writes", &writes);

    let mut denies = policy.deny_paths.clone();
    denies.extend(policy.deny.iter().cloned());
    let denies_str = if denies.is_empty() {
        "none".to_string()
    } else {
        denies.join(", ")
    };
    row("denies", &denies_str);

    let budgets_str = style.facts(&[
        &format!("{} calls/min", policy.budgets.calls_per_min),
        &format!("{} changes/run", policy.budgets.changes_per_run),
        &format!("{} denials/min", policy.budgets.max_denials_per_min),
        &format!("{} ttl", policy.budgets.run_ttl),
    ]);
    row("budgets", &budgets_str);

    let redact_v = if policy.redact.vault_secrets {
        "vault secrets: yes"
    } else {
        "vault secrets: no"
    };
    let redact_p = format!("patterns: {}", policy.redact.patterns);
    row("redact", &style.facts(&[redact_v, &redact_p]));

    out
}

fn profile_label(profile: &Profile) -> &'static str {
    match profile {
        Profile::Observe => "observe",
        Profile::Operate => "operate",
        Profile::Oncall => "oncall",
    }
}

fn scope_summary(style: &Style, scope: &Scope) -> String {
    let mut parts = Vec::new();
    if !scope.tags.is_empty() {
        parts.push(format!("tags: {}", scope.tags.join(", ")));
    }
    if !scope.hosts.is_empty() {
        parts.push(format!("hosts: {}", scope.hosts.join(", ")));
    }
    if parts.is_empty() {
        "no hosts".to_string()
    } else {
        style.facts(&parts.iter().map(|s| s.as_str()).collect::<Vec<_>>())
    }
}

// ---------------------------------------------------------------- test files

/// Expected decision of a `# expect:` marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpectedDecision {
    Allow,
    Ask,
    Deny,
}

/// One command from a policy test file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestLine {
    pub line_no: usize,
    pub command: String,
    pub expect: Option<ExpectedDecision>,
}

/// One evaluated line, ready for the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestOutcome {
    pub line_no: usize,
    pub command: String,
    pub expect: Option<ExpectedDecision>,
    pub got: Decision,
}

/// Parse a test file: one command per line, `# expect: allow|ask|deny` optional.
pub(crate) fn parse_test_file(text: &str) -> Result<Vec<TestLine>> {
    let mut lines = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        if raw.trim().is_empty() {
            continue;
        }
        let (code, comment) = split_comment(raw);
        let command = code.trim();
        if command.is_empty() {
            continue;
        }
        let expect = match comment {
            Some(comment) => parse_expect(comment, line_no)?,
            None => None,
        };
        lines.push(TestLine {
            line_no,
            command: command.to_string(),
            expect,
        });
    }
    Ok(lines)
}

/// Split a line into code and trailing comment. A `#` starts a comment only at
/// a word boundary outside quotes; quotes and backslash escapes are respected.
fn split_comment(line: &str) -> (&str, Option<&str>) {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if !single => escaped = true,
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '#' if !single && !double => {
                let at_word_start = match line[..index].chars().next_back() {
                    None => true,
                    Some(previous) => previous.is_whitespace(),
                };
                if at_word_start {
                    return (&line[..index], Some(&line[index + 1..]));
                }
            }
            _ => {}
        }
    }
    (line, None)
}

/// Parse the `expect:` marker out of a comment. Unknown comments are ignored;
/// an `expect:` with a bad value is an error.
fn parse_expect(comment: &str, line_no: usize) -> Result<Option<ExpectedDecision>> {
    let trimmed = comment.trim();
    let Some((marker, value)) = trimmed.split_once(':') else {
        return Ok(None);
    };
    if !marker.trim().eq_ignore_ascii_case("expect") {
        return Ok(None);
    }
    let value = value.split_whitespace().next().unwrap_or("");
    let expect = match value.to_ascii_lowercase().as_str() {
        "allow" => ExpectedDecision::Allow,
        "ask" => ExpectedDecision::Ask,
        "deny" => ExpectedDecision::Deny,
        "" => bail!("line {line_no}: missing expectation after 'expect:'"),
        _ => bail!("line {line_no}: invalid expectation '{value}' (want allow, ask or deny)"),
    };
    Ok(Some(expect))
}

fn expected_label(expect: Option<ExpectedDecision>) -> &'static str {
    match expect {
        Some(ExpectedDecision::Allow) => "allow",
        Some(ExpectedDecision::Ask) => "ask",
        Some(ExpectedDecision::Deny) => "deny",
        None => "-",
    }
}

/// `allow`, `ask` or `deny`, without the tier or reason.
pub(crate) fn decision_kind(decision: &Decision) -> &'static str {
    match decision {
        Decision::Allow { .. } => "allow",
        Decision::Ask { .. } => "ask",
        Decision::Deny { .. } => "deny",
    }
}

pub(crate) fn decision_reason(decision: &Decision) -> &str {
    match decision {
        Decision::Allow { reason } | Decision::Ask { reason, .. } | Decision::Deny { reason } => {
            reason
        }
    }
}

pub(crate) fn is_mismatch(expect: Option<ExpectedDecision>, got: &Decision) -> bool {
    match expect {
        None => false,
        Some(ExpectedDecision::Allow) => !matches!(got, Decision::Allow { .. }),
        Some(ExpectedDecision::Ask) => !matches!(got, Decision::Ask { .. }),
        Some(ExpectedDecision::Deny) => !matches!(got, Decision::Deny { .. }),
    }
}

pub(crate) fn count_mismatches(rows: &[TestOutcome]) -> usize {
    rows.iter()
        .filter(|row| is_mismatch(row.expect, &row.got))
        .count()
}

/// Shorten a command to at most `max` characters, counting characters not bytes.
#[cfg(test)]
pub(crate) fn truncate_command(command: &str, max: usize) -> String {
    ui::truncate(command, max)
}

fn test_badge(style: &Style, expect: Option<ExpectedDecision>, got: &Decision) -> String {
    match expect {
        Some(_) => {
            if is_mismatch(expect, got) {
                style.paint(Tone::Bad, &format!("{} fail", style.glyph(Glyph::Bad)))
            } else {
                style.paint(Tone::Ok, &format!("{} pass", style.glyph(Glyph::Ok)))
            }
        }
        None => style.paint(Tone::Dim, "-"),
    }
}

/// Aligned table of the evaluated lines and a final summary.
pub(crate) fn render_test_table(style: &Style, rows: &[TestOutcome]) -> String {
    let line_width = rows
        .iter()
        .map(|row| row.line_no.to_string().len())
        .max()
        .unwrap_or(1)
        .max(1);

    let term_width = ui::width();
    let badge_width = 6;
    let expect_width = 6;
    let got_width = 5;

    let fixed_overhead = line_width + 2 + badge_width + 2 + expect_width + 2 + got_width + 2 + 25;
    let max_cmd_w = term_width.saturating_sub(fixed_overhead).clamp(25, 70);

    let command_width = rows
        .iter()
        .map(|row| ui::display_width(&ui::truncate(&row.command, max_cmd_w)))
        .max()
        .unwrap_or(0)
        .max(7);

    let mut out = String::new();
    out.push_str(&format!(
        "{}  {}  {}  {}  {}  {}\n",
        ui::pad(&style.label("#"), line_width),
        ui::pad(&style.label("result"), badge_width),
        ui::pad(&style.label("expect"), expect_width),
        ui::pad(&style.label("got"), got_width),
        ui::pad(&style.label("command"), command_width),
        style.label("reason")
    ));

    for row in rows {
        let badge = test_badge(style, row.expect, &row.got);
        let cmd = ui::truncate(&row.command, max_cmd_w);
        let reason = style.paint(Tone::Dim, &format!("({})", decision_reason(&row.got)));
        out.push_str(&format!(
            "{}  {}  {}  {}  {}  {}\n",
            ui::pad(&row.line_no.to_string(), line_width),
            ui::pad(&badge, badge_width),
            ui::pad(expected_label(row.expect), expect_width),
            ui::pad(decision_kind(&row.got), got_width),
            ui::pad(&cmd, command_width),
            reason,
        ));
    }

    let mismatches = count_mismatches(rows);
    let summary = if mismatches == 0 {
        format!(
            "{} {} commands, 0 mismatches\n",
            style.mark(Glyph::Ok),
            rows.len()
        )
    } else {
        format!(
            "{} {} commands, {} mismatch(es)\n",
            style.mark(Glyph::Bad),
            rows.len(),
            mismatches
        )
    };
    out.push_str(&summary);
    out
}

/// Analyze and decide every parsed line. The decider is the test seam.
pub(crate) fn evaluate_lines(
    lines: &[TestLine],
    analyzer: &dyn Analyzer,
    mut decide: impl FnMut(&TestLine, &Analysis) -> Result<Decision>,
) -> Result<Vec<TestOutcome>> {
    let mut rows = Vec::with_capacity(lines.len());
    for line in lines {
        let analysis = analyzer.analyze(&line.command);
        let got = decide(line, &analysis)?;
        rows.push(TestOutcome {
            line_no: line.line_no,
            command: line.command.clone(),
            expect: line.expect,
            got,
        });
    }
    Ok(rows)
}

async fn run_test(
    style: &Style,
    config: &AppConfig,
    file: &str,
    policy_name: &str,
    host: Option<&str>,
) -> Result<()> {
    let path = Path::new(file);
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let lines = parse_test_file(&text)?;
    let facts = host_facts(config, host).await?;
    let policy = policy::resolve(config, policy_name)
        .map_err(|e| anyhow::anyhow!("policy '{policy_name}': {e}"))?;
    let store =
        AiStore::open(config).map_err(|e| anyhow::anyhow!("opening the AI runs database: {e}"))?;
    let engine = DefaultPolicyEngine::new(Arc::new(store));
    let analyzer = ShellAnalyzer::new();
    let rows = evaluate_lines(&lines, &analyzer, |line, analysis| {
        let call = test_tool_call(line, &facts, analysis);
        Ok(engine.decide(&policy, &call, Some(analysis), &facts))
    })?;
    print!("{}", render_test_table(style, &rows));
    let mismatches = count_mismatches(&rows);
    if mismatches > 0 {
        bail!("{mismatches} mismatch(es)");
    }
    Ok(())
}

fn test_tool_call(line: &TestLine, host: &HostFacts, analysis: &Analysis) -> ToolCall {
    ToolCall {
        run_id: "policy-test".to_string(),
        call_id: format!("test-{}", line.line_no),
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
        command: Some(line.command.clone()),
        path: None,
        args: serde_json::Value::Null,
    }
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
    use crate::services::ai::types::{RiskTag, SimpleCommand, Tier};
    use std::collections::BTreeSet;

    fn allow(reason: &str) -> Decision {
        Decision::Allow {
            reason: reason.to_string(),
        }
    }

    fn ask(reason: &str) -> Decision {
        Decision::Ask {
            tier: Tier::OutOfBand,
            reason: reason.to_string(),
        }
    }

    fn deny(reason: &str) -> Decision {
        Decision::Deny {
            reason: reason.to_string(),
        }
    }

    fn outcome(
        line_no: usize,
        command: &str,
        expect: Option<ExpectedDecision>,
        got: Decision,
    ) -> TestOutcome {
        TestOutcome {
            line_no,
            command: command.to_string(),
            expect,
            got,
        }
    }

    // ------------------------------------------------------------ parsing

    #[test]
    fn parse_skips_blank_and_comment_lines() {
        let lines = parse_test_file("\n   \n# a note\n   # indented note\nls\n").unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].command, "ls");
        assert_eq!(lines[0].line_no, 5);
    }

    #[test]
    fn parse_command_without_expectation() {
        let lines = parse_test_file("ls -la\n").unwrap();
        assert_eq!(
            lines,
            vec![TestLine {
                line_no: 1,
                command: "ls -la".to_string(),
                expect: None,
            }]
        );
    }

    #[test]
    fn parse_each_expectation() {
        let lines = parse_test_file(
            "ls # expect: allow\nsystemctl restart nginx # expect: ask\nrm -rf / # expect: deny\n",
        )
        .unwrap();
        assert_eq!(lines[0].expect, Some(ExpectedDecision::Allow));
        assert_eq!(lines[1].expect, Some(ExpectedDecision::Ask));
        assert_eq!(lines[2].expect, Some(ExpectedDecision::Deny));
    }

    #[test]
    fn parse_marker_and_value_case_insensitively() {
        let lines = parse_test_file("ls # EXPECT: DENY\n").unwrap();
        assert_eq!(lines[0].expect, Some(ExpectedDecision::Deny));
        assert_eq!(lines[0].command, "ls");
    }

    #[test]
    fn parse_ignores_other_comments() {
        let lines = parse_test_file("ls # just a note\n").unwrap();
        assert_eq!(lines[0].expect, None);
        assert_eq!(lines[0].command, "ls");
    }

    #[test]
    fn parse_keeps_hash_inside_a_word() {
        let lines = parse_test_file("echo a#b # expect: allow\n").unwrap();
        assert_eq!(lines[0].command, "echo a#b");
        assert_eq!(lines[0].expect, Some(ExpectedDecision::Allow));
    }

    #[test]
    fn parse_keeps_hash_in_single_quotes() {
        let lines = parse_test_file("echo '# not a comment'\n").unwrap();
        assert_eq!(lines[0].command, "echo '# not a comment'");
        assert_eq!(lines[0].expect, None);
    }

    #[test]
    fn parse_keeps_hash_in_double_quotes() {
        let lines = parse_test_file("echo \"# not a comment\"\n").unwrap();
        assert_eq!(lines[0].command, "echo \"# not a comment\"");
    }

    #[test]
    fn parse_respects_escaped_hash_outside_quotes() {
        let lines = parse_test_file("echo \\# literal # expect: deny\n").unwrap();
        assert_eq!(lines[0].command, "echo \\# literal");
        assert_eq!(lines[0].expect, Some(ExpectedDecision::Deny));
    }

    #[test]
    fn parse_respects_escaped_quote_inside_double_quotes() {
        let lines = parse_test_file("echo \"x\\\"#\" # expect: ask\n").unwrap();
        assert_eq!(lines[0].command, "echo \"x\\\"#\"");
        assert_eq!(lines[0].expect, Some(ExpectedDecision::Ask));
    }

    #[test]
    fn parse_ignores_hash_glued_to_a_quote() {
        let lines = parse_test_file("echo 'a'#b\n").unwrap();
        assert_eq!(lines[0].command, "echo 'a'#b");
        assert_eq!(lines[0].expect, None);
    }

    #[test]
    fn parse_comment_after_quoted_argument() {
        let lines = parse_test_file("printf '# x' # expect: deny\n").unwrap();
        assert_eq!(lines[0].command, "printf '# x'");
        assert_eq!(lines[0].expect, Some(ExpectedDecision::Deny));
    }

    #[test]
    fn parse_trims_the_command() {
        let lines = parse_test_file("   ls -l   \n").unwrap();
        assert_eq!(lines[0].command, "ls -l");
    }

    #[test]
    fn parse_handles_crlf() {
        let lines = parse_test_file("ls\r\nexit\r\n").unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].command, "ls");
        assert_eq!(lines[1].command, "exit");
    }

    #[test]
    fn parse_reports_invalid_expectation_with_line_number() {
        let error = parse_test_file("# note\n\nls\nbad # expect: maybe\n").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("line 4"), "{message}");
        assert!(message.contains("maybe"), "{message}");
    }

    #[test]
    fn parse_reports_missing_expectation_value() {
        let error = parse_test_file("ls # expect:\n").unwrap_err();
        assert!(error.to_string().contains("line 1"));
    }

    #[test]
    fn parse_stops_at_the_first_bad_line() {
        let error = parse_test_file("ls # expect: nope\nrm # expect: deny\n").unwrap_err();
        assert!(error.to_string().contains("line 1"));
    }

    // ------------------------------------------------------------ decisions

    #[test]
    fn decision_kind_ignores_tier_and_reason() {
        assert_eq!(decision_kind(&allow("r")), "allow");
        assert_eq!(decision_kind(&ask("r")), "ask");
        assert_eq!(decision_kind(&deny("r")), "deny");
    }

    #[test]
    fn decision_reason_is_exposed_for_every_variant() {
        assert_eq!(decision_reason(&allow("a")), "a");
        assert_eq!(decision_reason(&ask("b")), "b");
        assert_eq!(decision_reason(&deny("c")), "c");
    }

    #[test]
    fn lines_without_expectation_never_mismatch() {
        assert!(!is_mismatch(None, &allow("r")));
        assert!(!is_mismatch(None, &ask("r")));
        assert!(!is_mismatch(None, &deny("r")));
    }

    #[test]
    fn matching_expectations_do_not_mismatch() {
        assert!(!is_mismatch(Some(ExpectedDecision::Allow), &allow("r")));
        assert!(!is_mismatch(Some(ExpectedDecision::Ask), &ask("r")));
        assert!(!is_mismatch(Some(ExpectedDecision::Deny), &deny("r")));
    }

    #[test]
    fn differing_expectations_mismatch() {
        assert!(is_mismatch(Some(ExpectedDecision::Deny), &allow("r")));
        assert!(is_mismatch(Some(ExpectedDecision::Allow), &ask("r")));
        assert!(is_mismatch(Some(ExpectedDecision::Ask), &deny("r")));
    }

    #[test]
    fn count_mismatches_counts_only_expected_differences() {
        let rows = vec![
            outcome(1, "a", Some(ExpectedDecision::Allow), allow("r")),
            outcome(2, "b", Some(ExpectedDecision::Deny), allow("r")),
            outcome(3, "c", None, deny("r")),
            outcome(4, "d", Some(ExpectedDecision::Ask), ask("r")),
        ];
        assert_eq!(count_mismatches(&rows), 1);
    }

    // ------------------------------------------------------------ table

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate_command("abc", 3), "abc");
        assert_eq!(truncate_command("abcd", 3), "ab…");
        assert_eq!(
            truncate_command(&"é".repeat(61), 60),
            format!("{}…", "é".repeat(59))
        );
    }

    #[test]
    fn truncate_keeps_exactly_sixty() {
        let command = "x".repeat(60);
        assert_eq!(truncate_command(&command, 60), command);
    }

    #[test]
    fn table_has_aligned_columns() {
        let rows = vec![
            outcome(
                1,
                "ls -la",
                Some(ExpectedDecision::Allow),
                allow("read only"),
            ),
            outcome(
                2,
                "systemctl restart nginx",
                Some(ExpectedDecision::Deny),
                ask("needs approval"),
            ),
            outcome(3, "rm -rf /", None, deny("destructive")),
        ];
        let text = render_test_table(&Style::plain(), &rows);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5);
        assert!(lines[0].starts_with("#  RESULT  EXPECT  GOT    COMMAND"));
        assert!(lines[0].ends_with("REASON"));
        assert!(lines[1].starts_with("1  ✓ pass  allow   allow  ls -la"));
        assert!(lines[2].starts_with("2  ✗ fail  deny    ask    systemctl restart nginx"));
        assert!(lines[3].starts_with("3  -       -       deny   rm -rf /"));
        assert_eq!(lines[4], "✗ 3 commands, 1 mismatch(es)");

        let reason_col = ui::display_width(&lines[1][..lines[1].rfind('(').unwrap()]);
        for line in &lines[1..4] {
            assert_eq!(
                ui::display_width(&line[..line.rfind('(').unwrap()]),
                reason_col
            );
        }
    }

    #[test]
    fn table_shows_reasons() {
        let rows = vec![outcome(1, "ls", None, allow("within read paths"))];
        let text = render_test_table(&Style::plain(), &rows);
        assert!(text.contains("(within read paths)"));
        assert!(text.contains("1 commands, 0 mismatches"));
    }

    #[test]
    fn table_handles_no_rows() {
        let text = render_test_table(&Style::plain(), &[]);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("#  RESULT  EXPECT  GOT"));
        assert_eq!(lines[1], "✓ 0 commands, 0 mismatches");
    }

    #[test]
    fn table_truncates_long_commands() {
        let command = "a".repeat(120);
        let rows = vec![outcome(1, &command, None, allow("r"))];
        let text = render_test_table(&Style::plain(), &rows);
        assert!(text.contains('…'));
        assert!(!text.contains(&"a".repeat(100)));
    }

    #[test]
    fn table_colored_has_ansi_escapes() {
        let rows = vec![
            outcome(1, "ls", Some(ExpectedDecision::Allow), allow("ok")),
            outcome(2, "rm", Some(ExpectedDecision::Allow), deny("denied")),
        ];
        let style = Style {
            color: true,
            unicode: true,
        };
        let text = render_test_table(&style, &rows);
        assert!(text.contains("\x1b[32m✓ pass\x1b[0m"));
        assert!(text.contains("\x1b[31m✗ fail\x1b[0m"));
    }

    // ------------------------------------------------------------ evaluation

    #[test]
    fn evaluate_lines_uses_the_injected_decider() {
        let parsed = parse_test_file("ls # expect: allow\nrm -rf / # expect: deny\n").unwrap();
        let analyzer = StaticAnalyzer(Analysis {
            commands: Vec::new(),
            tags: BTreeSet::new(),
            unknown_reason: None,
        });
        let rows = evaluate_lines(&parsed, &analyzer, |line, _| {
            if line.command.starts_with("ls") {
                Ok(allow("read"))
            } else {
                Ok(deny("destructive"))
            }
        })
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].line_no, 1);
        assert_eq!(rows[0].got, allow("read"));
        assert_eq!(rows[1].line_no, 2);
        assert_eq!(rows[1].got, deny("destructive"));
        assert_eq!(count_mismatches(&rows), 0);
    }

    #[test]
    fn evaluate_lines_counts_mismatches_against_expectations() {
        let parsed = parse_test_file("ls # expect: deny\n").unwrap();
        let analyzer = StaticAnalyzer(Analysis {
            commands: Vec::new(),
            tags: BTreeSet::new(),
            unknown_reason: None,
        });
        let rows = evaluate_lines(&parsed, &analyzer, |_, _| Ok(allow("read"))).unwrap();
        assert_eq!(count_mismatches(&rows), 1);
    }

    #[test]
    fn evaluate_lines_propagates_decider_errors() {
        let parsed = parse_test_file("ls\n").unwrap();
        let analyzer = StaticAnalyzer(Analysis {
            commands: Vec::new(),
            tags: BTreeSet::new(),
            unknown_reason: None,
        });
        let error = evaluate_lines(&parsed, &analyzer, |_, _| bail!("store locked")).unwrap_err();
        assert_eq!(error.to_string(), "store locked");
    }

    #[test]
    fn test_tool_call_names_the_line_and_command() {
        let line = TestLine {
            line_no: 7,
            command: "ls -la".to_string(),
            expect: None,
        };
        let analysis = Analysis {
            commands: vec![SimpleCommand {
                program: "ls".to_string(),
                args: vec!["-la".to_string()],
                wrappers: Vec::new(),
                writes_to: Vec::new(),
                tags: [RiskTag::Read].into_iter().collect(),
                rule: None,
            }],
            tags: [RiskTag::Read].into_iter().collect(),
            unknown_reason: None,
        };
        let host = HostFacts {
            name: "web-1".to_string(),
            ..HostFacts::default()
        };
        let call = test_tool_call(&line, &host, &analysis);
        assert_eq!(call.run_id, "policy-test");
        assert_eq!(call.call_id, "test-7");
        assert_eq!(call.tool, ToolName::RunReadonly);
        assert_eq!(call.host.as_deref(), Some("web-1"));
        assert_eq!(call.command.as_deref(), Some("ls -la"));
    }

    #[test]
    fn test_tool_call_marks_writes_as_run() {
        let line = TestLine {
            line_no: 1,
            command: "rm -rf /".to_string(),
            expect: None,
        };
        let analysis = Analysis {
            commands: Vec::new(),
            tags: [RiskTag::Destructive].into_iter().collect(),
            unknown_reason: None,
        };
        let call = test_tool_call(&line, &HostFacts::default(), &analysis);
        assert_eq!(call.tool, ToolName::Run);
        assert_eq!(call.host, None);
    }

    // ------------------------------------------------------------ list

    #[test]
    fn list_shows_the_builtin_default_when_missing() {
        let text = render_list(&Style::plain(), &BTreeMap::new());
        assert_eq!(text, "default  observe   (built-in)\n");
    }

    #[test]
    fn list_shows_profile_and_scope() {
        let mut policies = BTreeMap::new();
        let mut policy = Policy {
            profile: Profile::Operate,
            ..Policy::default()
        };
        policy.scope.tags = vec!["prod".to_string()];
        policy.scope.hosts = vec!["web-1".to_string(), "db-1".to_string()];
        policies.insert("prod".to_string(), policy);
        let text = render_list(&Style::plain(), &policies);
        assert_eq!(
            text,
            "prod     operate   tags: prod · hosts: web-1, db-1\ndefault  observe   (built-in)\n"
        );
    }

    #[test]
    fn list_omits_builtin_when_default_exists() {
        let mut policies = BTreeMap::new();
        policies.insert("default".to_string(), Policy::default());
        policies.insert(
            "oncall".to_string(),
            Policy {
                profile: Profile::Oncall,
                ..Policy::default()
            },
        );
        let text = render_list(&Style::plain(), &policies);
        assert!(text.contains("default  observe   no hosts\n"));
        assert!(text.contains("oncall   oncall    no hosts\n"));
        assert!(!text.contains("(built-in)"));
    }

    #[test]
    fn list_colored_has_dim_dot() {
        let mut policies = BTreeMap::new();
        let mut policy = Policy::default();
        policy.scope.tags = vec!["prod".to_string()];
        policy.scope.hosts = vec!["web-1".to_string()];
        policies.insert("prod".to_string(), policy);
        let style = Style {
            color: true,
            unicode: true,
        };
        let text = render_list(&style, &policies);
        assert!(text.contains("\x1b[90m·\x1b[0m"));
    }

    #[test]
    fn scope_summary_empty() {
        assert_eq!(
            scope_summary(&Style::plain(), &Scope::default()),
            "no hosts"
        );
    }

    #[test]
    fn show_formats_policy_fields() {
        let mut policy = Policy {
            profile: Profile::Operate,
            ..Policy::default()
        };
        policy.scope.tags = vec!["prod".to_string()];
        policy.scope.hosts = vec!["web-1".to_string()];
        policy.approval.in_client = vec!["staging".to_string()];
        policy.approval.two_person = vec!["prod".to_string()];
        policy.read_paths = vec!["/var/log/*".to_string()];
        policy.write_paths = vec!["/var/www/*".to_string()];
        policy.deny_paths = vec!["/etc/shadow".to_string()];
        policy.deny = vec!["reboot".to_string()];

        let text = render_show(&Style::plain(), "prod", &policy);
        assert!(text.contains("policy prod · profile operate"));
        assert!(text.contains("scope       tags: prod · hosts: web-1"));
        assert!(text.contains("approval    default: out-of-band · in-client on tags: staging · two-person on tags: prod"));
        assert!(text.contains("reads       /var/log/*"));
        assert!(text.contains("writes      /var/www/*"));
        assert!(text.contains("denies      /etc/shadow, reboot"));
        assert!(text.contains("budgets     30 calls/min · 10 changes/run · 5 denials/min · 2h ttl"));
        assert!(text.contains("redact      vault secrets: yes · patterns: default"));
    }

    #[test]
    fn show_colored_has_ansi_escapes() {
        let policy = Policy::default();
        let style = Style {
            color: true,
            unicode: true,
        };
        let text = render_show(&style, "default", &policy);
        assert!(text.contains("\x1b[90mscope\x1b[0m"));
        assert!(text.contains("\x1b[90mbudgets\x1b[0m"));
    }

    #[test]
    fn policy_json_round_trips() {
        let policy = Policy {
            profile: Profile::Operate,
            ..Policy::default()
        };
        let json = serde_json::to_string_pretty(&policy).unwrap();
        let back: Policy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }

    struct StaticAnalyzer(Analysis);

    impl Analyzer for StaticAnalyzer {
        fn analyze(&self, _command: &str) -> Analysis {
            self.0.clone()
        }
    }
}
