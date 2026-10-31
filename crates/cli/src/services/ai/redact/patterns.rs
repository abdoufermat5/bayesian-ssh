//! The compiled regexes, built once.

use std::sync::LazyLock;

use regex::Regex;
use tracing::warn;

/// Regexes compiled once per process.
///
/// Building cannot fail for these literals (a test checks every rule), but a
/// failure disables only that rule and is logged without the pattern text.
pub(super) static PATTERNS: LazyLock<Patterns> = LazyLock::new(Patterns::build);

pub(super) struct Patterns {
    pub(super) private_key: Option<Regex>,
    pub(super) private_key_truncated: Option<Regex>,
    pub(super) aws_key: Option<Regex>,
    pub(super) aws_secret: Option<Regex>,
    pub(super) github_token: Option<Regex>,
    pub(super) gitlab_token: Option<Regex>,
    pub(super) slack_token: Option<Regex>,
    pub(super) stripe_key: Option<Regex>,
    pub(super) jwt: Option<Regex>,
    pub(super) bearer: Option<Regex>,
    pub(super) url_credentials: Option<Regex>,
    pub(super) key_value: Option<Regex>,
    pub(super) env_line: Option<Regex>,
}

impl Patterns {
    fn build() -> Self {
        Self {
            // RSA, EC, DSA, OPENSSH, ENCRYPTED, PKCS#8 and PGP blocks.
            private_key: compile(
                "private-key",
                r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY[A-Z0-9 ]*-----.*?-----END [A-Z0-9 ]*PRIVATE KEY[A-Z0-9 ]*-----",
            ),
            private_key_truncated: compile(
                "private-key",
                r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY[A-Z0-9 ]*-----.*$",
            ),
            aws_key: compile(
                "aws-key",
                r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA|ANVA|AIPA)[A-Z0-9]{16}\b",
            ),
            aws_secret: compile(
                "aws-secret",
                r#"(?i)(?P<pre>aws[_-]?secret[_-]?access[_-]?key\s*["']?\s*[:=]\s*)(?P<q>["']?)(?P<value>[A-Za-z0-9/+=]{40})(?P<postv>["']?)"#,
            ),
            github_token: compile(
                "github-token",
                r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})",
            ),
            gitlab_token: compile("gitlab-token", r"\bglpat-[A-Za-z0-9_-]{20,}"),
            slack_token: compile("slack-token", r"\bxox[abprs]-[A-Za-z0-9-]{10,}"),
            stripe_key: compile("stripe-key", r"\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,}"),
            jwt: compile(
                "jwt",
                r"\beyJ[A-Za-z0-9_-]{4,}\.eyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{4,}",
            ),
            bearer: compile(
                "bearer",
                r"(?i)(?P<pre>\bbearer\s+)(?P<value>[A-Za-z0-9._~+/=-]{20,})",
            ),
            url_credentials: compile(
                "url-credentials",
                r"\b(?P<scheme>[A-Za-z][A-Za-z0-9+.-]{1,15})://(?P<user>[^\s:/@]{1,64}):(?P<pass>[^\s@/]{1,256})@",
            ),
            key_value: compile(
                "key-value",
                r#"(?i)(?P<key>(?:[A-Za-z0-9-]*[_.-])?(?:password|passwd|pwd|secret|token|api[_-]?key|access[_-]?key|auth|credentials?|private[_-]?key)(?:[_.-][A-Za-z0-9]+)*)(?P<postq>["']?)(?P<sep>\s*[:=]\s*)(?P<q>["']?)(?P<value>[^\s"'`,;#<>()\[\]{}]+)(?P<postv>["']?)"#,
            ),
            env_line: compile(
                "env-secret",
                r#"(?m)^(?P<key>[A-Za-z_][A-Za-z0-9_]*)=(?P<q>["']?)(?P<value>[^\s"'#<>]+)(?P<postv>["']?)\r?$"#,
            ),
        }
    }
}

fn compile(rule: &str, pattern: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(_) => {
            // Never log the pattern or the input: only the rule name.
            warn!(rule, "redaction pattern failed to compile; rule disabled");
            None
        }
    }
}
