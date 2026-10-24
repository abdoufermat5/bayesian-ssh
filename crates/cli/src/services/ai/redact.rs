//! Secret redaction before output reaches the model.
//!
//! Owner: W2. Stub from the scaffold; replace freely.

use super::types::{RedactRules, Redaction, Redactor};

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
}

impl Redactor for SecretRedactor {
    fn redact(&self, text: &str) -> (String, Vec<Redaction>) {
        let _ = (&self.vault_secrets, &self.rules);
        (text.to_string(), Vec::new())
    }
}
