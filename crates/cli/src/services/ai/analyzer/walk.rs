//! AST walker: finds every simple command, peels wrappers (`sudo`, `env`, `sh -c`...),
//! and asks the rule table what each one does.

use std::collections::BTreeSet;

use crate::services::ai::types::{RiskTag, SimpleCommand};

use super::ast::{GroupKind, Node, Part, Redir, RedirKind, SimpleCmd, Word};
use super::lexer::MAX_DEPTH;
use super::parser::parse_program;
use super::rules::{classify, Arg, Class};
use super::special::find_exec_commands;

/// Directories whose programs we trust by name. Anything else with a path is unknown:
/// `/tmp/x/ls` is not `ls`.
const SYSTEM_DIRS: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/games",
    "/snap/bin",
    "/opt/homebrew/bin",
];

/// Variables that change what a command loads or runs.
const RISKY_VARS: &[&str] = &[
    "PATH",
    "IFS",
    "ENV",
    "BASH_ENV",
    "PROMPT_COMMAND",
    "PS4",
    "SHELLOPTS",
    "BASHOPTS",
    "CDPATH",
    "GLOBIGNORE",
    "PAGER",
    "MANPAGER",
    "EDITOR",
    "VISUAL",
    "SYSTEMD_PAGER",
    "SYSTEMD_EDITOR",
    "SUDO_EDITOR",
    "SUDO_ASKPASS",
    "SSH_ASKPASS",
    "HOSTALIASES",
    "LOCPATH",
    "NLSPATH",
    "TERMINFO",
    "TERMCAP",
    "GCONV_PATH",
    "LESSOPEN",
    "LESSCLOSE",
    "HISTFILE",
    "HOME",
];
const RISKY_VAR_PREFIXES: &[&str] = &[
    "LD_",
    "DYLD_",
    "GIT_",
    "PERL",
    "PYTHON",
    "RUBY",
    "NODE_",
    "JAVA_TOOL",
    "_JAVA_",
    "BASH_FUNC_",
];

#[derive(Clone, Default)]
struct Ctx {
    /// Wrappers peeled so far, outermost first.
    wrappers: Vec<String>,
    /// Tags every command in scope gets (`sudo` adds privilege, `/dev/tcp` adds network).
    extra: BTreeSet<RiskTag>,
    /// Redirection targets written to, from the command and from enclosing groups.
    writes: Vec<String>,
    depth: usize,
}

impl Ctx {
    fn fresh(&self) -> Ctx {
        Ctx {
            depth: self.depth,
            ..Ctx::default()
        }
    }

    fn wrapped(&self, name: &str) -> Ctx {
        let mut c = self.clone();
        c.wrappers.push(name.to_string());
        c
    }
}

#[derive(Default)]
pub struct Walker {
    pub commands: Vec<SimpleCommand>,
    pub unknown: Option<String>,
}

pub fn walk(root: &Node) -> Walker {
    let mut w = Walker::default();
    w.node(root, &Ctx::default());
    w
}

/// What a wrapper's options looked like.
struct Opts {
    /// Index of the first argument that is not an option.
    rest: usize,
    /// Flags seen: `-i`, `--login`.
    seen: Vec<String>,
    /// Options with values: (`-u`, `root`).
    values: Vec<(String, String)>,
}

impl Opts {
    fn has(&self, names: &[&str]) -> bool {
        self.seen
            .iter()
            .chain(self.values.iter().map(|(k, _)| k))
            .any(|s| names.contains(&s.as_str()))
    }

    fn value(&self, names: &[&str]) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| names.contains(&k.as_str()))
            .map(|(_, v)| v.as_str())
    }
}

/// Option grammar of a wrapper.
struct Spec {
    /// Short flags without a value.
    flags: &'static str,
    /// Short flags with a value (attached or next argument).
    with_arg: &'static str,
    /// Long options: (`--name`, takes a value).
    long: &'static [(&'static str, bool)],
    /// Accept `-5` style numbers (`nice -5`).
    numeric: bool,
}

fn parse_opts(args: &[Arg], spec: &Spec) -> Result<Opts, String> {
    let mut o = Opts {
        rest: 0,
        seen: Vec::new(),
        values: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let t = args[i].text.as_str();
        if t == "--" {
            i += 1;
            break;
        }
        if !t.starts_with('-') || t == "-" {
            break;
        }
        if let Some(long) = t.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            let full = format!("--{name}");
            let Some((_, takes)) = spec.long.iter().find(|(n, _)| *n == full) else {
                return Err(format!("unknown option {full}"));
            };
            if *takes {
                let v = match value {
                    Some(v) => v.to_string(),
                    None => {
                        i += 1;
                        args.get(i)
                            .map(|a| a.text.clone())
                            .ok_or_else(|| format!("{full} needs a value"))?
                    }
                };
                o.values.push((full, v));
            } else {
                o.seen.push(full);
            }
            i += 1;
            continue;
        }
        if spec.numeric && t[1..].chars().all(|c| c.is_ascii_digit()) {
            i += 1;
            continue;
        }
        let chars: Vec<char> = t[1..].chars().collect();
        for (k, ch) in chars.iter().enumerate() {
            if spec.with_arg.contains(*ch) {
                let rest: String = chars[k + 1..].iter().collect();
                let v = if rest.is_empty() {
                    i += 1;
                    args.get(i)
                        .map(|a| a.text.clone())
                        .ok_or_else(|| format!("-{ch} needs a value"))?
                } else {
                    rest
                };
                o.values.push((format!("-{ch}"), v));
                break;
            } else if spec.flags.contains(*ch) {
                o.seen.push(format!("-{ch}"));
            } else {
                return Err(format!("unknown option -{ch}"));
            }
        }
        i += 1;
    }
    o.rest = i;
    Ok(o)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `program` is trusted by name when it has no path or lives in a system directory.
fn trusted_path(program: &str) -> bool {
    match program.rfind('/') {
        None => true,
        Some(p) => SYSTEM_DIRS.contains(&&program[..p]),
    }
}

fn is_assignment(text: &str) -> bool {
    let Some(eq) = text.find('=') else {
        return false;
    };
    let name = text[..eq].strip_suffix('+').unwrap_or(&text[..eq]);
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Walker {
    fn note_unknown(&mut self, reason: impl Into<String>) {
        if self.unknown.is_none() {
            self.unknown = Some(reason.into());
        }
    }

    // ------------------------------------------------------------ nodes

    fn node(&mut self, node: &Node, ctx: &Ctx) {
        match node {
            Node::Simple(cmd) => self.simple(cmd, ctx),
            Node::Pipeline(stages) => {
                for s in stages {
                    self.node(s, ctx);
                }
            }
            Node::Seq(items) => {
                for s in items {
                    self.node(s, ctx);
                }
            }
            Node::Group {
                kind,
                body,
                words,
                redirs,
            } => {
                let mut inner = ctx.clone();
                self.redirs(redirs, &mut inner);
                let fresh = ctx.fresh();
                for w in words {
                    self.subs(&w.subs, &fresh);
                }
                let _ = matches!(kind, GroupKind::Function);
                for s in body {
                    self.node(s, &inner);
                }
            }
        }
    }

    /// Commands inside expansions run on their own, without our wrappers or redirections.
    fn subs(&mut self, subs: &[Node], fresh: &Ctx) {
        for s in subs {
            self.node(s, fresh);
        }
    }

    fn redirs(&mut self, redirs: &[Redir], ctx: &mut Ctx) {
        let fresh = ctx.fresh();
        for r in redirs {
            self.subs(&r.target.subs, &fresh);
            self.subs(&r.subs, &fresh);
            let target = r.target.text();
            if target.starts_with("/dev/tcp/") || target.starts_with("/dev/udp/") {
                ctx.extra.insert(RiskTag::Network);
                ctx.extra.insert(RiskTag::ExfilRisk);
            }
            let writes = match r.kind {
                RedirKind::Out
                | RedirKind::Append
                | RedirKind::Clobber
                | RedirKind::ReadWrite
                | RedirKind::OutErr
                | RedirKind::AppendErr => true,
                RedirKind::DupOut => {
                    !(target == "-"
                        || (!target.is_empty() && target.chars().all(|c| c.is_ascii_digit())))
                }
                _ => false,
            };
            if writes && !harmless_target(&target) {
                if is_block_device(&target) {
                    ctx.extra.insert(RiskTag::Destructive);
                }
                ctx.writes.push(target);
            }
        }
    }

    fn simple(&mut self, cmd: &SimpleCmd, ctx: &Ctx) {
        let fresh = ctx.fresh();
        let mut ctx = ctx.clone();
        for a in &cmd.assigns {
            self.subs(&a.value.subs, &fresh);
            self.check_var(&a.name);
        }
        for w in &cmd.words {
            self.subs(&w.subs, &fresh);
        }
        self.redirs(&cmd.redirs, &mut ctx);
        let args: Vec<Arg> = cmd.words.iter().flat_map(word_args).collect();
        if args.is_empty() {
            if !ctx.writes.is_empty() {
                let class = Class::new("redirection", &[RiskTag::Write]);
                self.emit(":", &[], &ctx, class);
            }
            return;
        }
        self.command(args, &ctx);
    }

    fn check_var(&mut self, name: &str) {
        if RISKY_VARS.contains(&name) || RISKY_VAR_PREFIXES.iter().any(|p| name.starts_with(p)) {
            self.note_unknown(format!(
                "sets {name}, which changes what commands load or run"
            ));
        }
    }

    // ------------------------------------------------------------ emitting

    fn emit(&mut self, program: &str, args: &[Arg], ctx: &Ctx, class: Class) {
        let mut tags = class.tags;
        tags.extend(ctx.extra.iter().copied());
        if !ctx.writes.is_empty() {
            tags.insert(RiskTag::Write);
        }
        if let Some(reason) = class.unknown {
            self.note_unknown(reason);
        }
        self.commands.push(SimpleCommand {
            program: program.to_string(),
            args: args.iter().map(|a| a.text.clone()).collect(),
            wrappers: ctx.wrappers.clone(),
            writes_to: ctx.writes.clone(),
            tags,
            rule: Some(class.rule),
        });
    }

    fn script(&mut self, text: &str, ctx: &Ctx) {
        if ctx.depth + 1 > MAX_DEPTH {
            self.note_unknown("shell strings nested too deep");
            return;
        }
        match parse_program(text, ctx.depth + 1) {
            Ok(node) => {
                let mut inner = ctx.clone();
                inner.depth += 1;
                self.node(&node, &inner);
            }
            Err(e) => self.note_unknown(format!("could not parse an inner shell string: {e}")),
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, args: Vec<Arg>, ctx: &Ctx) {
        let first = &args[0];
        if first.dynamic {
            let class = Class::unknown(
                first.text.clone(),
                format!("command name '{}' is only known at run time", first.text),
            );
            self.emit(&first.text, &args[1..], ctx, class);
            return;
        }
        if ctx.wrappers.len() > 24 {
            self.note_unknown("wrappers nested too deep");
            return;
        }
        let text = first.text.clone();
        let prog = basename(&text).to_string();
        let rest = &args[1..];
        if !trusted_path(&text) {
            let class = Class::unknown(
                prog.clone(),
                format!("'{text}' runs a program outside the system directories"),
            );
            self.emit(&text, rest, ctx, class);
            return;
        }
        match prog.as_str() {
            "sudo" | "doas" | "pkexec" => self.sudo(&text, &prog, rest, ctx),
            "su" => self.su(&text, rest, ctx),
            "env" => self.env(&text, rest, ctx),
            "timeout" | "nice" | "ionice" | "nohup" | "stdbuf" | "setsid" | "time" | "command"
            | "builtin" | "exec" | "taskset" | "flock" => {
                self.simple_wrapper(&text, &prog, rest, ctx)
            }
            "busybox" if !rest.is_empty() => self.command(rest.to_vec(), &ctx.wrapped("busybox")),
            "busybox" => {
                let class = Class::unknown("busybox", "busybox without an applet");
                self.emit(&text, rest, ctx, class)
            }
            "xargs" => self.xargs(&text, rest, ctx),
            "watch" => self.watch(&text, rest, ctx),
            "sh" | "bash" | "dash" | "zsh" | "ksh" | "ash" | "mksh" | "rbash" | "fish" | "csh"
            | "tcsh" => self.shell(&text, &prog, rest, ctx),
            "eval" => {
                let class = Class::unknown("eval", "eval runs a string built at run time");
                self.emit(&text, rest, ctx, class);
                if rest.iter().all(|a| !a.dynamic) && !rest.is_empty() {
                    let joined = rest
                        .iter()
                        .map(|a| a.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    self.script(&joined, &ctx.wrapped("eval"));
                }
            }
            "find" => {
                let class = classify("find", rest);
                self.emit(&text, rest, ctx, class);
                match find_exec_commands(rest) {
                    Ok(list) => {
                        for (opt, inner) in list {
                            self.command(inner, &ctx.wrapped(&format!("find {opt}")));
                        }
                    }
                    Err(e) => self.note_unknown(e),
                }
            }
            _ => {
                if matches!(
                    prog.as_str(),
                    "export" | "declare" | "typeset" | "readonly" | "local"
                ) {
                    for a in rest {
                        if is_assignment(&a.text) {
                            let name = a
                                .text
                                .split('=')
                                .next()
                                .unwrap_or("")
                                .trim_end_matches('+')
                                .to_string();
                            self.check_var(&name);
                        }
                    }
                }
                let class = classify(&prog, rest);
                self.emit(&text, rest, ctx, class);
            }
        }
    }

    fn leaf_unknown(&mut self, text: &str, prog: &str, rest: &[Arg], ctx: &Ctx, reason: String) {
        let class = Class::unknown(prog, reason);
        self.emit(text, rest, ctx, class);
    }

    fn sudo(&mut self, text: &str, prog: &str, rest: &[Arg], ctx: &Ctx) {
        let spec = match prog {
            "doas" => Spec {
                flags: "nsL",
                with_arg: "uC",
                long: &[],
                numeric: false,
            },
            "pkexec" => Spec {
                flags: "",
                with_arg: "",
                long: &[
                    ("--user", true),
                    ("--disable-internal-agent", false),
                    ("--keep-cwd", false),
                    ("--version", false),
                    ("--help", false),
                ],
                numeric: false,
            },
            _ => Spec {
                flags: "AbEeHhiKklnPSsVvB",
                with_arg: "CDgpRrTtUu",
                long: &[
                    ("--askpass", false),
                    ("--background", false),
                    ("--preserve-env", false),
                    ("--edit", false),
                    ("--set-home", false),
                    ("--help", false),
                    ("--login", false),
                    ("--remove-timestamp", false),
                    ("--reset-timestamp", false),
                    ("--list", false),
                    ("--non-interactive", false),
                    ("--preserve-groups", false),
                    ("--stdin", false),
                    ("--shell", false),
                    ("--validate", false),
                    ("--version", false),
                    ("--user", true),
                    ("--group", true),
                    ("--host", true),
                    ("--prompt", true),
                    ("--role", true),
                    ("--type", true),
                    ("--other-user", true),
                    ("--chdir", true),
                    ("--close-from", true),
                    ("--command-timeout", true),
                    ("--chroot", true),
                ],
                numeric: false,
            },
        };
        let opts = match parse_opts(rest, &spec) {
            Ok(o) => o,
            Err(e) => return self.leaf_unknown(text, prog, rest, ctx, format!("{prog}: {e}")),
        };
        let mut inner_ctx = ctx.wrapped(prog);
        inner_ctx.extra.insert(RiskTag::Privilege);
        let mut idx = opts.rest;
        while idx < rest.len() && is_assignment(&rest[idx].text) {
            let name = rest[idx].text.split('=').next().unwrap_or("").to_string();
            self.check_var(name.trim_end_matches('+'));
            idx += 1;
        }
        let inner = &rest[idx..];
        if opts.has(&["-e", "--edit"]) {
            let class = Class::new(
                format!("{prog} (edit)"),
                &[RiskTag::Privilege, RiskTag::Write],
            );
            return self.emit(text, rest, ctx, class);
        }
        if inner.is_empty()
            && opts.has(&[
                "-l",
                "--list",
                "-v",
                "--validate",
                "-k",
                "-K",
                "--remove-timestamp",
                "--reset-timestamp",
                "-V",
                "--version",
                "-h",
                "--help",
            ])
        {
            let class = Class::new(format!("{prog} (no command)"), &[RiskTag::Privilege]);
            return self.emit(text, rest, ctx, class);
        }
        if opts.has(&["-l", "--list"]) {
            // `sudo -l cmd` only checks permission.
            let class = Class::new(format!("{prog} -l"), &[RiskTag::Privilege]);
            return self.emit(text, rest, ctx, class);
        }
        if opts.has(&["-s", "--shell", "-i", "--login"]) {
            if inner.is_empty() {
                return self.leaf_unknown(
                    text,
                    prog,
                    rest,
                    &inner_ctx,
                    format!("{prog}: opens an interactive shell"),
                );
            }
            if inner.iter().any(|a| a.dynamic) {
                return self.leaf_unknown(
                    text,
                    prog,
                    rest,
                    &inner_ctx,
                    format!("{prog}: shell command only known at run time"),
                );
            }
            let joined = inner
                .iter()
                .map(|a| a.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            return self.script(&joined, &inner_ctx);
        }
        if inner.is_empty() {
            return self.leaf_unknown(
                text,
                prog,
                rest,
                &inner_ctx,
                format!("{prog} without a command"),
            );
        }
        self.command(inner.to_vec(), &inner_ctx);
    }

    fn su(&mut self, text: &str, rest: &[Arg], ctx: &Ctx) {
        let mut inner_ctx = ctx.wrapped("su");
        inner_ctx.extra.insert(RiskTag::Privilege);
        let mut i = 0;
        let mut cmd: Option<&Arg> = None;
        while i < rest.len() {
            let t = rest[i].text.as_str();
            i += 1;
            match t {
                "-c" | "--command" => {
                    let Some(c) = rest.get(i) else {
                        return self.leaf_unknown(
                            text,
                            "su",
                            rest,
                            &inner_ctx,
                            "su -c without a command".into(),
                        );
                    };
                    cmd = Some(c);
                    i += 1;
                }
                "-s" | "--shell" | "-g" | "--group" | "-G" | "--supp-group" | "-w" => i += 1,
                _ if t.starts_with("--command=") => {
                    let s = t["--command=".len()..].to_string();
                    return self.script(&s, &inner_ctx);
                }
                _ => {}
            }
        }
        match cmd {
            Some(c) if !c.dynamic => {
                let s = c.text.clone();
                self.script(&s, &inner_ctx)
            }
            Some(_) => self.leaf_unknown(
                text,
                "su",
                rest,
                &inner_ctx,
                "su -c string only known at run time".into(),
            ),
            None => self.leaf_unknown(
                text,
                "su",
                rest,
                &inner_ctx,
                "su opens an interactive shell".into(),
            ),
        }
    }

    fn env(&mut self, text: &str, rest: &[Arg], ctx: &Ctx) {
        let spec = Spec {
            flags: "i0v",
            with_arg: "uCSP",
            long: &[
                ("--ignore-environment", false),
                ("--null", false),
                ("--unset", true),
                ("--chdir", true),
                ("--split-string", true),
                ("--debug", false),
                ("--help", false),
                ("--version", false),
            ],
            numeric: false,
        };
        let opts = match parse_opts(rest, &spec) {
            Ok(o) => o,
            Err(e) => return self.leaf_unknown(text, "env", rest, ctx, format!("env: {e}")),
        };
        if opts.has(&["-S", "--split-string", "-P"]) {
            return self.leaf_unknown(
                text,
                "env",
                rest,
                ctx,
                "env -S/-P builds the command at run time".into(),
            );
        }
        let mut idx = opts.rest;
        while idx < rest.len() && is_assignment(&rest[idx].text) {
            let name = rest[idx].text.split('=').next().unwrap_or("").to_string();
            self.check_var(name.trim_end_matches('+'));
            idx += 1;
        }
        let inner = &rest[idx..];
        if inner.is_empty() {
            let class = Class::new("env (print)", &[RiskTag::Read]);
            return self.emit(text, rest, ctx, class);
        }
        self.command(inner.to_vec(), &ctx.wrapped("env"));
    }

    /// `timeout`, `nice`, `nohup`...: options, optional leading positionals, then the command.
    fn simple_wrapper(&mut self, text: &str, prog: &str, rest: &[Arg], ctx: &Ctx) {
        let (spec, positionals, no_command_read) = match prog {
            "timeout" => (
                Spec {
                    flags: "v",
                    with_arg: "ks",
                    long: &[
                        ("--kill-after", true),
                        ("--signal", true),
                        ("--preserve-status", false),
                        ("--foreground", false),
                        ("--verbose", false),
                    ],
                    numeric: false,
                },
                1,
                false,
            ),
            "nice" => (
                Spec {
                    flags: "",
                    with_arg: "n",
                    long: &[("--adjustment", true)],
                    numeric: true,
                },
                0,
                true,
            ),
            "ionice" => (
                Spec {
                    flags: "thp",
                    with_arg: "cnpPu",
                    long: &[
                        ("--class", true),
                        ("--classdata", true),
                        ("--pid", true),
                        ("--pgid", true),
                        ("--uid", true),
                        ("--ignore", false),
                        ("--help", false),
                    ],
                    numeric: false,
                },
                0,
                false,
            ),
            "nohup" => (
                Spec {
                    flags: "",
                    with_arg: "",
                    long: &[("--help", false), ("--version", false)],
                    numeric: false,
                },
                0,
                true,
            ),
            "stdbuf" => (
                Spec {
                    flags: "",
                    with_arg: "ioe",
                    long: &[("--input", true), ("--output", true), ("--error", true)],
                    numeric: false,
                },
                0,
                false,
            ),
            "setsid" => (
                Spec {
                    flags: "cfw",
                    with_arg: "",
                    long: &[("--ctty", false), ("--fork", false), ("--wait", false)],
                    numeric: false,
                },
                0,
                false,
            ),
            "time" => (
                Spec {
                    flags: "pv",
                    with_arg: "",
                    long: &[("--portability", false), ("--verbose", false)],
                    numeric: false,
                },
                0,
                true,
            ),
            "command" => (
                Spec {
                    flags: "pvV",
                    with_arg: "",
                    long: &[],
                    numeric: false,
                },
                0,
                true,
            ),
            "builtin" => (
                Spec {
                    flags: "",
                    with_arg: "",
                    long: &[],
                    numeric: false,
                },
                0,
                true,
            ),
            "exec" => (
                Spec {
                    flags: "cl",
                    with_arg: "a",
                    long: &[],
                    numeric: false,
                },
                0,
                true,
            ),
            "taskset" => (
                Spec {
                    flags: "acpr",
                    with_arg: "",
                    long: &[
                        ("--all-tasks", false),
                        ("--cpu-list", false),
                        ("--pid", false),
                        ("--reset-affinity", false),
                    ],
                    numeric: false,
                },
                1,
                false,
            ),
            _ => (
                // flock
                Spec {
                    flags: "sxunoFh",
                    with_arg: "wEc",
                    long: &[
                        ("--shared", false),
                        ("--exclusive", false),
                        ("--unlock", false),
                        ("--nonblock", false),
                        ("--nb", false),
                        ("--no-fork", false),
                        ("--close", false),
                        ("--verbose", false),
                        ("--timeout", true),
                        ("--wait", true),
                        ("--conflict-exit-code", true),
                        ("--command", true),
                        ("--help", false),
                    ],
                    numeric: false,
                },
                1,
                false,
            ),
        };
        let opts = match parse_opts(rest, &spec) {
            Ok(o) => o,
            Err(e) => return self.leaf_unknown(text, prog, rest, ctx, format!("{prog}: {e}")),
        };
        if prog == "command" && opts.has(&["-v", "-V"]) {
            let class = Class::new("command -v", &[RiskTag::Read]);
            return self.emit(text, rest, ctx, class);
        }
        if (prog == "ionice" && opts.has(&["-p", "-P", "-u", "--pid", "--pgid", "--uid"]))
            || (prog == "taskset" && opts.has(&["-p", "--pid"]))
        {
            let class = Class::new(
                format!("{prog} (other processes)"),
                &[RiskTag::ServiceChange],
            );
            return self.emit(text, rest, ctx, class);
        }
        if prog == "flock" {
            if let Some(cmd) = opts.value(&["-c", "--command"]) {
                let cmd = cmd.to_string();
                return self.script(&cmd, &ctx.wrapped("flock"));
            }
        }
        let idx = opts.rest + positionals;
        let inner = rest.get(idx..).unwrap_or(&[]);
        if inner.is_empty() {
            return if no_command_read {
                let class = Class::new(format!("{prog} (no command)"), &[RiskTag::Read]);
                self.emit(text, rest, ctx, class)
            } else {
                self.leaf_unknown(text, prog, rest, ctx, format!("{prog} without a command"))
            };
        }
        self.command(inner.to_vec(), &ctx.wrapped(prog));
    }

    fn xargs(&mut self, text: &str, rest: &[Arg], ctx: &Ctx) {
        let mut i = 0;
        let mut repl: Option<String> = None;
        while i < rest.len() {
            let t = rest[i].text.as_str();
            if t == "--" {
                i += 1;
                break;
            }
            if !t.starts_with('-') || t == "-" {
                break;
            }
            i += 1;
            if let Some(long) = t.strip_prefix("--") {
                let (name, value) = match long.split_once('=') {
                    Some((n, v)) => (n, Some(v)),
                    None => (long, None),
                };
                match name {
                    "null" | "no-run-if-empty" | "verbose" | "interactive" | "open-tty"
                    | "exit" | "show-limits" | "help" | "version" => {}
                    "arg-file" | "delimiter" | "max-args" | "max-chars" | "max-procs"
                    | "process-slot-var" => {
                        if value.is_none() {
                            i += 1;
                        }
                    }
                    "replace" => repl = Some(value.unwrap_or("{}").to_string()),
                    "eof" | "max-lines" => {}
                    _ => {
                        return self.leaf_unknown(
                            text,
                            "xargs",
                            rest,
                            ctx,
                            format!("xargs: unknown option --{name}"),
                        )
                    }
                }
                continue;
            }
            let chars: Vec<char> = t[1..].chars().collect();
            for (k, ch) in chars.iter().enumerate() {
                let tail: String = chars[k + 1..].iter().collect();
                match ch {
                    '0' | 'r' | 't' | 'p' | 'o' | 'x' | 'h' | 'V' => {}
                    'I' | 'E' | 'L' | 'n' | 'P' | 's' | 'a' | 'd' => {
                        let v = if tail.is_empty() {
                            let v = rest.get(i).map(|a| a.text.clone()).unwrap_or_default();
                            i += 1;
                            v
                        } else {
                            tail
                        };
                        if *ch == 'I' {
                            repl = Some(v);
                        }
                        break;
                    }
                    'i' => {
                        repl = Some(if tail.is_empty() { "{}".into() } else { tail });
                        break;
                    }
                    'l' | 'e' => break,
                    _ => {
                        return self.leaf_unknown(
                            text,
                            "xargs",
                            rest,
                            ctx,
                            format!("xargs: unknown option -{ch}"),
                        )
                    }
                }
            }
        }
        let mut inner: Vec<Arg> = rest[i.min(rest.len())..]
            .iter()
            .map(|a| match &repl {
                Some(r) if !r.is_empty() && a.text.contains(r.as_str()) => {
                    Arg::placeholder(&a.text)
                }
                _ => a.clone(),
            })
            .collect();
        if inner.is_empty() {
            inner.push(Arg::lit("echo"));
        }
        if repl.is_none() {
            inner.push(Arg::placeholder("<xargs input>"));
        }
        self.command(inner, &ctx.wrapped("xargs"));
    }

    fn watch(&mut self, text: &str, rest: &[Arg], ctx: &Ctx) {
        let spec = Spec {
            flags: "bcdegpqrtwxhv",
            with_arg: "n",
            long: &[
                ("--interval", true),
                ("--differences", false),
                ("--beep", false),
                ("--color", false),
                ("--no-color", false),
                ("--errexit", false),
                ("--chgexit", false),
                ("--exec", false),
                ("--precise", false),
                ("--no-title", false),
                ("--no-wrap", false),
                ("--help", false),
                ("--version", false),
                ("--equexit", true),
            ],
            numeric: false,
        };
        let opts = match parse_opts(rest, &spec) {
            Ok(o) => o,
            Err(e) => return self.leaf_unknown(text, "watch", rest, ctx, format!("watch: {e}")),
        };
        let inner = &rest[opts.rest..];
        if inner.is_empty() {
            return self.leaf_unknown(text, "watch", rest, ctx, "watch without a command".into());
        }
        let joined = inner
            .iter()
            .map(|a| a.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        self.script(&joined, &ctx.wrapped("watch"));
    }

    fn shell(&mut self, text: &str, prog: &str, rest: &[Arg], ctx: &Ctx) {
        let mut c_flag = false;
        let mut s_flag = false;
        let mut i = 0;
        while i < rest.len() {
            let t = rest[i].text.as_str();
            if t == "--" {
                i += 1;
                break;
            }
            if let Some(long) = t.strip_prefix("--") {
                match long.split('=').next().unwrap_or(long) {
                    "login" | "norc" | "noprofile" | "posix" | "restricted" | "verbose"
                    | "noediting" | "debugger" | "dump-strings" | "dump-po-strings" | "version"
                    | "help" => i += 1,
                    "rcfile" | "init-file" => i += 2,
                    other => {
                        return self.leaf_unknown(
                            text,
                            prog,
                            rest,
                            ctx,
                            format!("{prog}: unknown option --{other}"),
                        )
                    }
                }
            } else if (t.starts_with('-') || t.starts_with('+')) && t.len() > 1 {
                let mut skip_value = false;
                for ch in t[1..].chars() {
                    match ch {
                        'c' if t.starts_with('-') => c_flag = true,
                        's' if t.starts_with('-') => s_flag = true,
                        'o' | 'O' => skip_value = true,
                        'a' | 'b' | 'e' | 'f' | 'h' | 'i' | 'l' | 'm' | 'n' | 'p' | 'u' | 'v'
                        | 'x' | 'B' | 'C' | 'E' | 'H' | 'P' | 'T' | 'r' | 'k' | 'D' | 'I' | 'L' => {
                        }
                        _ => {
                            return self.leaf_unknown(
                                text,
                                prog,
                                rest,
                                ctx,
                                format!("{prog}: unknown option {t}"),
                            )
                        }
                    }
                }
                i += 1;
                if skip_value {
                    i += 1;
                }
            } else {
                break;
            }
        }
        if c_flag {
            let Some(script) = rest.get(i) else {
                return self.leaf_unknown(
                    text,
                    prog,
                    rest,
                    ctx,
                    format!("{prog} -c without a string"),
                );
            };
            if script.dynamic {
                return self.leaf_unknown(
                    text,
                    prog,
                    rest,
                    ctx,
                    format!("{prog} -c string only known at run time"),
                );
            }
            let s = script.text.clone();
            return self.script(&s, &ctx.wrapped(&format!("{prog} -c")));
        }
        let reason = if i < rest.len() && !s_flag {
            format!("{prog} runs a script file")
        } else {
            format!("{prog} reads commands from its input")
        };
        self.leaf_unknown(text, prog, rest, ctx, reason);
    }
}

// ---------------------------------------------------------------- words

fn harmless_target(t: &str) -> bool {
    t == "/dev/null"
        || t == "/dev/stdout"
        || t == "/dev/stderr"
        || t == "/dev/tty"
        || t.starts_with("/dev/fd/")
}

fn is_block_device(t: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "/dev/sd",
        "/dev/hd",
        "/dev/vd",
        "/dev/xvd",
        "/dev/nvme",
        "/dev/mmcblk",
        "/dev/loop",
        "/dev/dm-",
        "/dev/md",
        "/dev/mapper/",
        "/dev/mem",
        "/dev/kmem",
        "/dev/port",
        "/dev/disk/",
    ];
    PREFIXES.iter().any(|p| t.starts_with(p))
}

/// One word becomes one argument, or several after brace expansion.
fn word_args(w: &Word) -> Vec<Arg> {
    let text = w.text();
    let dynamic = w.is_dynamic();
    let first_dyn = matches!(w.parts.first(), Some(Part::Dynamic(_)));
    let glob_lead = matches!(
        w.parts.first(),
        Some(Part::Lit { text, quoted: false }) if text.starts_with(['*', '?', '['])
    );
    let has_brace = w.parts.iter().any(|p| {
        matches!(p, Part::Lit { text, quoted: false } if text.contains('{') && text.contains('}'))
    });
    if has_brace {
        if dynamic {
            return vec![Arg {
                text,
                dynamic: true,
                flaggy: true,
            }];
        }
        let chars: Vec<(char, bool)> = w
            .parts
            .iter()
            .flat_map(|p| match p {
                Part::Lit { text, quoted } => {
                    text.chars().map(|c| (c, *quoted)).collect::<Vec<_>>()
                }
                Part::Dynamic(_) => Vec::new(),
            })
            .collect();
        let mut budget = 256usize;
        return match brace_expand(&chars, &mut budget) {
            Some(items) => items
                .into_iter()
                .map(|t| Arg {
                    flaggy: t.starts_with(['*', '?', '[']),
                    text: t,
                    dynamic: false,
                })
                .collect(),
            None => vec![Arg {
                text,
                dynamic: true,
                flaggy: true,
            }],
        };
    }
    let flaggy = (dynamic && (first_dyn || text.starts_with('-'))) || glob_lead;
    vec![Arg {
        text,
        dynamic,
        flaggy,
    }]
}

enum Brace {
    Literal,
    TooBig,
    Alts(usize, Vec<Vec<(char, bool)>>),
}

fn brace_expand(cs: &[(char, bool)], budget: &mut usize) -> Option<Vec<String>> {
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == ('{', false) {
            match parse_brace(cs, i) {
                Brace::TooBig => return None,
                Brace::Alts(close, alts) => {
                    let mut out = Vec::new();
                    for alt in alts {
                        let mut combined: Vec<(char, bool)> = cs[..i].to_vec();
                        combined.extend(alt);
                        combined.extend_from_slice(&cs[close + 1..]);
                        out.extend(brace_expand(&combined, budget)?);
                        if out.len() > *budget {
                            return None;
                        }
                    }
                    return Some(out);
                }
                Brace::Literal => {}
            }
        }
        i += 1;
    }
    Some(vec![cs.iter().map(|c| c.0).collect()])
}

fn parse_brace(cs: &[(char, bool)], open: usize) -> Brace {
    let mut depth = 0usize;
    let mut commas: Vec<usize> = Vec::new();
    let mut close = None;
    for (k, &(c, quoted)) in cs.iter().enumerate().skip(open + 1) {
        if quoted {
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => {
                close = Some(k);
                break;
            }
            '}' => depth -= 1,
            ',' if depth == 0 => commas.push(k),
            _ => {}
        }
    }
    let Some(close) = close else {
        return Brace::Literal;
    };
    if !commas.is_empty() {
        let mut alts = Vec::new();
        let mut start = open + 1;
        for &c in commas.iter().chain(std::iter::once(&close)) {
            alts.push(cs[start..c].to_vec());
            start = c + 1;
        }
        return Brace::Alts(close, alts);
    }
    // `{a..z}`, `{1..10..2}`
    let body: String = cs[open + 1..close].iter().map(|c| c.0).collect();
    let pieces: Vec<&str> = body.split("..").collect();
    if pieces.len() < 2 || pieces.len() > 3 {
        return Brace::Literal;
    }
    let step = match pieces.get(2) {
        Some(s) => match s.parse::<i64>() {
            Ok(v) if v != 0 => v.abs(),
            _ => return Brace::Literal,
        },
        None => 1,
    };
    let items: Vec<String> =
        if let (Ok(a), Ok(b)) = (pieces[0].parse::<i64>(), pieces[1].parse::<i64>()) {
            if (b - a).unsigned_abs() / step as u64 > 256 {
                return Brace::TooBig;
            }
            let mut v = Vec::new();
            let mut x = a;
            while if a <= b { x <= b } else { x >= b } {
                v.push(x.to_string());
                x += if a <= b { step } else { -step };
            }
            v
        } else {
            let (mut a, mut b) = (pieces[0].chars(), pieces[1].chars());
            match (a.next(), a.next(), b.next(), b.next()) {
                (Some(x), None, Some(y), None)
                    if x.is_ascii_alphabetic() && y.is_ascii_alphabetic() =>
                {
                    let (x, y) = (x as i64, y as i64);
                    let mut v = Vec::new();
                    let mut c = x;
                    while if x <= y { c <= y } else { c >= y } {
                        v.push(((c as u8) as char).to_string());
                        c += if x <= y { step } else { -step };
                    }
                    v
                }
                _ => return Brace::Literal,
            }
        };
    Brace::Alts(
        close,
        items
            .into_iter()
            .map(|s| s.chars().map(|c| (c, false)).collect())
            .collect(),
    )
}
