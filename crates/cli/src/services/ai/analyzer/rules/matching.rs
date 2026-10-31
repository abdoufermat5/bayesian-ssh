//! Matching a program's arguments against one rule.

use super::types::{Arg, Class};
use super::types::{Case, Rule};
use crate::services::ai::types::RiskTag;

/// Glob with `*` (any run, `/` included) and `?`.
pub fn glob_match(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) && p[pi] != '*' {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Does `arg` carry the option or word `pat`?
/// - `-x` (one letter): any short cluster containing it (`-nx`, `-xf`).
/// - `--name`: that long option, with `=value`, or an abbreviation of it (getopt allows them).
/// - anything with `*` or `?`: glob on the whole argument.
/// - otherwise exact.
pub fn arg_matches(arg: &str, pat: &str) -> bool {
    if pat.contains('*') || pat.contains('?') {
        return glob_match(pat, arg);
    }
    let pb = pat.as_bytes();
    if pb.len() == 2 && pb[0] == b'-' && pb[1] != b'-' {
        return arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains(&pat[1..]);
    }
    if pat.starts_with("--") {
        if !arg.starts_with("--") {
            return false;
        }
        let name = arg.split('=').next().unwrap_or(arg);
        return name.len() >= 3 && pat.starts_with(name);
    }
    arg == pat
}

fn any_match(args: &[Arg], pats: &[&str]) -> bool {
    args.iter()
        .any(|a| pats.iter().any(|p| arg_matches(&a.text, p)))
}

fn alt_match(alts: &str, text: &str) -> bool {
    alts.split('|').any(|a| glob_match(a, text))
}

/// Matches the `sub` pattern at `args[k..]`; returns how many arguments it used.
fn sub_matches(pat: &str, args: &[Arg], k: usize) -> Option<usize> {
    let mut used = 0usize;
    let mut end = false;
    for (i, w) in pat.split_whitespace().enumerate() {
        if w == "$" {
            end = true;
            break;
        }
        let a = args.get(k + i)?;
        if a.text.starts_with('-') || !alt_match(w, &a.text) {
            return None;
        }
        used += 1;
    }
    if end
        && args[(k + used).min(args.len())..]
            .iter()
            .any(|a| !a.text.starts_with('-'))
    {
        return None;
    }
    Some(used)
}

/// Index of the first argument after the leading global options.
fn skip_globals(globals: &[(&str, bool)], args: &[Arg]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        let a = &args[i].text;
        if !a.starts_with('-') {
            return Some(i);
        }
        if a == "--" {
            return None;
        }
        let name = if a.starts_with("--") {
            a.split('=').next().unwrap_or(a)
        } else {
            &a[..a.char_indices().nth(2).map_or(a.len(), |(n, _)| n)]
        };
        match globals.iter().find(|(g, _)| *g == name) {
            Some((_, takes)) => {
                let attached = a.contains('=') || (!a.starts_with("--") && a.len() > 2);
                i += if *takes && !attached { 2 } else { 1 };
            }
            None => return None,
        }
    }
    Some(args.len())
}

pub(super) fn eval_rule(rule: &Rule, prog: &str, args: &[Arg]) -> Class {
    if let [only] = args
        && matches!(only.text.as_str(), "--version" | "--help" | "-h" | "-V")
    {
        return Class::new(format!("{prog} version"), &[RiskTag::Read]);
    }
    let sub_start = skip_globals(rule.globals, args);
    for case in rule.cases {
        let mut after = 0usize; // first argument after the subcommand
        if !case.sub.is_empty() {
            let Some(k) = sub_start else { continue };
            match case
                .sub
                .iter()
                .flat_map(|p| p.split(';'))
                .find_map(|p| sub_matches(p, args, k))
            {
                Some(used) => after = k + used,
                None => continue,
            }
        }
        if !case.has.is_empty() && !any_match(args, case.has) {
            continue;
        }
        if any_match(args, case.lacks) {
            continue;
        }
        if let Some(max) = case.max_pos
            && args[after.min(args.len())..]
                .iter()
                .filter(|a| !a.text.starts_with('-'))
                .count()
                > max
        {
            continue;
        }
        let conditional = !case.lacks.is_empty() || case.max_pos.is_some();
        let read_only = case.tags.iter().all(|t| *t == RiskTag::Read);
        if read_only && conditional && args.iter().any(|a| a.flaggy) {
            return Class::unknown(
                rule_name(prog, case),
                format!(
                    "{prog}: an argument is only known at run time and could change what it does"
                ),
            );
        }
        if case.tags.contains(&RiskTag::Unknown) {
            return Class::unknown(
                rule_name(prog, case),
                format!("{prog}: runs an arbitrary command"),
            );
        }
        return Class::new(rule_name(prog, case), case.tags);
    }
    Class::unknown(
        prog,
        format!("{prog}: subcommand or options not in the rule table"),
    )
}

fn rule_name(prog: &str, case: &Case) -> String {
    if case.name.is_empty() {
        prog.to_string()
    } else {
        format!("{prog} {}", case.name)
    }
}
