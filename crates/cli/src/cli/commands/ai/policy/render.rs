//! Rendering of `bssh ai policy list` and `bssh ai policy show`.

use crate::services::ai::types::{Policy, Profile, Scope, Tier};
use crate::services::ai::ui::{self, Style, Tone};
use std::collections::BTreeMap;

/// `bssh ai policy list`: one policy per line, plus the built-in default.
pub(super) fn render_list(style: &Style, policies: &BTreeMap<String, Policy>) -> String {
    // "default" is seven characters, the same as the minimum column width below.
    let width = policies
        .keys()
        .map(|name| name.chars().count())
        .max()
        .unwrap_or(0)
        .max(7);
    let mut out = String::new();
    for (name, policy) in policies {
        out.push_str(&format!(
            "{}  {}  {}\n",
            ui::pad(name, width),
            ui::pad(profile_label(&policy.profile), 8),
            scope_summary(style, &policy.scope)
        ));
    }
    if !policies.contains_key("default") {
        out.push_str(&format!(
            "{}  {}  (built-in)\n",
            ui::pad("default", width),
            ui::pad("observe", 8)
        ));
    }
    out
}

/// `bssh ai policy show`: human-readable policy details.
pub(super) fn render_show(
    style: &Style,
    name: &str,
    policy: &Policy,
    trusted_tag_sources: &[String],
) -> String {
    let mut out = String::new();
    let header = style.facts(&[
        &format!("policy {name}"),
        &format!("profile {}", profile_label(&policy.profile)),
    ]);
    out.push_str(&format!("{header}\n\n"));

    let label_w = 10;
    let mut row = |label: &str, val: &str| {
        let l = style.paint(Tone::Dim, label);
        out.push_str(&format!("  {}  {val}\n", ui::pad(&l, label_w)));
    };

    row("scope", &scope_summary(style, &policy.scope));

    let default_tier = match policy.approval.default {
        Tier::InClient => "in-client",
        Tier::OutOfBand => "out-of-band",
        Tier::TwoPerson => "two-person",
    };
    let mut app_parts = vec![format!("default: {default_tier}")];
    if !policy.approval.in_client.is_empty() {
        app_parts.push(format!(
            "in-client on tags: {}",
            policy.approval.in_client.join(", ")
        ));
    }
    if !policy.approval.two_person.is_empty() {
        app_parts.push(format!(
            "two-person on tags: {}",
            policy.approval.two_person.join(", ")
        ));
    }
    if !policy.approval.two_person_hosts.is_empty() {
        app_parts.push(format!(
            "two-person on hosts: {}",
            policy.approval.two_person_hosts.join(", ")
        ));
    }
    let app_facts: Vec<&str> = app_parts.iter().map(String::as_str).collect();
    row("approval", &style.facts(&app_facts));

    let tag_trust = if trusted_tag_sources.is_empty() {
        "hand-added connections only".to_string()
    } else {
        format!(
            "hand-added connections · sources: {}",
            trusted_tag_sources.join(", ")
        )
    };
    row("tag trust", &tag_trust);

    let reads = if policy.read_paths.is_empty() {
        "any in scope".to_string()
    } else {
        policy.read_paths.join(", ")
    };
    row("reads", &reads);

    let writes = if policy.write_paths.is_empty() {
        "none".to_string()
    } else {
        policy.write_paths.join(", ")
    };
    row("writes", &writes);

    let denies_str = if policy.deny_paths.is_empty() && policy.deny.is_empty() {
        "none".to_string()
    } else {
        let mut denies: Vec<&str> = policy.deny_paths.iter().map(String::as_str).collect();
        denies.extend(policy.deny.iter().map(String::as_str));
        denies.join(", ")
    };
    row("denies", &denies_str);

    let budgets_str = style.facts(&[
        &format!("{} calls/min", policy.budgets.calls_per_min),
        &format!("{} changes/run", policy.budgets.changes_per_run),
        &format!("{} denials/min", policy.budgets.max_denials_per_min),
        &format!("{} ttl", policy.budgets.run_ttl),
    ]);
    row("budgets", &budgets_str);

    let redact_v = if policy.redact.vault_secrets {
        "vault secrets: yes"
    } else {
        "vault secrets: no"
    };
    let redact_p = format!("patterns: {}", policy.redact.patterns);
    row("redact", &style.facts(&[redact_v, &redact_p]));

    out
}

fn profile_label(profile: &Profile) -> &'static str {
    match profile {
        Profile::Observe => "observe",
        Profile::Operate => "operate",
        Profile::Oncall => "oncall",
    }
}

fn scope_summary(style: &Style, scope: &Scope) -> String {
    let mut parts = Vec::new();
    if !scope.tags.is_empty() {
        parts.push(format!("tags: {}", scope.tags.join(", ")));
    }
    if !scope.hosts.is_empty() {
        parts.push(format!("hosts: {}", scope.hosts.join(", ")));
    }
    if parts.is_empty() {
        "no hosts".to_string()
    } else {
        style.facts(&parts.iter().map(String::as_str).collect::<Vec<_>>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_shows_the_builtin_default_when_missing() {
        let text = render_list(&Style::plain(), &BTreeMap::new());
        assert_eq!(text, "default  observe   (built-in)\n");
    }

    #[test]
    fn list_shows_profile_and_scope() {
        let mut policies = BTreeMap::new();
        let mut policy = Policy {
            profile: Profile::Operate,
            ..Policy::default()
        };
        policy.scope.tags = vec!["prod".to_string()];
        policy.scope.hosts = vec!["web-1".to_string(), "db-1".to_string()];
        policies.insert("prod".to_string(), policy);
        let text = render_list(&Style::plain(), &policies);
        assert_eq!(
            text,
            "prod     operate   tags: prod · hosts: web-1, db-1\ndefault  observe   (built-in)\n"
        );
    }

    #[test]
    fn list_omits_builtin_when_default_exists() {
        let mut policies = BTreeMap::new();
        policies.insert("default".to_string(), Policy::default());
        policies.insert(
            "oncall".to_string(),
            Policy {
                profile: Profile::Oncall,
                ..Policy::default()
            },
        );
        let text = render_list(&Style::plain(), &policies);
        assert!(text.contains("default  observe   no hosts\n"));
        assert!(text.contains("oncall   oncall    no hosts\n"));
        assert!(!text.contains("(built-in)"));
    }

    #[test]
    fn list_colored_has_dim_dot() {
        let mut policies = BTreeMap::new();
        let mut policy = Policy::default();
        policy.scope.tags = vec!["prod".to_string()];
        policy.scope.hosts = vec!["web-1".to_string()];
        policies.insert("prod".to_string(), policy);
        let style = Style {
            color: true,
            unicode: true,
        };
        let text = render_list(&style, &policies);
        assert!(text.contains("\x1b[90m·\x1b[0m"));
    }

    #[test]
    fn list_pads_to_the_longest_name_or_seven() {
        let mut policies = BTreeMap::new();
        policies.insert("a-very-long-policy-name".to_string(), Policy::default());
        let text = render_list(&Style::plain(), &policies);
        assert!(text.starts_with("a-very-long-policy-name  observe   no hosts\n"));
    }

    #[test]
    fn scope_summary_empty() {
        assert_eq!(
            scope_summary(&Style::plain(), &Scope::default()),
            "no hosts"
        );
    }

    #[test]
    fn show_formats_policy_fields() {
        let mut policy = Policy {
            profile: Profile::Operate,
            ..Policy::default()
        };
        policy.scope.tags = vec!["prod".to_string()];
        policy.scope.hosts = vec!["web-1".to_string()];
        policy.approval.in_client = vec!["staging".to_string()];
        policy.approval.two_person = vec!["prod".to_string()];
        policy.approval.two_person_hosts = vec!["db-*".to_string()];
        policy.read_paths = vec!["/var/log/*".to_string()];
        policy.write_paths = vec!["/var/www/*".to_string()];
        policy.deny_paths = vec!["/etc/shadow".to_string()];
        policy.deny = vec!["reboot".to_string()];

        let text = render_show(&Style::plain(), "prod", &policy, &["aws-prod".to_string()]);
        assert!(text.contains("policy prod · profile operate"));
        assert!(text.contains("scope       tags: prod · hosts: web-1"));
        assert!(text.contains("approval    default: out-of-band · in-client on tags: staging · two-person on tags: prod · two-person on hosts: db-*"));
        assert!(text.contains("tag trust   hand-added connections · sources: aws-prod"));
        assert!(text.contains("reads       /var/log/*"));
        assert!(text.contains("writes      /var/www/*"));
        assert!(text.contains("denies      /etc/shadow, reboot"));
        assert!(text.contains("budgets     30 calls/min · 10 changes/run · 5 denials/min · 2h ttl"));
        assert!(text.contains("redact      vault secrets: yes · patterns: default"));
    }

    #[test]
    fn show_lists_deny_paths_before_deny_programs() {
        let policy = Policy {
            deny_paths: vec!["/etc/shadow".to_string()],
            deny: vec!["reboot".to_string()],
            ..Policy::default()
        };
        let text = render_show(&Style::plain(), "default", &policy, &[]);
        assert!(text.contains("denies      /etc/shadow, reboot"));
    }

    #[test]
    fn show_colored_has_ansi_escapes() {
        let policy = Policy::default();
        let style = Style {
            color: true,
            unicode: true,
        };
        let text = render_show(&style, "default", &policy, &[]);
        assert!(text.contains("\x1b[90mscope\x1b[0m"));
        assert!(text.contains("\x1b[90mbudgets\x1b[0m"));
        assert!(text.contains("\x1b[90mtag trust\x1b[0m"));
    }

    #[test]
    fn policy_json_round_trips() {
        let policy = Policy {
            profile: Profile::Operate,
            ..Policy::default()
        };
        let json = serde_json::to_string_pretty(&policy).unwrap();
        let back: Policy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, policy);
    }
}
