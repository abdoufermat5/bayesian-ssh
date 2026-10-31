//! `bssh ai policy test`: evaluate a file of commands against a policy.

use super::table::render_test_table;
use super::test_file::{count_mismatches, parse_test_file, TestLine, TestOutcome};
use crate::config::AppConfig;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::policy::{self, DefaultPolicyEngine};
use crate::services::ai::store::AiStore;
use crate::services::ai::types::{
    Analysis, Analyzer, Decision, Enforcement, HostFacts, PolicyEngine, ToolCall, ToolName,
};
use crate::services::ai::ui::Style;
use crate::services::SshService;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::sync::Arc;

/// The file, policy and optional host `bssh ai policy test` runs against.
pub(super) struct TestRun<'a> {
    pub(super) file: &'a str,
    pub(super) policy: &'a str,
    pub(super) host: Option<&'a str>,
}

/// Analyze and decide every parsed line. The decider is the test seam.
///
/// # Errors
///
/// Returns the decider's error; the analyzer itself is infallible.
pub(super) fn evaluate_lines(
    lines: Vec<TestLine>,
    analyzer: &dyn Analyzer,
    mut decide: impl FnMut(&TestLine, &Analysis) -> Result<Decision>,
) -> Result<Vec<TestOutcome>> {
    let mut rows = Vec::with_capacity(lines.len());
    for line in lines {
        let analysis = analyzer.analyze(&line.command);
        let got = decide(&line, &analysis)?;
        rows.push(TestOutcome {
            line_no: line.line_no,
            command: line.command,
            expect: line.expect,
            got,
        });
    }
    Ok(rows)
}

/// `bssh ai policy test`: print the table and fail on the first mismatch count.
///
/// # Errors
///
/// Fails when the file cannot be read or parsed, when the policy is unknown, when the AI
/// runs database cannot be opened, or when any line disagrees with its expectation.
pub(super) async fn run_test(
    style: &Style,
    config: &AppConfig,
    request: TestRun<'_>,
) -> Result<()> {
    let path = Path::new(request.file);
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let lines = parse_test_file(&text)?;
    let facts = host_facts(config, request.host).await?;
    let policy = policy::resolve(config, request.policy)
        .map_err(|e| anyhow::anyhow!("policy '{}': {e}", request.policy))?;
    let store =
        AiStore::open(config).map_err(|e| anyhow::anyhow!("opening the AI runs database: {e}"))?;
    let engine = DefaultPolicyEngine::new(Arc::new(store));
    let analyzer = ShellAnalyzer::new();
    let rows = evaluate_lines(lines, &analyzer, |line, analysis| {
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
    let connection =
        crate::cli::utils::resolve_connection(&service, name, "use", true, config).await?;
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
    use crate::cli::commands::ai::policy::test_support::{allow, deny};
    use crate::services::ai::types::{RiskTag, SimpleCommand};
    use std::collections::BTreeSet;

    struct StaticAnalyzer(Analysis);

    impl Analyzer for StaticAnalyzer {
        fn analyze(&self, _command: &str) -> Analysis {
            self.0.clone()
        }
    }

    fn empty_analysis() -> Analysis {
        Analysis {
            commands: Vec::new(),
            tags: BTreeSet::new(),
            unknown_reason: None,
        }
    }

    #[test]
    fn evaluate_lines_uses_the_injected_decider() {
        let parsed = parse_test_file("ls # expect: allow\nrm -rf / # expect: deny\n").unwrap();
        let analyzer = StaticAnalyzer(empty_analysis());
        let rows = evaluate_lines(parsed, &analyzer, |line, _| {
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
        let analyzer = StaticAnalyzer(empty_analysis());
        let rows = evaluate_lines(parsed, &analyzer, |_, _| Ok(allow("read"))).unwrap();
        assert_eq!(count_mismatches(&rows), 1);
    }

    #[test]
    fn evaluate_lines_propagates_decider_errors() {
        let parsed = parse_test_file("ls\n").unwrap();
        let analyzer = StaticAnalyzer(empty_analysis());
        let error = evaluate_lines(parsed, &analyzer, |_, _| bail!("store locked")).unwrap_err();
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
                reads_from: Vec::new(),
                glob_args: Vec::new(),
                cwd: None,
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
}
