//! Policy `deny` entries, matched on parsed commands (design 6.4): `rm -rf /`, `mkfs *`,
//! `dd of=/dev/*`. The text is not compared: quoting tricks, `sudo`, `sh -c`, flag order
//! and `/bin/rm` all end up as the same program and arguments.

use crate::services::ai::types::{Analysis, SimpleCommand};

use super::rules::{arg_matches, glob_match};

/// Does any command in `analysis` match the deny `pattern`?
///
/// A pattern is a program followed by flags and positional arguments:
/// - the program matches the command's program name (`mkfs` also matches `mkfs.ext4`);
/// - each flag must be present: `-rf` needs `r` and `f` in any short options (`-r -f`,
///   `-fr`, `--recursive --force` all count); `--no-preserve-root` needs that long option;
/// - each positional is a glob matched, in order, against the command's positional
///   arguments (`/` also matches `/*`, `//` and `/.`); a lone `*` matches anything;
/// - extra arguments on the command don't matter;
/// - a leading wrapper name (`sudo rm -rf /`) must be among the command's wrappers.
pub fn matches_deny(pattern: &str, analysis: &Analysis) -> bool {
    let words = pattern_words(pattern);
    if words.is_empty() {
        return false;
    }
    analysis.commands.iter().any(|c| command_matches(&words, c))
}

fn pattern_words(pattern: &str) -> Vec<String> {
    if let Ok(super::ast::Node::Seq(items)) = super::parser::parse_program(pattern, 0) {
        if let [super::ast::Node::Simple(cmd)] = items.as_slice() {
            if cmd.assigns.is_empty() && cmd.redirs.is_empty() {
                return cmd.words.iter().map(|w| w.text()).collect();
            }
        }
    }
    pattern.split_whitespace().map(str::to_string).collect()
}

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn command_matches(words: &[String], cmd: &SimpleCommand) -> bool {
    let Some((first, rest)) = words.split_first() else {
        return false;
    };
    if cmd
        .wrappers
        .iter()
        .any(|w| basename(w.split(' ').next().unwrap_or(w)) == basename(first))
    {
        return rest.is_empty() || command_matches(rest, cmd);
    }
    let prog = basename(&cmd.program);
    let pat_prog = basename(first);
    if !(glob_match(pat_prog, prog) || prog.starts_with(&format!("{pat_prog}."))) {
        return false;
    }
    let (flags, positionals): (Vec<&String>, Vec<&String>) =
        rest.iter().partition(|w| w.starts_with('-') && w.len() > 1);
    let short_letters: String = cmd
        .args
        .iter()
        .filter(|a| a.starts_with('-') && !a.starts_with("--") && a.len() > 1)
        .flat_map(|a| a[1..].chars())
        .collect();
    for f in flags {
        if !flag_present(f, &short_letters, &cmd.args) {
            return false;
        }
    }
    let cmd_pos: Vec<&str> = cmd
        .args
        .iter()
        .filter(|a| !a.starts_with('-') || a.len() == 1)
        .map(String::as_str)
        .collect();
    let mut cursor = 0;
    for p in positionals {
        if p == "*" {
            continue;
        }
        let Some(found) = cmd_pos[cursor.min(cmd_pos.len())..]
            .iter()
            .position(|a| path_glob(p, a) || path_glob(p, &normalize(a)))
        else {
            return false;
        };
        cursor += found + 1;
    }
    true
}

/// Short option letters and the long options that mean the same.
const ALIASES: &[(char, &str)] = &[
    ('r', "--recursive"),
    ('R', "--recursive"),
    ('f', "--force"),
    ('F', "--flush"),
    ('X', "--delete-chain"),
    ('P', "--policy"),
];

fn flag_present(flag: &str, short_letters: &str, args: &[String]) -> bool {
    if flag.starts_with("--") {
        if args.iter().any(|a| arg_matches(a, flag)) {
            return true;
        }
        return ALIASES
            .iter()
            .any(|(l, long)| *long == flag && short_letters.contains(*l))
            || (flag == "--recursive" && short_letters.contains('R'));
    }
    flag[1..].chars().all(|l| {
        short_letters.contains(l)
            || (l == 'r' && short_letters.contains('R'))
            || (l == 'R' && short_letters.contains('r'))
            || ALIASES
                .iter()
                .any(|(al, long)| *al == l && args.iter().any(|a| arg_matches(a, long)))
    })
}

/// Glob for pattern arguments, the way a shell matches paths: `*` and `?` never match `/`,
/// and `**` matches across directories. So `rm -rf /*` covers `/`, `/*` and a top-level
/// directory such as `/var`, but not `/var/www/cache`, which goes through the normal
/// decision (usually ask) instead; `dd of=/dev/**` covers every device path.
fn path_glob(pat: &str, text: &str) -> bool {
    if pat.contains("**") {
        return glob_match(pat, text);
    }
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && p[pi] != '*' && (p[pi] == t[ti] || (p[pi] == '?' && t[ti] != '/')) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star.filter(|_| t[mark] != '/') {
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

/// `//`, `/.`, `/*` and `/..` are the root directory for deny purposes.
fn normalize(arg: &str) -> String {
    let mut s = String::with_capacity(arg.len());
    for c in arg.chars() {
        if !(c == '/' && s.ends_with('/')) {
            s.push(c);
        }
    }
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    if matches!(s.as_str(), "/*" | "/." | "/.." | "/.*") {
        return "/".into();
    }
    s
}
