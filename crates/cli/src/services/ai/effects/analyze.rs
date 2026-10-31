//! Turns an analysis into `Effects`: network, privilege and per-command effects.

use crate::services::ai::types::{Analysis, RiskTag, SimpleCommand};

use super::model::{Effect, Effects, Verb};
use super::programs::known;
use super::vars::{assignments, expand};

/// Effects of one analyzed command, paths exactly as the analyzer kept them.
pub fn effects(analysis: &Analysis) -> Effects {
    build(analysis, "")
}

/// Like `effects`, but `$A` and `${A}` are replaced by what the same command
/// line assigned earlier (`A=/var/tmp/x;`, `export A=...`). Unknown variables
/// stay as written.
pub fn effects_for(command: &str, analysis: &Analysis) -> Effects {
    let vars = assignments(command);
    let mut out = build(analysis, command);
    if vars.is_empty() {
        return out;
    }
    for item in &mut out.items {
        for t in &mut item.targets {
            *t = expand(t, &vars);
        }
        if let Some(d) = item.dest.as_mut() {
            *d = expand(d, &vars);
        }
    }
    out
}

const NETWORK_PROGRAMS: &[&str] = &[
    "curl", "wget", "nc", "ncat", "netcat", "scp", "sftp", "ssh", "ftp", "telnet", "socat", "ping",
    "dig", "nslookup",
];

/// Read-only programs; seen with a redirect they only write the redirect.
const PLAIN_READERS: &[&str] = &[
    "echo",
    "printf",
    "cat",
    "head",
    "tail",
    "sort",
    "uniq",
    "cut",
    "tr",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "sed",
    "ls",
    "date",
    "df",
    "du",
    "ps",
    "who",
    "w",
    "id",
    "hostname",
    "uname",
    "uptime",
    "free",
    "journalctl",
    "dmesg",
    "stat",
    "wc",
    "env",
    "printenv",
    "true",
    "false",
    ":",
    "test",
    "[",
    "yes",
    "seq",
    "lsblk",
    "ss",
];

fn build(analysis: &Analysis, command: &str) -> Effects {
    let mut out = Effects {
        not_understood: analysis.unknown_reason.clone(),
        network: analysis.tags.contains(&RiskTag::Network),
        ..Default::default()
    };
    if out.not_understood.is_some() || analysis.tags.contains(&RiskTag::Unknown) {
        // Obfuscated or unparsed: anything could be in there.
        out.deletes = true;
        out.sudo = mentions_privilege(command);
    }
    let mut find_roots = String::new();
    for c in &analysis.commands {
        let program = base(&c.program);
        if c.wrappers
            .iter()
            .any(|w| matches!(first_word(w), "sudo" | "doas" | "pkexec" | "su"))
            || matches!(program, "sudo" | "doas" | "pkexec" | "su")
        {
            out.sudo = true;
        }
        if c.tags.contains(&RiskTag::Network)
            || NETWORK_PROGRAMS.contains(&program)
            || (matches!(program, "rsync" | "scp") && c.args.iter().any(|a| a.contains(':')))
        {
            out.network = true;
        }
        let c = &with_placeholders(c, &find_roots);
        if program == "find" {
            find_roots = find_roots_of(&c.args);
        }
        command_effects(c, program, &mut out);
    }
    out
}

fn base(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}

/// Does unparsed text hand out privileges? Checked loosely on purpose.
fn mentions_privilege(command: &str) -> bool {
    command
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .any(|w| matches!(w, "sudo" | "doas" | "pkexec"))
}

/// `find -exec rm {} \;` and `xargs rm`: the paths come from elsewhere, say so.
fn with_placeholders(c: &SimpleCommand, find_roots: &str) -> SimpleCommand {
    let mut c = c.clone();
    let found = c
        .wrappers
        .iter()
        .any(|w| w.starts_with("find -exec") || w.starts_with("find -ok"));
    let piped = c.wrappers.iter().any(|w| w == "xargs");
    if found || piped {
        let label = if found {
            format!("files found under {find_roots}")
        } else {
            "paths read from input".to_string()
        };
        for a in &mut c.args {
            if a.contains("{}") {
                *a = label.clone();
            }
        }
    }
    c
}

pub(super) fn find_roots_of(args: &[String]) -> String {
    let roots: Vec<&str> = args
        .iter()
        .take_while(|a| !a.starts_with('-') && !matches!(a.as_str(), "!" | "(" | ")"))
        .map(String::as_str)
        .collect();
    if roots.is_empty() {
        ".".into()
    } else {
        roots.join(" ")
    }
}

pub(super) fn push(out: &mut Effects, verb: Verb, targets: Vec<String>, dest: Option<String>) {
    if matches!(verb, Verb::Deletes | Verb::Removes | Verb::WritesDevice) {
        out.deletes = true;
    }
    out.items.push(Effect {
        verb,
        targets,
        dest,
    });
}

/// Options that take a value, so the value isn't taken for an operand.
/// Short ones are looked up as `-m`, long ones as `--mode`.
pub(super) fn operands(args: &[String], value_opts: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if a == "--" {
            out.extend(args[i..].iter().cloned());
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            if !long.contains('=') && value_opts.contains(&a.as_str()) {
                i += 1;
            }
        } else if a.len() > 1 && a.starts_with('-') {
            let chars: Vec<char> = a.chars().skip(1).collect();
            for (n, ch) in chars.iter().enumerate() {
                if value_opts.contains(&format!("-{ch}").as_str()) {
                    if n + 1 == chars.len() {
                        i += 1;
                    }
                    break;
                }
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

pub(super) fn has_flag(args: &[String], short: char, long: &str) -> bool {
    args.iter().any(|a| {
        a == long
            || a.starts_with(&format!("{long}="))
            || (a.starts_with('-') && !a.starts_with("--") && a.contains(short))
    })
}

pub(super) fn value_of(args: &[String], short: &str, long: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == short || a == long {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(&format!("{long}=")) {
            return Some(v.to_string());
        }
    }
    None
}

fn is_sink(path: &str) -> bool {
    matches!(
        path,
        "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/tty" | "/dev/zero"
    ) || path.starts_with("/dev/fd/")
}

/// An edit of `path`, or a device write.
pub(super) fn write_to(out: &mut Effects, paths: &[String]) {
    let mut files = Vec::new();
    for p in paths.iter().filter(|p| !is_sink(p)) {
        if p.starts_with("/dev/") {
            push(out, Verb::WritesDevice, vec![p.clone()], None);
        } else {
            files.push(p.clone());
        }
    }
    if !files.is_empty() {
        push(out, Verb::Edits, files, None);
    }
}

fn command_effects(c: &SimpleCommand, program: &str, out: &mut Effects) {
    let before = out.items.len();
    if !known(c, program, out) {
        // Read-only commands add nothing; a redirect is listed below. Anything
        // else that writes or isn't understood must still be visible.
        let reads_only = c.tags.iter().all(|t| *t == RiskTag::Read);
        let only_redirects = !c.writes_to.is_empty()
            && (PLAIN_READERS.contains(&program) || program == ":")
            && c.tags
                .iter()
                .all(|t| matches!(t, RiskTag::Read | RiskTag::Write));
        if !(reads_only && c.writes_to.is_empty() || only_redirects) {
            if c.tags.contains(&RiskTag::Destructive) || c.tags.contains(&RiskTag::Unknown) {
                out.deletes = true;
            }
            push(out, Verb::Runs, vec![program.to_string()], None);
        }
    }
    if !c.writes_to.is_empty() {
        write_to(out, &c.writes_to);
    }
    if c.wrappers.iter().any(|w| w == "xargs") {
        for item in &mut out.items[before..] {
            if item.targets.is_empty() {
                item.targets.push("paths read from input".into());
            }
        }
    }
}
