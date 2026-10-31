//! Built-in profiles and policy resolution.

use super::scope::ALL_HOSTS;
use crate::config::AppConfig;
use crate::services::ai::types::{Budgets, Policy, Profile, Scope};

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
