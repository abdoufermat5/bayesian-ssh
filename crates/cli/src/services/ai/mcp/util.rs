//! Small pure helpers: shell quoting, globs, path checks, durations, hashes.
//!
//!

use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::time::Duration;

/// Quote one word for a POSIX shell: `it's` -> `'it'\''s'`.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Glob over paths: `*` and `?` stay inside one path segment, `**` crosses `/`.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some((b'*', rest)) => {
                if rest.first() == Some(&b'*') {
                    let rest = &rest[1..];
                    (0..=t.len()).any(|i| go(rest, &t[i..]))
                } else {
                    let mut i = 0;
                    loop {
                        if go(rest, &t[i..]) {
                            return true;
                        }
                        if i >= t.len() || t[i] == b'/' {
                            return false;
                        }
                        i += 1;
                    }
                }
            }
            Some((b'?', rest)) => !t.is_empty() && t[0] != b'/' && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

/// A deny/read path entry matches a path when the glob matches it, or it names a directory
/// that contains it (`/etc/ssh` covers `/etc/ssh/sshd_config`).
pub fn path_matches(pattern: &str, path: &str) -> bool {
    if glob_match(pattern, path) {
        return true;
    }
    let dir = pattern.trim_end_matches('/');
    !dir.is_empty() && !dir.contains(['*', '?']) && path.starts_with(&format!("{dir}/"))
}

/// Absolute path without `.`/`..`/empty segments. Agents can't escape deny paths with `..`.
pub fn normalize_path(path: &str) -> Result<String> {
    if path.contains('\0') || path.contains('\n') {
        bail!("invalid characters in path");
    }
    if !path.starts_with('/') {
        bail!("path must be absolute");
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => bail!("'..' is not allowed in paths"),
            s => parts.push(s),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

/// `"90s"`, `"15m"`, `"2h"`, `"1d"`, `"1h30m"`; a bare number is seconds.
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if s.is_empty() {
        bail!("empty duration");
    }
    if let Ok(n) = s.parse::<u64>() {
        return Ok(Duration::from_secs(n));
    }
    let mut total = 0u64;
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let n: u64 = digits
            .parse()
            .map_err(|_| anyhow::anyhow!("bad duration '{s}'"))?;
        digits.clear();
        let mult = match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86_400,
            _ => bail!("bad duration '{s}'"),
        };
        total = total.saturating_add(n.saturating_mul(mult));
    }
    if !digits.is_empty() {
        bail!("bad duration '{s}'");
    }
    Ok(Duration::from_secs(total))
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn rfc3339_in(d: Duration) -> String {
    (chrono::Utc::now()
        + chrono::Duration::from_std(d).unwrap_or_else(|_| chrono::Duration::days(1)))
    .to_rfc3339()
}

/// Cut at a char boundary; the bool says whether anything was removed.
pub fn truncate_chars(s: &str, max_chars: usize) -> (String, bool) {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => (s[..i].to_string(), true),
        None => (s.to_string(), false),
    }
}

/// Unit names as systemd accepts them; keeps quotes and spaces out of generated commands.
pub fn valid_unit(unit: &str) -> bool {
    !unit.is_empty()
        && unit.len() <= 256
        && unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | ':' | '-' | '\\'))
}

/// `--since` values: `2h ago`, `2026-10-24 10:00:00`, `yesterday`.
pub fn valid_since(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | ':' | '-' | '.' | '+'))
}

/// Short random id fragment.
pub fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("abc"), "'abc'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(sh_quote("$(rm -rf /)"), "'$(rm -rf /)'");
    }

    #[test]
    fn globs() {
        assert!(glob_match("/etc/*.conf", "/etc/a.conf"));
        assert!(!glob_match("/etc/*.conf", "/etc/x/a.conf"));
        assert!(glob_match("/etc/**", "/etc/x/y/z"));
        assert!(glob_match("/var/log/**/*.log", "/var/log/nginx/error.log"));
        assert!(glob_match("web-?", "web-1"));
        assert!(!glob_match("web-?", "web-12"));
    }

    #[test]
    fn dir_patterns_cover_children() {
        assert!(path_matches("/etc/ssh", "/etc/ssh/sshd_config"));
        assert!(path_matches("/etc/ssh/", "/etc/ssh/sshd_config"));
        assert!(!path_matches("/etc/ssh", "/etc/sshd"));
        assert!(path_matches("/root/.ssh/*", "/root/.ssh/id_rsa"));
        assert!(path_matches("**/.env", "/srv/app/.env"));
    }

    #[test]
    fn normalizing_paths() {
        assert_eq!(
            normalize_path("/etc//nginx/./a.conf").unwrap(),
            "/etc/nginx/a.conf"
        );
        assert!(normalize_path("/etc/../root/.ssh/id_rsa").is_err());
        assert!(normalize_path("relative/path").is_err());
        assert!(normalize_path("/a\0b").is_err());
        assert_eq!(normalize_path("/").unwrap(), "/");
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::from_secs(5400));
        assert_eq!(parse_duration("45").unwrap(), Duration::from_secs(45));
        assert!(parse_duration("").is_err());
        assert!(parse_duration("2x").is_err());
        assert!(parse_duration("h").is_err());
    }

    #[test]
    fn validators() {
        assert!(valid_unit("nginx.service"));
        assert!(valid_unit("getty@tty1.service"));
        assert!(!valid_unit("nginx; rm -rf /"));
        assert!(!valid_unit(""));
        assert!(valid_since("2h ago"));
        assert!(valid_since("2026-10-24 10:00:00"));
        assert!(!valid_since("1h' ; id #"));
    }

    #[test]
    fn truncation_is_char_safe() {
        let (s, t) = truncate_chars("héllo wörld", 4);
        assert_eq!(s, "héll");
        assert!(t);
        let (s, t) = truncate_chars("abc", 10);
        assert_eq!(s, "abc");
        assert!(!t);
    }
}
