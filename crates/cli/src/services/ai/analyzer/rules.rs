//! Rule database: program + subcommand + flags -> risk tags.
//!
//! Data first: `RULES` holds programs whose risk depends on the subcommand or flags,
//! `READ_ONLY` the ones that never change anything. Anything not listed is unknown.
//! Programs with logic that doesn't fit the table live in `special.rs`.
//!
//! Matching rules (first case that applies wins, so list dangerous cases first):
//! - `sub`: consecutive arguments right after the global options, e.g. `"rollout status"`.
//!   Words accept `a|b` alternatives and `*` globs; a trailing `$` word means "no more
//!   positional arguments". `;` separates whole alternative phrases (`config get;cache dir`).
//! - `has` / `lacks`: argument patterns (see `arg_matches`).
//! - A global option the table doesn't list makes the subcommand unknown, never a guess.

use crate::services::ai::types::RiskTag;
use std::collections::BTreeSet;

use super::special;

/// One shell word after expansion, as the rules see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    pub text: String,
    /// Contains an expansion we can't resolve (`$x`, `$(...)`).
    pub dynamic: bool,
    /// Could turn into an option (or a different word entirely) at run time.
    pub flaggy: bool,
}

impl Arg {
    pub fn lit(text: &str) -> Self {
        Arg {
            text: text.to_string(),
            dynamic: false,
            flaggy: false,
        }
    }

    /// Stands for words that arrive later (`xargs` input, `find {}`).
    pub fn placeholder(text: &str) -> Self {
        Arg {
            text: text.to_string(),
            dynamic: true,
            flaggy: true,
        }
    }
}

/// What the table says about one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    pub tags: BTreeSet<RiskTag>,
    pub rule: String,
    pub unknown: Option<String>,
}

impl Class {
    pub fn new(rule: impl Into<String>, tags: &[RiskTag]) -> Self {
        Class {
            tags: tags.iter().copied().collect(),
            rule: rule.into(),
            unknown: None,
        }
    }

    pub fn unknown(rule: impl Into<String>, reason: impl Into<String>) -> Self {
        Class {
            tags: [RiskTag::Unknown].into_iter().collect(),
            rule: rule.into(),
            unknown: Some(reason.into()),
        }
    }
}

pub struct Case {
    pub name: &'static str,
    pub sub: &'static [&'static str],
    pub has: &'static [&'static str],
    pub lacks: &'static [&'static str],
    /// Most positional (non-dash) arguments allowed after the subcommand.
    pub max_pos: Option<usize>,
    pub tags: &'static [RiskTag],
}

pub struct Rule {
    pub programs: &'static [&'static str],
    /// Options allowed before the subcommand: (name, takes a value).
    pub globals: &'static [(&'static str, bool)],
    pub cases: &'static [Case],
}

const ANY: Case = Case {
    name: "",
    sub: &[],
    has: &[],
    lacks: &[],
    max_pos: None,
    tags: &[],
};

macro_rules! case {
    ($name:expr, [$($t:ident),*] $(, $k:ident : $v:expr)* $(,)?) => {
        Case { name: $name, tags: &[$(RiskTag::$t),*], $($k: $v,)* ..ANY }
    };
}

macro_rules! rule {
    ([$($p:expr),+], $g:expr, [$($c:expr),* $(,)?]) => {
        Rule { programs: &[$($p),+], globals: $g, cases: &[$($c),*] }
    };
}

// ---------------------------------------------------------------- matching

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

fn eval_rule(rule: &Rule, prog: &str, args: &[Arg]) -> Class {
    if let [only] = args {
        if matches!(only.text.as_str(), "--version" | "--help" | "-h" | "-V") {
            return Class::new(format!("{prog} version"), &[RiskTag::Read]);
        }
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
        if let Some(max) = case.max_pos {
            if args[after.min(args.len())..]
                .iter()
                .filter(|a| !a.text.starts_with('-'))
                .count()
                > max
            {
                continue;
            }
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

// ---------------------------------------------------------------- entry point

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

/// Shell builtins that evaluate parts of their arguments as expressions.
const EVAL_BUILTINS: &[&str] = &[
    "test", "[", "[[", "let", "declare", "typeset", "local", "export", "readonly", "unset",
    "printf", "read", "((",
];

/// Programs and builtins that run what they are given.
const RUNS_CODE: &[&str] = &[
    "eval",
    "source",
    ".",
    "trap",
    "mapfile",
    "readarray",
    "coproc",
    "alias",
    "enable",
    "fc",
    "complete",
    "compgen",
    "at",
    "batch",
    "make",
    "gmake",
    "systemd-run",
    "script",
    "expect",
    "screen",
    "tmux",
    "strace",
    "ltrace",
    "gdb",
    "chroot",
    "unshare",
    "nsenter",
    "setpriv",
    "runuser",
    "ansible",
    "ansible-playbook",
    "vim",
    "vi",
    "nano",
    "emacs",
    "ed",
    "ex",
    "pico",
    "micro",
    "htop",
    "iotop",
];

const INTERPRETERS: &[&str] = &[
    "python",
    "python2",
    "python3",
    "perl",
    "ruby",
    "node",
    "nodejs",
    "php",
    "lua",
    "luajit",
    "tclsh",
    "wish",
    "Rscript",
    "R",
    "julia",
    "deno",
    "bun",
    "pwsh",
    "osascript",
    "groovy",
    "java",
    "jshell",
    "scala",
    "awk-script",
    "irb",
    "pry",
    "ipython",
];

/// Never change anything (output goes to stdout; redirections are tagged separately).
const READ_ONLY: &[&str] = &[
    // files and text
    "cat",
    "tac",
    "nl",
    "more",
    "head",
    "tail",
    "grep",
    "egrep",
    "fgrep",
    "zgrep",
    "zegrep",
    "zfgrep",
    "zcat",
    "bzcat",
    "xzcat",
    "zstdcat",
    "lz4cat",
    "ls",
    "dir",
    "vdir",
    "ll",
    "stat",
    "file",
    "wc",
    "cut",
    "tr",
    "column",
    "paste",
    "join",
    "comm",
    "diff",
    "cmp",
    "md5sum",
    "sha1sum",
    "sha224sum",
    "sha256sum",
    "sha384sum",
    "sha512sum",
    "b2sum",
    "cksum",
    "sum",
    "base64",
    "base32",
    "hexdump",
    "od",
    "strings",
    "nm",
    "readelf",
    "objdump",
    "rev",
    "fold",
    "fmt",
    "expand",
    "unexpand",
    "pr",
    "numfmt",
    "tsort",
    "jq",
    "readlink",
    "realpath",
    "dirname",
    "basename",
    "pwd",
    "namei",
    "du",
    "df",
    "locate",
    "mlocate",
    "plocate",
    "which",
    "whereis",
    "type",
    "whatis",
    "apropos",
    "lsattr",
    "getfacl",
    "getcap",
    "wc",
    "expr",
    "factor",
    "bc",
    "seq",
    "sleep",
    // processes, system, hardware
    "ps",
    "pstree",
    "pgrep",
    "pidof",
    "pmap",
    "free",
    "uptime",
    "w",
    "who",
    "whoami",
    "id",
    "groups",
    "last",
    "lastb",
    "lastlog",
    "users",
    "logname",
    "uname",
    "hostid",
    "nproc",
    "arch",
    "lscpu",
    "lsblk",
    "lsmod",
    "lspci",
    "lsusb",
    "lsns",
    "lshw",
    "lsof",
    "lslocks",
    "lslogins",
    "lsipc",
    "lsmem",
    "vmstat",
    "iostat",
    "mpstat",
    "pidstat",
    "lsb_release",
    "getent",
    "getconf",
    "locale",
    "tty",
    "printenv",
    "blkid",
    "findmnt",
    "lvs",
    "vgs",
    "pvs",
    "lvdisplay",
    "vgdisplay",
    "pvdisplay",
    "lvscan",
    "vgscan",
    "pvscan",
    "ntpstat",
    "modinfo",
    "atq",
    "mailq",
    "pg_isready",
    "netstat",
    // shell builtins that only touch the shell's own state
    "echo",
    "printf",
    "true",
    "false",
    ":",
    "test",
    "[",
    "[[",
    "((",
    "cd",
    "pushd",
    "popd",
    "dirs",
    "export",
    "unset",
    "local",
    "declare",
    "typeset",
    "readonly",
    "shift",
    "return",
    "exit",
    "break",
    "continue",
    "wait",
    "jobs",
    "read",
    "getopts",
    "let",
    "umask",
    "ulimit",
    "hash",
    "history",
    "set",
    "unalias",
    "bg",
    "fg",
    "times",
    "help",
];

// ---------------------------------------------------------------- the table

const NO_GLOBALS: &[(&str, bool)] = &[];

const SYSTEMCTL_GLOBALS: &[(&str, bool)] = &[
    ("--user", false),
    ("--system", false),
    ("--no-pager", false),
    ("--no-legend", false),
    ("--no-ask-password", false),
    ("--no-block", false),
    ("--full", false),
    ("-l", false),
    ("--all", false),
    ("-a", false),
    ("--quiet", false),
    ("-q", false),
    ("--now", false),
    ("--force", false),
    ("-f", false),
    ("--failed", false),
    ("--show-types", false),
    ("-H", true),
    ("--host", true),
    ("-M", true),
    ("--machine", true),
    ("-n", true),
    ("--lines", true),
    ("-o", true),
    ("--output", true),
    ("-t", true),
    ("--type", true),
    ("--state", true),
    ("-p", true),
    ("--property", true),
    ("--root", true),
    ("--signal", true),
    ("-s", true),
    ("--kill-who", true),
];

const DOCKER_GLOBALS: &[(&str, bool)] = &[
    ("-H", true),
    ("--host", true),
    ("--context", true),
    ("-c", true),
    ("--config", true),
    ("-l", true),
    ("--log-level", true),
    ("-D", false),
    ("--debug", false),
    ("--tls", false),
    ("--tlsverify", false),
    ("--tlscacert", true),
    ("--tlscert", true),
    ("--tlskey", true),
];

const KUBECTL_GLOBALS: &[(&str, bool)] = &[
    ("-n", true),
    ("--namespace", true),
    ("--context", true),
    ("--kubeconfig", true),
    ("-s", true),
    ("--server", true),
    ("--user", true),
    ("--cluster", true),
    ("--as", true),
    ("--as-group", true),
    ("--request-timeout", true),
    ("-v", true),
    ("--v", true),
    ("--cache-dir", true),
    ("--insecure-skip-tls-verify", false),
    ("--certificate-authority", true),
    ("--tls-server-name", true),
];

const HELM_GLOBALS: &[(&str, bool)] = &[
    ("-n", true),
    ("--namespace", true),
    ("--kube-context", true),
    ("--kubeconfig", true),
    ("--debug", false),
    ("--burst-limit", true),
];

const GIT_GLOBALS: &[(&str, bool)] = &[
    ("-C", true),
    ("--no-pager", false),
    ("-P", false),
    ("--paginate", false),
    ("-p", false),
    ("--no-optional-locks", false),
    ("--git-dir", true),
    ("--work-tree", true),
    ("--bare", false),
    ("--no-replace-objects", false),
    ("--literal-pathspecs", false),
];

const IP_GLOBALS: &[(&str, bool)] = &[
    ("-s", false),
    ("-d", false),
    ("-4", false),
    ("-6", false),
    ("-r", false),
    ("-o", false),
    ("-j", false),
    ("-p", false),
    ("-br", false),
    ("-c", false),
    ("-a", false),
    ("-t", false),
    ("-n", true),
    ("-netns", true),
];

const APT_GLOBALS: &[(&str, bool)] = &[
    ("-y", false),
    ("--yes", false),
    ("-q", false),
    ("-qq", false),
    ("--quiet", false),
    ("-f", false),
    ("-s", false),
    ("--simulate", false),
    ("--dry-run", false),
    ("--no-install-recommends", false),
    ("--assume-yes", false),
    ("-V", false),
    ("-o", true),
    ("-t", true),
    ("--target-release", true),
];

const SUPERVISOR_GLOBALS: &[(&str, bool)] = &[("-c", true), ("-s", true), ("-u", true)];

const NPM_GLOBALS: &[(&str, bool)] = &[
    ("-g", false),
    ("--global", false),
    ("--json", false),
    ("-C", true),
    ("--prefix", true),
    ("--depth", true),
    ("--long", false),
    ("--silent", false),
];

pub const RULES: &[Rule] = &[
    rule!(
        ["systemctl"],
        SYSTEMCTL_GLOBALS,
        [
            case!("poweroff", [Destructive], sub: &["poweroff|reboot|halt|kexec|suspend|hibernate|hybrid-sleep|suspend-then-hibernate|soft-reboot|rescue|emergency|exit|default|kill|clean|isolate"]),
            case!("read", [Read], sub: &[
                "status|show|cat|help|get-default|is-*|list-*|show-environment|show-types",
                "$",
            ]),
            case!("service change", [ServiceChange], sub: &[
                "start|stop|restart|reload|try-restart|reload-or-restart|try-reload-or-restart|condrestart|force-reload|enable|disable|mask|unmask|reenable|daemon-reload|daemon-reexec|reset-failed|freeze|thaw|preset|preset-all|set-default|import-environment|set-environment|unset-environment|link|revert|add-wants|add-requires|edit|set-property|switch-root|bind|mount-image",
            ]),
        ]
    ),
    rule!(
        ["journalctl"],
        NO_GLOBALS,
        [
            case!("maintenance", [Write], has: &["--vacuum-size", "--vacuum-time", "--vacuum-files", "--rotate", "--flush", "--sync", "--relinquish-var", "--smart-relinquish-var", "--setup-keys", "--update-catalog", "--cursor-file"]),
            case!("read", [Read]),
        ]
    ),
    rule!(
        ["loginctl"],
        &[
            ("--no-pager", false),
            ("--no-legend", false),
            ("-a", false),
            ("--all", false),
            ("-l", false),
            ("--full", false),
            ("-H", true),
            ("--host", true),
            ("-o", true),
            ("--output", true),
            ("-n", true),
            ("--lines", true),
            ("-p", true),
            ("--property", true)
        ],
        [
            case!("read", [Read], sub: &["list-*|show-*|status|session-status|user-status|seat-status|show-session|show-user|show-seat;$"]),
        ]
    ),
    rule!(
        ["timedatectl", "hostnamectl", "localectl"],
        &[
            ("--no-pager", false),
            ("-H", true),
            ("--host", true),
            ("-p", true),
            ("--property", true),
            ("--value", false),
            ("-a", false),
            ("--all", false)
        ],
        [case!("read", [Read], sub: &["status|show|list-*;$"]),]
    ),
    rule!(
        ["resolvectl", "systemd-resolve"],
        &[
            ("--no-pager", false),
            ("-4", false),
            ("-6", false),
            ("-i", true),
            ("--interface", true),
            ("-t", true),
            ("--type", true),
            ("-c", true),
            ("--class", true)
        ],
        [
            case!("read", [Read, Network], sub: &["query|status|statistics|show-cache|show-server-state|service|openpgp|tlsa;$"], lacks: &["--flush-caches", "--reset-statistics", "--reset-server-features"]),
        ]
    ),
    rule!(
        ["networkctl"],
        &[
            ("--no-pager", false),
            ("--no-legend", false),
            ("-a", false),
            ("--all", false),
            ("-s", false),
            ("--stats", false)
        ],
        [case!("read", [Read], sub: &["list|status|lldp;$"])]
    ),
    rule!(
        ["coredumpctl"],
        &[("--no-pager", false), ("--no-legend", false), ("-1", false)],
        [
            case!("read", [Read], sub: &["list|info"], lacks: &["-o", "--output", "-D", "--directory"])
        ]
    ),
    rule!(
        ["busctl"],
        &[
            ("--no-pager", false),
            ("--no-legend", false),
            ("--user", false),
            ("--system", false),
            ("--list", false),
            ("-l", false),
            ("--acquired", false),
            ("--activatable", false),
            ("--unique", false)
        ],
        [case!("read", [Read], sub: &["list|status|tree|introspect|monitor|capture"])]
    ),
    rule!(
        ["systemd-analyze"],
        &[
            ("--no-pager", false),
            ("--user", false),
            ("--system", false),
            ("--man", false),
            ("-H", true),
            ("--host", true)
        ],
        [
            case!("read", [Read], sub: &["time|blame|critical-chain|plot|dot|dump|verify|calendar|timespan|timestamp|security|cat-config|unit-paths|exit-status|syscall-filter|condition;$"])
        ]
    ),
    rule!(
        ["supervisorctl"],
        SUPERVISOR_GLOBALS,
        [
            case!("read", [Read], sub: &["status|tail|avail|version|pid|maintail|help"]),
            case!("service change", [ServiceChange], sub: &["start|stop|restart|reload|update|add|remove|signal|reread|shutdown|clear|fg"]),
        ]
    ),
    rule!(
        ["pm2"],
        NO_GLOBALS,
        [
            case!("read", [Read], sub: &["list|ls|status|jlist|prettylist|logs|describe|show|info|monit|env|ping|report|--version"], lacks: &["--flush"]),
            case!("destructive", [Destructive], sub: &["delete|del|kill|flush|unstartup|uninstall"]),
            case!("service change", [ServiceChange], sub: &["start|stop|restart|reload|gracefulReload|scale|reset|save|startup|resurrect|update"]),
        ]
    ),
    rule!(
        ["crontab"],
        NO_GLOBALS,
        [
            case!("remove", [Destructive], has: &["-r", "--remove"]),
            case!("list", [Read], has: &["-l", "--list"], lacks: &["-e", "-i"]),
            case!("edit", [Write]),
        ]
    ),
    rule!(
        ["docker", "podman", "nerdctl"],
        DOCKER_GLOBALS,
        [
            case!("destructive", [Destructive], sub: &[
                "rm|rmi|kill|prune",
                "container rm|kill|prune",
                "image rm|prune",
                "volume rm|prune",
                "network rm|prune",
                "system prune",
                "builder prune",
                "buildx prune",
                "compose down|rm|kill",
                "stack rm",
                "service rm",
                "swarm leave",
                "node rm",
                "context rm",
                "secret rm",
                "config rm",
                "plugin rm",
            ]),
            case!("read", [Read], sub: &[
                "ps|images|logs|inspect|info|version|stats|top|port|diff|history|events|search|ls",
                "container ls|ps|list|logs|inspect|stats|top|port|diff",
                "image ls|list|inspect|history",
                "network ls|list|inspect",
                "volume ls|list|inspect",
                "system df|info|events",
                "compose ps|logs|ls|config|images|top|port|version|events",
                "context ls|show|inspect",
                "node ls|inspect|ps",
                "service ls|ps|inspect|logs",
                "stack ls|ps|services",
                "secret ls|inspect",
                "config ls|inspect",
                "plugin ls",
                "buildx ls|inspect|du",
            ]),
            case!("service change", [ServiceChange], sub: &[
                "start|stop|restart|pause|unpause",
                "container start|stop|restart|pause|unpause",
                "compose start|stop|restart|pause|unpause|up|create",
                "service scale|update|rollback",
                "stack deploy",
                "node update",
            ]),
            case!("pull", [Write, Network], sub: &["pull", "image pull", "build", "image build", "compose pull", "compose build", "buildx build", "login", "logout"]),
            case!("create", [Write], sub: &["volume create", "network create", "create", "container create", "update", "rename", "tag", "image tag"]),
            case!("push", [Network, ExfilRisk], sub: &["push", "image push", "compose push", "manifest push"]),
            case!("copy", [Write], sub: &["cp", "container cp", "commit", "save", "load", "import", "export"]),
        ]
    ),
    rule!(
        ["kubectl", "oc"],
        KUBECTL_GLOBALS,
        [
            case!("delete", [Destructive], sub: &["delete", "drain"]),
            case!("read", [Read], sub: &[
                "get|describe|logs|top|api-resources|api-versions|version|cluster-info|explain|diff|events|wait",
                "auth can-i|whoami",
                "rollout status|history",
                "config view|current-context|get-contexts|get-clusters|get-users",
            ]),
            case!("rollout", [ServiceChange], sub: &["rollout restart|undo|pause|resume", "scale", "cordon", "uncordon", "taint", "autoscale"]),
            case!("change", [Write], sub: &["apply", "patch", "edit", "replace", "annotate", "label", "set", "create", "expose", "config use-context", "config set-context", "config set", "config unset", "config delete-context", "certificate approve", "certificate deny"]),
        ]
    ),
    rule!(
        ["helm"],
        HELM_GLOBALS,
        [
            case!("uninstall", [Destructive], sub: &["uninstall|delete", "repo remove"]),
            case!("read", [Read], sub: &["list|ls|status|get|history|show|search|version|env|template|lint|verify;repo list;dependency list;dep list;plugin list"]),
            case!("change", [ServiceChange], sub: &["install|upgrade|rollback|test"]),
            case!("fetch", [Write, Network], sub: &["repo add|update|index;pull;package;dependency update|build;dep update|build"]),
            case!("push", [Network, ExfilRisk], sub: &["push", "registry login"]),
        ]
    ),
    rule!(
        ["git"],
        GIT_GLOBALS,
        [
            case!("clean", [Destructive], sub: &["clean|gc|prune|filter-branch|filter-repo|rm|update-ref|replace"]),
            case!("reset hard", [Destructive], sub: &["reset"], has: &["--hard", "--merge", "--keep"]),
            case!("branch delete", [Destructive], sub: &["branch|tag"], has: &["-d", "-D", "--delete"]),
            case!("branch change", [Write], sub: &["branch|tag"], has: &["-m", "-M", "-c", "-C", "-u", "-f", "--force", "--set-upstream-to", "--unset-upstream", "--edit-description", "--move", "--copy"]),
            case!("push force", [Destructive, Network, ExfilRisk], sub: &["push"], has: &["--force", "-f", "--force-with-lease", "--delete", "-d", "--mirror", "--prune", ":*"]),
            case!("push", [Network, ExfilRisk], sub: &["push|send-email"]),
            case!("network", [Write, Network], sub: &["clone|fetch|pull|submodule|lfs"]),
            case!("remote read", [Network], sub: &["ls-remote"]),
            case!("config read", [Read], sub: &["config"], has: &["--get", "--get-all", "--get-regexp", "--get-urlmatch", "--list", "-l", "--show-origin", "--show-scope"], lacks: &["--add", "--unset", "--unset-all", "--replace-all", "--edit", "-e", "--rename-section", "--remove-section", "--file", "-f", "--global", "--system"]),
            case!("read", [Read], sub: &[
                "status|log|diff|show|ls-files|ls-tree|rev-parse|rev-list|describe|blame|annotate|grep|shortlog|whatchanged|cat-file|name-rev|count-objects|diff-tree|diff-index|diff-files|show-ref|show-branch|merge-base|for-each-ref|check-ignore|var|verify-commit|verify-tag|version|help|fsck|cherry|range-diff|show-index|get-tar-commit-id",
                "remote show|get-url",
                "remote $",
                "stash list|show",
                "worktree list",
                "submodule status",
                "branch|tag $",
                "reflog $|show|exists",
            ], lacks: &["--output", "--ext-diff", "--textconv", "--no-index", "--lost-found", "--exec", "--upload-pack", "--open-files-in-pager", "-O"]),
            case!("branch list", [Read], sub: &["branch|tag"], has: &["-l", "--list", "-a", "-r", "-v", "-vv", "--contains", "--merged", "--no-merged", "--show-current", "--points-at"]),
            case!("write", [Write], sub: &["add|commit|checkout|switch|restore|merge|rebase|cherry-pick|revert|apply|am|mv|stash|init|config|tag|branch|reset|notes|worktree|bisect|sparse-checkout|maintenance|pack-refs|repack|remote|rerere|submodule|commit-tree|update-index|write-tree|hash-object|mktag|mktree|symbolic-ref|pull"], lacks: &["--exec", "-x"]),
        ]
    ),
    rule!(
        ["ip"],
        IP_GLOBALS,
        [
            case!("netns exec", [Unknown], sub: &["netns exec|e|attach"]),
            case!("addr show", [Read], sub: &[
                "addr|address|a|ad|addres|addresses show|list|lst|ls|l",
                "addr|address|a|ad|addres|addresses $",
                "route|r|ro|rou|rout show|list|lst|ls|get|l",
                "route|r|ro|rou|rout $",
                "link|l|li|lin show|list|lst|ls|xstats|l",
                "link|l|li|lin $",
                "neigh|neighbour|neighbor|n|ne show|list|lst|ls|get|l",
                "neigh|neighbour|neighbor|n|ne $",
                "rule|ru show|list|lst|ls",
                "rule|ru $",
                "netns list|ls|identify|pids|list-id",
                "netns $",
                "tunnel show|list|lst|ls",
                "maddr show|list|ls",
                "mroute show|list|ls",
                "tcp_metrics show|list|ls",
                "vrf show|list|ls|identify|pids",
                "monitor",
                "help",
            ]),
            case!("network change", [ServiceChange], sub: &["addr|address|a|route|r|link|l|neigh|neighbour|neighbor|n|rule|ru|netns|tunnel|maddr|mroute|vrf|tuntap|xfrm|l2tp|fou|macsec|token|tcp_metrics|ntable|nexthop|mptcp|sr|ioam *"]),
        ]
    ),
    rule!(
        ["ss"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-K", "--kill"]),
            case!("kill sockets", [Destructive])
        ]
    ),
    rule!(
        ["dmesg"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-c", "-C", "--clear", "--read-clear", "-n", "--console-level", "-D", "--console-off", "-E", "--console-on"])
        ]
    ),
    rule!(
        ["hostname"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-F", "--file", "-b", "--boot"], max_pos: Some(0)),
            case!("set", [ServiceChange])
        ]
    ),
    rule!(
        ["sort"],
        NO_GLOBALS,
        [
            case!("runs a program", [Unknown], has: &["--compress-program"]),
            case!("read", [Read], lacks: &["-o", "--output"]),
            case!("output", [Write])
        ]
    ),
    rule!(
        ["uniq", "xxd"],
        NO_GLOBALS,
        [case!("read", [Read], max_pos: Some(1))]
    ),
    rule!(
        ["shuf"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-o", "--output", "--random-source"]),
            case!("output", [Write])
        ]
    ),
    rule!(
        ["sar", "sadf"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-o"]),
            case!("output", [Write])
        ]
    ),
    rule!(
        ["yq"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-i", "--inplace"]),
            case!("in place", [Write])
        ]
    ),
    rule!(
        ["fuser"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["-k", "--kill"])]
    ),
    rule!(
        ["arp"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["-s", "--set", "-d", "--delete", "-f", "--file"])]
    ),
    rule!(
        ["route"],
        NO_GLOBALS,
        [
            case!("read", [Read], max_pos: Some(0)),
            case!("change", [ServiceChange])
        ]
    ),
    rule!(
        ["ethtool"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-s", "-K", "-G", "-A", "-C", "-L", "-E", "-f", "-p", "-t", "-W", "-N", "-U", "-F", "--change", "--offload", "--set-ring", "--pause", "--coalesce", "--set-channels", "--set-eeprom", "--flash", "--identify", "--test", "--set-dump", "--config-ntuple", "--config-nfc", "--set-priv-flags", "--set-eee", "--set-fec", "--per-queue"]),
            case!("change", [ServiceChange])
        ]
    ),
    rule!(
        ["smartctl"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-s", "--smart", "-t", "--test", "-X", "--abort", "-S", "--saveauto", "-o", "--offlineauto", "--set", "-C", "--captive"]),
            case!("change", [Write])
        ]
    ),
    rule!(
        ["dmidecode"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["--dump-file", "--from-dump"])]
    ),
    rule!(
        ["nvidia-smi"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-pm", "-r", "--gpu-reset", "-pl", "--power-limit", "-ac", "-rac", "-lgc", "-rgc", "-mig", "-c", "-e", "-acp", "-am", "-fdm", "-lmc", "-rmc"]),
            case!("change", [ServiceChange])
        ]
    ),
    rule!(
        ["mdadm"],
        NO_GLOBALS,
        [
            case!("read", [Read], has: &["--detail", "-D", "--examine", "-E", "--query", "-Q", "--detail-platform", "--version", "-V"], lacks: &["--stop", "-S", "--create", "-C", "--assemble", "-A", "--manage", "--add", "-a", "--remove", "-r", "--fail", "-f", "--grow", "-G", "--zero-superblock"]),
            case!("change", [Destructive])
        ]
    ),
    rule!(
        ["fdisk", "sfdisk", "gdisk", "sgdisk", "parted", "cfdisk"],
        NO_GLOBALS,
        [
            case!("list", [Read], has: &["-l", "--list", "-s", "--show-size", "-d", "--dump", "-p", "--print", "-P", "--list-free", "--json", "-J"], lacks: &["--delete", "-n", "-N", "--new", "--delete-all", "-o", "-Z", "--zap-all", "--backup", "--restore", "-O", "--load-backup"]),
            case!("partition", [Destructive]),
        ]
    ),
    rule!(
        ["mount"],
        NO_GLOBALS,
        [
            case!("read", [Read], max_pos: Some(0), lacks: &["-a", "--all"]),
            case!("mount", [ServiceChange])
        ]
    ),
    rule!(
        ["swapon"],
        NO_GLOBALS,
        [
            case!("read", [Read], has: &["--show", "-s", "--summary"]),
            case!("swap", [ServiceChange])
        ]
    ),
    rule!(
        ["umount", "swapoff", "modprobe", "insmod", "rmmod", "depmod"],
        NO_GLOBALS,
        [case!("system change", [ServiceChange])]
    ),
    rule!(
        ["sysctl"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-w", "--write", "-p", "--load", "--system", "-f", "*=*"]),
            case!("write", [ServiceChange])
        ]
    ),
    rule!(
        ["ldconfig"],
        NO_GLOBALS,
        [
            case!("read", [Read], has: &["-p", "--print-cache"]),
            case!("rebuild", [Write])
        ]
    ),
    rule!(
        ["update-alternatives"],
        NO_GLOBALS,
        [
            case!("read", [Read], has: &["--display", "--list", "--get-selections", "--query"]),
            case!("change", [Write])
        ]
    ),
    rule!(
        ["logrotate"],
        NO_GLOBALS,
        [
            case!("debug", [Read], has: &["-d", "--debug"]),
            case!("rotate", [Write, ServiceChange])
        ]
    ),
    rule!(
        ["nginx"],
        NO_GLOBALS,
        [
            case!("test", [Read], has: &["-t", "-T", "-v", "-V", "-h", "-?"], lacks: &["-s"]),
            case!("signal", [ServiceChange]),
        ]
    ),
    rule!(
        ["apachectl", "apache2ctl", "httpd", "apache2"],
        NO_GLOBALS,
        [
            case!("test", [Read], has: &["configtest", "status", "fullstatus", "-t", "-S", "-M", "-v", "-V", "-l", "-L", "-h"], lacks: &["-k", "start", "stop", "restart", "graceful", "graceful-stop"]),
            case!("signal", [ServiceChange]),
        ]
    ),
    rule!(
        ["haproxy"],
        NO_GLOBALS,
        [
            case!("check", [Read], has: &["-c", "-v", "-vv"], lacks: &["-D", "-sf", "-st", "-x", "-W", "-db"]),
            case!("start", [ServiceChange])
        ]
    ),
    rule!(
        ["sshd"],
        NO_GLOBALS,
        [
            case!("check", [Read], has: &["-t", "-T"], lacks: &["-D", "-d"]),
            case!("start", [ServiceChange])
        ]
    ),
    rule!(
        ["visudo"],
        NO_GLOBALS,
        [
            case!("check", [Read], has: &["-c", "--check"], lacks: &["-s"]),
            case!("edit", [Write, Privilege])
        ]
    ),
    rule!(
        ["ufw"],
        &[("--dry-run", false)],
        [
            case!("destructive", [Destructive, ServiceChange], sub: &["reset|delete|--force"]),
            case!("read", [Read], sub: &["status|show|version;app list|info"]),
            case!("change", [ServiceChange]),
        ]
    ),
    rule!(
        ["firewall-cmd"],
        NO_GLOBALS,
        [
            case!("read", [Read], has: &["--list-*", "--get-*", "--query-*", "--state", "--info-*", "--version", "--check-config", "--help"], lacks: &["--add*", "--remove*", "--reload", "--set*", "--panic*", "--runtime-to-permanent", "--complete-reload", "--new*", "--delete*", "--load-zone-defaults", "--lockdown-*", "--change*", "--direct", "--policy"]),
            case!("change", [ServiceChange]),
        ]
    ),
    rule!(
        [
            "iptables",
            "ip6tables",
            "iptables-legacy",
            "ip6tables-legacy",
            "iptables-nft",
            "ip6tables-nft",
            "ebtables",
            "arptables"
        ],
        NO_GLOBALS,
        [
            case!("flush", [Destructive, ServiceChange], has: &["-F", "--flush", "-X", "--delete-chain", "-P", "--policy"]),
            case!("list", [Read], has: &["-L", "--list", "-S", "--list-rules", "-C", "--check"], lacks: &["-A", "--append", "-I", "--insert", "-D", "--delete", "-R", "--replace", "-N", "--new-chain", "-E", "--rename-chain", "-Z", "--zero"]),
            case!("change", [ServiceChange]),
        ]
    ),
    rule!(
        ["nft"],
        &[
            ("-n", false),
            ("-a", false),
            ("-s", false),
            ("-j", false),
            ("--json", false),
            ("-t", false),
            ("-nn", false)
        ],
        [
            case!("flush", [Destructive, ServiceChange], sub: &["flush|destroy|delete"]),
            case!("list", [Read], sub: &["list|monitor|describe|--version"]),
            case!("change", [ServiceChange], sub: &["add|create|insert|replace|rename|reset|import|-f|--file|-i|--interactive"]),
        ]
    ),
    rule!(
        ["chronyc"],
        &[
            ("-n", false),
            ("-4", false),
            ("-6", false),
            ("-h", true),
            ("-p", true),
            ("-a", false)
        ],
        [
            case!("read", [Read], sub: &["tracking|sources|sourcestats|activity|ntpdata|serverstats|rtcdata|selectdata|clients|smoothing|authdata|waitsync|help|manual"]),
            case!("change", [ServiceChange]),
        ]
    ),
    rule!(
        ["ntpq"],
        NO_GLOBALS,
        [
            case!("read", [Read], has: &["-p", "--peers", "-c", "--command", "-n"], lacks: &["-i", "--interactive"])
        ]
    ),
    rule!(
        ["apt", "apt-get", "aptitude"],
        APT_GLOBALS,
        [
            case!("remove", [Destructive, Write], sub: &["remove|purge|autoremove|autopurge|erase"]),
            case!("read", [Read], sub: &["list|show|search|policy|depends|rdepends|showpkg|madison|pkgnames|stats|check|why|why-not|--version|-v|help"]),
            case!("install", [Write, Network], sub: &["install|update|upgrade|dist-upgrade|full-upgrade|reinstall|source|build-dep|download|safe-upgrade|forget-new|changelog|satisfy"]),
            case!("clean", [Write], sub: &["clean|autoclean|edit-sources|auto-clean"]),
        ]
    ),
    rule!(
        ["apt-cache"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["gencaches", "add", "-p", "-s"])]
    ),
    rule!(
        ["dpkg"],
        NO_GLOBALS,
        [
            case!("remove", [Destructive, Write], has: &["-r", "-P", "--remove", "--purge"]),
            case!("install", [Write], has: &["-i", "--install", "--unpack", "--configure", "--triggers-only", "--set-selections", "--clear-selections", "--add-architecture", "--remove-architecture", "--force-*", "--clear-avail", "--merge-avail", "--update-avail", "--forget-old-unavail"]),
            case!("read", [Read], has: &["-l", "-L", "-s", "-S", "-p", "-V", "--list", "--listfiles", "--status", "--search", "--print-avail", "--get-selections", "--print-architecture", "--print-foreign-architectures", "--version", "--verify", "--audit", "-C", "-I", "--info", "--contents", "-c"]),
        ]
    ),
    rule!(["dpkg-query"], NO_GLOBALS, [case!("read", [Read])]),
    rule!(
        ["rpm"],
        NO_GLOBALS,
        [
            case!("query", [Read], has: &["-q", "--query", "-V", "--verify", "--version", "--showrc", "--eval"], lacks: &["--erase", "--install", "--upgrade", "--freshen", "--import", "--rebuilddb", "--initdb"]),
            case!("erase", [Destructive, Write], has: &["-e", "--erase"]),
            case!("install", [Write], has: &["-i", "-U", "-F", "--install", "--upgrade", "--freshen", "--import", "--rebuilddb", "--initdb"]),
        ]
    ),
    rule!(
        ["yum", "dnf", "microdnf", "dnf5"],
        &[
            ("-y", false),
            ("--assumeyes", false),
            ("-q", false),
            ("--quiet", false),
            ("-C", false),
            ("--cacheonly", false),
            ("--refresh", false),
            ("--nogpgcheck", false),
            ("--enablerepo", true),
            ("--disablerepo", true),
            ("--setopt", true),
            ("-x", true),
            ("--exclude", true),
            ("--releasever", true),
            ("--assumeno", false),
            ("--best", false),
            ("--allowerasing", false),
            ("--skip-broken", false),
            ("-e", true),
            ("--errorlevel", true)
        ],
        [
            case!("remove", [Destructive, Write], sub: &["remove|erase|autoremove|remove-*|rm"]),
            case!("read", [Read], sub: &["list|info|search|provides|whatprovides|repolist|repoinfo|deplist|check|repoquery|version|help|updateinfo|alias|needs-restarting|leaves;history list|info|summary|userinstalled;module list|info;group list|info|summary;repo list|info"]),
            case!("check-update", [Read, Network], sub: &["check-update|check-upgrade"]),
            case!("install", [Write, Network], sub: &["install|update|upgrade|reinstall|downgrade|distro-sync|distribution-synchronization|makecache|groupinstall|swap|mark|clean|upgrade-minimal|update-minimal|offline|system-upgrade|config-manager|copr|reposync|download|builddep;group install;module install|enable|disable;history undo|redo|rollback"]),
        ]
    ),
    rule!(
        ["zypper"],
        &[
            ("-n", false),
            ("--non-interactive", false),
            ("-q", false),
            ("--quiet", false),
            ("--no-refresh", false),
            ("-x", false),
            ("--xmlout", false)
        ],
        [
            case!("remove", [Destructive, Write], sub: &["remove|rm"]),
            case!("read", [Read], sub: &["search|se|info|if|list-updates|lu|repos|lr|packages|pa|patches|lp|patch-check|pchk|products|pd|versioncmp|vcmp|needs-rebooting|ps|verify|ve"]),
            case!("install", [Write, Network], sub: &["install|in|update|up|dist-upgrade|dup|refresh|ref|addrepo|ar|removerepo-no|patch|source-install|si"]),
        ]
    ),
    rule!(
        ["apk"],
        &[
            ("-q", false),
            ("--quiet", false),
            ("-v", false),
            ("--verbose", false),
            ("--no-cache", false),
            ("--update", false),
            ("-U", false),
            ("-p", true),
            ("--root", true),
            ("--repository", true),
            ("-X", true)
        ],
        [
            case!("remove", [Destructive, Write], sub: &["del"]),
            case!("read", [Read], sub: &["info|list|search|policy|version|stats|dot|audit"]),
            case!("install", [Write, Network], sub: &["add|update|upgrade|fix|cache|fetch|verify-no"]),
        ]
    ),
    rule!(
        ["pacman"],
        NO_GLOBALS,
        [
            case!("remove", [Destructive, Write], has: &["-R", "--remove"]),
            case!("query", [Read], has: &["-Q", "--query", "-V", "--version", "-h", "--help"], lacks: &["-S", "-U", "-R", "--sync", "--upgrade"]),
            case!("sync", [Write, Network], has: &["-S", "-U", "--sync", "--upgrade", "-D", "--database", "-F", "--files"]),
        ]
    ),
    rule!(
        ["snap"],
        NO_GLOBALS,
        [
            case!("remove", [Destructive, Write], sub: &["remove|abort|disconnect|unalias|forget"]),
            case!("read", [Read], sub: &["list|info|find|version|services|changes|change|connections|interfaces|logs|get|model|warnings|known|help|--version"]),
            case!("install", [Write, Network], sub: &["install|refresh|download|set|connect|start|stop|restart|enable|disable|revert|switch|alias|save|restore|run-no"]),
        ]
    ),
    rule!(
        ["brew"],
        NO_GLOBALS,
        [
            case!("uninstall", [Destructive, Write], sub: &["uninstall|remove|rm|cleanup|untap|unlink"]),
            case!("read", [Read], sub: &["list|ls|info|search|outdated|deps|uses|config|doctor|leaves|log|desc|missing|commands|help;services list"]),
            case!("install", [Write, Network], sub: &["install|upgrade|update|reinstall|tap|link|pin|unpin|fetch;services start|stop|restart"]),
        ]
    ),
    rule!(
        ["pip", "pip3", "pipx"],
        &[
            ("-q", false),
            ("--quiet", false),
            ("-v", false),
            ("--verbose", false),
            ("--disable-pip-version-check", false),
            ("--no-input", false),
            ("--isolated", false),
            ("--python", true)
        ],
        [
            case!("uninstall", [Destructive, Write], sub: &["uninstall"]),
            case!("read", [Read], sub: &["list|show|freeze|check|debug|inspect|help|hash;config list|get;cache dir|info|list;index versions"]),
            case!("install", [Write, Network], sub: &["install|download|wheel|upgrade|reinstall|inject;cache purge|remove|clear;config set|unset|edit"]),
        ]
    ),
    rule!(
        ["npm", "pnpm", "yarn"],
        NPM_GLOBALS,
        [
            case!("uninstall", [Destructive, Write], sub: &["uninstall|remove|rm|un|unlink|prune;cache clean|rm"]),
            case!("read", [Read], sub: &["ls|list|ll|la|outdated|view|info|show|v|audit|prefix|root|bin|why|explain|doctor|ping|search|help|whoami|fund;config get|list;owner ls;team ls;access ls;dist-tag ls"], lacks: &["fix", "--fix"]),
            case!("install", [Write, Network], sub: &["install|i|add|ci|update|up|upgrade|link|publish|dedupe|rebuild|init|set|login|logout|adduser|deprecate|pack|version;config set|delete;cache add;dist-tag add|rm;owner add|rm"]),
        ]
    ),
    rule!(
        ["gem"],
        NO_GLOBALS,
        [
            case!("uninstall", [Destructive, Write], sub: &["uninstall|cleanup"]),
            case!("read", [Read], sub: &["list|info|search|environment|contents|query|dependency|which|help|--version|outdated|specification|stale|sources"]),
            case!("install", [Write, Network], sub: &["install|update|fetch|push|pristine|sources"]),
        ]
    ),
    rule!(
        ["cargo"],
        NO_GLOBALS,
        [
            case!("uninstall", [Destructive, Write], sub: &["uninstall"]),
            case!("install", [Write, Network], sub: &["install|publish|login|add|update|fetch"]),
            case!("read", [Read], sub: &["version|tree|metadata|search|pkgid|locate-project|verify-project|help"]),
        ]
    ),
    rule!(
        ["openssl"],
        NO_GLOBALS,
        [
            case!("s_client", [Network], sub: &["s_client|s_time|ocsp|ciphers-no"]),
            case!("read", [Read], sub: &["x509|crl|verify|version|asn1parse|dgst|rand|ciphers|list|errstr|prime|speed|storeutl|pkey|rsa|ec|pkcs12|pkcs7|req|sha256|sha1|md5"], lacks: &["-out", "-keyout", "-sign", "-signkey", "-CAcreateserial", "-writerand", "-new", "-newkey", "-genkey", "-CA", "-CAkey", "-set_serial", "-x509", "-gen"]),
            case!("write", [Write], sub: &["genrsa|genpkey|gendsa|ecparam|req|ca|enc|rsautl|pkeyutl|smime|cms|passwd|dhparam|x509|crl|pkcs12|rsa|ec|pkey|dgst|rand|spkac|ts|srp"]),
        ]
    ),
    rule!(
        ["ssh-keygen"],
        NO_GLOBALS,
        [
            case!("read", [Read], has: &["-l", "-F", "-B"], lacks: &["-R", "-r", "-t", "-N", "-p", "-y", "-e", "-i", "-s", "-f?"]),
            case!("generate", [Write]),
        ]
    ),
    rule!(
        ["tcpdump", "tshark", "dumpcap"],
        NO_GLOBALS,
        [
            case!("capture write", [Network, Write], has: &["-w", "--write", "-W", "-C", "-G", "-z"]),
            case!("capture", [Network]),
        ]
    ),
    rule!(
        [
            "useradd",
            "groupadd",
            "usermod",
            "groupmod",
            "chage",
            "chsh",
            "chfn",
            "gpasswd",
            "newusers",
            "adduser",
            "addgroup",
            "passwd",
            "chpasswd",
            "setcap",
            "vipw",
            "vigr",
            "pwconv",
            "grpconv",
            "chattr",
            "setfacl",
            "setsebool",
            "semanage",
            "chcon",
            "restorecon",
            "setenforce"
        ],
        NO_GLOBALS,
        [case!("account change", [Write, Privilege])]
    ),
    rule!(
        ["userdel", "groupdel", "deluser", "delgroup"],
        NO_GLOBALS,
        [case!("account removal", [Destructive, Privilege])]
    ),
    rule!(
        [
            "gzip", "bzip2", "xz", "zstd", "lz4", "gunzip", "bunzip2", "unxz", "unzstd", "unlz4",
            "pigz", "pbzip2", "pxz"
        ],
        NO_GLOBALS,
        [
            case!("to stdout", [Read], has: &["-c", "-l", "-t", "--stdout", "--to-stdout", "--list", "--test"]),
            case!("in place", [Write]),
        ]
    ),
    rule!(
        ["unzip", "zipinfo"],
        NO_GLOBALS,
        [
            case!("list", [Read], has: &["-l", "-t", "-p", "-Z", "-v", "-z"], lacks: &["-d", "-o"]),
            case!("extract", [Write]),
        ]
    ),
    rule!(
        ["7z", "7za", "7zr"],
        NO_GLOBALS,
        [
            case!("list", [Read], sub: &["l|t|i"], lacks: &["-o*"]),
            case!("change", [Write]),
        ]
    ),
    rule!(
        ["zip", "unzipsfx", "zipcloak", "zipnote", "zipsplit"],
        NO_GLOBALS,
        [case!("write", [Write])]
    ),
    rule!(
        ["mysqladmin"],
        &[
            ("-u", true),
            ("-p", false),
            ("-h", true),
            ("-P", true),
            ("--user", true),
            ("--host", true),
            ("--port", true),
            ("-S", true),
            ("--socket", true)
        ],
        [
            case!("read", [Read], sub: &["status|ping|version|variables|processlist|extended-status|proc|stat"])
        ]
    ),
    // Programs that look read-only but can run a command or write a file through an option.
    rule!(
        ["less"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["+*", "-k", "--lesskey-file", "-o", "--log-file", "-O", "--LOG-FILE"])
        ]
    ),
    rule!(
        ["man"],
        NO_GLOBALS,
        [
            case!("read", [Read], lacks: &["-P", "--pager", "-H", "--html", "-B", "--browser", "-C", "--config-file", "-T", "-t", "-Z", "-z", "-K", "--global-apropos"])
        ]
    ),
    rule!(
        ["rg"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["--pre", "--hostname-bin"])]
    ),
    rule!(
        ["ag", "ack"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["--pager", "--output-file"])]
    ),
    rule!(
        ["tree"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["-o"])]
    ),
    rule!(
        ["xmllint"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["--output", "-o", "--save"])]
    ),
    rule!(
        ["sensors"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["-s", "--set"])]
    ),
    rule!(
        ["iptables-save", "ip6tables-save"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["-f", "--file"])]
    ),
    rule!(
        ["file"],
        NO_GLOBALS,
        [case!("read", [Read], lacks: &["-C", "--compile"])]
    ),
    rule!(
        ["docker-compose"],
        &[
            ("-f", true),
            ("--file", true),
            ("-p", true),
            ("--project-name", true),
            ("--project-directory", true),
            ("--profile", true),
            ("--env-file", true),
            ("-H", true),
            ("--host", true),
            ("--context", true),
            ("--ansi", true),
            ("--verbose", false),
            ("--no-ansi", false),
            ("--compatibility", false)
        ],
        [
            case!("destructive", [Destructive], sub: &["down|rm|kill"]),
            case!("read", [Read], sub: &["ps|logs|config|images|top|port|version|events|ls"]),
            case!("service change", [ServiceChange], sub: &["start|stop|restart|pause|unpause|up|create|scale"]),
            case!("pull", [Write, Network], sub: &["pull|build"]),
            case!("push", [Network, ExfilRisk], sub: &["push"]),
        ]
    ),
];
