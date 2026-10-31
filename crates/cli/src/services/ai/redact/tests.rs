use super::patterns::PATTERNS;
use super::rules::marker;
use super::*;

use crate::services::ai::types::{RedactRules, Redaction, Redactor};
use std::time::{Duration, Instant};

// Synthetic values only: built here, never real credentials.

fn fake_aws_key() -> String {
    format!("AKIA{}", "IOSFODNN7EXAMPLE")
}

fn fake_aws_secret() -> String {
    "Ab1/".repeat(10)
}

fn fake_github_token() -> String {
    format!("ghp_{}", "Ab1".repeat(12))
}

fn fake_github_pat() -> String {
    format!("github_pat_{}", "A1b2C3d4E5".repeat(3))
}

fn fake_gitlab_token() -> String {
    format!("glpat-{}", "Ab1_xy".repeat(4))
}

fn fake_slack_token() -> String {
    // Assembled at run time so secret scanners do not flag the fixture.
    format!("xoxb-{}-{}", "1234".repeat(3), "abcd".repeat(6))
}

fn fake_stripe_key() -> String {
    format!("sk_live_{}", "Ab1".repeat(8))
}

fn fake_jwt() -> &'static str {
    "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
     eyJzdWIiOiIxMjM0NTY3ODkwIn0.\
     dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"
}

fn fake_bearer() -> String {
    "Zx9-._~+/AbCdEfGhIjKlMnOpQrStUvWx".to_string()
}

fn fake_value() -> &'static str {
    "Vh8kQ2mN4pQ7"
}

fn default_redactor() -> SecretRedactor {
    SecretRedactor::new(Vec::new(), &RedactRules::default())
}

fn redact(text: &str) -> (String, Vec<Redaction>) {
    default_redactor().redact(text)
}

fn with_vault(secrets: &[&str]) -> SecretRedactor {
    SecretRedactor::new(
        secrets.iter().map(|s| (*s).to_string()).collect(),
        &RedactRules::default(),
    )
}

fn kinds(redactions: &[Redaction]) -> Vec<(&str, usize)> {
    redactions
        .iter()
        .map(|r| (r.kind.as_str(), r.count))
        .collect()
}

fn assert_redacted(text: &str, kind: &str) {
    let (out, redactions) = redact(text);
    let expected = marker(kind);
    assert!(
        out.contains(&expected),
        "expected {expected:?} in {out:?} for input {text:?}"
    );
    assert_eq!(kinds(&redactions), vec![(kind, 1)], "input {text:?}");
}

fn assert_untouched(text: &str) {
    let (out, redactions) = redact(text);
    assert_eq!(out, text, "input {text:?}");
    assert!(redactions.is_empty(), "input {text:?}: {redactions:?}");
}

// ------------------------------------------------------------ patterns

#[test]
fn every_pattern_compiles() {
    let p = &*PATTERNS;
    let rules = [
        ("private-key", p.private_key.is_some()),
        ("private-key-truncated", p.private_key_truncated.is_some()),
        ("aws-key", p.aws_key.is_some()),
        ("aws-secret", p.aws_secret.is_some()),
        ("github-token", p.github_token.is_some()),
        ("gitlab-token", p.gitlab_token.is_some()),
        ("slack-token", p.slack_token.is_some()),
        ("stripe-key", p.stripe_key.is_some()),
        ("jwt", p.jwt.is_some()),
        ("bearer", p.bearer.is_some()),
        ("url-credentials", p.url_credentials.is_some()),
        ("key-value", p.key_value.is_some()),
        ("env-secret", p.env_line.is_some()),
    ];
    for (rule, present) in rules {
        assert!(present, "rule {rule} did not compile");
    }
}

// ------------------------------------------------------------ aws

#[test]
fn aws_access_key_detected() {
    let text = format!("export KEY={}", fake_aws_key());
    assert_redacted(&text, "aws-key");
}

#[test]
fn aws_access_key_inside_a_word_is_not_matched() {
    assert_untouched(&format!("x{}", fake_aws_key()));
}

#[test]
fn aws_secret_access_key_value_detected() {
    let text = format!("aws_secret_access_key = {}", fake_aws_secret());
    let (out, redactions) = redact(&text);
    assert_eq!(
        out,
        format!("aws_secret_access_key = {}", marker("aws-secret"))
    );
    assert_eq!(kinds(&redactions), vec![("aws-secret", 1)]);
}

// ------------------------------------------------------------ private keys

fn pem_block(label: &str, body: &str) -> String {
    format!("-----BEGIN {label}-----\n{body}\n-----END {label}-----")
}

#[test]
fn rsa_private_key_block_replaced_whole() {
    let body = "MIIEowIBAAKCAQEAxFq7b1n0FAKEkeyFAKEkey0123456789abcdefghijklm";
    let pem = pem_block("RSA PRIVATE KEY", body);
    let (out, redactions) = redact(&format!("before\n{pem}\nafter"));
    assert_eq!(out, format!("before\n{}\nafter", marker("private-key")));
    assert!(!out.contains(body));
    assert_eq!(kinds(&redactions), vec![("private-key", 1)]);
}

#[test]
fn ec_private_key_block_detected() {
    let pem = pem_block("EC PRIVATE KEY", "MHcCAQEEIFakeECFakeECFakeECFakeECFakeEC");
    assert_redacted(&pem, "private-key");
}

#[test]
fn openssh_private_key_block_detected() {
    let pem = pem_block(
        "OPENSSH PRIVATE KEY",
        "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ",
    );
    assert_redacted(&pem, "private-key");
}

#[test]
fn encrypted_private_key_block_detected() {
    let pem = pem_block("ENCRYPTED PRIVATE KEY", &"MIIE".repeat(16));
    assert_redacted(&pem, "private-key");
}

#[test]
fn pgp_private_key_block_detected() {
    let pem = pem_block("PGP PRIVATE KEY BLOCK", "lQOYBFakeFakeFakeFakeFakeFakeFake");
    assert_redacted(&pem, "private-key");
}

#[test]
fn truncated_private_key_block_redacts_to_the_end() {
    let text = "prefix\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmU";
    let (out, redactions) = redact(text);
    assert_eq!(out, format!("prefix\n{}", marker("private-key")));
    assert_eq!(kinds(&redactions), vec![("private-key", 1)]);
}

// ------------------------------------------------------------ tokens

#[test]
fn github_classic_token_detected() {
    assert_redacted(&format!("token: {}", fake_github_token()), "github-token");
}

#[test]
fn github_fine_grained_token_detected() {
    assert_redacted(&format!("token: {}", fake_github_pat()), "github-token");
}

#[test]
fn short_github_token_ignored() {
    assert_untouched("ghp_shorttoken");
}

#[test]
fn gitlab_token_detected() {
    assert_redacted(&format!("TOKEN={}", fake_gitlab_token()), "gitlab-token");
}

#[test]
fn short_gitlab_token_ignored() {
    assert_untouched("glpat-tooshort");
}

#[test]
fn slack_token_detected() {
    assert_redacted(&format!("SLACK={}", fake_slack_token()), "slack-token");
}

#[test]
fn stripe_key_detected() {
    assert_redacted(&format!("STRIPE={}", fake_stripe_key()), "stripe-key");
}

#[test]
fn jwt_detected() {
    assert_redacted(&format!("authorization payload {}", fake_jwt()), "jwt");
}

#[test]
fn bearer_token_keeps_the_header_prefix() {
    let text = format!("Authorization: Bearer {}", fake_bearer());
    let (out, redactions) = redact(&text);
    assert_eq!(out, format!("Authorization: Bearer {}", marker("bearer")));
    assert_eq!(kinds(&redactions), vec![("bearer", 1)]);
}

#[test]
fn bare_bearer_token_detected() {
    assert_redacted(&format!("sent Bearer {}", fake_bearer()), "bearer");
}

#[test]
fn short_bearer_value_ignored() {
    assert_untouched("Authorization: Bearer short");
}

#[test]
fn basic_auth_is_left_alone() {
    // Decision: only the Bearer scheme is redacted. Basic credentials are a
    // blob bssh cannot tell apart from other base64, and vault values cover
    // the exact secrets; the design names Bearer only (section 8).
    assert_untouched("Authorization: Basic dXNlcjpwYXNz");
}

// ------------------------------------------------------------ urls

#[test]
fn url_credentials_keep_scheme_user_and_host() {
    let text = "postgres://app_user:s3cr3t-P4ss@db.internal:5432/app";
    let (out, redactions) = redact(text);
    assert_eq!(
        out,
        "postgres://app_user:«redacted:url-credentials»@db.internal:5432/app"
    );
    assert_eq!(kinds(&redactions), vec![("url-credentials", 1)]);
}

#[test]
fn url_without_credentials_untouched() {
    assert_untouched("https://example.com:8443/healthz");
}

#[test]
fn url_credentials_are_idempotent() {
    let text = "ssh://deploy:Vh8kQ2mN4p@host.example";
    let (first, _) = redact(text);
    let (second, redactions) = redact(&first);
    assert_eq!(second, first);
    assert!(redactions.is_empty());
}

// ------------------------------------------------------------ key=value

#[test]
fn yaml_password_detected() {
    let text = format!("password: {}", fake_value());
    let (out, redactions) = redact(&text);
    assert_eq!(out, format!("password: {}", marker("password")));
    assert_eq!(kinds(&redactions), vec![("password", 1)]);
}

#[test]
fn json_password_keeps_quotes() {
    let text = format!("{{\"password\": \"{}\"}}", fake_value());
    let (out, redactions) = redact(&text);
    assert_eq!(out, format!("{{\"password\": \"{}\"}}", marker("password")));
    assert_eq!(kinds(&redactions), vec![("password", 1)]);
}

#[test]
fn cli_password_flag_detected() {
    let text = format!("--password={}", fake_value());
    let (out, redactions) = redact(&text);
    assert_eq!(out, format!("--password={}", marker("password")));
    assert_eq!(kinds(&redactions), vec![("password", 1)]);
}

#[test]
fn db_password_env_var_detected() {
    let text = format!("DB_PASSWORD={}", fake_value());
    let (out, redactions) = redact(&text);
    assert_eq!(out, format!("DB_PASSWORD={}", marker("password")));
    assert_eq!(kinds(&redactions), vec![("password", 1)]);
}

#[test]
fn client_secret_detected_as_secret() {
    let text = format!("client_secret = \"{}\"", fake_value());
    let (out, redactions) = redact(&text);
    assert_eq!(out, format!("client_secret = \"{}\"", marker("secret")));
    assert_eq!(kinds(&redactions), vec![("secret", 1)]);
}

#[test]
fn uppercase_secret_key_detected() {
    let text = format!("SECRET_KEY={}", fake_value());
    assert_redacted(&text, "secret");
}

#[test]
fn api_key_with_high_entropy_detected_as_secret() {
    let text = format!("api_key = {}", fake_value());
    assert_redacted(&text, "secret");
}

#[test]
fn api_key_with_low_entropy_ignored() {
    assert_untouched("api_key = abc123");
}

#[test]
fn placeholder_password_values_ignored() {
    for text in [
        "password: changeme",
        "password: null",
        "password: ***",
        "password: <your-password>",
        "password: your_password",
    ] {
        assert_untouched(text);
    }
}

#[test]
fn variable_reference_password_value_ignored() {
    assert_untouched("password: ${DB_PASSWORD}");
    assert_untouched("password: $DB_PASSWORD");
}

#[test]
fn short_password_value_ignored() {
    assert_untouched(&format!("{}: abc12", "password"));
}

#[test]
fn dash_p_flag_is_not_a_password_key() {
    assert_untouched("-p Vh8kQ2mN4p");
}

#[test]
fn tokenizer_is_not_a_token_key() {
    assert_untouched("tokenizer: standard");
}

#[test]
fn myauthenticator_is_not_an_auth_key() {
    assert_untouched("myauthenticator = Vh8kQ2mN4p");
}

#[test]
fn key_value_is_idempotent() {
    let text = format!("password={} client_secret={}", fake_value(), fake_value());
    let (first, _) = redact(&text);
    let (second, redactions) = redact(&first);
    assert_eq!(second, first);
    assert!(redactions.is_empty(), "{redactions:?}");
}

// ------------------------------------------------------------ .env lines

#[test]
fn env_line_with_high_entropy_value_detected() {
    let value = "9fK2mQ7xL4vN8pR1sT6wY3zB5cD0eH7j";
    let text = format!("DEPLOY_RUN_ID={value}");
    let (out, redactions) = redact(&text);
    assert_eq!(out, format!("DEPLOY_RUN_ID={}", marker("env-secret")));
    assert_eq!(kinds(&redactions), vec![("env-secret", 1)]);
}

#[test]
fn env_line_with_short_value_ignored() {
    assert_untouched("DEPLOY_NAME=staging-eu-west");
}

// ------------------------------------------------------------ vault

#[test]
fn vault_secret_replaced() {
    let secret = "correct-horse-battery-staple";
    let redactor = with_vault(&[secret]);
    let (out, redactions) = redactor.redact(&format!("password was {secret} here"));
    assert_eq!(out, format!("password was {} here", marker("vault-secret")));
    assert_eq!(kinds(&redactions), vec![("vault-secret", 1)]);
}

#[test]
fn vault_secret_trimmed_before_matching() {
    let redactor = with_vault(&["  another-long-secret  "]);
    let (out, redactions) = redactor.redact("another-long-secret");
    assert_eq!(out, marker("vault-secret"));
    assert_eq!(kinds(&redactions), vec![("vault-secret", 1)]);
}

#[test]
fn short_vault_secret_ignored() {
    let redactor = with_vault(&["abcde"]);
    let (out, redactions) = redactor.redact("abcde");
    assert_eq!(out, "abcde");
    assert!(redactions.is_empty());
}

#[test]
fn multiline_vault_secret_replaced() {
    let secret = "line-one\nline-two\nline-three";
    let redactor = with_vault(&[secret]);
    let (out, redactions) = redactor.redact(&format!("start\n{secret}\nend"));
    assert_eq!(out, format!("start\n{}\nend", marker("vault-secret")));
    assert_eq!(kinds(&redactions), vec![("vault-secret", 1)]);
}

#[test]
fn vault_secrets_can_be_disabled() {
    let rules = RedactRules {
        vault_secrets: false,
        patterns: "default".to_string(),
    };
    let redactor = SecretRedactor::new(vec!["correct-horse-battery".to_string()], &rules);
    let (out, redactions) = redactor.redact("correct-horse-battery");
    assert_eq!(out, "correct-horse-battery");
    assert!(redactions.is_empty());
}

#[test]
fn patterns_off_leaves_only_vault_secrets() {
    let rules = RedactRules {
        vault_secrets: true,
        patterns: "off".to_string(),
    };
    let redactor = SecretRedactor::new(vec!["correct-horse-battery".to_string()], &rules);
    let text = format!("correct-horse-battery {}", fake_aws_key());
    let (out, redactions) = redactor.redact(&text);
    assert_eq!(
        out,
        format!("{} {}", marker("vault-secret"), fake_aws_key())
    );
    assert_eq!(kinds(&redactions), vec![("vault-secret", 1)]);
}

#[test]
fn unknown_patterns_value_behaves_as_default() {
    let rules = RedactRules {
        vault_secrets: true,
        patterns: "custom".to_string(),
    };
    let redactor = SecretRedactor::new(Vec::new(), &rules);
    let (out, redactions) = redactor.redact(&fake_aws_key());
    assert_eq!(out, marker("aws-key"));
    assert_eq!(kinds(&redactions), vec![("aws-key", 1)]);
}

#[test]
fn overlapping_vault_secrets_longest_first() {
    let redactor = with_vault(&["abcdef", "abcdef123456"]);
    let (out, redactions) = redactor.redact("abcdef123456 and abcdef");
    assert_eq!(
        out,
        format!("{} and {}", marker("vault-secret"), marker("vault-secret"))
    );
    assert_eq!(kinds(&redactions), vec![("vault-secret", 2)]);
}

// ------------------------------------------------------------ bookkeeping

#[test]
fn counts_are_summed_per_kind() {
    let text = format!("{} {}", fake_aws_key(), fake_aws_key());
    let (out, redactions) = redact(&text);
    assert_eq!(out, format!("{} {}", marker("aws-key"), marker("aws-key")));
    assert_eq!(kinds(&redactions), vec![("aws-key", 2)]);
}

#[test]
fn multiple_secrets_on_one_line() {
    let text = format!(
        "aws={} github={} password={}",
        fake_aws_key(),
        fake_github_token(),
        fake_value()
    );
    let (out, redactions) = redact(&text);
    assert_eq!(
        out,
        format!(
            "aws={} github={} password={}",
            marker("aws-key"),
            marker("github-token"),
            marker("password")
        )
    );
    assert_eq!(
        kinds(&redactions),
        vec![("aws-key", 1), ("github-token", 1), ("password", 1)]
    );
}

#[test]
fn redactions_are_sorted_by_kind() {
    let text = format!(
        "password={} stripe={} aws={}",
        fake_value(),
        fake_stripe_key(),
        fake_aws_key()
    );
    let (_, redactions) = redact(&text);
    assert_eq!(
        kinds(&redactions),
        vec![("aws-key", 1), ("password", 1), ("stripe-key", 1)]
    );
}

#[test]
fn redaction_is_idempotent() {
    let text = format!(
        "key {} jwt {} url postgres://u:Vh8kQ2mN4p@h/db password={}",
        fake_aws_key(),
        fake_jwt(),
        fake_value()
    );
    let (first, redactions) = redact(&text);
    assert!(!redactions.is_empty());
    let (second, again) = redact(&first);
    assert_eq!(second, first);
    assert!(again.is_empty(), "{again:?}");
}

#[test]
fn non_secrets_are_untouched() {
    let cases = [
        "commit 9fceb02d0ae598e95dc970b74767f19372d61af8",
        "job 550e8400-e29b-41d4-a716-446655440000 finished",
        "sshd[123]: Accepted publickey for deploy from 10.0.0.5 port 51234",
        "GET /healthz 200 in 3ms",
        "Bearer",
        "tokenizer",
    ];
    for text in cases {
        assert_untouched(text);
    }
}

#[test]
fn unicode_text_does_not_panic_and_is_preserved() {
    let text = format!("café: naïve — {} — ключ", fake_aws_key());
    let (out, redactions) = redact(&text);
    assert_eq!(out, format!("café: naïve — {} — ключ", marker("aws-key")));
    assert_eq!(kinds(&redactions), vec![("aws-key", 1)]);
}

#[test]
fn multibyte_text_around_markers_is_preserved() {
    let text = format!("ünïcodé {} password={}; 尾", fake_value(), fake_value());
    let (out, redactions) = redact(&text);
    assert!(out.starts_with("ünïcodé "));
    assert!(out.ends_with("; 尾"));
    assert!(out.contains(&marker("password")));
    assert_eq!(kinds(&redactions), vec![("password", 1)]);
}

#[test]
fn empty_input_is_unchanged() {
    let (out, redactions) = redact("");
    assert!(out.is_empty());
    assert!(redactions.is_empty());
}

#[test]
fn large_mixed_input_is_fast() {
    let line = "2026-10-24T12:00:00Z host sshd[42]: session opened for deploy\n\
                job 550e8400-e29b-41d4-a716-446655440000 done in 12ms\n\
                curl -sS https://example.com/healthz -> 200\n";
    let mut corpus = String::with_capacity(300 * 1024);
    while corpus.len() < 256 * 1024 {
        corpus.push_str(line);
        corpus.push_str(&format!("export KEY={}\n", fake_aws_key()));
        corpus.push_str(&format!("password: {}\n", fake_value()));
    }

    let started = Instant::now();
    let (out, redactions) = redact(&corpus);
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(3),
        "redacting 256 KiB took {elapsed:?}"
    );
    assert!(out.contains(&marker("aws-key")));
    assert!(out.contains(&marker("password")));
    assert!(redactions.iter().any(|r| r.kind == "aws-key"));
}
