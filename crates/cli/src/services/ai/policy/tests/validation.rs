//! Pins the warnings for unknown policy fields and empty scopes.

use crate::config::AppConfig;
use crate::services::ai::policy::validation_warnings_in;

/// A config with one `default` policy written exactly as `json` says.
fn config_with(json: &str) -> AppConfig {
    let mut config = AppConfig::default_for_env("test");
    config.ai = serde_json::from_str(json).unwrap();
    config
}

#[test]
fn unknown_policy_fields_are_reported_not_ignored() {
    let raw = r#"{"ai":{"policies":{"default":{
        "profile":"operate",
        "scope":{"tags":["prod"],"host":{"web-1":true}},
        "approval":{"in-client":["prod"]},
        "read_path":["/var/log/**"]
    }}}}"#;
    let config =
        config_with(r#"{"policies":{"default":{"profile":"operate","scope":{"tags":["prod"]}}}}"#);
    let w = validation_warnings_in(Some(raw), &config, "default");
    assert!(
        w.iter()
            .any(|m| m.contains("'host'") && m.contains("scope")),
        "{w:?}"
    );
    assert!(
        w.iter()
            .any(|m| m.contains("'in-client'") && m.contains("approval")),
        "{w:?}"
    );
    assert!(
        w.iter()
            .any(|m| m.contains("'read_path'") && m.contains("known:")),
        "{w:?}"
    );
    // No empty-scope warning: scope.tags has an entry.
    assert!(!w.iter().any(|m| m.contains("empty scope")), "{w:?}");
}

#[test]
fn an_explicit_policy_with_an_empty_scope_warns_but_the_builtin_default_does_not() {
    let config = config_with(
        r#"{"policies":{"locked":{"profile":"operate"},"default":{"profile":"observe"}}}"#,
    );
    let w = validation_warnings_in(None, &config, "locked");
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("policy 'locked' has an empty scope"), "{w:?}");
    assert!(w[0].contains("\"*\""), "names the wildcard: {w:?}");

    // `default` is explicit here, so it is checked like any other policy.
    assert!(validation_warnings_in(None, &config, "default")
        .iter()
        .any(|m| m.contains("empty scope")));
    // With no explicit default, the built-in observe policy covers every host.
    let builtin = config_with(r#"{"policies":{}}"#);
    assert!(validation_warnings_in(None, &builtin, "default").is_empty());
}

#[test]
fn a_clean_policy_has_no_warnings() {
    let raw = r#"{"ai":{"policies":{"prod":{
        "profile":"operate",
        "scope":{"tags":["prod"],"hosts":["web-*"]},
        "approval":{"default":"out-of-band","in_client":["staging"],"two_person":["prod"]},
        "read_paths":["/srv/**"],
        "deny":["reboot"],
        "budgets":{"calls_per_min":10,"changes_per_run":2,"run_ttl":"1h","max_denials_per_min":3},
        "redact":{"vault_secrets":true,"patterns":"off"}
    }}}}"#;
    let config =
        config_with(r#"{"policies":{"prod":{"profile":"operate","scope":{"tags":["prod"]}}}}"#);
    assert_eq!(
        validation_warnings_in(Some(raw), &config, "prod"),
        Vec::<String>::new()
    );
}
