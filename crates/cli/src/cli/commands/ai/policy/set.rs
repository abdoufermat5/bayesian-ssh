//! `bssh ai policy set`: write a policy into this environment's `config.json`.

use super::render::render_show;
use crate::config::AppConfig;
use crate::services::ai::policy;
use crate::services::ai::types::{Profile, Scope};
use crate::services::ai::ui::{Style, Tone};
use anyhow::{Context, Result, bail};

/// What `bssh ai policy set` was asked to write.
pub(super) struct SetPolicy<'a> {
    pub(super) name: &'a str,
    pub(super) profile: Option<&'a str>,
    pub(super) scope: ScopeArgs,
}

/// The `--tag`, `--host` and `--all-hosts` flags, validated together.
#[derive(Debug, Clone, Default)]
pub(super) struct ScopeArgs {
    pub(super) tags: Vec<String>,
    pub(super) hosts: Vec<String>,
    pub(super) all_hosts: bool,
}

/// The scope the flags ask for.
enum ScopeSelection {
    /// `--all-hosts`: every host, expressed as the `*` tag.
    AllHosts,
    /// `--tag` and/or `--host`: replaces the stored scope.
    Named(Scope),
    /// Neither: keep the stored scope.
    Stored,
}

impl ScopeArgs {
    /// # Errors
    ///
    /// Fails when `--all-hosts` is combined with tags or hosts.
    fn selection(self) -> Result<ScopeSelection> {
        if self.all_hosts {
            if !self.tags.is_empty() || !self.hosts.is_empty() {
                bail!(
                    "give either --all-hosts or --tag/--host, not both: --all-hosts already \
                     covers every host"
                );
            }
            return Ok(ScopeSelection::AllHosts);
        }
        if self.tags.is_empty() && self.hosts.is_empty() {
            Ok(ScopeSelection::Stored)
        } else {
            Ok(ScopeSelection::Named(Scope {
                tags: self.tags,
                hosts: self.hosts,
            }))
        }
    }
}

/// `bssh ai policy set`: write a policy into this environment's `config.json`.
///
/// Only the profile and the scope are set: the approval rules, paths and budgets of an
/// existing policy stay as written, and a new one starts from its profile's defaults. Saving
/// goes through `AppConfig::save`, the same atomic write every other config change uses.
///
/// # Errors
///
/// Fails on an empty name, on `--all-hosts` combined with `--tag`/`--host`, on a new policy
/// without `--profile`, on an unknown profile, on a scope that reaches no host, or when the
/// config cannot be written.
pub(super) fn set_policy(
    style: &Style,
    config: &mut AppConfig,
    request: SetPolicy<'_>,
) -> Result<()> {
    let name = request.name.trim();
    if name.is_empty() {
        bail!("give the policy a name, for example `default`");
    }
    let selection = request.scope.selection()?;
    let existing = config.ai.policies.get(name).cloned();
    let profile = match request.profile {
        Some(p) => parse_profile(p)?,
        None => existing
            .as_ref()
            .map(|p| p.profile.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "give --profile for the new policy '{name}': observe, operate or oncall"
                )
            })?,
    };
    let mut pol = existing.unwrap_or_default();
    pol.profile = profile;
    match selection {
        ScopeSelection::AllHosts => {
            pol.scope = Scope {
                tags: vec![policy::ALL_HOSTS.to_string()],
                hosts: Vec::new(),
            };
        }
        ScopeSelection::Named(scope) => pol.scope = scope,
        ScopeSelection::Stored => {
            // Keep a scope that already reaches hosts; refuse to write one that reaches none.
            if pol.scope.tags.is_empty() && pol.scope.hosts.is_empty() {
                bail!(
                    "policy '{name}' would reach no host: add --tag <TAG>, --host <HOST> or \
                     --all-hosts"
                );
            }
        }
    }
    config.ai.policies.insert(name.to_string(), pol);
    config
        .save()
        .with_context(|| format!("write {}", config.config_file().display()))?;

    // Show what was written, with the profile's defaults applied.
    let saved = policy::resolve(config, name)?;
    print!(
        "{}",
        render_show(style, name, &saved, &config.ai.trusted_tag_sources)
    );
    for warning in policy::validation_warnings(config, name) {
        println!("  {}  {warning}", style.paint(Tone::Warn, "warn"));
    }
    println!(
        "\nSaved to {}. Restart your agent to apply it.",
        config.config_file().display()
    );
    Ok(())
}

fn parse_profile(value: &str) -> Result<Profile> {
    match value.trim().to_ascii_lowercase().as_str() {
        "observe" => Ok(Profile::Observe),
        "operate" => Ok(Profile::Operate),
        "oncall" => Ok(Profile::Oncall),
        other => bail!("unknown profile '{other}': use observe, operate or oncall"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request<'a>(name: &'a str, profile: Option<&'a str>, scope: ScopeArgs) -> SetPolicy<'a> {
        SetPolicy {
            name,
            profile,
            scope,
        }
    }

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    /// Every refusal below happens before the config is written.
    #[test]
    fn set_policy_refuses_a_scope_that_reaches_nothing_and_bad_flags() {
        let style = Style::plain();
        let mut config = AppConfig::default_for_env("bssh-test-no-write");
        let err = set_policy(
            &style,
            &mut config,
            request("locked", Some("operate"), ScopeArgs::default()),
        )
        .unwrap_err();
        assert!(err.to_string().contains("would reach no host"), "{err}");
        assert!(
            err.to_string().contains("--all-hosts"),
            "names the way out: {err}"
        );

        let err = set_policy(
            &style,
            &mut config,
            request("default", None, ScopeArgs::default()),
        )
        .unwrap_err();
        assert!(err.to_string().contains("give --profile"), "{err}");

        let err = set_policy(
            &style,
            &mut config,
            request("default", Some("admin"), ScopeArgs::default()),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("use observe, operate or oncall"),
            "{err}"
        );

        let err = set_policy(
            &style,
            &mut config,
            request(
                "default",
                Some("operate"),
                ScopeArgs {
                    tags: tags(&["prod"]),
                    hosts: Vec::new(),
                    all_hosts: true,
                },
            ),
        )
        .unwrap_err();
        assert!(err.to_string().contains("either --all-hosts"), "{err}");
    }

    #[test]
    fn set_policy_refuses_an_empty_name_first() {
        let err = set_policy(
            &Style::plain(),
            &mut AppConfig::default_for_env("bssh-test-no-write"),
            request(
                "  ",
                None,
                ScopeArgs {
                    all_hosts: true,
                    ..Default::default()
                },
            ),
        )
        .unwrap_err();
        assert!(err.to_string().contains("give the policy a name"), "{err}");
    }

    #[test]
    fn parse_profile_accepts_known_profiles_only() {
        assert_eq!(parse_profile(" Observe ").unwrap(), Profile::Observe);
        assert_eq!(parse_profile("operate").unwrap(), Profile::Operate);
        assert_eq!(parse_profile("ONCALL").unwrap(), Profile::Oncall);
        let err = parse_profile("admin").unwrap_err();
        assert_eq!(
            err.to_string(),
            "unknown profile 'admin': use observe, operate or oncall"
        );
    }
}
