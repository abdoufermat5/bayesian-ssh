//! "What this does": a plain account of a command's effects, read from the
//! analyzer's parse, never from the agent. Approval cards, `bssh ai watch` and
//! the agent's own results all show it, so a person can approve a one-line
//! script without reading it character by character.
//!
//! It is shown as fact, so it must never under-report: anything that is not
//! read-only and not recognised appears as `runs <program>`, and `deletes` is
//! true whenever a delete happens or can't be ruled out.

use std::collections::HashMap;

use crate::services::ai::types::{Analysis, RiskTag, SimpleCommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verb {
    Creates,
    /// Creates directories (`mkdir`, `install -d`); worded "creates".
    CreatesDirs,
    Moves,
    Copies,
    Deletes,
    Edits,
    /// Overwrites a block or character device (`dd of=/dev/sda`).
    WritesDevice,
    /// Permissions or ownership.
    Changes,
    Restarts,
    Reloads,
    Starts,
    Stops,
    Enables,
    Disables,
    Mounts,
    Unmounts,
    Installs,
    /// Package removal.
    Removes,
    /// A read that needs approval (a file outside `read_paths`).
    Reads,
    /// Anything bssh can't put a better word on.
    Runs,
}

impl Verb {
    pub fn word(self) -> &'static str {
        match self {
            Verb::Creates | Verb::CreatesDirs => "creates",
            Verb::Moves => "moves",
            Verb::Copies => "copies",
            Verb::Deletes => "deletes",
            Verb::Edits => "edits",
            Verb::WritesDevice => "writes",
            Verb::Changes => "changes",
            Verb::Restarts => "restarts",
            Verb::Reloads => "reloads",
            Verb::Starts => "starts",
            Verb::Stops => "stops",
            Verb::Enables => "enables",
            Verb::Disables => "disables",
            Verb::Mounts => "mounts",
            Verb::Unmounts => "unmounts",
            Verb::Installs => "installs",
            Verb::Removes => "removes",
            Verb::Reads => "reads",
            Verb::Runs => "runs",
        }
    }
}

/// One thing a command does: `moves /var/www/releases/2026-08-14 → /var/tmp/x/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    pub verb: Verb,
    /// Paths, units or packages, as written (or as expanded by `effects_for`).
    pub targets: Vec<String>,
    /// Where moves and copies go.
    pub dest: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effects {
    /// In command order; read-only commands are left out.
    pub items: Vec<Effect>,
    /// True whenever anything is deleted, or deleting can't be ruled out.
    pub deletes: bool,
    pub sudo: bool,
    pub network: bool,
    /// Set when the analyzer couldn't parse everything: the summary says so.
    pub not_understood: Option<String>,
}

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

fn find_roots_of(args: &[String]) -> String {
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

fn push(out: &mut Effects, verb: Verb, targets: Vec<String>, dest: Option<String>) {
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
fn operands(args: &[String], value_opts: &[&str]) -> Vec<String> {
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

fn has_flag(args: &[String], short: char, long: &str) -> bool {
    args.iter().any(|a| {
        a == long
            || a.starts_with(&format!("{long}="))
            || (a.starts_with('-') && !a.starts_with("--") && a.contains(short))
    })
}

fn value_of(args: &[String], short: &str, long: &str) -> Option<String> {
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
fn write_to(out: &mut Effects, paths: &[String]) {
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

/// Handles programs bssh knows; false means "not recognised".
fn known(c: &SimpleCommand, program: &str, out: &mut Effects) -> bool {
    let args = &c.args[..];
    match program {
        "mkdir" => {
            let t = operands(args, &["-m", "--mode", "-Z", "--context"]);
            push(out, Verb::CreatesDirs, t, None);
        }
        "install" => {
            let t = operands(
                args,
                &[
                    "-m",
                    "--mode",
                    "-o",
                    "--owner",
                    "-g",
                    "--group",
                    "-S",
                    "--suffix",
                    "-Z",
                    "--context",
                    "-t",
                    "--target-directory",
                ],
            );
            if has_flag(args, 'd', "--directory") {
                push(out, Verb::CreatesDirs, t, None);
            } else if let Some(dest) = value_of(args, "-t", "--target-directory") {
                push(out, Verb::Copies, t, Some(dest));
            } else if t.len() >= 2 {
                let (dest, src) = t.split_last().expect("len >= 2");
                push(out, Verb::Copies, src.to_vec(), Some(dest.clone()));
            } else {
                return false;
            }
        }
        "touch" => {
            let t = operands(args, &["-d", "--date", "-t", "-r", "--reference"]);
            push(out, Verb::Creates, t, None);
        }
        "ln" => {
            let t = operands(args, &["-t", "--target-directory", "-S", "--suffix"]);
            if t.is_empty() {
                return false;
            }
            // `ln -s TARGET LINK`; with one operand the link lands in `.`.
            let (link, source) = if t.len() == 1 {
                (t[0].clone(), None)
            } else {
                let (link, rest) = t.split_last().expect("not empty");
                (link.clone(), Some(rest.join(" ")))
            };
            push(out, Verb::Creates, vec![link], source);
        }
        "mv" | "cp" => {
            let verb = if program == "mv" {
                Verb::Moves
            } else {
                Verb::Copies
            };
            let mut t = operands(
                args,
                &[
                    "-t",
                    "--target-directory",
                    "-S",
                    "--suffix",
                    "-Z",
                    "--context",
                ],
            );
            if let Some(dest) = value_of(args, "-t", "--target-directory") {
                push(out, verb, t, Some(dest));
            } else if t.len() >= 2 {
                let dest = t.pop();
                push(out, verb, t, dest);
            } else {
                return false;
            }
        }
        "rsync" => {
            let mut t = operands(
                args,
                &[
                    "-e",
                    "--rsh",
                    "--exclude",
                    "--include",
                    "--exclude-from",
                    "--include-from",
                    "-f",
                    "--filter",
                    "--port",
                    "--bwlimit",
                    "--chmod",
                    "--chown",
                    "--rsync-path",
                    "--files-from",
                    "--log-file",
                    "-T",
                    "--temp-dir",
                    "--backup-dir",
                    "--timeout",
                    "--max-size",
                    "--min-size",
                    "--link-dest",
                    "--compare-dest",
                ],
            );
            if t.len() < 2 {
                return false;
            }
            let dest = t.pop();
            if args.iter().any(|a| a.starts_with("--remove-source-files")) {
                push(out, Verb::Deletes, t.clone(), None);
            }
            if args
                .iter()
                .any(|a| a.starts_with("--delete") || a == "--del" || a == "--remove-sent-files")
            {
                let at = dest.clone().unwrap_or_default();
                push(
                    out,
                    Verb::Deletes,
                    vec![format!("files in {at} missing from the source")],
                    None,
                );
            }
            push(out, Verb::Copies, t, dest);
        }
        "rm" | "rmdir" | "unlink" => {
            let t = operands(args, &[]);
            push(out, Verb::Deletes, t, None);
        }
        "shred" => {
            let t = operands(
                args,
                &["-n", "--iterations", "-s", "--size", "--random-source"],
            );
            let verb = if has_flag(args, 'u', "--remove") {
                Verb::Deletes
            } else {
                Verb::Edits
            };
            push(out, verb, t, None);
        }
        "truncate" => {
            let t = operands(
                args,
                &["-s", "--size", "-r", "--reference", "-o", "--io-blocks"],
            );
            push(out, Verb::Edits, t, None);
        }
        "find" => return find_effect(c, out),
        "sed" => return sed_effect(args, out),
        "tee" => {
            let t = operands(args, &[]);
            if t.is_empty() {
                return false;
            }
            write_to(out, &t);
        }
        "dd" => {
            let Some(of) = args.iter().find_map(|a| a.strip_prefix("of=")) else {
                return false;
            };
            write_to(out, &[of.to_string()]);
        }
        "chmod" | "chown" | "chgrp" => {
            // `chmod -x f`: a leading `-x` is a mode, not an option.
            let known_opts = |a: &String| {
                a.starts_with("--")
                    || (a.len() > 1
                        && a.starts_with('-')
                        && a[1..].chars().all(|ch| "RvcfhHLP".contains(ch)))
            };
            let rest: Vec<String> = args.iter().filter(|a| !known_opts(a)).cloned().collect();
            let by_ref = args.iter().any(|a| a.starts_with("--reference"));
            let skip = usize::from(!by_ref);
            push(
                out,
                Verb::Changes,
                rest.into_iter().skip(skip).collect(),
                None,
            );
        }
        "systemctl" => return systemctl_effect(args, out),
        "service" => {
            let ops = operands(args, &[]);
            let (Some(unit), Some(action)) = (ops.first(), ops.get(1)) else {
                return false;
            };
            return unit_action(out, action, std::slice::from_ref(unit));
        }
        p if c.program.contains("/init.d/") => {
            let ops = operands(args, &[]);
            let Some(action) = ops.first() else {
                return false;
            };
            return unit_action(out, action, &[p.to_string()]);
        }
        "reboot" => push(out, Verb::Restarts, vec!["the host".into()], None),
        "poweroff" | "halt" => push(out, Verb::Stops, vec!["the host".into()], None),
        "shutdown" => {
            let verb = if has_flag(args, 'r', "--reboot") {
                Verb::Restarts
            } else {
                Verb::Stops
            };
            push(out, verb, vec!["the host".into()], None);
        }
        "kill" | "pkill" | "killall" => return kill_effect(program, args, out),
        "useradd" | "adduser" | "usermod" | "userdel" | "deluser" | "groupadd" | "addgroup"
        | "groupmod" | "groupdel" | "delgroup" | "passwd" | "chpasswd" => {
            let kind = if program.contains("group") {
                "group"
            } else {
                "user"
            };
            let ops = operands(args, &[]);
            let name = ops.last().cloned().unwrap_or_default();
            let target = if name.is_empty() {
                format!("{kind}s")
            } else {
                format!("{kind} {name}")
            };
            if program.contains("del") {
                out.deletes = true;
            }
            push(out, Verb::Changes, vec![target], None);
        }
        "crontab" => {
            if has_flag(args, 'r', "--remove") && !args.iter().any(|a| a == "-l") {
                push(out, Verb::Deletes, vec!["the crontab".into()], None);
            } else if args.iter().any(|a| a == "-e") {
                push(out, Verb::Edits, vec!["the crontab".into()], None);
            } else if args.iter().any(|a| a == "-l") {
                return false;
            } else if !operands(args, &["-u"]).is_empty() {
                push(out, Verb::Edits, vec!["the crontab".into()], None);
            } else {
                return false;
            }
        }
        "docker" | "podman" => return container_effect(args, out),
        "git" => return git_effect(args, out),
        "mount" => {
            let t = operands(
                args,
                &[
                    "-t",
                    "--types",
                    "-o",
                    "--options",
                    "-L",
                    "-U",
                    "--source",
                    "--target",
                ],
            );
            if has_flag(args, 'a', "--all") {
                push(
                    out,
                    Verb::Mounts,
                    vec!["all filesystems in fstab".into()],
                    None,
                );
            } else if let Some(point) = t.last() {
                push(out, Verb::Mounts, vec![point.clone()], None);
            } else {
                return false;
            }
        }
        "umount" => {
            let t = operands(args, &["-t", "--types", "-O"]);
            if has_flag(args, 'a', "--all") {
                push(out, Verb::Unmounts, vec!["all filesystems".into()], None);
            } else if t.is_empty() {
                return false;
            } else {
                push(out, Verb::Unmounts, t, None);
            }
        }
        _ => return package_effect(program, args, out),
    }
    true
}

fn find_effect(c: &SimpleCommand, out: &mut Effects) -> bool {
    let args = &c.args;
    let roots = find_roots_of(args);
    if args.iter().any(|a| a == "-delete") {
        push(
            out,
            Verb::Deletes,
            vec![format!("files found under {roots}")],
            None,
        );
        return true;
    }
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if matches!(a.as_str(), "-fprint" | "-fprint0" | "-fprintf" | "-fls") {
            files.extend(it.next().cloned());
        }
    }
    if !files.is_empty() {
        write_to(out, &files);
    }
    let runs_others = args
        .iter()
        .any(|a| matches!(a.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"));
    // The commands behind -exec are listed on their own; plain find reads.
    !files.is_empty() || runs_others || c.tags.iter().all(|t| *t == RiskTag::Read)
}

fn sed_effect(args: &[String], out: &mut Effects) -> bool {
    let mut in_place = false;
    let mut script_given = false;
    for a in args {
        if a.starts_with("--in-place") {
            in_place = true;
        } else if a.starts_with("--expression") || a.starts_with("--file") {
            script_given = true;
        } else if a.starts_with('-') && !a.starts_with("--") {
            for ch in a.chars().skip(1) {
                match ch {
                    'i' => {
                        in_place = true;
                        break;
                    }
                    'e' | 'f' => {
                        script_given = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    if !in_place {
        return false;
    }
    let mut files = operands(
        args,
        &["-e", "-f", "-l", "--expression", "--file", "--line-length"],
    );
    if !script_given && !files.is_empty() {
        files.remove(0);
    }
    if files.is_empty() {
        return false;
    }
    write_to(out, &files);
    true
}

fn unit_action(out: &mut Effects, action: &str, units: &[String]) -> bool {
    let verb = match action {
        "restart" | "try-restart" | "condrestart" | "force-restart" => Verb::Restarts,
        "reload" | "reload-or-restart" | "try-reload-or-restart" | "force-reload" => Verb::Reloads,
        "start" => Verb::Starts,
        "stop" | "kill" => Verb::Stops,
        "enable" | "unmask" => Verb::Enables,
        "disable" | "mask" => Verb::Disables,
        _ => return false,
    };
    push(out, verb, units.to_vec(), None);
    true
}

fn systemctl_effect(args: &[String], out: &mut Effects) -> bool {
    let ops = operands(
        args,
        &[
            "-t",
            "--type",
            "-p",
            "--property",
            "-s",
            "--signal",
            "-H",
            "--host",
            "-M",
            "--machine",
            "-n",
            "--lines",
            "-o",
            "--output",
            "--kill-whom",
            "--root",
        ],
    );
    let Some((action, units)) = ops.split_first() else {
        return false;
    };
    match action.as_str() {
        "daemon-reload" | "daemon-reexec" => {
            push(out, Verb::Reloads, vec!["systemd".into()], None);
            true
        }
        "reboot" | "kexec" => {
            push(out, Verb::Restarts, vec!["the host".into()], None);
            true
        }
        "poweroff" | "halt" | "suspend" | "hibernate" => {
            push(out, Verb::Stops, vec!["the host".into()], None);
            true
        }
        a => {
            if !unit_action(out, a, units) {
                return false;
            }
            if args.iter().any(|x| x == "--now") {
                match a {
                    "enable" => push(out, Verb::Starts, units.to_vec(), None),
                    "disable" | "mask" => push(out, Verb::Stops, units.to_vec(), None),
                    _ => {}
                }
            }
            true
        }
    }
}

fn kill_effect(program: &str, args: &[String], out: &mut Effects) -> bool {
    if args.iter().any(|a| a == "-l" || a == "-L" || a == "--list") {
        return false;
    }
    let signal = if program == "kill" {
        args.iter()
            .find(|a| a.starts_with('-') && a.len() > 1 && !a.starts_with("--"))
            .map(|a| a[1..].to_ascii_uppercase())
            .or_else(|| value_of(args, "-s", "--signal"))
    } else {
        args.iter()
            .find(|a| {
                a.starts_with('-')
                    && a.len() > 1
                    && !a.starts_with("--")
                    && (a[1..].chars().all(|ch| ch.is_ascii_digit())
                        || a[1..].chars().all(|ch| ch.is_ascii_uppercase()))
            })
            .map(|a| a[1..].to_string())
    };
    let signal = signal
        .map(|s| s.trim_start_matches("SIG").to_string())
        .unwrap_or_default();
    if signal == "0" {
        return false;
    }
    let value_opts: &[&str] = match program {
        "kill" => &["-s", "-n", "--signal"],
        "pkill" => &["-u", "-U", "-g", "-G", "-P", "-s", "-t", "-c", "--signal"],
        _ => &["-u", "-s", "-g", "--user", "--signal", "-y", "-o"],
    };
    let mut names: Vec<String> = Vec::new();
    // `kill -9 123`: numeric and signal-name options are not operands.
    let cleaned: Vec<String> = args
        .iter()
        .filter(|a| {
            !(a.starts_with('-')
                && a.len() > 1
                && (a[1..].chars().all(|ch| ch.is_ascii_digit())
                    || (program == "kill"
                        && a[1..]
                            .chars()
                            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()))))
        })
        .cloned()
        .collect();
    for op in operands(&cleaned, value_opts) {
        names.push(if program == "kill" {
            format!("pid {op}")
        } else {
            op
        });
    }
    if names.is_empty() {
        names.push("matching processes".into());
    }
    let verb = if signal == "HUP" || signal == "1" {
        Verb::Reloads
    } else {
        Verb::Stops
    };
    push(out, verb, names, None);
    true
}

fn container_effect(args: &[String], out: &mut Effects) -> bool {
    let ops = operands(
        args,
        &[
            "-H",
            "--host",
            "-c",
            "--context",
            "--config",
            "-l",
            "--log-level",
        ],
    );
    let mut it = ops.iter().map(String::as_str);
    let Some(mut sub) = it.next() else {
        return false;
    };
    let mut kind = "";
    if matches!(
        sub,
        "container" | "image" | "volume" | "network" | "system" | "builder"
    ) {
        kind = sub;
        let Some(s) = it.next() else {
            return false;
        };
        sub = s;
    }
    let names: Vec<String> = it.map(str::to_string).collect();
    let label = |default: &str, names: Vec<String>| -> Vec<String> {
        let k = if kind.is_empty() || kind == "system" {
            default
        } else {
            kind
        };
        names.into_iter().map(|n| format!("{k} {n}")).collect()
    };
    match sub {
        "rm" => {
            let t = label("container", names);
            push(out, Verb::Deletes, nonempty(t, "containers"), None);
        }
        "rmi" => {
            let t = label("image", names);
            push(out, Verb::Deletes, nonempty(t, "images"), None);
        }
        "prune" => {
            let what = match kind {
                "" | "container" => "unused containers",
                "system" => "unused containers, images and networks",
                "image" => "unused images",
                "volume" => "unused volumes",
                "network" => "unused networks",
                _ => "unused data",
            };
            push(out, Verb::Deletes, vec![what.into()], None);
        }
        "stop" | "kill" => push(
            out,
            Verb::Stops,
            nonempty(label("container", names), "containers"),
            None,
        ),
        "restart" => push(
            out,
            Verb::Restarts,
            nonempty(label("container", names), "containers"),
            None,
        ),
        "start" => push(
            out,
            Verb::Starts,
            nonempty(label("container", names), "containers"),
            None,
        ),
        _ => return false,
    }
    true
}

fn nonempty(v: Vec<String>, fallback: &str) -> Vec<String> {
    if v.is_empty() {
        vec![fallback.to_string()]
    } else {
        v
    }
}

fn git_effect(args: &[String], out: &mut Effects) -> bool {
    let ops = operands(args, &["-C", "-c", "--git-dir", "--work-tree"]);
    match ops.first().map(String::as_str) {
        Some("clean") => {
            push(out, Verb::Deletes, vec!["untracked files".into()], None);
            true
        }
        Some("reset") if args.iter().any(|a| a == "--hard") => {
            push(out, Verb::Deletes, vec!["uncommitted changes".into()], None);
            true
        }
        _ => false,
    }
}

fn package_effect(program: &str, args: &[String], out: &mut Effects) -> bool {
    const APT: (&[&str], &[&str]) = (
        &[
            "install",
            "reinstall",
            "upgrade",
            "dist-upgrade",
            "full-upgrade",
        ],
        &["remove", "purge", "autoremove", "autopurge"],
    );
    const DNF: (&[&str], &[&str]) = (
        &[
            "install",
            "reinstall",
            "localinstall",
            "upgrade",
            "update",
            "downgrade",
            "in",
            "up",
        ],
        &["remove", "erase", "autoremove", "rm"],
    );
    const NPM: (&[&str], &[&str]) = (
        &["install", "i", "add", "ci", "update", "up", "upgrade"],
        &["uninstall", "remove", "rm", "un", "unlink", "r"],
    );
    let value_opts: &[&str] = &[
        "-o",
        "-t",
        "-c",
        "-p",
        "-X",
        "-r",
        "-i",
        "-e",
        "-f",
        "--root",
        "--installroot",
        "--prefix",
        "--registry",
        "--target",
        "--index-url",
        "--extra-index-url",
        "--requirement",
        "--constraint",
        "--editable",
        "--find-links",
        "--config",
    ];
    let mut args = args;
    let mut program = program;
    // `python -m pip install x`.
    if program.starts_with("python") {
        match args {
            [m, p, rest @ ..] if m.as_str() == "-m" && matches!(p.as_str(), "pip" | "pip3") => {
                args = rest;
                program = "pip";
            }
            _ => return false,
        }
    }
    let (installs, removes): (&[&str], &[&str]) = match program {
        "apt" | "apt-get" | "aptitude" => APT,
        "dnf" | "yum" | "microdnf" | "zypper" => DNF,
        "apk" => (&["add", "upgrade"], &["del"]),
        "pip" | "pip3" => (&["install"], &["uninstall"]),
        "npm" | "pnpm" | "yarn" => NPM,
        "gem" | "cargo" => (&["install"], &["uninstall"]),
        "snap" => (&["install", "refresh"], &["remove"]),
        "brew" => (
            &["install", "upgrade", "reinstall"],
            &["uninstall", "remove", "rm"],
        ),
        "dpkg" => {
            let t = operands(args, &["-t", "--root", "--admindir"]);
            if args.iter().any(|a| a == "-i" || a == "--install") {
                push(out, Verb::Installs, nonempty(t, "packages"), None);
            } else if args
                .iter()
                .any(|a| matches!(a.as_str(), "-r" | "-P" | "--remove" | "--purge"))
            {
                push(out, Verb::Removes, nonempty(t, "packages"), None);
            } else {
                return false;
            }
            return true;
        }
        _ => return false,
    };
    let ops = operands(args, value_opts);
    let Some((sub, pkgs)) = ops.split_first() else {
        return false;
    };
    let verb = if installs.contains(&sub.as_str()) {
        Verb::Installs
    } else if removes.contains(&sub.as_str()) {
        Verb::Removes
    } else {
        return false;
    };
    push(out, verb, pkgs.to_vec(), None);
    true
}

// ------------------------------------------------------------------ variables

/// `NAME=value` assignments made by the command line, in order. Only plain
/// statements count (`A=x;`, `export A=x`); `A=x cmd` is scoped to `cmd`,
/// and values built by command substitution are unknown.
fn assignments(command: &str) -> HashMap<String, String> {
    let mut vars: HashMap<String, String> = HashMap::new();
    for line in command_lines(command) {
        let words = shell_words(&line);
        let mut it = words.iter().peekable();
        let declares = it.peek().is_some_and(|w| {
            matches!(
                w.text.as_str(),
                "export" | "declare" | "typeset" | "readonly" | "local"
            )
        });
        if declares {
            it.next();
        }
        let rest: Vec<&Word> = it.collect();
        if rest.first().is_some_and(|w| w.text == "unset") {
            for w in &rest[1..] {
                vars.remove(&w.text);
            }
            continue;
        }
        let all_assign = !rest.is_empty()
            && rest.iter().all(|w| {
                assignment_name(&w.text).is_some()
                    || (declares && is_name(&w.text))
                    || (declares && w.text.starts_with('-'))
            });
        if !all_assign {
            continue;
        }
        for w in rest {
            let Some((name, value)) = w.text.split_once('=').filter(|(n, _)| is_name(n)) else {
                continue;
            };
            if w.substitution {
                vars.remove(name);
            } else {
                let v = expand(value, &vars).replace(LITERAL_DOLLAR, "$");
                vars.insert(name.to_string(), v);
            }
        }
    }
    vars
}

fn assignment_name(word: &str) -> Option<&str> {
    word.split_once('=').map(|(n, _)| n).filter(|n| is_name(n))
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

struct Word {
    /// Quotes removed; a `$` that is not special (single-quoted, escaped) is
    /// held as `\u{1}` until the value is final.
    text: String,
    /// Holds `$(...)` or backticks.
    substitution: bool,
}

const LITERAL_DOLLAR: char = '\u{1}';

/// Words of a simple line with quotes removed; `|`, `&` and redirects become
/// words of their own so they spoil an "all assignments" check.
fn shell_words(line: &str) -> Vec<Word> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut subst = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    fn flush(words: &mut Vec<Word>, cur: &mut String, started: &mut bool, subst: &mut bool) {
        if *started {
            words.push(Word {
                text: std::mem::take(cur),
                substitution: *subst,
            });
            *started = false;
            *subst = false;
        }
    }
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('\''), '$') => cur.push(LITERAL_DOLLAR),
            (Some('"'), '\\') => {
                if let Some(n) = chars.next() {
                    match n {
                        '$' => cur.push(LITERAL_DOLLAR),
                        '"' | '\\' | '`' => cur.push(n),
                        _ => {
                            cur.push('\\');
                            cur.push(n);
                        }
                    }
                }
            }
            (Some('"'), '`') => {
                subst = true;
                cur.push(c);
            }
            (Some('"'), '$') if chars.peek() == Some(&'(') => {
                subst = true;
                cur.push(c);
            }
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                started = true;
                match chars.next() {
                    Some('$') => cur.push(LITERAL_DOLLAR),
                    Some(n) => cur.push(n),
                    None => {}
                }
            }
            (None, '`') => {
                started = true;
                subst = true;
                cur.push(c);
            }
            (None, '$') => {
                started = true;
                if chars.peek() == Some(&'(') {
                    subst = true;
                }
                cur.push(c);
            }
            (None, c) if c.is_whitespace() => flush(&mut words, &mut cur, &mut started, &mut subst),
            (None, '|' | '&' | '<' | '>' | '(' | ')') => {
                flush(&mut words, &mut cur, &mut started, &mut subst);
                words.push(Word {
                    text: c.to_string(),
                    substitution: false,
                });
            }
            (None, c) => {
                started = true;
                cur.push(c);
            }
        }
    }
    flush(&mut words, &mut cur, &mut started, &mut subst);
    words
}

/// `$A` and `${A}` replaced where `A` is known; everything else stays.
fn expand(text: &str, vars: &HashMap<String, String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let (name, end) = if chars.get(i + 1) == Some(&'{') {
            match chars[i + 2..].iter().position(|c| *c == '}') {
                Some(p) => (
                    chars[i + 2..i + 2 + p].iter().collect::<String>(),
                    i + 3 + p,
                ),
                None => (String::new(), i + 1),
            }
        } else {
            let n: String = chars[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
                .collect();
            let len = n.chars().count();
            (n, i + 1 + len)
        };
        match vars.get(&name).filter(|_| is_name(&name)) {
            Some(v) => {
                out.push_str(v);
                i = end;
            }
            None => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

// -------------------------------------------------------------------- summary

fn is_path(t: &str) -> bool {
    !t.contains(char::is_whitespace)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn unique<'a>(names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    for n in names {
        if !seen.contains(&n) {
            seen.push(n);
        }
    }
    seen
}

/// One line: `creates 2 directories · moves 4 paths · deletes nothing · sudo no`.
pub fn summary(e: &Effects) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut verbs: Vec<Verb> = e
        .items
        .iter()
        .map(|i| match i.verb {
            Verb::CreatesDirs => Verb::Creates,
            v => v,
        })
        .collect();
    verbs.sort();
    verbs.dedup();
    for v in verbs {
        let items: Vec<&Effect> = e
            .items
            .iter()
            .filter(|i| match v {
                Verb::Creates => matches!(i.verb, Verb::Creates | Verb::CreatesDirs),
                _ => i.verb == v,
            })
            .collect();
        let targets: Vec<&str> = items
            .iter()
            .flat_map(|i| i.targets.iter().map(String::as_str))
            .collect();
        let counted = matches!(
            v,
            Verb::Creates
                | Verb::Moves
                | Verb::Copies
                | Verb::Deletes
                | Verb::Edits
                | Verb::Changes
        );
        let phrase = if counted {
            let paths = targets.iter().filter(|t| is_path(t)).count();
            let dirs = items.iter().all(|i| i.verb == Verb::CreatesDirs);
            let mut bits = Vec::new();
            // An effect without operands (`rm` alone) still counts once.
            let n = if targets.is_empty() {
                items.len()
            } else {
                paths
            };
            if n > 0 {
                bits.push(if dirs {
                    plural(n, "directory", "directories")
                } else {
                    plural(n, "path", "paths")
                });
            }
            bits.extend(
                unique(targets.iter().copied().filter(|t| !is_path(t)))
                    .into_iter()
                    .map(str::to_string),
            );
            bits.join(", ")
        } else if targets.is_empty() {
            match v {
                Verb::Installs | Verb::Removes => "packages".into(),
                Verb::Runs => "a command".into(),
                _ => "a service".into(),
            }
        } else {
            unique(targets.iter().copied()).join(", ")
        };
        let word = if v == Verb::WritesDevice {
            "writes device"
        } else {
            v.word()
        };
        parts.push(format!("{word} {phrase}"));
    }
    let lists_deletes = e
        .items
        .iter()
        .any(|i| matches!(i.verb, Verb::Deletes | Verb::Removes));
    if !e.deletes {
        parts.push("deletes nothing".into());
    } else if !lists_deletes {
        parts.push("may delete data".into());
    }
    parts.push(format!("sudo {}", if e.sudo { "yes" } else { "no" }));
    if e.network {
        parts.push("uses the network".into());
    }
    if e.not_understood.is_some() {
        parts.push("part of it not understood".into());
    }
    parts.join(" · ")
}

// -------------------------------------------------------------- command_lines

/// A shell line split into the commands a person reads, one per line: split
/// at `;`, `&&`, `||` and newlines outside quotes, with quotes, pipes and
/// here-doc bodies kept intact, `\` continuations joined, comments dropped
/// and `set -e…` boilerplate left out. Display only; the exact text is what
/// runs.
pub fn command_lines(command: &str) -> Vec<String> {
    let chars: Vec<char> = command.chars().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut backtick = false;
    // Here-docs opened on this line: (delimiter, strip leading tabs).
    let mut pending: Vec<(String, bool)> = Vec::new();
    let mut i = 0;
    let at = |i: usize| chars.get(i).copied();
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            {
                cur.push(c);
                if c == q {
                    quote = None;
                } else if c == '\\' && q == '"' {
                    if let Some(n) = at(i + 1) {
                        cur.push(n);
                        i += 1;
                    }
                }
                i += 1;
                continue;
            }
        }
        let word_start = cur.is_empty() || cur.ends_with(char::is_whitespace);
        match c {
            '\\' => match at(i + 1) {
                Some('\n') => {
                    if !cur.is_empty() && !cur.ends_with(' ') {
                        cur.push(' ');
                    }
                    i += 1;
                    while at(i + 1).is_some_and(|n| n == ' ' || n == '\t') {
                        i += 1;
                    }
                }
                Some(n) => {
                    cur.push(c);
                    cur.push(n);
                    i += 1;
                }
                None => cur.push(c),
            },
            '\'' | '"' => {
                quote = Some(c);
                cur.push(c);
            }
            '`' => {
                backtick = !backtick;
                cur.push(c);
            }
            '#' if word_start && depth == 0 && !backtick => {
                while at(i + 1).is_some_and(|n| n != '\n') {
                    i += 1;
                }
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            '<' if at(i + 1) == Some('<') && at(i + 2) != Some('<') && depth == 0 => {
                // Here-doc: remember the delimiter, read the body at the newline.
                cur.push_str("<<");
                i += 2;
                let strip = at(i) == Some('-');
                if strip {
                    cur.push('-');
                    i += 1;
                }
                while at(i).is_some_and(|n| n == ' ' || n == '\t') {
                    cur.push(chars[i]);
                    i += 1;
                }
                let mut delim = String::new();
                let mut q: Option<char> = None;
                while let Some(n) = at(i) {
                    match q {
                        Some(qc) if n == qc => q = None,
                        Some(_) => delim.push(n),
                        None if n == '\'' || n == '"' => q = Some(n),
                        None if n.is_whitespace()
                            || matches!(n, ';' | '&' | '|' | '<' | '>' | ')') =>
                        {
                            break
                        }
                        None => delim.push(n),
                    }
                    cur.push(n);
                    i += 1;
                }
                pending.push((delim, strip));
                continue;
            }
            ';' | '&' | '|' | '\n' if depth > 0 || backtick => cur.push(c),
            '\n' => {
                if !pending.is_empty() {
                    // Body lines up to each terminator belong to this command.
                    for (delim, strip) in std::mem::take(&mut pending) {
                        cur.push('\n');
                        loop {
                            i += 1;
                            let start = i;
                            while at(i).is_some_and(|n| n != '\n') {
                                i += 1;
                            }
                            let body: String = chars[start..i].iter().collect();
                            cur.push_str(&body);
                            let check = if strip {
                                body.trim_start_matches('\t')
                            } else {
                                body.as_str()
                            };
                            if check == delim || i >= chars.len() {
                                break;
                            }
                            cur.push('\n');
                        }
                    }
                    lines.push(std::mem::take(&mut cur));
                } else if cur.trim_end().ends_with('|') && !cur.trim_end().ends_with("||") {
                    cur.push(' ');
                    while at(i + 1).is_some_and(|n| n == ' ' || n == '\t') {
                        i += 1;
                    }
                } else {
                    lines.push(std::mem::take(&mut cur));
                }
            }
            ';' | '&' | '|' if !pending.is_empty() => cur.push(c),
            ';' => lines.push(std::mem::take(&mut cur)),
            '&' if at(i + 1) == Some('&') => {
                i += 1;
                lines.push(std::mem::take(&mut cur));
            }
            '|' if at(i + 1) == Some('|') => {
                i += 1;
                lines.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
        i += 1;
    }
    lines.push(cur);
    lines
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !is_boilerplate(l))
        .collect()
}

fn is_boilerplate(line: &str) -> bool {
    let mut words = line.split_whitespace();
    words.next() == Some("set")
        && words.all(|w| {
            w.starts_with('-')
                || w.starts_with('+')
                || matches!(w, "pipefail" | "errexit" | "nounset" | "xtrace")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::analyzer::ShellAnalyzer;
    use crate::services::ai::types::Analyzer;

    const DEMO: &str = "set -euo pipefail; A=/var/tmp/var-www-cleanup-2026-10-24; \
        install -d -m 0750 \"$A/releases\" \"$A/cache/assets\"; \
        mv /var/www/releases/2026-08-14 /var/www/releases/2026-09-02 /var/www/releases/2026-09-21 \"$A/releases/\"; \
        mv /var/www/cache/assets/bundle-old.js \"$A/cache/assets/\"; echo 'archive complete'";

    fn fx(cmd: &str) -> Effects {
        effects_for(cmd, &ShellAnalyzer.analyze(cmd))
    }

    fn sum(cmd: &str) -> String {
        summary(&fx(cmd))
    }

    #[test]
    fn the_demo_plan_reads_as_the_brief_says() {
        let e = fx(DEMO);
        assert_eq!(
            summary(&e),
            "creates 2 directories · moves 4 paths · deletes nothing · sudo no"
        );
        assert_eq!(
            e.items[0].targets,
            vec![
                "/var/tmp/var-www-cleanup-2026-10-24/releases",
                "/var/tmp/var-www-cleanup-2026-10-24/cache/assets"
            ]
        );
        assert_eq!(
            e.items[1].dest.as_deref(),
            Some("/var/tmp/var-www-cleanup-2026-10-24/releases/")
        );
        assert!(!e.deletes && !e.sudo && !e.network);
    }

    #[test]
    fn effects_alone_keeps_variables_as_written() {
        let a = ShellAnalyzer.analyze(DEMO);
        let e = effects(&a);
        assert_eq!(e.items[0].targets[0], "$A/releases");
    }

    #[test]
    fn variables_expand_from_export_braces_and_chains() {
        let e = fx("export A=/srv; B=\"$A/b\"; mkdir ${B}/x $C/y");
        assert_eq!(e.items[0].targets, vec!["/srv/b/x", "$C/y"]);
        let e = fx("A='$HOME'; rm $A");
        assert_eq!(e.items[0].targets, vec!["$HOME"]);
        let e = fx("A=$(pwd); rm $A/x");
        assert_eq!(e.items[0].targets, vec!["$A/x"]);
    }

    #[test]
    fn creating_and_copying() {
        assert_eq!(
            sum("touch /a /b; ln -sf /t /l"),
            "creates 3 paths · deletes nothing · sudo no"
        );
        let e = fx("ln -sf /opt/app-2 /opt/current");
        assert_eq!(e.items[0].targets, vec!["/opt/current"]);
        assert_eq!(e.items[0].dest.as_deref(), Some("/opt/app-2"));
        assert_eq!(
            sum("mkdir -p -m 0700 /a/b"),
            "creates 1 directory · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("cp -r /a /b /dst"),
            "copies 2 paths · deletes nothing · sudo no"
        );
        let s = sum("rsync -a -e ssh /a/ web:/b/");
        assert!(
            s.starts_with("copies 1 path") && s.contains("uses the network"),
            "{s}"
        );
    }

    #[test]
    fn deleting_in_all_its_forms() {
        for cmd in [
            "rm -rf /a /b",
            "rmdir /a",
            "find /tmp -name '*.log' -delete",
            "find /tmp -name '*.log' -exec rm {} \\;",
            "git clean -fdx",
            "git reset --hard HEAD~1",
            "docker rm -f web",
            "docker image rm foo",
            "docker system prune -af",
            "crontab -r",
            "apt-get remove -y nginx",
            "rsync -a --delete /a/ /b/",
            "dd if=/dev/zero of=/dev/sda",
            "userdel bob",
        ] {
            assert!(fx(cmd).deletes, "{cmd}");
            assert!(!summary(&fx(cmd)).contains("deletes nothing"), "{cmd}");
        }
        assert_eq!(sum("rm -rf /a /b"), "deletes 2 paths · sudo no");
        assert_eq!(
            sum("find /tmp -name x -exec rm {} \\;"),
            "deletes files found under /tmp · sudo no"
        );
        assert_eq!(
            sum("docker rm -f web db"),
            "deletes container web, container db · sudo no"
        );
    }

    #[test]
    fn editing_and_devices() {
        assert_eq!(
            sum("sed -i 's/a/b/' /etc/x /etc/y"),
            "edits 2 paths · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("sed -i.bak -e 's/a/b/' /etc/x"),
            "edits 1 path · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("echo x | tee -a /etc/x"),
            "edits 1 path · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("echo x >> /etc/x"),
            "edits 1 path · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("truncate -s 0 /var/log/x"),
            "edits 1 path · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("dd if=a of=/tmp/x"),
            "edits 1 path · deletes nothing · sudo no"
        );
        let s = sum("dd if=a of=/dev/sdb bs=1M");
        assert!(s.starts_with("writes device /dev/sdb"), "{s}");
        assert!(s.contains("may delete data"), "{s}");
        assert_eq!(sum("ls > /dev/null"), "deletes nothing · sudo no");
        assert_eq!(
            sum("echo hi > /tmp/f"),
            "edits 1 path · deletes nothing · sudo no"
        );
    }

    #[test]
    fn a_redirect_does_not_hide_the_command() {
        assert_eq!(
            sum("mv /a /b > /tmp/log"),
            "moves 1 path · edits 1 path · deletes nothing · sudo no"
        );
    }

    #[test]
    fn permissions_and_accounts() {
        assert_eq!(
            sum("chmod -R 750 /srv/app"),
            "changes 1 path · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("chmod -x /a /b"),
            "changes 2 paths · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("chown -R www:www /srv /var/x"),
            "changes 2 paths · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("useradd -m -s /bin/bash bob"),
            "changes user bob · deletes nothing · sudo no"
        );
    }

    #[test]
    fn services_and_processes() {
        assert_eq!(
            sum("sudo -n systemctl restart -- nginx"),
            "restarts nginx · deletes nothing · sudo yes"
        );
        assert_eq!(
            sum("systemctl enable --now nginx.service"),
            "starts nginx.service · enables nginx.service · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("service nginx reload"),
            "reloads nginx · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("systemctl daemon-reload"),
            "reloads systemd · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("kill -9 123 456"),
            "stops pid 123, pid 456 · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("pkill -f nginx"),
            "stops nginx · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("killall -HUP nginx"),
            "reloads nginx · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("docker restart web"),
            "restarts container web · deletes nothing · sudo no"
        );
        assert_eq!(sum("systemctl status nginx"), "deletes nothing · sudo no");
    }

    #[test]
    fn packages_and_mounts() {
        assert_eq!(
            sum("apt-get install -y nginx curl"),
            "installs nginx, curl · deletes nothing · sudo no · uses the network"
        );
        assert_eq!(sum("apt purge -y nginx"), "removes nginx · sudo no");
        assert_eq!(
            sum("pip install requests"),
            "installs requests · deletes nothing · sudo no · uses the network"
        );
        assert_eq!(sum("npm uninstall left-pad"), "removes left-pad · sudo no");
        assert_eq!(
            sum("mount /dev/sdb1 /mnt/data"),
            "mounts /mnt/data · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("umount /mnt/data"),
            "unmounts /mnt/data · deletes nothing · sudo no"
        );
        assert_eq!(
            sum("crontab -e"),
            "edits the crontab · deletes nothing · sudo no"
        );
    }

    #[test]
    fn anything_unrecognised_is_shown() {
        let s = sum("mytool --apply");
        assert!(s.starts_with("runs mytool"), "{s}");
        let s = sum("curl -o /tmp/x https://example.com");
        assert!(s.contains("uses the network"), "{s}");
        assert!(s.contains("/tmp/x") || s.contains("runs curl"), "{s}");
    }

    #[test]
    fn sudo_and_unparsed_commands() {
        assert!(fx("sudo -u deploy ls /").sudo);
        assert!(fx("doas rm /x").sudo);
        assert!(!fx("ls /").sudo);
        let e = fx("eval \"$(echo cm0gLXJm | base64 -d)\"");
        assert!(e.deletes && e.not_understood.is_some());
        let e = fx("sudo bash -c \"$X\"");
        assert!(e.sudo && e.deletes);
        let s = summary(&e);
        assert!(
            s.contains("may delete data") && s.contains("part of it not understood"),
            "{s}"
        );
        assert!(summary(&fx("if then")).contains("part of it not understood"));
    }

    #[test]
    fn summary_orders_and_counts_stably() {
        let e = Effects {
            items: vec![
                Effect {
                    verb: Verb::Moves,
                    targets: vec!["a".into(), "b".into()],
                    dest: Some("d".into()),
                },
                Effect {
                    verb: Verb::Creates,
                    targets: vec!["c".into()],
                    dest: None,
                },
                Effect {
                    verb: Verb::Moves,
                    targets: vec!["e".into()],
                    dest: Some("d".into()),
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            summary(&e),
            "creates 1 path · moves 3 paths · deletes nothing · sudo no"
        );
    }

    #[test]
    fn lines_split_where_the_shell_splits() {
        assert_eq!(
            command_lines(DEMO),
            vec![
                "A=/var/tmp/var-www-cleanup-2026-10-24",
                "install -d -m 0750 \"$A/releases\" \"$A/cache/assets\"",
                "mv /var/www/releases/2026-08-14 /var/www/releases/2026-09-02 /var/www/releases/2026-09-21 \"$A/releases/\"",
                "mv /var/www/cache/assets/bundle-old.js \"$A/cache/assets/\"",
                "echo 'archive complete'",
            ]
        );
        assert_eq!(
            command_lines("echo 'a; b' && ls"),
            vec!["echo 'a; b'", "ls"]
        );
        assert_eq!(
            command_lines("echo \"a && b\" || true"),
            vec!["echo \"a && b\"", "true"]
        );
    }

    #[test]
    fn lines_handle_continuations_pipes_comments_and_set() {
        assert_eq!(
            command_lines("mv a b \\\n   c d/\n# note\nls # trailing\n"),
            vec!["mv a b c d/", "ls"]
        );
        assert_eq!(
            command_lines("ps aux | grep x |\n  wc -l; echo $(a; b)"),
            vec!["ps aux | grep x | wc -l", "echo $(a; b)"]
        );
        assert_eq!(
            command_lines(
                "#!/bin/sh\nset -e\nset -euo pipefail\nset -x\nset +e\nset -o pipefail\nls"
            ),
            vec!["ls"]
        );
        assert_eq!(command_lines("echo ${#x} $#"), vec!["echo ${#x} $#"]);
    }

    #[test]
    fn heredoc_bodies_stay_with_their_command() {
        let script =
            "cat > /etc/x <<'EOF'\nline; one\n# not a comment\nEOF\nsystemctl reload nginx";
        assert_eq!(
            command_lines(script),
            vec![
                "cat > /etc/x <<'EOF'\nline; one\n# not a comment\nEOF",
                "systemctl reload nginx"
            ]
        );
        let tabbed = "cat <<-END; echo after\n\tbody\n\tEND\nls";
        assert_eq!(
            command_lines(tabbed),
            vec!["cat <<-END; echo after\n\tbody\n\tEND", "ls"]
        );
        let s = sum(script);
        assert!(
            s.contains("edits 1 path") && s.contains("reloads nginx"),
            "{s}"
        );
    }
}
