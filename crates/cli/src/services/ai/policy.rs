//! Policy engine: scope, deny > ask > allow, tiers, grants, budgets.
//!
//!
//! Order of evaluation in [`DefaultPolicyEngine::decide`] (first match wins, so deny always
//! beats ask and ask beats allow):
//!
//! 1. host out of scope, or the call has no host when it needs one: deny (counts as probing)
//! 2. budgets (`calls_per_min`, `changes_per_run`, `run_ttl`): deny
//! 3. profile `observe` and a write tool: deny
//! 4. `deny_paths`: the tool's path, then every path the command names as written — arguments
//!    (absolute, relative, globbed, `~`/`$HOME`, `--opt=` and attached-option values) and
//!    redirection targets read from or written to, with relative arguments resolved against a
//!    literal `cd`: a certain match denies
//! 5. `deny` patterns on the analyzed commands: deny
//! 6. a path `deny_paths` could not settle (a glob, or a path only known at run time): ask
//! 7. `write_paths` set and `edit_file` outside it: deny
//! 8. an active grant matches: allow
//! 9. read tools: allow (`read_file` only under `read_paths`, else ask);
//!    `run_readonly` / `run_many`: allow when the host has a jail, or the analysis is read-only,
//!    else ask
//! 10. write tools: ask, with the tier from the approval rules
//!
//! Every reason starts with the rule that produced it (`scope:`, `budget.calls_per_min:`...).

use super::store::{AiStore, CounterKind};
use super::types::{
    Analysis, Budgets, Decision, Grant, HostFacts, JailKind, Policy, PolicyEngine, Profile,
    RiskTag, Scope, Tier, ToolCall, ToolName,
};
use crate::config::AppConfig;
use crate::models::Connection;
use std::sync::Arc;
use std::time::Duration;

/// Scope tag meaning "every host". Used by the built-in default policy.
pub const ALL_HOSTS: &str = "*";

// ---------------------------------------------------------------- built-in profiles

/// Paths an agent may read without asking, unless the user sets `read_paths`.
const DEFAULT_READ_PATHS: &[&str] = &["/var/log/**", "/etc/**", "/srv/**"];

/// Paths that are never readable or usable as arguments. User entries are added to these.
const DEFAULT_DENY_PATHS: &[&str] = &[
    "/etc/shadow",
    "/etc/shadow-",
    "/etc/gshadow",
    "/etc/gshadow-",
    "/etc/sudoers",
    "/etc/sudoers.d/**",
    "/etc/security/opasswd",
    "/etc/krb5.keytab",
    "/etc/ssl/private/**",
    "/etc/wireguard/**",
    "/etc/kubernetes/**/*.conf",
    "/etc/rancher/**/*.yaml",
    "/root/**",
    "/proc/*/environ",
    "/proc/*/mem",
    // Private SSH host keys; `.pub` files are not matched, so they stay readable.
    "/etc/ssh/ssh_host_*_key",
    "**/*.key",
    "**/*.pem",
    "**/*.keytab",
    "**/id_rsa*",
    "**/id_ed25519*",
    "**/.ssh/**",
    "**/.env",
    "**/.netrc",
    "**/.pgpass",
    "**/.my.cnf",
    "**/.aws/credentials",
    "**/.docker/config.json",
    "**/.git-credentials",
    "**/.kube/config",
];

/// Command patterns that are always denied. User entries are added to these.
const DEFAULT_DENY: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "mkfs *",
    "dd of=/dev/**",
    "iptables -F",
    "shutdown *",
    "reboot",
    "halt",
    "poweroff",
];

/// The built-in policy for a profile. Scope is empty: callers set it.
pub fn builtin_profile(profile: Profile) -> Policy {
    let budgets = match profile {
        Profile::Observe => Budgets {
            calls_per_min: 30,
            changes_per_run: 0,
            ..Budgets::default()
        },
        Profile::Operate => Budgets::default(),
        Profile::Oncall => Budgets {
            calls_per_min: 60,
            changes_per_run: 20,
            run_ttl: "4h".into(),
            max_denials_per_min: 5,
        },
    };
    Policy {
        profile,
        scope: Scope::default(),
        read_paths: DEFAULT_READ_PATHS.iter().map(|s| s.to_string()).collect(),
        deny_paths: DEFAULT_DENY_PATHS.iter().map(|s| s.to_string()).collect(),
        deny: DEFAULT_DENY.iter().map(|s| s.to_string()).collect(),
        budgets,
        ..Policy::default()
    }
}

/// Policy by name from `config.ai.policies`, with its profile's defaults applied.
///
/// A missing `default` policy resolves to the built-in `observe` profile over **every** host:
/// interactive delegation acts for the user, who already has access to those hosts, and
/// observe can only read. An explicit policy with an empty `scope` means no host at all.
/// Any other missing name is an error.
///
/// Merge rules for an explicit policy, on top of its profile's built-in:
/// - `read_paths`: the user's list if not empty, else the built-in list;
/// - `deny_paths` and `deny`: built-in entries plus the user's (a policy can add, never drop);
/// - `budgets`: if equal to `Budgets::default()` (nothing set) the profile's budgets apply;
/// - everything else is taken from the user's policy as written.
pub fn resolve(config: &AppConfig, name: &str) -> anyhow::Result<Policy> {
    match config.ai.policies.get(name) {
        Some(user) => Ok(merge(user)),
        None if name == "default" => {
            let mut p = builtin_profile(Profile::Observe);
            p.scope.tags = vec![ALL_HOSTS.into()];
            Ok(p)
        }
        None => {
            let known: Vec<&str> = config.ai.policies.keys().map(String::as_str).collect();
            anyhow::bail!(
                "unknown AI policy '{name}' (defined: {})",
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            )
        }
    }
}

fn merge(user: &Policy) -> Policy {
    let base = builtin_profile(user.profile.clone());
    let mut out = user.clone();
    if out.read_paths.is_empty() {
        out.read_paths = base.read_paths;
    }
    out.deny_paths = union(base.deny_paths, &user.deny_paths);
    out.deny = union(base.deny, &user.deny);
    if user.budgets == Budgets::default() {
        out.budgets = base.budgets;
    }
    out
}

fn union(mut base: Vec<String>, extra: &[String]) -> Vec<String> {
    for e in extra {
        if !base.contains(e) {
            base.push(e.clone());
        }
    }
    base
}

// ---------------------------------------------------------------- scope

/// Is this connection visible to the agent at all?
pub fn host_in_scope(policy: &Policy, conn: &Connection) -> bool {
    in_scope(&policy.scope, &conn.name, &conn.tags)
}

fn in_scope(scope: &Scope, name: &str, tags: &[String]) -> bool {
    scope
        .tags
        .iter()
        .any(|t| t == ALL_HOSTS || tags.iter().any(|h| h == t))
        || scope.hosts.iter().any(|h| glob_match(h, name, false))
}

// ---------------------------------------------------------------- matching helpers

/// One glob token. `Star`/`DoubleStar` are both wide, so matching is a DP over the text
/// positions instead of backtracking: O(pattern × text) however many stars it has.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Lit(char),
    /// `?`
    Any,
    /// `*`
    Star,
    /// `**` in path mode: any run, `/` included.
    DoubleStar,
    /// `**/` in path mode: any run ending in `/`, or nothing at all.
    DoubleStarSlash,
    /// `[abc]`, `[a-z]`, `[!abc]`; ranges are inclusive.
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

/// Tokenize a glob. An unclosed `[` is a literal `[`, as in the shell.
fn tokens(pattern: &str, path_mode: bool) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = pattern.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '*' if path_mode && it.peek() == Some(&'*') => {
                it.next();
                if it.peek() == Some(&'/') {
                    it.next();
                    out.push(Tok::DoubleStarSlash);
                } else {
                    out.push(Tok::DoubleStar);
                }
            }
            '*' => out.push(Tok::Star),
            '?' => out.push(Tok::Any),
            '[' => {
                // Scan on a clone: an unclosed `[` is a literal bracket, not a class.
                let mut probe = it.clone();
                match class(&mut probe) {
                    Some(tok) => {
                        it = probe;
                        out.push(tok);
                    }
                    None => out.push(Tok::Lit('[')),
                }
            }
            c => out.push(Tok::Lit(c)),
        }
    }
    out
}

/// Parse a `[...]` body; `None` when there is no closing `]`.
fn class(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<Tok> {
    let mut negated = false;
    if matches!(it.peek(), Some('!') | Some('^')) {
        negated = true;
        it.next();
    }
    let mut ranges = Vec::new();
    while let Some(&c) = it.peek() {
        if c == ']' && !ranges.is_empty() {
            it.next();
            return Some(Tok::Class { negated, ranges });
        }
        it.next();
        let mut hi = c;
        // `a-z`, but a `-` before the closing `]` is a literal member.
        if it.peek() == Some(&'-') {
            let mut ahead = it.clone();
            ahead.next();
            if ahead.peek().is_some_and(|h| *h != ']') {
                it.next();
                hi = it.next().unwrap_or(c);
            }
        }
        ranges.push((c, hi));
    }
    None
}

fn class_matches(negated: bool, ranges: &[(char, char)], c: char) -> bool {
    ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi) != negated
}

/// Glob match. `*` matches any run of characters, `?` one character, `[abc]`/`[a-z]`/
/// `[!abc]` one character from a set. With `path_mode`, `*`, `?` and classes stop at `/`,
/// `**` crosses it, and `**/` also matches zero directories (`/etc/**/x` matches `/etc/x`).
pub(crate) fn glob_match(pattern: &str, text: &str, path_mode: bool) -> bool {
    let toks = tokens(pattern, path_mode);
    let t: Vec<char> = text.chars().collect();
    let mut cur = vec![false; t.len() + 1];
    cur[0] = true;
    for tok in &toks {
        let mut next = vec![false; t.len() + 1];
        match tok {
            Tok::Lit(c) => {
                for j in 1..=t.len() {
                    next[j] = cur[j - 1] && t[j - 1] == *c;
                }
            }
            Tok::Any => {
                for j in 1..=t.len() {
                    next[j] = cur[j - 1] && !(path_mode && t[j - 1] == '/');
                }
            }
            Tok::Class { negated, ranges } => {
                for j in 1..=t.len() {
                    next[j] = cur[j - 1]
                        && !(path_mode && t[j - 1] == '/')
                        && class_matches(*negated, ranges, t[j - 1]);
                }
            }
            Tok::Star => {
                next[0] = cur[0];
                for j in 1..=t.len() {
                    let carry = !(path_mode && t[j - 1] == '/') && next[j - 1];
                    next[j] = cur[j] || carry;
                }
            }
            Tok::DoubleStar => {
                next[0] = cur[0];
                for j in 1..=t.len() {
                    next[j] = cur[j] || next[j - 1];
                }
            }
            Tok::DoubleStarSlash => {
                next[0] = cur[0];
                let mut any = cur[0];
                for j in 1..=t.len() {
                    next[j] = cur[j] || (t[j - 1] == '/' && any);
                    any |= cur[j];
                }
            }
        }
        cur = next;
    }
    cur[t.len()]
}

/// Lexically normalize an absolute path (`/a/b/../c` -> `/a/c`). Relative paths are
/// returned as is: they can't be judged against absolute patterns.
pub(crate) fn normalize_path(path: &str) -> String {
    if !path.starts_with('/') {
        return path.to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

fn path_matches_any(patterns: &[String], path: &str) -> Option<String> {
    let norm = normalize_path(path);
    patterns
        .iter()
        .find(|p| glob_match(p, &norm, true))
        .cloned()
}

/// Does this text contain characters the shell expands as a glob?
fn has_glob(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// One path-like piece of a call.
struct Candidate {
    /// As written, glob characters kept.
    text: String,
    /// The shell expands this before the command runs, so it stands for several paths.
    glob: bool,
}

/// A `deny_paths` entry can hit a path three ways: the entry and the text are both literal
/// (`/etc/shadow`), a glob argument certainly expands to a literal entry (`/etc/sha*`), or
/// only some expansion could match an entry that is itself a glob (`/etc/*` against
/// `/etc/ssh/ssh_host_*_key`). The last case is not provable, so the caller asks.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathHit {
    Deny(String),
    Unsure(String),
}

/// Match one path-like operand against `deny_paths`, keeping glob characters as written.
fn deny_path_hit(patterns: &[String], candidate: &Candidate) -> Option<PathHit> {
    if !candidate.glob {
        return path_matches_any(patterns, &candidate.text).map(PathHit::Deny);
    }
    let norm = normalize_path(&candidate.text);
    let mut unsure = None;
    for p in patterns {
        if has_glob(p) {
            if glob_could_match(p, &norm) {
                unsure = Some(p.clone());
            }
        } else if glob_match(&norm, p, true) {
            return Some(PathHit::Deny(p.clone()));
        }
    }
    unsure.map(PathHit::Unsure)
}

/// Could the shell-expanded `glob` be a path that `pattern` matches? Both may hold glob
/// characters. Conservative where the expansion is not decidable: `true` means "could be".
fn glob_could_match(pattern: &str, glob: &str) -> bool {
    if !has_glob(glob) {
        return glob_match(pattern, glob, true);
    }
    if !has_glob(pattern) {
        return glob_match(glob, pattern, true);
    }
    // Compare path segments, letting `**` stand for any run of them. `*`, `?` and classes
    // stop at `/`, so one segment only ever meets one segment.
    let p: Vec<&str> = pattern.split('/').collect();
    let g: Vec<&str> = glob.split('/').collect();
    let mut dp = vec![vec![false; g.len() + 1]; p.len() + 1];
    dp[p.len()][g.len()] = true;
    for i in (0..p.len()).rev() {
        // Glob exhausted: only trailing `**` segments can still match nothing.
        dp[i][g.len()] = p[i] == "**" && dp[i + 1][g.len()];
        for j in (0..g.len()).rev() {
            dp[i][j] = if p[i] == "**" {
                dp[i + 1][j] || dp[i][j + 1]
            } else {
                segments_overlap(p[i], g[j]) && dp[i + 1][j + 1]
            };
        }
    }
    dp[0][0]
}

/// What one path segment glob can and cannot vary.
struct Seg {
    /// Literal start, up to the first wildcard.
    pre: String,
    /// Literal end, after the last wildcard.
    suf: String,
    /// Length in characters when the segment has no `*`; minimum length otherwise.
    len: Option<usize>,
    /// Token count, the minimum length.
    min: usize,
    /// `*…*`: no fixed start and no fixed end, so it matches anything.
    any: bool,
}

impl Seg {
    fn new(seg: &str) -> Self {
        let toks = tokens(seg, true);
        let pre: String = toks
            .iter()
            .map_while(|t| match t {
                Tok::Lit(c) => Some(*c),
                _ => None,
            })
            .collect();
        let suf: String = toks
            .iter()
            .rev()
            .map_while(|t| match t {
                Tok::Lit(c) => Some(*c),
                _ => None,
            })
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        let star = toks
            .iter()
            .any(|t| matches!(t, Tok::Star | Tok::DoubleStar));
        let min = toks
            .iter()
            .filter(|t| !matches!(t, Tok::Star | Tok::DoubleStar))
            .count();
        Seg {
            any: pre.is_empty() && suf.is_empty() && star,
            pre,
            suf,
            len: (!star).then_some(min),
            min,
        }
    }
}

/// Can two single-path-segment globs match the same text? Conservative: `true` when the
/// fixed starts, fixed ends and lengths don't rule it out.
fn segments_overlap(a: &str, b: &str) -> bool {
    let a = Seg::new(a);
    let b = Seg::new(b);
    if a.any || b.any {
        return true;
    }
    if !(a.pre.starts_with(&b.pre) || b.pre.starts_with(&a.pre)) {
        return false;
    }
    if !(a.suf.ends_with(&b.suf) || b.suf.ends_with(&a.suf)) {
        return false;
    }
    match (a.len, b.len) {
        (Some(x), Some(y)) => x == y,
        (Some(x), None) => x >= b.min,
        (None, Some(y)) => y >= a.min,
        (None, None) => true,
    }
}

/// Every path a call names, as written: absolute and relative arguments, `~`/`$HOME` and
/// `--opt=`/`-f/path` values, and redirection targets read from or written to. Relative
/// arguments after a literal `cd` are resolved against it. Also returns the first operand
/// that is a path only known at run time (`$DIR/x`), which cannot be proven safe.
fn path_candidates(analysis: Option<&Analysis>) -> (Vec<Candidate>, Option<String>) {
    let mut out = Vec::new();
    let mut unsure = None;
    let Some(a) = analysis else {
        return (out, unsure);
    };
    for c in &a.commands {
        let cwd = c.cwd.as_deref();
        for p in c.writes_to.iter() {
            push_operand(&mut out, &mut unsure, p, cwd, has_glob(p));
        }
        for p in c.reads_from.iter() {
            push_operand(&mut out, &mut unsure, p, cwd, has_glob(p));
        }
        for p in &c.args {
            let glob = c.glob_args.iter().any(|g| g == p);
            push_operand(&mut out, &mut unsure, p, cwd, glob);
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
    glob: bool,
) {
    if arg.len() > 1 && arg.starts_with('-') {
        if let Some((_, v)) = arg.split_once('=') {
            push_path(out, unsure, v, cwd, glob);
        } else if let Some(ix) = arg.find('/') {
            push_path(out, unsure, &arg[ix..], cwd, glob);
        }
        return;
    }
    push_path(out, unsure, arg, cwd, glob);
}

/// One path-like operand. `~`, `~user` and `$HOME` map to the part below the home directory,
/// which the `**/` deny patterns already cover; another `$VAR` path can't be judged.
fn push_path(
    out: &mut Vec<Candidate>,
    unsure: &mut Option<String>,
    raw: &str,
    cwd: Option<&str>,
    glob: bool,
) {
    if raw.is_empty() {
        return;
    }
    if raw.starts_with('/') {
        out.push(Candidate {
            text: raw.to_string(),
            glob,
        });
        return;
    }
    if let Some(rest) = raw.strip_prefix('~') {
        if let Some(ix) = rest.find('/') {
            let below = &rest[ix + 1..];
            if !below.is_empty() {
                out.push(Candidate {
                    text: below.to_string(),
                    glob,
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
                    glob,
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
            glob,
        });
    }
    out.push(Candidate {
        text: raw.to_string(),
        glob,
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

/// Parse `"2h"`, `"30m"`, `"45s"`, `"1d"`, `"1h30m"` or plain seconds.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(n) = s.parse::<u64>() {
        return Some(Duration::from_secs(n));
    }
    let mut total = 0u64;
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: u64 = num.parse().ok()?;
        num.clear();
        total += n.checked_mul(match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => return None,
        })?;
    }
    if !num.is_empty() {
        return None;
    }
    Some(Duration::from_secs(total))
}

/// Command lines the `deny` patterns are matched against: every analyzed simple command
/// (program as written and by base name, then its arguments), plus the raw command split on
/// shell separators when there is no analysis.
fn command_lines(call: &ToolCall, analysis: Option<&Analysis>) -> Vec<String> {
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
            if let Some(base) = c.program.rsplit('/').next() {
                if base != c.program {
                    out.push(join(base));
                }
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
fn matches_deny_fallback(
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
            lines
                .iter()
                .any(|l| glob_match(pat, l, false) || glob_match(&prefix, l, false))
        })
        .cloned()
}

/// A command is "low risk and reversible" when it was analyzed and carries none of the
/// dangerous tags.
fn low_risk(analysis: Option<&Analysis>) -> bool {
    match analysis {
        Some(a) => {
            a.unknown_reason.is_none()
                && !a.tags.iter().any(|t| {
                    matches!(
                        t,
                        RiskTag::Destructive
                            | RiskTag::Privilege
                            | RiskTag::Network
                            | RiskTag::ExfilRisk
                            | RiskTag::Unknown
                    )
                })
        }
        None => false,
    }
}

/// Tier for something that must ask (design 6.3).
fn tier_for(
    policy: &Policy,
    call: &ToolCall,
    analysis: Option<&Analysis>,
    host: &HostFacts,
) -> Tier {
    let has = |list: &[String]| host.tags.iter().any(|t| list.contains(t));
    if has(&policy.approval.two_person) {
        return Tier::TwoPerson;
    }
    // `edit_file` and `service` have no shell command to analyze; both are undoable.
    let reversible_tool = matches!(call.tool, ToolName::EditFile | ToolName::Service);
    if has(&policy.approval.in_client) && (low_risk(analysis) || reversible_tool) {
        return Tier::InClient;
    }
    policy.approval.default
}

fn needs_host(tool: ToolName) -> bool {
    !matches!(
        tool,
        ToolName::ListHosts
            | ToolName::Status
            | ToolName::RequestAccess
            | ToolName::ProposeChange
            | ToolName::ApplyChange
            | ToolName::Undo
    )
}

fn grant_matches(g: &Grant, call: &ToolCall, host: &str) -> bool {
    if g.tool != call.tool || !glob_match(&g.hosts, host, false) {
        return false;
    }
    if let Some(run) = &g.run_id {
        if *run != call.run_id {
            return false;
        }
    }
    match &g.target {
        None => true,
        Some(pat) => {
            let subject = match call.tool {
                ToolName::EditFile | ToolName::ReadFile => call.path.as_deref().map(normalize_path),
                ToolName::Service => call
                    .args
                    .get("unit")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                _ => call.command.clone(),
            };
            subject.is_some_and(|s| glob_match(pat, &s, call.tool == ToolName::EditFile))
        }
    }
}

// ---------------------------------------------------------------- circuit breaker

/// Out-of-scope probes within [`PROBE_WINDOW`] that trip the breaker.
pub const PROBE_LIMIT: u32 = 3;
pub const PROBE_WINDOW: Duration = Duration::from_secs(300);

/// Circuit breaker (design section 8): `Some(reason)` when the run should pause.
/// Trips on too many denials in a minute (`budgets.max_denials_per_min`, 0 = off) or on
/// repeated probing of out-of-scope hosts. The MCP run context calls this after each decision, emits
/// `AiEvent::CircuitBreaker` and stops the run.
pub fn circuit_breaker(store: &AiStore, run_id: &str, budgets: &Budgets) -> Option<String> {
    let minute = Duration::from_secs(60);
    if budgets.max_denials_per_min > 0 {
        let n = store
            .count_within(run_id, CounterKind::Denial, minute)
            .unwrap_or(0);
        if n >= budgets.max_denials_per_min {
            return Some(format!(
                "{n} calls denied in the last minute (limit {})",
                budgets.max_denials_per_min
            ));
        }
    }
    let probes = store
        .count_within(run_id, CounterKind::Probe, PROBE_WINDOW)
        .unwrap_or(0);
    if probes >= PROBE_LIMIT {
        return Some(format!(
            "{probes} attempts on hosts outside the policy scope in the last 5 minutes"
        ));
    }
    None
}

// ---------------------------------------------------------------- engine

pub struct DefaultPolicyEngine {
    store: Arc<AiStore>,
}

impl DefaultPolicyEngine {
    /// `store` gives access to active grants and budget counters.
    pub fn new(store: Arc<AiStore>) -> Self {
        Self { store }
    }

    fn note(&self, run_id: &str, kind: CounterKind) {
        if let Err(e) = self.store.note(run_id, kind) {
            tracing::warn!("policy: could not update counters: {e}");
        }
    }

    fn check_budgets(&self, policy: &Policy, call: &ToolCall) -> Option<Decision> {
        let b = &policy.budgets;
        let run = &call.run_id;
        if b.calls_per_min > 0 {
            let n = self
                .store
                .count_within(run, CounterKind::Call, Duration::from_secs(60))
                .unwrap_or(0);
            if n > b.calls_per_min {
                return Some(Decision::Deny {
                    reason: format!(
                        "budget.calls_per_min: {} calls in the last minute, limit is {}",
                        n - 1,
                        b.calls_per_min
                    ),
                });
            }
        }
        if b.changes_per_run > 0 && call.tool.is_write() {
            let n = self.store.changes_in_run(run).unwrap_or(0);
            if n >= b.changes_per_run {
                return Some(Decision::Deny {
                    reason: format!(
                        "budget.changes_per_run: {n} changes already made, limit is {}",
                        b.changes_per_run
                    ),
                });
            }
        }
        if let Some(ttl) = parse_duration(&b.run_ttl) {
            if let Ok(Some(rec)) = self.store.get_run(run) {
                if let Ok(start) = chrono::DateTime::parse_from_rfc3339(&rec.started) {
                    let age = chrono::Utc::now().signed_duration_since(start);
                    if age.to_std().map(|a| a > ttl).unwrap_or(false) {
                        return Some(Decision::Deny {
                            reason: format!("budget.run_ttl: the run is older than {}", b.run_ttl),
                        });
                    }
                }
            }
        }
        None
    }

    fn decide_inner(
        &self,
        policy: &Policy,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        host: &HostFacts,
    ) -> Decision {
        let host_name = if host.name.is_empty() {
            call.host.clone().unwrap_or_default()
        } else {
            host.name.clone()
        };
        let deny = |reason: String| Decision::Deny { reason };

        // 1. scope
        if host_name.is_empty() {
            if needs_host(call.tool) {
                return deny("target: the call names no host".into());
            }
        } else if !in_scope(&policy.scope, &host_name, &host.tags) {
            self.note(&call.run_id, CounterKind::Probe);
            return deny(format!(
                "scope: host '{host_name}' is not in this policy's scope"
            ));
        }

        // 2. budgets
        if let Some(d) = self.check_budgets(policy, call) {
            return d;
        }

        // 3. profile
        if policy.profile == Profile::Observe && call.tool.is_write() {
            return deny("profile.observe: the observe profile cannot change anything".into());
        }

        // 4. deny_paths: the tool's path, then every path the command names as written
        // (arguments, input and output redirections, `--opt=`/attached-option values,
        // `~`/`$HOME` paths and arguments relative to a literal `cd`)
        if let Some(p) = &call.path {
            if let Some(pat) = path_matches_any(&policy.deny_paths, p) {
                return deny(format!("deny_paths: '{p}' matches '{pat}'"));
            }
        }
        let (candidates, runtime_path) = path_candidates(analysis);
        let mut unsure =
            runtime_path.map(|p| format!("deny_paths: '{p}' is a path only known at run time"));
        for c in &candidates {
            match deny_path_hit(&policy.deny_paths, c) {
                Some(PathHit::Deny(pat)) => {
                    return deny(format!("deny_paths: '{}' matches '{pat}'", c.text));
                }
                Some(PathHit::Unsure(pat)) => {
                    unsure.get_or_insert_with(|| {
                        format!(
                            "deny_paths: '{}' could expand to a path matching '{pat}'",
                            c.text
                        )
                    });
                }
                None => {}
            }
        }

        // 5. deny patterns: on the parsed commands, so quoting, wrappers and `/bin/rm` can't
        // dodge them. The text matcher (whose `*` crosses `/`) only adds to that when bssh
        // couldn't fully parse the call; on a clean parse it would hard-deny
        // `rm -rf /var/www/cache` through `rm -rf /*`.
        let ast_hit = analysis.and_then(|a| {
            policy
                .deny
                .iter()
                .find(|p| super::analyzer::matches_deny(p, a))
                .cloned()
        });
        let hit = match analysis {
            Some(a) if a.unknown_reason.is_none() => ast_hit,
            _ => ast_hit.or_else(|| matches_deny_fallback(&policy.deny, call, analysis)),
        };
        if let Some(pat) = hit {
            return deny(format!("deny: the command matches '{pat}'"));
        }

        // A glob or a run-time path could still name a deny path, and the analyzer can't
        // settle it: never allow those automatically, whatever the grants say.
        if let Some(reason) = unsure {
            return Decision::Ask {
                tier: tier_for(policy, call, analysis, host),
                reason,
            };
        }

        // 6. write_paths
        if call.tool == ToolName::EditFile && !policy.write_paths.is_empty() {
            match &call.path {
                Some(p) if path_matches_any(&policy.write_paths, p).is_some() => {}
                Some(p) => {
                    return deny(format!("write_paths: '{p}' is outside the writable paths"))
                }
                None => return deny("write_paths: edit_file without a path".into()),
            }
        }

        // 7. grants
        match self.store.active_grants(Some(&call.run_id)) {
            Ok(grants) => {
                if let Some(g) = grants.iter().find(|g| grant_matches(g, call, &host_name)) {
                    return Decision::Allow {
                        reason: format!("grant: {} until {}", g.grant_id, g.expires_at),
                    };
                }
            }
            Err(e) => tracing::warn!("policy: could not read grants: {e}"),
        }

        // 8. reads
        let ask = |reason: String| Decision::Ask {
            tier: tier_for(policy, call, analysis, host),
            reason,
        };
        match call.tool {
            ToolName::ListHosts | ToolName::HostInfo | ToolName::Status => {
                return Decision::Allow {
                    reason: "read: metadata tools are always allowed in scope".into(),
                }
            }
            ToolName::Observe | ToolName::SearchLogs => {
                return Decision::Allow {
                    reason: "read: observe tools run read-only commands".into(),
                }
            }
            ToolName::ReadFile => {
                return match &call.path {
                    Some(p) if path_matches_any(&policy.read_paths, p).is_some() => {
                        Decision::Allow {
                            reason: format!("read_paths: '{p}' is readable"),
                        }
                    }
                    Some(p) => ask(format!("read_paths: '{p}' is outside the readable paths")),
                    None => deny("target: read_file without a path".into()),
                }
            }
            ToolName::RunReadonly | ToolName::RunMany => {
                if host.enforcement.jail != JailKind::None {
                    return Decision::Allow {
                        reason: format!(
                            "jail: the host enforces a {:?} read-only jail",
                            host.enforcement.jail
                        ),
                    };
                }
                return match analysis {
                    Some(a) if a.is_read_only() => Decision::Allow {
                        reason: "analyzer: every command is read-only".into(),
                    },
                    Some(a) => ask(format!(
                        "analyzer: not provably read-only ({}) and the host has no jail",
                        describe_tags(a)
                    )),
                    None => ask("analyzer: no analysis and the host has no jail".into()),
                };
            }
            ToolName::RequestAccess => {
                return Decision::Ask {
                    tier: policy.approval.default,
                    reason: "request_access: wider scope always needs a human".into(),
                }
            }
            _ => {}
        }

        // 9. writes
        ask(format!(
            "write: {:?} changes the host and needs approval",
            call.tool
        ))
    }
}

fn describe_tags(a: &Analysis) -> String {
    if let Some(r) = &a.unknown_reason {
        return format!("unknown: {r}");
    }
    a.tags
        .iter()
        .map(|t| format!("{t:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join(", ")
}

impl PolicyEngine for DefaultPolicyEngine {
    fn decide(
        &self,
        policy: &Policy,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        host: &HostFacts,
    ) -> Decision {
        self.note(&call.run_id, CounterKind::Call);
        let d = self.decide_inner(policy, call, analysis, host);
        if matches!(d, Decision::Deny { .. }) {
            self.note(&call.run_id, CounterKind::Denial);
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::{Enforcement, SimpleCommand};
    use chrono::Utc;
    use std::collections::BTreeSet;

    struct Fixture {
        _tmp: tempfile::TempDir,
        store: Arc<AiStore>,
        engine: DefaultPolicyEngine,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
        Fixture {
            engine: DefaultPolicyEngine::new(store.clone()),
            store,
            _tmp: tmp,
        }
    }

    fn operate() -> Policy {
        let mut p = builtin_profile(Profile::Operate);
        p.scope.tags = vec!["staging".into(), "prod".into()];
        p
    }

    fn host(name: &str, tags: &[&str], jail: JailKind) -> HostFacts {
        HostFacts {
            name: name.into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            enforcement: Enforcement {
                jail,
                ..Default::default()
            },
        }
    }

    fn call(tool: ToolName, host: Option<&str>) -> ToolCall {
        ToolCall {
            run_id: "run1".into(),
            call_id: "c1".into(),
            tool,
            host: host.map(String::from),
            command: None,
            path: None,
            args: serde_json::Value::Null,
        }
    }

    fn cmd_call(tool: ToolName, h: &str, command: &str) -> ToolCall {
        ToolCall {
            command: Some(command.into()),
            ..call(tool, Some(h))
        }
    }

    fn path_call(tool: ToolName, h: &str, path: &str) -> ToolCall {
        ToolCall {
            path: Some(path.into()),
            ..call(tool, Some(h))
        }
    }

    fn analysis(program: &str, args: &[&str], tags: &[RiskTag]) -> Analysis {
        let tags: BTreeSet<RiskTag> = tags.iter().copied().collect();
        Analysis {
            commands: vec![SimpleCommand {
                program: program.into(),
                args: args.iter().map(|s| s.to_string()).collect(),
                wrappers: vec![],
                writes_to: vec![],
                reads_from: vec![],
                glob_args: vec![],
                cwd: None,
                tags: tags.clone(),
                rule: None,
            }],
            tags,
            unknown_reason: None,
        }
    }

    fn unknown_analysis() -> Analysis {
        Analysis {
            commands: vec![],
            tags: [RiskTag::Unknown].into_iter().collect(),
            unknown_reason: Some("eval".into()),
        }
    }

    fn is_allow(d: &Decision) -> bool {
        matches!(d, Decision::Allow { .. })
    }
    fn is_deny(d: &Decision) -> bool {
        matches!(d, Decision::Deny { .. })
    }
    fn reason(d: &Decision) -> &str {
        match d {
            Decision::Allow { reason } | Decision::Deny { reason } => reason,
            Decision::Ask { reason, .. } => reason,
        }
    }
    fn tier(d: &Decision) -> Tier {
        match d {
            Decision::Ask { tier, .. } => *tier,
            other => panic!("expected ask, got {other:?}"),
        }
    }

    // ---- decision table

    #[test]
    fn decision_table() {
        use ToolName::*;
        let stg = host("web-1", &["staging"], JailKind::None);
        let stg_jail = host("web-1", &["staging"], JailKind::Landlock);
        let other = host("db-9", &["lab"], JailKind::None);
        let ro = analysis("cat", &["/etc/hosts"], &[RiskTag::Read]);
        let wr = analysis(
            "systemctl",
            &["restart", "nginx"],
            &[RiskTag::ServiceChange],
        );

        // (label, policy, call, analysis, host, expected: 'A' allow, 'K' ask, 'D' deny, rule fragment)
        type Case<'a> = (
            &'a str,
            Policy,
            ToolCall,
            Option<Analysis>,
            HostFacts,
            char,
            &'a str,
        );
        let mut cases: Vec<Case> = vec![
            (
                "list_hosts allowed",
                operate(),
                call(ListHosts, None),
                None,
                stg.clone(),
                'A',
                "read",
            ),
            (
                "status allowed",
                operate(),
                call(Status, None),
                None,
                stg.clone(),
                'A',
                "read",
            ),
            (
                "host_info in scope",
                operate(),
                call(HostInfo, Some("web-1")),
                None,
                stg.clone(),
                'A',
                "read",
            ),
            (
                "host_info out of scope",
                operate(),
                call(HostInfo, Some("db-9")),
                None,
                other.clone(),
                'D',
                "scope",
            ),
            (
                "observe in scope",
                operate(),
                call(Observe, Some("web-1")),
                None,
                stg.clone(),
                'A',
                "read",
            ),
            (
                "observe out of scope",
                operate(),
                call(Observe, Some("db-9")),
                None,
                other.clone(),
                'D',
                "scope",
            ),
            (
                "search_logs",
                operate(),
                call(SearchLogs, Some("web-1")),
                None,
                stg.clone(),
                'A',
                "read",
            ),
            (
                "observe without host",
                operate(),
                call(Observe, None),
                None,
                HostFacts::default(),
                'D',
                "target",
            ),
            (
                "read_file under read_paths",
                operate(),
                path_call(ReadFile, "web-1", "/var/log/nginx/error.log"),
                None,
                stg.clone(),
                'A',
                "read_paths",
            ),
            (
                "read_file outside read_paths asks",
                operate(),
                path_call(ReadFile, "web-1", "/home/bob/notes"),
                None,
                stg.clone(),
                'K',
                "read_paths",
            ),
            (
                "read_file dotdot escape asks",
                operate(),
                path_call(ReadFile, "web-1", "/var/log/../../home/bob/x"),
                None,
                stg.clone(),
                'K',
                "read_paths",
            ),
            (
                "read_file shadow denied",
                operate(),
                path_call(ReadFile, "web-1", "/etc/shadow"),
                None,
                stg.clone(),
                'D',
                "deny_paths",
            ),
            (
                "read_file dotdot to shadow denied",
                operate(),
                path_call(ReadFile, "web-1", "/var/log/../../etc/shadow"),
                None,
                stg.clone(),
                'D',
                "deny_paths",
            ),
            (
                "read_file ssh key denied",
                operate(),
                path_call(ReadFile, "web-1", "/home/bob/.ssh/id_ed25519"),
                None,
                stg.clone(),
                'D',
                "deny_paths",
            ),
            (
                "read_file .env denied",
                operate(),
                path_call(ReadFile, "web-1", "/srv/app/.env"),
                None,
                stg.clone(),
                'D',
                "deny_paths",
            ),
            (
                "read_file .key denied",
                operate(),
                path_call(ReadFile, "web-1", "/etc/ssl/site.key"),
                None,
                stg.clone(),
                'D',
                "deny_paths",
            ),
            (
                "read_file without path denied",
                operate(),
                call(ReadFile, Some("web-1")),
                None,
                stg.clone(),
                'D',
                "target",
            ),
            (
                "run_readonly jailed",
                operate(),
                cmd_call(RunReadonly, "web-1", "rm x"),
                Some(wr.clone()),
                stg_jail.clone(),
                'A',
                "jail",
            ),
            (
                "run_readonly read-only analysis",
                operate(),
                cmd_call(RunReadonly, "web-1", "cat /etc/hosts"),
                Some(ro.clone()),
                stg.clone(),
                'A',
                "analyzer",
            ),
            (
                "run_readonly write analysis asks",
                operate(),
                cmd_call(RunReadonly, "web-1", "systemctl restart nginx"),
                Some(wr.clone()),
                stg.clone(),
                'K',
                "analyzer",
            ),
            (
                "run_readonly unknown analysis asks",
                operate(),
                cmd_call(RunReadonly, "web-1", "eval x"),
                Some(unknown_analysis()),
                stg.clone(),
                'K',
                "analyzer",
            ),
            (
                "run_readonly without analysis asks",
                operate(),
                cmd_call(RunReadonly, "web-1", "ls"),
                None,
                stg.clone(),
                'K',
                "analyzer",
            ),
            (
                "run_many jailed",
                operate(),
                cmd_call(RunMany, "web-1", "uptime"),
                None,
                stg_jail.clone(),
                'A',
                "jail",
            ),
            (
                "run_many read-only",
                operate(),
                cmd_call(RunMany, "web-1", "uptime"),
                Some(analysis("uptime", &[], &[RiskTag::Read])),
                stg.clone(),
                'A',
                "analyzer",
            ),
            (
                "run_many out of scope",
                operate(),
                cmd_call(RunMany, "db-9", "uptime"),
                None,
                other.clone(),
                'D',
                "scope",
            ),
            (
                "jailed run_readonly deny pattern wins",
                operate(),
                cmd_call(RunReadonly, "web-1", "rm -rf /"),
                Some(analysis("rm", &["-rf", "/"], &[RiskTag::Destructive])),
                stg_jail.clone(),
                'D',
                "deny",
            ),
            (
                "jailed run_readonly reading shadow denied",
                operate(),
                cmd_call(RunReadonly, "web-1", "cat /etc/shadow"),
                Some(analysis("cat", &["/etc/shadow"], &[RiskTag::Read])),
                stg_jail.clone(),
                'D',
                "deny_paths",
            ),
            (
                "run asks",
                operate(),
                cmd_call(Run, "web-1", "systemctl restart nginx"),
                Some(wr.clone()),
                stg.clone(),
                'K',
                "write",
            ),
            (
                "run observe denied",
                {
                    let mut p = operate();
                    p.profile = Profile::Observe;
                    p
                },
                cmd_call(Run, "web-1", "ls"),
                Some(ro.clone()),
                stg.clone(),
                'D',
                "profile.observe",
            ),
            (
                "edit_file observe denied",
                {
                    let mut p = operate();
                    p.profile = Profile::Observe;
                    p
                },
                path_call(EditFile, "web-1", "/etc/nginx/a.conf"),
                None,
                stg.clone(),
                'D',
                "profile.observe",
            ),
            (
                "service observe denied",
                {
                    let mut p = operate();
                    p.profile = Profile::Observe;
                    p
                },
                call(Service, Some("web-1")),
                None,
                stg.clone(),
                'D',
                "profile.observe",
            ),
            (
                "apply_change observe denied",
                {
                    let mut p = operate();
                    p.profile = Profile::Observe;
                    p
                },
                call(ApplyChange, None),
                None,
                HostFacts::default(),
                'D',
                "profile.observe",
            ),
            (
                "propose_change operate asks",
                operate(),
                call(ProposeChange, None),
                None,
                HostFacts::default(),
                'K',
                "write",
            ),
            (
                "undo operate asks",
                operate(),
                call(Undo, None),
                None,
                HostFacts::default(),
                'K',
                "write",
            ),
            (
                "run deny pattern",
                operate(),
                cmd_call(Run, "web-1", "rm -rf /"),
                Some(analysis("rm", &["-rf", "/"], &[RiskTag::Destructive])),
                stg.clone(),
                'D',
                "deny",
            ),
            (
                "run deny pattern with trailing flag",
                operate(),
                cmd_call(Run, "web-1", "rm -rf / --no-preserve-root"),
                Some(analysis(
                    "rm",
                    &["-rf", "/", "--no-preserve-root"],
                    &[RiskTag::Destructive],
                )),
                stg.clone(),
                'D',
                "deny",
            ),
            (
                "run deny pattern via full path",
                operate(),
                cmd_call(Run, "web-1", "/bin/mkfs /dev/sda"),
                Some(analysis(
                    "/bin/mkfs",
                    &["/dev/sda"],
                    &[RiskTag::Destructive],
                )),
                stg.clone(),
                'D',
                "deny",
            ),
            (
                "run deny pattern raw text, no analysis",
                operate(),
                cmd_call(Run, "web-1", "ls; rm -rf /"),
                None,
                stg.clone(),
                'D',
                "deny",
            ),
            (
                "rm -rf on a deep path asks, rm -rf /* only covers the top level",
                operate(),
                cmd_call(Run, "web-1", "sudo bash -c 'rm -rf /var/www/cache'"),
                Some(analysis(
                    "rm",
                    &["-rf", "/var/www/cache"],
                    &[RiskTag::Destructive, RiskTag::Privilege],
                )),
                stg.clone(),
                'K',
                "write",
            ),
            (
                "rm -rf on a top-level directory is denied",
                operate(),
                cmd_call(Run, "web-1", "rm -rf /var"),
                Some(analysis("rm", &["-rf", "/var"], &[RiskTag::Destructive])),
                stg.clone(),
                'D',
                "rm -rf /*",
            ),
            (
                "run deny glob dd",
                operate(),
                cmd_call(Run, "web-1", "dd of=/dev/sda if=x"),
                Some(analysis(
                    "dd",
                    &["of=/dev/sda", "if=x"],
                    &[RiskTag::Destructive],
                )),
                stg.clone(),
                'D',
                "deny",
            ),
            (
                "run write to deny_path via redirect",
                operate(),
                cmd_call(Run, "web-1", "echo x > /etc/shadow"),
                Some({
                    let mut a = analysis("echo", &["x"], &[RiskTag::Write]);
                    a.commands[0].writes_to.push("/etc/shadow".into());
                    a
                }),
                stg.clone(),
                'D',
                "deny_paths",
            ),
            (
                "run touching ssh dir denied",
                operate(),
                cmd_call(Run, "web-1", "cat /root/.ssh/authorized_keys"),
                Some(analysis(
                    "cat",
                    &["/root/.ssh/authorized_keys"],
                    &[RiskTag::Read],
                )),
                stg.clone(),
                'D',
                "deny_paths",
            ),
            (
                "edit_file default tier",
                operate(),
                path_call(EditFile, "web-1", "/etc/nginx/a.conf"),
                None,
                stg.clone(),
                'K',
                "write",
            ),
            (
                "edit_file outside write_paths denied",
                {
                    let mut p = operate();
                    p.write_paths = vec!["/etc/nginx/**".into()];
                    p
                },
                path_call(EditFile, "web-1", "/etc/passwd"),
                None,
                stg.clone(),
                'D',
                "write_paths",
            ),
            (
                "edit_file inside write_paths asks",
                {
                    let mut p = operate();
                    p.write_paths = vec!["/etc/nginx/**".into()];
                    p
                },
                path_call(EditFile, "web-1", "/etc/nginx/conf.d/a.conf"),
                None,
                stg.clone(),
                'K',
                "write",
            ),
            (
                "edit_file dotdot out of write_paths denied",
                {
                    let mut p = operate();
                    p.write_paths = vec!["/etc/nginx/**".into()];
                    p
                },
                path_call(EditFile, "web-1", "/etc/nginx/../passwd"),
                None,
                stg.clone(),
                'D',
                "write_paths",
            ),
            (
                "request_access asks even in observe",
                {
                    let mut p = operate();
                    p.profile = Profile::Observe;
                    p
                },
                call(RequestAccess, None),
                None,
                HostFacts::default(),
                'K',
                "request_access",
            ),
            (
                "empty scope means no host",
                {
                    let mut p = operate();
                    p.scope = Scope::default();
                    p
                },
                call(Observe, Some("web-1")),
                None,
                stg.clone(),
                'D',
                "scope",
            ),
            (
                "scope by host name",
                {
                    let mut p = operate();
                    p.scope = Scope {
                        tags: vec![],
                        hosts: vec!["web-*".into()],
                    };
                    p
                },
                call(Observe, Some("web-1")),
                None,
                stg.clone(),
                'A',
                "read",
            ),
            (
                "scope wildcard tag",
                {
                    let mut p = operate();
                    p.scope.tags = vec![ALL_HOSTS.into()];
                    p
                },
                call(Observe, Some("db-9")),
                None,
                other.clone(),
                'A',
                "read",
            ),
        ];
        // drop the unused moves
        let _ = &mut cases;

        // One store for all cases; each case gets its own run id so counters don't mix.
        let fx = fixture();
        for (n, (label, policy, mut c, a, h, want, frag)) in cases.into_iter().enumerate() {
            c.run_id = format!("case-{n}");
            let d = fx.engine.decide(&policy, &c, a.as_ref(), &h);
            let got = match d {
                Decision::Allow { .. } => 'A',
                Decision::Ask { .. } => 'K',
                Decision::Deny { .. } => 'D',
            };
            assert_eq!(got, want, "case '{label}': got {d:?}");
            assert!(
                reason(&d).contains(frag),
                "case '{label}': reason '{}' should name rule '{frag}'",
                reason(&d)
            );
        }
    }

    // ---- tiers

    #[test]
    fn tier_two_person_for_critical_tags() {
        let fx = fixture();
        let mut p = operate();
        p.approval.two_person = vec!["prod-db".into()];
        p.scope.tags.push("prod-db".into());
        let h = host("db-1", &["prod-db", "prod"], JailKind::None);
        let d = fx
            .engine
            .decide(&p, &cmd_call(ToolName::Run, "db-1", "ls"), None, &h);
        assert_eq!(tier(&d), Tier::TwoPerson);
    }

    #[test]
    fn tier_two_person_beats_in_client() {
        let fx = fixture();
        let mut p = operate();
        p.approval.two_person = vec!["prod".into()];
        p.approval.in_client = vec!["prod".into()];
        let h = host("web-1", &["prod"], JailKind::None);
        let a = analysis("systemctl", &["reload", "nginx"], &[RiskTag::ServiceChange]);
        let d = fx
            .engine
            .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), Some(&a), &h);
        assert_eq!(tier(&d), Tier::TwoPerson);
    }

    #[test]
    fn tier_in_client_for_low_risk_on_listed_tag() {
        let fx = fixture();
        let mut p = operate();
        p.approval.in_client = vec!["staging".into()];
        let h = host("web-1", &["staging"], JailKind::None);
        let a = analysis("systemctl", &["reload", "nginx"], &[RiskTag::ServiceChange]);
        let d = fx
            .engine
            .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), Some(&a), &h);
        assert_eq!(tier(&d), Tier::InClient);
    }

    #[test]
    fn tier_in_client_not_for_risky_tags() {
        for risky in [
            RiskTag::Destructive,
            RiskTag::Privilege,
            RiskTag::Network,
            RiskTag::ExfilRisk,
            RiskTag::Unknown,
        ] {
            let fx = fixture();
            let mut p = operate();
            p.approval.in_client = vec!["staging".into()];
            let h = host("web-1", &["staging"], JailKind::None);
            let a = analysis("x", &[], &[RiskTag::Write, risky]);
            let d = fx
                .engine
                .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), Some(&a), &h);
            assert_eq!(tier(&d), Tier::OutOfBand, "{risky:?}");
        }
    }

    #[test]
    fn tier_in_client_not_for_unanalyzed_run_or_unknown_reason() {
        let fx = fixture();
        let mut p = operate();
        p.approval.in_client = vec!["staging".into()];
        let h = host("web-1", &["staging"], JailKind::None);
        let d = fx
            .engine
            .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), None, &h);
        assert_eq!(tier(&d), Tier::OutOfBand);
        let d = fx.engine.decide(
            &p,
            &cmd_call(ToolName::Run, "web-1", "x"),
            Some(&unknown_analysis()),
            &h,
        );
        assert_eq!(tier(&d), Tier::OutOfBand);
    }

    #[test]
    fn tier_in_client_for_edit_file_and_service_on_listed_tag() {
        let fx = fixture();
        let mut p = operate();
        p.approval.in_client = vec!["staging".into()];
        let h = host("web-1", &["staging"], JailKind::None);
        let d = fx.engine.decide(
            &p,
            &path_call(ToolName::EditFile, "web-1", "/etc/nginx/a"),
            None,
            &h,
        );
        assert_eq!(tier(&d), Tier::InClient);
        let d = fx
            .engine
            .decide(&p, &call(ToolName::Service, Some("web-1")), None, &h);
        assert_eq!(tier(&d), Tier::InClient);
    }

    #[test]
    fn tier_in_client_never_for_untagged_prod() {
        let fx = fixture();
        let mut p = operate();
        p.approval.in_client = vec!["staging".into()];
        let h = host("web-9", &["prod"], JailKind::None);
        let d = fx.engine.decide(
            &p,
            &path_call(ToolName::EditFile, "web-9", "/etc/nginx/a"),
            None,
            &h,
        );
        assert_eq!(tier(&d), Tier::OutOfBand);
    }

    #[test]
    fn tier_follows_configured_default() {
        let fx = fixture();
        let mut p = operate();
        p.approval.default = Tier::TwoPerson;
        let h = host("web-1", &["staging"], JailKind::None);
        let d = fx
            .engine
            .decide(&p, &cmd_call(ToolName::Run, "web-1", "x"), None, &h);
        assert_eq!(tier(&d), Tier::TwoPerson);
    }

    // ---- grants

    fn grant(tool: ToolName, hosts: &str, target: Option<&str>, run: Option<&str>) -> Grant {
        Grant {
            grant_id: "g1".into(),
            run_id: run.map(String::from),
            tool,
            hosts: hosts.into(),
            target: target.map(String::from),
            expires_at: (Utc::now() + chrono::Duration::minutes(30)).to_rfc3339(),
        }
    }

    #[test]
    fn grant_turns_ask_into_allow() {
        let fx = fixture();
        fx.store
            .add_grant(&grant(
                ToolName::EditFile,
                "web-*",
                Some("/etc/nginx/**"),
                None,
            ))
            .unwrap();
        let h = host("web-1", &["staging"], JailKind::None);
        let d = fx.engine.decide(
            &operate(),
            &path_call(ToolName::EditFile, "web-1", "/etc/nginx/a.conf"),
            None,
            &h,
        );
        assert!(is_allow(&d), "{d:?}");
        assert!(reason(&d).contains("grant: g1"));
    }

    #[test]
    fn grant_for_another_tool_host_or_target_does_not_apply() {
        let fx = fixture();
        fx.store
            .add_grant(&grant(
                ToolName::EditFile,
                "web-*",
                Some("/etc/nginx/**"),
                None,
            ))
            .unwrap();
        let h = host("web-1", &["staging"], JailKind::None);
        let p = operate();
        let d = fx.engine.decide(
            &p,
            &path_call(ToolName::EditFile, "web-1", "/etc/other/a"),
            None,
            &h,
        );
        assert!(matches!(d, Decision::Ask { .. }));
        let d = fx
            .engine
            .decide(&p, &call(ToolName::Service, Some("web-1")), None, &h);
        assert!(matches!(d, Decision::Ask { .. }));
        let h2 = host("app-1", &["staging"], JailKind::None);
        let d = fx.engine.decide(
            &p,
            &path_call(ToolName::EditFile, "app-1", "/etc/nginx/a"),
            None,
            &h2,
        );
        assert!(matches!(d, Decision::Ask { .. }));
    }

    #[test]
    fn grant_for_service_unit() {
        let fx = fixture();
        fx.store
            .add_grant(&grant(ToolName::Service, "*", Some("nginx*"), Some("run1")))
            .unwrap();
        let h = host("web-1", &["staging"], JailKind::None);
        let mut c = call(ToolName::Service, Some("web-1"));
        c.args = serde_json::json!({"unit": "nginx.service", "action": "reload"});
        assert!(is_allow(&fx.engine.decide(&operate(), &c, None, &h)));
        c.args = serde_json::json!({"unit": "postgres.service"});
        assert!(matches!(
            fx.engine.decide(&operate(), &c, None, &h),
            Decision::Ask { .. }
        ));
    }

    #[test]
    fn grant_of_another_run_is_ignored() {
        let fx = fixture();
        fx.store
            .add_grant(&grant(ToolName::Service, "*", None, Some("other-run")))
            .unwrap();
        let h = host("web-1", &["staging"], JailKind::None);
        let d = fx.engine.decide(
            &operate(),
            &call(ToolName::Service, Some("web-1")),
            None,
            &h,
        );
        assert!(matches!(d, Decision::Ask { .. }));
    }

    #[test]
    fn revoked_and_expired_grants_do_not_apply() {
        let fx = fixture();
        fx.store
            .add_grant(&grant(ToolName::Service, "*", None, None))
            .unwrap();
        fx.store.revoke_grant("g1").unwrap();
        let mut old = grant(ToolName::Service, "*", None, None);
        old.grant_id = "g2".into();
        old.expires_at = (Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
        fx.store.add_grant(&old).unwrap();
        let h = host("web-1", &["staging"], JailKind::None);
        let d = fx.engine.decide(
            &operate(),
            &call(ToolName::Service, Some("web-1")),
            None,
            &h,
        );
        assert!(matches!(d, Decision::Ask { .. }));
    }

    #[test]
    fn grant_never_overrides_deny() {
        let fx = fixture();
        fx.store
            .add_grant(&grant(ToolName::Run, "*", None, None))
            .unwrap();
        let h = host("web-1", &["staging"], JailKind::None);
        let a = analysis("rm", &["-rf", "/"], &[RiskTag::Destructive]);
        let d = fx.engine.decide(
            &operate(),
            &cmd_call(ToolName::Run, "web-1", "rm -rf /"),
            Some(&a),
            &h,
        );
        assert!(is_deny(&d), "{d:?}");
        // and not over the observe profile either
        let mut p = operate();
        p.profile = Profile::Observe;
        let d = fx
            .engine
            .decide(&p, &cmd_call(ToolName::Run, "web-1", "ls"), None, &h);
        assert!(is_deny(&d));
    }

    // ---- budgets and circuit breaker

    #[test]
    fn calls_per_min_budget_denies_over_limit() {
        let fx = fixture();
        let mut p = operate();
        p.budgets.calls_per_min = 3;
        let h = host("web-1", &["staging"], JailKind::None);
        let c = call(ToolName::Observe, Some("web-1"));
        for _ in 0..3 {
            assert!(is_allow(&fx.engine.decide(&p, &c, None, &h)));
        }
        let d = fx.engine.decide(&p, &c, None, &h);
        assert!(is_deny(&d));
        assert!(reason(&d).contains("budget.calls_per_min"));
    }

    #[test]
    fn calls_per_min_zero_means_unlimited() {
        let fx = fixture();
        let mut p = operate();
        p.budgets.calls_per_min = 0;
        let h = host("web-1", &["staging"], JailKind::None);
        for _ in 0..50 {
            assert!(is_allow(&fx.engine.decide(
                &p,
                &call(ToolName::Observe, Some("web-1")),
                None,
                &h
            )));
        }
    }

    #[test]
    fn calls_budget_is_per_run() {
        let fx = fixture();
        let mut p = operate();
        p.budgets.calls_per_min = 1;
        let h = host("web-1", &["staging"], JailKind::None);
        let mut c = call(ToolName::Observe, Some("web-1"));
        assert!(is_allow(&fx.engine.decide(&p, &c, None, &h)));
        c.run_id = "run2".into();
        assert!(is_allow(&fx.engine.decide(&p, &c, None, &h)));
    }

    #[test]
    fn changes_per_run_budget_denies_writes_only() {
        let fx = fixture();
        let mut p = operate();
        p.budgets.changes_per_run = 2;
        let h = host("web-1", &["staging"], JailKind::None);
        let w = cmd_call(ToolName::Run, "web-1", "touch x");
        assert!(matches!(
            fx.engine.decide(&p, &w, None, &h),
            Decision::Ask { .. }
        ));
        fx.store.note_change("run1", "a").unwrap();
        fx.store.note_change("run1", "b").unwrap();
        let d = fx.engine.decide(&p, &w, None, &h);
        assert!(is_deny(&d));
        assert!(reason(&d).contains("budget.changes_per_run"));
        // reads still fine
        assert!(is_allow(&fx.engine.decide(
            &p,
            &call(ToolName::Observe, Some("web-1")),
            None,
            &h
        )));
    }

    #[test]
    fn run_ttl_budget_denies_old_runs() {
        use crate::services::ai::store::RunRecord;
        let fx = fixture();
        fx.store
            .start_run(&RunRecord {
                id: "run1".into(),
                client: "c".into(),
                on_behalf_of: "alice".into(),
                policy: "default".into(),
                started: (Utc::now() - chrono::Duration::hours(3)).to_rfc3339(),
                ended: None,
                reason: None,
            })
            .unwrap();
        let h = host("web-1", &["staging"], JailKind::None);
        let d = fx.engine.decide(
            &operate(),
            &call(ToolName::Observe, Some("web-1")),
            None,
            &h,
        );
        assert!(is_deny(&d));
        assert!(reason(&d).contains("budget.run_ttl"));
    }

    #[test]
    fn denials_are_counted_and_trip_the_breaker() {
        let fx = fixture();
        let mut p = operate();
        p.budgets.max_denials_per_min = 3;
        let h = host("web-1", &["staging"], JailKind::None);
        let bad = path_call(ToolName::ReadFile, "web-1", "/etc/shadow");
        for _ in 0..2 {
            assert!(is_deny(&fx.engine.decide(&p, &bad, None, &h)));
        }
        assert!(circuit_breaker(&fx.store, "run1", &p.budgets).is_none());
        assert!(is_deny(&fx.engine.decide(&p, &bad, None, &h)));
        let why = circuit_breaker(&fx.store, "run1", &p.budgets).unwrap();
        assert!(why.contains("denied"), "{why}");
        assert!(circuit_breaker(&fx.store, "other", &p.budgets).is_none());
    }

    #[test]
    fn breaker_off_when_limit_is_zero() {
        let fx = fixture();
        let mut p = operate();
        p.budgets.max_denials_per_min = 0;
        let h = host("web-1", &["staging"], JailKind::None);
        let bad = path_call(ToolName::ReadFile, "web-1", "/etc/shadow");
        for _ in 0..10 {
            fx.engine.decide(&p, &bad, None, &h);
        }
        assert!(circuit_breaker(&fx.store, "run1", &p.budgets).is_none());
    }

    #[test]
    fn probing_out_of_scope_hosts_trips_the_breaker() {
        let fx = fixture();
        let mut p = operate();
        p.budgets.max_denials_per_min = 0;
        let other = host("db-9", &["lab"], JailKind::None);
        for i in 0..PROBE_LIMIT {
            assert!(
                circuit_breaker(&fx.store, "run1", &p.budgets).is_none(),
                "{i}"
            );
            fx.engine
                .decide(&p, &call(ToolName::Observe, Some("db-9")), None, &other);
        }
        let why = circuit_breaker(&fx.store, "run1", &p.budgets).unwrap();
        assert!(why.contains("outside the policy scope"), "{why}");
    }

    // ---- resolve / scope

    #[test]
    fn missing_default_is_observe_over_every_host() {
        let cfg = AppConfig::default();
        let p = resolve(&cfg, "default").unwrap();
        assert_eq!(p.profile, Profile::Observe);
        assert!(in_scope(&p.scope, "anything", &[]));
        assert!(!p.read_paths.is_empty());
        assert!(!p.deny_paths.is_empty());
        assert!(!p.deny.is_empty());
        assert_eq!(p.budgets.changes_per_run, 0);
    }

    #[test]
    fn unknown_named_policy_is_an_error() {
        let mut cfg = AppConfig::default();
        cfg.ai.policies.insert("ops".into(), Policy::default());
        let e = resolve(&cfg, "nope").unwrap_err().to_string();
        assert!(e.contains("nope") && e.contains("ops"), "{e}");
    }

    #[test]
    fn explicit_policy_with_empty_scope_means_no_host() {
        let mut cfg = AppConfig::default();
        cfg.ai.policies.insert("default".into(), Policy::default());
        let p = resolve(&cfg, "default").unwrap();
        assert!(!in_scope(&p.scope, "web-1", &["staging".into()]));
    }

    #[test]
    fn resolve_merges_user_policy_over_builtin() {
        let mut cfg = AppConfig::default();
        let user: Policy = serde_json::from_str(
            r#"{"profile":"operate","scope":{"tags":["staging"]},
                "deny_paths":["/srv/secret/**"],"deny":["kill -9 *"],
                "read_paths":["/opt/**"],"budgets":{"calls_per_min":5}}"#,
        )
        .unwrap();
        cfg.ai.policies.insert("ops".into(), user);
        let p = resolve(&cfg, "ops").unwrap();
        assert_eq!(p.profile, Profile::Operate);
        assert_eq!(p.read_paths, vec!["/opt/**".to_string()]);
        assert!(p.deny_paths.contains(&"/srv/secret/**".to_string()));
        assert!(
            p.deny_paths.contains(&"/etc/shadow".to_string()),
            "built-ins stay"
        );
        assert!(p.deny.contains(&"kill -9 *".to_string()));
        assert!(p.deny.contains(&"mkfs *".to_string()));
        assert_eq!(p.budgets.calls_per_min, 5);
    }

    #[test]
    fn resolve_uses_profile_budgets_when_none_set() {
        let mut cfg = AppConfig::default();
        let user: Policy =
            serde_json::from_str(r#"{"profile":"oncall","scope":{"tags":["x"]}}"#).unwrap();
        cfg.ai.policies.insert("oc".into(), user);
        let p = resolve(&cfg, "oc").unwrap();
        assert_eq!(p.budgets.calls_per_min, 60);
        assert_eq!(p.budgets.changes_per_run, 20);
    }

    #[test]
    fn resolve_keeps_user_empty_write_paths_and_approval() {
        let mut cfg = AppConfig::default();
        let user: Policy = serde_json::from_str(
            r#"{"profile":"operate","approval":{"two_person":["prod-db"]},"write_paths":["/etc/nginx/**"]}"#,
        )
        .unwrap();
        cfg.ai.policies.insert("ops".into(), user);
        let p = resolve(&cfg, "ops").unwrap();
        assert_eq!(p.approval.two_person, vec!["prod-db".to_string()]);
        assert_eq!(p.write_paths, vec!["/etc/nginx/**".to_string()]);
    }

    #[test]
    fn host_in_scope_by_tag_name_and_wildcard() {
        let mut conn = Connection::new(
            "web-1".into(),
            "h".into(),
            "u".into(),
            22,
            None,
            None,
            false,
            None,
        );
        conn.tags = vec!["staging".into()];
        let mut p = Policy::default();
        assert!(!host_in_scope(&p, &conn));
        p.scope.tags = vec!["staging".into()];
        assert!(host_in_scope(&p, &conn));
        p.scope.tags = vec!["prod".into()];
        assert!(!host_in_scope(&p, &conn));
        p.scope.hosts = vec!["web-*".into()];
        assert!(host_in_scope(&p, &conn));
        p.scope = Scope {
            tags: vec![ALL_HOSTS.into()],
            hosts: vec![],
        };
        assert!(host_in_scope(&p, &conn));
    }

    // ---- helpers

    #[test]
    fn glob_basics() {
        assert!(glob_match("web-*", "web-12", false));
        assert!(!glob_match("web-*", "db-1", false));
        assert!(glob_match("mkfs *", "mkfs /dev/sda", false));
        assert!(glob_match("a?c", "abc", false));
        assert!(glob_match("*", "", false));
        assert!(!glob_match("a", "ab", false));
    }

    #[test]
    fn glob_path_mode() {
        assert!(glob_match("/var/log/**", "/var/log/a/b.log", true));
        assert!(!glob_match("/var/log/*", "/var/log/a/b.log", true));
        assert!(glob_match("/var/log/*", "/var/log/a.log", true));
        assert!(glob_match("**/.env", "/.env", true));
        assert!(glob_match("**/.env", "/srv/app/.env", true));
        assert!(!glob_match("**/.env", "/srv/app/.envrc", true));
        assert!(glob_match("/etc/**/x", "/etc/x", true));
        assert!(glob_match("**/*.key", "/a/b/site.key", true));
        assert!(glob_match("**/id_rsa*", "/home/u/id_rsa.pub", true));
    }

    #[test]
    fn normalize_paths() {
        assert_eq!(normalize_path("/a/b/../c"), "/a/c");
        assert_eq!(normalize_path("/a//b/./c"), "/a/b/c");
        assert_eq!(normalize_path("/../.."), "/");
        assert_eq!(normalize_path("rel/../x"), "rel/../x");
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_duration("1h30m"), Some(Duration::from_secs(5400)));
        assert_eq!(parse_duration("45s"), Some(Duration::from_secs(45)));
        assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86400)));
        assert_eq!(parse_duration("90"), Some(Duration::from_secs(90)));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("2x"), None);
        assert_eq!(parse_duration("h"), None);
        assert_eq!(parse_duration("5m3"), None);
    }

    // ---- deny paths, globs and cd tracking

    use crate::services::ai::analyzer::ShellAnalyzer;
    use crate::services::ai::types::Analyzer;

    fn operated() -> Policy {
        let mut p = builtin_profile(Profile::Operate);
        p.scope.tags = vec!["staging".into()];
        p
    }

    fn plain_host() -> HostFacts {
        HostFacts {
            name: "web-1".into(),
            tags: vec!["staging".into()],
            enforcement: Enforcement::default(),
        }
    }

    fn analyzed(command: &str) -> Analysis {
        ShellAnalyzer::new().analyze(command)
    }

    fn readonly_call(command: &str) -> ToolCall {
        ToolCall {
            run_id: "run1".into(),
            call_id: "c1".into(),
            tool: ToolName::RunReadonly,
            host: Some("web-1".into()),
            command: Some(command.into()),
            path: None,
            args: serde_json::Value::Null,
        }
    }

    fn file_call(path: &str) -> ToolCall {
        ToolCall {
            tool: ToolName::ReadFile,
            path: Some(path.into()),
            ..readonly_call("")
        }
    }

    /// `run_readonly` on a plain host, with the command really analyzed.
    fn decide(command: &str) -> Decision {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
        let engine = DefaultPolicyEngine::new(store);
        engine.decide(
            &operated(),
            &readonly_call(command),
            Some(&analyzed(command)),
            &plain_host(),
        )
    }

    #[test]
    fn deny_paths_covers_operands_redirections_and_globs() {
        for cmd in [
            // input redirections were not recorded at all before
            "cat < /etc/shadow",
            "cat 0</etc/shadow",
            "cat <>/etc/shadow",
            "{ cat; } < /etc/shadow",
            // programs that copy a deny path to stdout
            "base64 /etc/shadow",
            "xxd /etc/shadow",
            "od -c /etc/shadow",
            "hexdump -C /etc/shadow",
            "strings /etc/shadow",
            // unexpanded globs stand for the path they name
            "cat /etc/sha*",
            "cat /etc/shado?",
            "cat /etc/[s]hadow",
            "cat /etc/*",
            // a literal `cd` changes where relative arguments resolve
            "cd /etc && cat shadow",
            "cd /root && cat .ssh/id_rsa",
            // a relative path with no `cd` still matches the `**/` deny entries
            "cat .ssh/id_rsa",
            // home-relative arguments
            "cat ~/.ssh/id_rsa",
            "cat $HOME/.ssh/id_rsa",
            "cat ${HOME}/.ssh/id_rsa",
            "cat ~root/.ssh/id_ed25519",
            // values attached to options
            "diff --to-file=/etc/shadow /etc/hosts",
            "diff -f/etc/shadow /etc/hosts",
            // new default deny entries
            "cat /etc/krb5.keytab",
            "cat /etc/ssl/private/site.key",
            "cat /etc/ssh/ssh_host_rsa_key",
            "cat /root/.bashrc",
            "cat /proc/1/environ",
            "cat ~/.netrc",
            "cat ~/.pgpass",
            "cat ~/.kube/config",
            "cat ~/.docker/config.json",
            "cat /etc/rancher/k3s/config.yaml",
            "cat /etc/kubernetes/admin.conf",
            "cat /etc/security/opasswd",
            "cat /etc/sudoers",
            "cat /etc/gshadow-",
        ] {
            let d = decide(cmd);
            assert!(matches!(d, Decision::Deny { .. }), "{cmd:?}: {d:?}");
            assert!(
                reason(&d).starts_with("deny_paths:"),
                "{cmd:?}: {}",
                reason(&d)
            );
        }
    }

    #[test]
    fn unprovable_paths_ask_and_grants_do_not_cover_them() {
        // A glob that only *might* be a deny path is not provable: ask, never allow.
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
        store
            .add_grant(&Grant {
                grant_id: "g1".into(),
                run_id: None,
                tool: ToolName::RunReadonly,
                hosts: "*".into(),
                target: None,
                expires_at: (Utc::now() + chrono::Duration::minutes(30)).to_rfc3339(),
            })
            .unwrap();
        let engine = DefaultPolicyEngine::new(store);
        for (cmd, frag) in [
            ("cat /etc/ssh/*", "could expand"),
            ("cat $DIR/notes", "run time"),
            ("cat ${DIR:-/etc}/shadow", "run time"),
        ] {
            let d = engine.decide(
                &operated(),
                &readonly_call(cmd),
                Some(&analyzed(cmd)),
                &plain_host(),
            );
            assert!(matches!(d, Decision::Ask { .. }), "{cmd:?}: {d:?}");
            assert!(reason(&d).contains(frag), "{cmd:?}: {}", reason(&d));
        }
        // A `cd` we can't follow is never allowed: the `$D` target is a run-time path and
        // the relative `shadow` after it is unprovable.
        let cmd = "cd $D && cat shadow";
        let d = engine.decide(
            &operated(),
            &readonly_call(cmd),
            Some(&analyzed(cmd)),
            &plain_host(),
        );
        assert!(matches!(d, Decision::Ask { .. }), "{d:?}");
        assert!(
            reason(&d).starts_with("deny_paths:") || reason(&d).starts_with("analyzer:"),
            "{}",
            reason(&d)
        );
    }

    #[test]
    fn read_only_commands_without_secrets_still_pass() {
        for cmd in [
            "cat /etc/hosts",
            "cat /etc/os-release",
            "ls -la /var/log",
            "cat /etc/ssh/ssh_host_rsa_key.pub",
            "cat /etc/ssh/sshd_config",
            "journalctl -u nginx --since '1 hour ago'",
            "find /var/log -name '*.log' -mtime +7",
            "cd /tmp && ls",
            "diff /etc/hosts /etc/hostname",
            "grep -r error /var/log/nginx",
            "awk -F: '{print $1}' /etc/passwd",
        ] {
            let d = decide(cmd);
            assert!(
                matches!(d, Decision::Allow { .. }),
                "{cmd:?}: {d:?} ({})",
                reason(&d)
            );
        }
    }

    #[test]
    fn candidates_cover_option_values_and_home_paths() {
        let texts = |a: &Analysis| -> Vec<String> {
            path_candidates(Some(a))
                .0
                .into_iter()
                .map(|c| c.text)
                .collect()
        };
        let a = analyzed("diff --to-file=/etc/shadow -f/tmp/x a");
        let (cands, unsure) = path_candidates(Some(&a));
        let got: Vec<&str> = cands.iter().map(|c| c.text.as_str()).collect();
        assert!(got.contains(&"/etc/shadow"), "{got:?}");
        assert!(got.contains(&"/tmp/x"), "{got:?}");
        assert!(got.contains(&"a"), "{got:?}");
        assert!(unsure.is_none());

        let a = analyzed("cat ${HOME}/.ssh/id_rsa");
        assert_eq!(texts(&a), vec![".ssh/id_rsa".to_string()]);

        let a = analyzed("cat $DIR/x");
        let (_, unsure) = path_candidates(Some(&a));
        assert_eq!(unsure.as_deref(), Some("$DIR/x"));

        // `$(...)` is not a variable reference: the analyzer walks what is inside.
        let a = analyzed("cat $(pwd)");
        let (_, unsure) = path_candidates(Some(&a));
        assert!(unsure.is_none(), "{unsure:?}");
    }

    #[test]
    fn default_deny_paths_cover_secrets_and_keep_public_host_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
        let engine = DefaultPolicyEngine::new(store);
        let decide_path = |p: &str| engine.decide(&operated(), &file_call(p), None, &plain_host());
        for p in [
            "/etc/ssh/ssh_host_rsa_key",
            "/etc/ssh/ssh_host_ed25519_key",
            "/etc/krb5.keytab",
            "/var/lib/secrets/app.keytab",
            "/etc/ssl/private/site.key",
            "/etc/wireguard/wg0.conf",
            "/etc/gshadow-",
            "/etc/security/opasswd",
            "/etc/sudoers",
            "/root/.bashrc",
            "/home/u/.kube/config",
            "/etc/kubernetes/manifests/admin.conf",
            "/etc/rancher/k3s/config.yaml",
            "/home/u/.netrc",
            "/home/u/.pgpass",
            "/home/u/.my.cnf",
            "/home/u/.docker/config.json",
            "/srv/app/.git-credentials",
            "/proc/1/environ",
            "/proc/2/mem",
        ] {
            assert!(matches!(decide_path(p), Decision::Deny { .. }), "{p}");
        }
        for p in [
            "/etc/ssh/ssh_host_rsa_key.pub",
            "/etc/ssh/ssh_host_ed25519_key.pub",
            "/etc/ssh/sshd_config",
            "/etc/hostname",
        ] {
            assert!(
                matches!(decide_path(p), Decision::Allow { .. }),
                "{p}: {:?}",
                decide_path(p)
            );
        }
    }

    #[test]
    fn glob_matching_stays_polynomial() {
        // Backtracking over this many stars was exponential; the DP is O(pattern × text).
        let text = "a".repeat(20_000);
        let pattern = format!("{}b", "*a".repeat(40));
        let start = std::time::Instant::now();
        assert!(!glob_match(&pattern, &text, false));
        assert!(glob_match(&pattern, &format!("{text}b"), false));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn glob_classes() {
        assert!(glob_match("/etc/[s]hadow", "/etc/shadow", true));
        assert!(glob_match("/etc/[a-z]hadow", "/etc/shadow", true));
        assert!(glob_match("/etc/[!x]hadow", "/etc/shadow", true));
        assert!(!glob_match("/etc/[!s]hadow", "/etc/shadow", true));
        // An unclosed `[` is a literal bracket, as in the shell.
        assert!(glob_match("/etc/[x", "/etc/[x", true));
        assert!(!glob_match("/etc/[x", "/etc/x", true));
        // Globs stay globs for a literal deny path.
        let glob = |t: &str| Candidate {
            text: t.into(),
            glob: true,
        };
        assert!(deny_path_hit(&["/etc/shadow".into()], &glob("/etc/[s]hadow")).is_some());
        assert!(matches!(
            deny_path_hit(&["/etc/shadow".into()], &glob("/etc/shado?")),
            Some(PathHit::Deny(_))
        ));
        // Two globs only tell us a path is possible.
        assert!(matches!(
            deny_path_hit(&["**/id_rsa*".into()], &glob(".ssh/id_rs*")),
            Some(PathHit::Unsure(_))
        ));
        assert!(deny_path_hit(&["**/*.key".into()], &glob("/var/log/*.log")).is_none());
        // A quoted pattern is not expanded by the shell: it names no path.
        let literal = Candidate {
            text: "*.log".into(),
            glob: false,
        };
        assert!(deny_path_hit(&["**/id_rsa*".into()], &literal).is_none());
    }

    // ---- glob semantics

    /// The recursive matcher the DP one replaced, kept as the reference for its semantics.
    /// `[` classes are the one deliberate extension, so they stay out of the alphabet.
    fn old_rec(p: &[char], t: &[char], path_mode: bool) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => {
                let double = path_mode && p.get(1) == Some(&'*');
                if double {
                    if p.get(2) == Some(&'/') && old_rec(&p[3..], t, path_mode) {
                        return true;
                    }
                    (0..=t.len()).any(|i| old_rec(&p[2..], &t[i..], path_mode))
                } else {
                    let mut i = 0;
                    loop {
                        if old_rec(&p[1..], &t[i..], path_mode) {
                            return true;
                        }
                        if i >= t.len() || (path_mode && t[i] == '/') {
                            return false;
                        }
                        i += 1;
                    }
                }
            }
            Some('?') => match t.first() {
                Some(c) if !(path_mode && *c == '/') => old_rec(&p[1..], &t[1..], path_mode),
                _ => false,
            },
            Some(c) => t.first() == Some(c) && old_rec(&p[1..], &t[1..], path_mode),
        }
    }

    fn old_glob(pattern: &str, text: &str, path_mode: bool) -> bool {
        let p: Vec<char> = pattern.chars().collect();
        let t: Vec<char> = text.chars().collect();
        old_rec(&p, &t, path_mode)
    }

    #[test]
    fn new_matcher_agrees_with_the_old_one() {
        let alphabet = ['a', 'b', '/', '*', '?'];
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n as u64) as usize
        };
        for path_mode in [false, true] {
            for _ in 0..20_000 {
                let plen = next(7) + 1;
                let tlen = next(9);
                let pat: String = (0..plen).map(|_| alphabet[next(alphabet.len())]).collect();
                let text: String = (0..tlen).map(|_| alphabet[next(alphabet.len())]).collect();
                assert_eq!(
                    glob_match(&pat, &text, path_mode),
                    old_glob(&pat, &text, path_mode),
                    "pattern {pat:?} text {text:?} path_mode {path_mode}"
                );
            }
        }
    }
}
