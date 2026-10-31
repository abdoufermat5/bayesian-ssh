//! Warnings for policy fields serde would silently drop, and for empty scopes.

use crate::config::AppConfig;

use super::profile::resolve;

/// Fields serde reads from [`Policy`](super::super::types::Policy); anything else in the file is ignored.
const POLICY_FIELDS: &[&str] = &[
    "profile",
    "scope",
    "approval",
    "read_paths",
    "deny_paths",
    "write_paths",
    "deny",
    "budgets",
    "redact",
];
/// Fields serde reads from [`Scope`](super::super::types::Scope).
const SCOPE_FIELDS: &[&str] = &["tags", "hosts"];
/// Fields serde reads from [`ApprovalRules`](super::super::types::ApprovalRules).
const APPROVAL_FIELDS: &[&str] = &["default", "in_client", "two_person", "two_person_hosts"];

/// Problems in `name` as written in the environment's `config.json` that serde would
/// silently drop: unknown fields (a typo like `scopes` or `in-client`) and a policy with an
/// empty scope, which reaches no host at all.
///
/// [`Policy`](super::super::types::Policy) does not use `deny_unknown_fields` because that turns a typo into a config
/// that will not load at all; these warnings are shown by `bssh ai policy show`,
/// `bssh ai doctor` and at `bssh ai mcp` startup instead.
pub fn validation_warnings(config: &AppConfig, name: &str) -> Vec<String> {
    let raw = std::fs::read_to_string(config.config_file()).ok();
    validation_warnings_in(raw.as_deref(), config, name)
}

/// [`validation_warnings`] with the config file's text passed in (tests).
pub fn validation_warnings_in(raw: Option<&str>, config: &AppConfig, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let written = raw
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| {
            v.get("ai")
                .and_then(|ai| ai.get("policies"))
                .and_then(|p| p.get(name))
                .cloned()
        });
    if let Some(p) = &written {
        let root = format!("ai.policies.{name}");
        unknown_fields(name, p, POLICY_FIELDS, &root, &mut out);
        for (field, known) in [("scope", SCOPE_FIELDS), ("approval", APPROVAL_FIELDS)] {
            if let Some(inner) = p.get(field) {
                unknown_fields(name, inner, known, &format!("{root}.{field}"), &mut out);
            }
        }
    }
    // Only a policy written by hand can have an empty scope: the built-in default covers
    // every host.
    if config.ai.policies.contains_key(name) {
        if let Ok(p) = resolve(config, name) {
            if p.scope.tags.is_empty() && p.scope.hosts.is_empty() {
                out.push(format!(
                    "policy '{name}' has an empty scope, so it reaches no host: add scope.tags, \
                     scope.hosts or \"*\" for every host"
                ));
            }
        }
    }
    out
}

fn unknown_fields(
    policy: &str,
    value: &serde_json::Value,
    known: &[&str],
    path: &str,
    out: &mut Vec<String>,
) {
    let Some(map) = value.as_object() else {
        return;
    };
    for key in map.keys() {
        if !known.contains(&key.as_str()) {
            out.push(format!(
                "policy '{policy}': unknown field '{key}' in {path}; it is ignored (known: {})",
                known.join(", ")
            ));
        }
    }
}
