//! Every path-like operand a call names, as written.

use super::deny_paths::{Candidate, Expansion};
use crate::services::ai::types::Analysis;

/// Every path a call names, as written: absolute and relative arguments, `~`/`$HOME` and
/// `--opt=`/`-f/path` values, and redirection targets read from or written to. Relative
/// arguments after a literal `cd` are resolved against it. Also returns the first operand
/// that is a path only known at run time (`$DIR/x`), which cannot be proven safe.
pub(super) fn path_candidates(analysis: Option<&Analysis>) -> (Vec<Candidate>, Option<String>) {
    let mut out = Vec::new();
    let mut unsure = None;
    let Some(a) = analysis else {
        return (out, unsure);
    };
    for c in &a.commands {
        let cwd = c.cwd.as_deref();
        for p in c.writes_to.iter() {
            push_operand(&mut out, &mut unsure, p, cwd, Expansion::of_text(p));
        }
        for p in c.reads_from.iter() {
            push_operand(&mut out, &mut unsure, p, cwd, Expansion::of_text(p));
        }
        for p in &c.args {
            let expansion = if c.glob_args.iter().any(|g| g == p) {
                Expansion::Glob
            } else {
                Expansion::Literal
            };
            push_operand(&mut out, &mut unsure, p, cwd, expansion);
        }
    }
    (out, unsure)
}

/// A command argument. Flags can carry a path with `=` (`--to-file=/etc/shadow`) or attached
/// to a short option (`-f/etc/shadow`); other flags name no path.
fn push_operand(
    out: &mut Vec<Candidate>,
    unsure: &mut Option<String>,
    arg: &str,
    cwd: Option<&str>,
    expansion: Expansion,
) {
    if arg.len() > 1 && arg.starts_with('-') {
        if let Some((_, v)) = arg.split_once('=') {
            push_path(out, unsure, v, cwd, expansion);
        } else if let Some(ix) = arg.find('/') {
            push_path(out, unsure, &arg[ix..], cwd, expansion);
        }
        return;
    }
    push_path(out, unsure, arg, cwd, expansion);
}

/// One path-like operand. `~`, `~user` and `$HOME` map to the part below the home directory,
/// which the `**/` deny patterns already cover; another `$VAR` path can't be judged.
fn push_path(
    out: &mut Vec<Candidate>,
    unsure: &mut Option<String>,
    raw: &str,
    cwd: Option<&str>,
    expansion: Expansion,
) {
    if raw.is_empty() {
        return;
    }
    if raw.starts_with('/') {
        out.push(Candidate {
            text: raw.to_string(),
            expansion,
        });
        return;
    }
    if let Some(rest) = raw.strip_prefix('~') {
        if let Some(ix) = rest.find('/') {
            let below = &rest[ix + 1..];
            if !below.is_empty() {
                out.push(Candidate {
                    text: below.to_string(),
                    expansion,
                });
            }
        }
        return;
    }
    if raw.starts_with('$') {
        let Some((name, rest)) = split_var(raw) else {
            return;
        };
        if name == "HOME" {
            let below = rest.strip_prefix('/').unwrap_or(rest);
            if !below.is_empty() {
                out.push(Candidate {
                    text: below.to_string(),
                    expansion,
                });
            }
        } else if (rest.is_empty() || rest.starts_with('/')) && unsure.is_none() {
            *unsure = Some(raw.to_string());
        }
        return;
    }
    if let Some(dir) = cwd {
        out.push(Candidate {
            text: format!("{}/{}", dir.trim_end_matches('/'), raw),
            expansion,
        });
    }
    out.push(Candidate {
        text: raw.to_string(),
        expansion,
    });
}

/// Split a leading shell variable reference: (`NAME`, rest) for `$NAME` and `${NAME}`.
/// `$(...)`, `$((...))` and `$'...'` have no static name and yield nothing; the analyzer
/// already walks the commands inside them.
fn split_var(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix('$')?;
    if rest.starts_with(['(', '\'', '"']) {
        return None;
    }
    if let Some(body) = rest.strip_prefix('{') {
        let end = body.find('}')?;
        return Some((&body[..end], &body[end + 1..]));
    }
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some((&rest[..end], &rest[end..]))
}
