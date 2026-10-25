//! Policy engine: scope, deny > ask > allow, tiers, grants, budgets.
//!
//!
//! Order of evaluation in [`DefaultPolicyEngine::decide`] (first match wins, so deny always
//! beats ask and ask beats allow):
//!
//! 1. host out of scope, or the call has no host when it needs one: deny (counts as probing)
//! 2. budgets (`calls_per_min`, `changes_per_run`, `run_ttl`): deny
//! 3. profile `observe` and a write tool: deny
//! 4. `deny_paths` (file path, redirection targets, absolute path arguments): deny
//! 5. `deny` patterns on the analyzed commands: deny
//! 6. `write_paths` set and `edit_file` outside it: deny
//! 7. an active grant matches: allow
//! 8. read tools: allow (`read_file` only under `read_paths`, else ask);
//!    `run_readonly` / `run_many`: allow when the jail runs as the low-privilege agent
//!    principal (so file permissions enforce `deny_paths`), or the analysis is read-only,
//!    else ask
//! 9. write tools: ask, with the tier from the approval rules
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
    "/etc/sudoers.d/**",
    "**/*.key",
    "**/*.pem",
    "**/id_rsa*",
    "**/id_ed25519*",
    "**/.ssh/**",
    "**/.env",
    "**/.aws/credentials",
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

/// Glob match. `*` matches any run of characters, `?` one character.
/// With `path_mode`, `*` and `?` stop at `/`, `**` crosses it, and `**/` also matches
/// zero directories (`/etc/**/x` matches `/etc/x`).
pub(crate) fn glob_match(pattern: &str, text: &str, path_mode: bool) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_rec(&p, &t, path_mode)
}

fn glob_rec(p: &[char], t: &[char], path_mode: bool) -> bool {
    match p.first() {
        None => t.is_empty(),
        Some('*') => {
            let double = path_mode && p.get(1) == Some(&'*');
            if double {
                // `**/` may match zero directories.
                if p.get(2) == Some(&'/') && glob_rec(&p[3..], t, path_mode) {
                    return true;
                }
                (0..=t.len()).any(|i| glob_rec(&p[2..], &t[i..], path_mode))
            } else {
                let mut i = 0;
                loop {
                    if glob_rec(&p[1..], &t[i..], path_mode) {
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
            Some(c) if !(path_mode && *c == '/') => glob_rec(&p[1..], &t[1..], path_mode),
            _ => false,
        },
        Some(c) => t.first() == Some(c) && glob_rec(&p[1..], &t[1..], path_mode),
    }
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

        // 4. deny_paths
        let mut paths: Vec<&str> = Vec::new();
        if let Some(p) = &call.path {
            paths.push(p);
        }
        if let Some(a) = analysis {
            for c in &a.commands {
                paths.extend(c.writes_to.iter().map(String::as_str));
                paths.extend(
                    c.args
                        .iter()
                        .filter(|a| a.starts_with('/'))
                        .map(String::as_str),
                );
            }
        }
        for p in paths {
            if let Some(pat) = path_matches_any(&policy.deny_paths, p) {
                return deny(format!("deny_paths: '{p}' matches '{pat}'"));
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
                // A jail stops writes, but deny paths are only enforced when it runs as the
                // low-privilege agent principal: Landlock can only allow, and bubblewrap masks
                // literal deny paths only (not globs such as `**/.ssh/**`).
                if host.enforcement.jail != JailKind::None && host.enforcement.certificates {
                    return Decision::Allow {
                        reason: format!(
                            "jail: the host enforces a {:?} read-only jail and it runs as the \
                             low-privilege agent principal",
                            host.enforcement.jail
                        ),
                    };
                }
                let boundary = match host.enforcement.jail {
                    JailKind::None => "the host has no jail",
                    _ => "the host's jail cannot enforce deny_paths",
                };
                return match analysis {
                    Some(a) if a.is_read_only() => Decision::Allow {
                        reason: "analyzer: every command is read-only".into(),
                    },
                    Some(a) => ask(format!(
                        "analyzer: not provably read-only ({}) and {boundary}",
                        describe_tags(a)
                    )),
                    None => ask(format!("analyzer: no analysis and {boundary}")),
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

    /// The same host, but it trusts the bssh CA and has the `bssh-agent` user.
    fn host_trusting_ca(name: &str, tags: &[&str], jail: JailKind) -> HostFacts {
        HostFacts {
            enforcement: Enforcement {
                jail,
                certificates: true,
                agent_user: true,
            },
            ..host(name, tags, jail)
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
        let stg_bwrap = host("web-1", &["staging"], JailKind::Bubblewrap);
        let stg_agent = host_trusting_ca("web-1", &["staging"], JailKind::Landlock);
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
                "run_readonly jailed by bubblewrap as the login user",
                operate(),
                cmd_call(RunReadonly, "web-1", "rm x"),
                Some(wr.clone()),
                stg_bwrap.clone(),
                'K',
                "analyzer",
            ),
            (
                "run_readonly jailed as the agent principal",
                operate(),
                cmd_call(RunReadonly, "web-1", "rm x"),
                Some(wr.clone()),
                stg_agent.clone(),
                'A',
                "jail",
            ),
            (
                "run_readonly landlock as the login user asks",
                operate(),
                cmd_call(RunReadonly, "web-1", "rm x"),
                Some(wr.clone()),
                stg_jail.clone(),
                'K',
                "analyzer",
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
                "run_many jailed as the agent principal",
                operate(),
                cmd_call(RunMany, "web-1", "uptime"),
                None,
                stg_agent.clone(),
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
    fn a_jail_auto_allows_only_when_it_enforces_deny_paths() {
        let fx = fixture();
        let login = host("web-1", &["staging"], JailKind::Landlock);
        let agent = host_trusting_ca("web-1", &["staging"], JailKind::Landlock);
        let bwrap = host("web-1", &["staging"], JailKind::Bubblewrap);
        let bwrap_agent = host_trusting_ca("web-1", &["staging"], JailKind::Bubblewrap);
        let c = cmd_call(ToolName::RunReadonly, "web-1", "rm x");
        let wr = analysis("rm", &["x"], &[RiskTag::Destructive]);

        // As the login user, Landlock can't mask deny_paths and bubblewrap only masks literal
        // ones: the analyzer decides.
        for h in [&login, &bwrap] {
            let d = fx.engine.decide(&operate(), &c, Some(&wr), h);
            assert!(matches!(d, Decision::Ask { .. }), "{h:?}: {d:?}");
            assert!(
                reason(&d).contains("cannot enforce deny_paths"),
                "{}",
                reason(&d)
            );
        }
        // The low-privilege agent principal does provide it, with either jail.
        for h in [&agent, &bwrap_agent] {
            let d = fx.engine.decide(&operate(), &c, Some(&wr), h);
            assert!(is_allow(&d), "{h:?}: {d:?}");
            assert!(reason(&d).starts_with("jail:"), "{}", reason(&d));
        }
        // A provably read-only command still runs under the login-user Landlock jail.
        let ro = analysis("cat", &["/etc/hosts"], &[RiskTag::Read]);
        let d = fx.engine.decide(&operate(), &c, Some(&ro), &login);
        assert!(is_allow(&d), "{d:?}");
        assert!(reason(&d).starts_with("analyzer:"), "{}", reason(&d));
    }

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
}
