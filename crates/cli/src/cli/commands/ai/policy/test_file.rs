//! Policy test files: one command per line, with an optional `# expect:` marker.

use crate::services::ai::types::Decision;

/// Why a policy test file could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(super) enum TestFileError {
    #[error("line {line_no}: missing expectation after 'expect:'")]
    MissingExpectation { line_no: usize },
    #[error("line {line_no}: invalid expectation '{value}' (want allow, ask or deny)")]
    InvalidExpectation { line_no: usize, value: String },
}

/// Expected decision of a `# expect:` marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExpectedDecision {
    Allow,
    Ask,
    Deny,
}

/// One command from a policy test file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TestLine {
    pub(super) line_no: usize,
    pub(super) command: String,
    pub(super) expect: Option<ExpectedDecision>,
}

/// One evaluated line, ready for the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TestOutcome {
    pub(super) line_no: usize,
    pub(super) command: String,
    pub(super) expect: Option<ExpectedDecision>,
    pub(super) got: Decision,
}

/// Parse a test file: one command per line, `# expect: allow|ask|deny` optional.
///
/// # Errors
///
/// Returns [`TestFileError`] for an `expect:` marker whose value is missing or not one of
/// `allow`, `ask` and `deny`.
pub(super) fn parse_test_file(text: &str) -> Result<Vec<TestLine>, TestFileError> {
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
fn parse_expect(comment: &str, line_no: usize) -> Result<Option<ExpectedDecision>, TestFileError> {
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
        "" => return Err(TestFileError::MissingExpectation { line_no }),
        _ => {
            return Err(TestFileError::InvalidExpectation {
                line_no,
                value: value.to_string(),
            });
        }
    };
    Ok(Some(expect))
}

/// The `expect` column text: `allow`, `ask`, `deny` or `-`.
pub(super) fn expected_label(expect: Option<ExpectedDecision>) -> &'static str {
    match expect {
        Some(ExpectedDecision::Allow) => "allow",
        Some(ExpectedDecision::Ask) => "ask",
        Some(ExpectedDecision::Deny) => "deny",
        None => "-",
    }
}

/// `allow`, `ask` or `deny`, without the tier or reason.
pub(super) fn decision_kind(decision: &Decision) -> &'static str {
    match decision {
        Decision::Allow { .. } => "allow",
        Decision::Ask { .. } => "ask",
        Decision::Deny { .. } => "deny",
    }
}

/// The reason a [`Decision`] carries.
pub(super) fn decision_reason(decision: &Decision) -> &str {
    match decision {
        Decision::Allow { reason } | Decision::Ask { reason, .. } | Decision::Deny { reason } => {
            reason
        }
    }
}

/// Whether a line's expectation disagrees with the decision it got.
pub(super) fn is_mismatch(expect: Option<ExpectedDecision>, got: &Decision) -> bool {
    match expect {
        None => false,
        Some(ExpectedDecision::Allow) => !matches!(got, Decision::Allow { .. }),
        Some(ExpectedDecision::Ask) => !matches!(got, Decision::Ask { .. }),
        Some(ExpectedDecision::Deny) => !matches!(got, Decision::Deny { .. }),
    }
}

/// How many rows disagree with their expectation.
pub(super) fn count_mismatches(rows: &[TestOutcome]) -> usize {
    rows.iter()
        .filter(|row| is_mismatch(row.expect, &row.got))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::ai::policy::test_support::{allow, ask, deny, outcome};

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
        assert_eq!(
            error,
            TestFileError::InvalidExpectation {
                line_no: 4,
                value: "maybe".to_string(),
            }
        );
        let message = error.to_string();
        assert!(message.contains("line 4"), "{message}");
        assert!(message.contains("maybe"), "{message}");
    }

    #[test]
    fn parse_reports_missing_expectation_value() {
        let error = parse_test_file("ls # expect:\n").unwrap_err();
        assert_eq!(error, TestFileError::MissingExpectation { line_no: 1 });
        assert_eq!(
            error.to_string(),
            "line 1: missing expectation after 'expect:'"
        );
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

    #[test]
    fn expected_label_names_each_expectation() {
        assert_eq!(expected_label(None), "-");
        assert_eq!(expected_label(Some(ExpectedDecision::Allow)), "allow");
        assert_eq!(expected_label(Some(ExpectedDecision::Ask)), "ask");
        assert_eq!(expected_label(Some(ExpectedDecision::Deny)), "deny");
    }
}
