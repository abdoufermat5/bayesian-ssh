//! Entry point: special cases, the rule table, then the read-only and risky program lists.

use super::lists::{EVAL_BUILTINS, INTERPRETERS, READ_ONLY, RUNS_CODE};
use super::matching::eval_rule;
use super::table::RULES;
use super::types::{Arg, Class};
use crate::services::ai::analyzer::special;
use crate::services::ai::types::RiskTag;

/// Classify one program (basename) with its arguments, wrappers already removed.
pub fn classify(prog: &str, args: &[Arg]) -> Class {
    if let Some(f) = special::lookup(prog) {
        return f(prog, args);
    }
    if let Some(rule) = RULES.iter().find(|r| r.programs.contains(&prog)) {
        return eval_rule(rule, prog, args);
    }
    if EVAL_BUILTINS.contains(&prog)
        && args
            .iter()
            .any(|a| !a.dynamic && (a.text.contains("$(") || a.text.contains('`')))
    {
        return Class::unknown(
            prog,
            format!("{prog}: argument holds a command substitution the builtin would run"),
        );
    }
    if READ_ONLY.contains(&prog) {
        return Class::new(prog, &[RiskTag::Read]);
    }
    if let Some(class) = classify_prefixed(prog) {
        return class;
    }
    if RUNS_CODE.contains(&prog) {
        return Class::unknown(prog, format!("{prog} runs arbitrary code"));
    }
    if INTERPRETERS
        .iter()
        .any(|i| prog == *i || prog.strip_prefix(i).is_some_and(is_version_suffix))
    {
        return Class::unknown(
            prog,
            format!("{prog} is an interpreter: it runs arbitrary code"),
        );
    }
    Class::unknown(prog, format!("unknown program '{prog}'"))
}

/// `python3`, `python3.11`, `perl5.36`
fn is_version_suffix(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.')
}

fn classify_prefixed(prog: &str) -> Option<Class> {
    use RiskTag::*;
    if prog == "mkfs" || prog.starts_with("mkfs.") || prog == "mke2fs" || prog == "mkswap" {
        return Some(Class::new("mkfs", &[Destructive]));
    }
    if prog == "fsck" || prog.starts_with("fsck.") || matches!(prog, "e2fsck" | "xfs_repair") {
        return Some(Class::new("fsck", &[Destructive]));
    }
    None
}
