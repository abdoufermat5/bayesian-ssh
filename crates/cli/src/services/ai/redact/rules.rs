//! One function per redaction rule, and the helpers they share.

use std::collections::HashMap;

use regex::{Captures, Regex};

use super::redactor::{Counts, MARKER_CLOSE, MARKER_OPEN};

pub(super) fn apply_pattern(
    regex: Option<&Regex>,
    kind: &'static str,
    text: &str,
    counts: &mut Counts,
) -> String {
    let Some(regex) = regex else {
        return text.to_string();
    };
    regex
        .replace_all(text, |_: &Captures<'_>| {
            *counts.entry(kind).or_insert(0) += 1;
            marker(kind)
        })
        .into_owned()
}

/// `aws_secret_access_key = <40 base64-ish chars>`: only the value is replaced.
pub(super) fn apply_aws_secret(regex: Option<&Regex>, text: &str, counts: &mut Counts) -> String {
    let Some(regex) = regex else {
        return text.to_string();
    };
    regex
        .replace_all(text, |caps: &Captures<'_>| {
            *counts.entry("aws-secret").or_insert(0) += 1;
            let (open, close) = (&caps["q"], &caps["postv"]);
            format!("{}{open}{}{close}", &caps["pre"], marker("aws-secret"))
        })
        .into_owned()
}

/// `Authorization: Bearer <token>` and bare `Bearer <token>`: the prefix stays.
pub(super) fn apply_bearer(regex: Option<&Regex>, text: &str, counts: &mut Counts) -> String {
    let Some(regex) = regex else {
        return text.to_string();
    };
    regex
        .replace_all(text, |caps: &Captures<'_>| {
            *counts.entry("bearer").or_insert(0) += 1;
            format!("{}{}", &caps["pre"], marker("bearer"))
        })
        .into_owned()
}

/// `scheme://user:pass@host`: keep scheme, user and host, replace the password.
pub(super) fn apply_url_credentials(
    regex: Option<&Regex>,
    text: &str,
    counts: &mut Counts,
) -> String {
    let Some(regex) = regex else {
        return text.to_string();
    };
    regex
        .replace_all(text, |caps: &Captures<'_>| {
            let password = &caps["pass"];
            if is_marker(password) {
                return whole(caps);
            }
            *counts.entry("url-credentials").or_insert(0) += 1;
            format!(
                "{}://{}:{}@",
                &caps["scheme"],
                &caps["user"],
                marker("url-credentials")
            )
        })
        .into_owned()
}

/// `password=...`, `client_secret: "..."`, `--password=...`, `SECRET_KEY=...`.
pub(super) fn apply_key_value(regex: Option<&Regex>, text: &str, counts: &mut Counts) -> String {
    let Some(regex) = regex else {
        return text.to_string();
    };
    regex
        .replace_all(text, |caps: &Captures<'_>| {
            let start = caps.get(0).map_or(0, |m| m.start());
            if starts_mid_word(text, start) {
                return whole(caps);
            }
            let key = &caps["key"];
            let value = &caps["value"];
            let Some(kind) = classify_key_value(key, value) else {
                return whole(caps);
            };
            *counts.entry(kind).or_insert(0) += 1;
            let (open, close) = (&caps["q"], &caps["postv"]);
            format!(
                "{key}{}{}{open}{}{close}",
                &caps["postq"],
                &caps["sep"],
                marker(kind)
            )
        })
        .into_owned()
}

/// `.env`-style `KEY=<high-entropy value>` lines whose key is not in the list.
pub(super) fn apply_env_secrets(regex: Option<&Regex>, text: &str, counts: &mut Counts) -> String {
    let Some(regex) = regex else {
        return text.to_string();
    };
    regex
        .replace_all(text, |caps: &Captures<'_>| {
            let value = &caps["value"];
            if value.chars().count() < 24 || is_marker(value) || shannon_entropy(value) < 4.0 {
                return whole(caps);
            }
            *counts.entry("env-secret").or_insert(0) += 1;
            let (open, close) = (&caps["q"], &caps["postv"]);
            format!("{}={open}{}{close}", &caps["key"], marker("env-secret"))
        })
        .into_owned()
}

pub(super) fn whole(caps: &Captures<'_>) -> String {
    caps.get(0)
        .map_or_else(String::new, |m| m.as_str().to_string())
}

// ---------------------------------------------------------------- helpers

pub(super) fn marker(kind: &str) -> String {
    format!("{MARKER_OPEN}{kind}{MARKER_CLOSE}")
}

fn is_marker(value: &str) -> bool {
    value.starts_with(MARKER_OPEN)
}

/// True when the match starts inside a word, e.g. `mypassword` must not match
/// the `password` rule while `--password` and `DB_PASSWORD` may.
fn starts_mid_word(text: &str, start: usize) -> bool {
    text.get(..start)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// Values that carry no secret: placeholders, variable references, markers.
fn is_placeholder(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || is_marker(value) || value.starts_with('$') {
        return true;
    }
    if value.starts_with('<') && value.ends_with('>') {
        return true;
    }
    if value
        .chars()
        .all(|c| matches!(c, '*' | 'x' | 'X' | '.' | '?' | '_' | '-'))
    {
        return true;
    }
    matches!(
        value.to_ascii_lowercase().as_str(),
        "null"
            | "nil"
            | "none"
            | "true"
            | "false"
            | "undefined"
            | "password"
            | "secret"
            | "token"
            | "example"
            | "todo"
            | "changeme"
            | "change_me"
            | "change-me"
            | "placeholder"
            | "your_password"
            | "your-password"
    )
}

/// `password`-style keys answer to the password kind; the rest to `secret`.
fn classify_key_value(key: &str, value: &str) -> Option<&'static str> {
    if is_placeholder(value) {
        return None;
    }
    let len = value.chars().count();
    if len < 6 {
        return None;
    }
    let key = key.to_ascii_lowercase();
    let password_like = key.contains("password") || key.contains("passwd") || key.contains("pwd");
    let strong = password_like || key.contains("secret") || key.contains("token");
    let high_entropy = shannon_entropy(value) >= 3.0 && char_classes(value) >= 2;
    if high_entropy || (strong && len >= 8) {
        Some(if password_like { "password" } else { "secret" })
    } else {
        None
    }
}

fn shannon_entropy(value: &str) -> f64 {
    let mut frequencies: HashMap<char, usize> = HashMap::new();
    let mut total = 0usize;
    for c in value.chars() {
        *frequencies.entry(c).or_insert(0) += 1;
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    let total = total as f64;
    frequencies
        .values()
        .map(|&count| {
            let p = count as f64 / total;
            -p * p.log2()
        })
        .sum()
}

fn char_classes(value: &str) -> usize {
    let mut seen = [false; 4];
    for c in value.chars() {
        if c.is_lowercase() {
            seen[0] = true;
        } else if c.is_uppercase() {
            seen[1] = true;
        } else if c.is_ascii_digit() {
            seen[2] = true;
        } else if !c.is_whitespace() {
            seen[3] = true;
        }
    }
    seen.iter().filter(|class| **class).count()
}
