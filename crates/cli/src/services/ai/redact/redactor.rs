//! `SecretRedactor`: applies the rules in their fixed order.

use std::collections::BTreeMap;

use crate::services::ai::types::{RedactRules, Redaction, Redactor};

use super::patterns::PATTERNS;
use super::rules::{
    apply_aws_secret, apply_bearer, apply_env_secrets, apply_key_value, apply_pattern,
    apply_url_credentials, marker,
};

pub(super) const MARKER_OPEN: &str = "«redacted:";
pub(super) const MARKER_CLOSE: &str = "»";

/// Kind counts, sorted by kind. All kinds are static strings.
pub(super) type Counts = BTreeMap<&'static str, usize>;

/// Exact values from the vault plus the pattern rules that are active.
pub struct SecretRedactor {
    vault_secrets: Vec<String>,
    rules: RedactRules,
}

impl SecretRedactor {
    /// `vault_secrets`: exact values to remove (passwords, tokens from the bssh vault).
    pub fn new(vault_secrets: Vec<String>, rules: &RedactRules) -> Self {
        Self {
            vault_secrets,
            rules: rules.clone(),
        }
    }

    /// Replace exact vault values, longest first so a value that contains a
    /// shorter one is removed whole. Values shorter than six characters are
    /// too likely to match ordinary text and are skipped.
    fn redact_vault(&self, text: &str, counts: &mut Counts) -> String {
        let mut secrets: Vec<&str> = self
            .vault_secrets
            .iter()
            .map(|secret| secret.trim())
            .filter(|secret| secret.chars().count() >= 6 && !secret.contains(MARKER_OPEN))
            .collect();
        // Longest first by bytes: a value that contains another is always longer.
        secrets.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| b.cmp(a)));
        secrets.dedup();

        let mut out = text.to_string();
        for secret in secrets {
            if secret.is_empty() || !out.contains(secret) {
                continue;
            }
            let hits = out.matches(secret).count();
            out = out.replace(secret, &marker("vault-secret"));
            *counts.entry("vault-secret").or_insert(0) += hits;
        }
        out
    }
}

impl Redactor for SecretRedactor {
    fn redact(&self, text: &str) -> (String, Vec<Redaction>) {
        let patterns = &*PATTERNS;
        let enabled = self.rules.patterns != "off";
        let mut counts = Counts::new();
        let mut out = text.to_string();

        if enabled {
            // Whole blocks first: their bodies must not be picked apart by the
            // token rules. A block with no END line is dropped to the end.
            out = apply_pattern(
                patterns.private_key.as_ref(),
                "private-key",
                &out,
                &mut counts,
            );
            out = apply_pattern(
                patterns.private_key_truncated.as_ref(),
                "private-key",
                &out,
                &mut counts,
            );
        }
        if self.rules.vault_secrets {
            out = self.redact_vault(&out, &mut counts);
        }
        if enabled {
            out = apply_pattern(patterns.aws_key.as_ref(), "aws-key", &out, &mut counts);
            out = apply_aws_secret(patterns.aws_secret.as_ref(), &out, &mut counts);
            out = apply_pattern(
                patterns.github_token.as_ref(),
                "github-token",
                &out,
                &mut counts,
            );
            out = apply_pattern(
                patterns.gitlab_token.as_ref(),
                "gitlab-token",
                &out,
                &mut counts,
            );
            out = apply_pattern(
                patterns.slack_token.as_ref(),
                "slack-token",
                &out,
                &mut counts,
            );
            out = apply_pattern(
                patterns.stripe_key.as_ref(),
                "stripe-key",
                &out,
                &mut counts,
            );
            out = apply_pattern(patterns.jwt.as_ref(), "jwt", &out, &mut counts);
            out = apply_bearer(patterns.bearer.as_ref(), &out, &mut counts);
            out = apply_url_credentials(patterns.url_credentials.as_ref(), &out, &mut counts);
            out = apply_key_value(patterns.key_value.as_ref(), &out, &mut counts);
            out = apply_env_secrets(patterns.env_line.as_ref(), &out, &mut counts);
        }

        let redactions = counts
            .into_iter()
            .map(|(kind, count)| Redaction {
                kind: kind.to_string(),
                count,
            })
            .collect();
        (out, redactions)
    }
}
