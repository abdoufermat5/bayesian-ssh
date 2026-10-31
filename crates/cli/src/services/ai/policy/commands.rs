//! Text matching of `deny` patterns, for calls the analyzer could not fully parse.

use super::glob::{glob_match, GlobMode};
use crate::services::ai::types::{Analysis, ToolCall};

/// Command lines the `deny` patterns are matched against: every analyzed simple command
/// (program as written and by base name, then its arguments), plus the raw command split on
/// shell separators when there is no analysis.
pub(super) fn command_lines(call: &ToolCall, analysis: Option<&Analysis>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(a) = analysis {
        for c in &a.commands {
            let args = c.args.join(" ");
            let join = |prog: &str| {
                if args.is_empty() {
                    prog.to_string()
                } else {
                    format!("{prog} {args}")
                }
            };
            out.push(join(&c.program));
            if let Some(base) = c.program.rsplit('/').next()
                && base != c.program
            {
                out.push(join(base));
            }
        }
    }
    if let Some(cmd) = &call.command {
        out.push(cmd.trim().to_string());
        if analysis.is_none() {
            for part in cmd.split(['\n', ';', '|', '&']) {
                let part = part.trim();
                if !part.is_empty() {
                    out.push(part.to_string());
                }
            }
        }
    }
    out
}

/// Text matcher for deny patterns, used when a call has no analysis (or the AST matcher
/// misses). It matches each pattern against
/// the whole command line or its leading words (`"rm -rf /"` also hits `rm -rf / --x`).
/// The AST matcher (`super::analyzer::matches_deny`) runs first.
pub(super) fn matches_deny_fallback(
    patterns: &[String],
    call: &ToolCall,
    analysis: Option<&Analysis>,
) -> Option<String> {
    tracing::debug!("deny patterns: using the built-in fallback matcher");
    let lines = command_lines(call, analysis);
    patterns
        .iter()
        .find(|pat| {
            let prefix = format!("{pat} *");
            lines.iter().any(|l| {
                glob_match(pat, l, GlobMode::Text) || glob_match(&prefix, l, GlobMode::Text)
            })
        })
        .cloned()
}
