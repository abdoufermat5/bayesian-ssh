use super::{config_str, expand_tilde, InventoryProvider};
use crate::models::{DiscoveredHost, Source};
use anyhow::{Context, Result};
use std::path::PathBuf;

pub struct SshConfigProvider;

impl InventoryProvider for SshConfigProvider {
    fn fetch(&self, source: &Source) -> Result<Vec<DiscoveredHost>> {
        let path = config_str(&source.config, "path")
            .map(|p| expand_tilde(&p))
            .or_else(|| dirs::home_dir().map(|h| h.join(".ssh/config")))
            .unwrap_or_else(|| PathBuf::from("~/.ssh/config"));

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read SSH config {}", path.display()))?;

        Ok(parse_ssh_config(&content)
            .into_iter()
            .map(|host| DiscoveredHost {
                external_id: host.alias.clone(),
                name: host.alias.clone(),
                host: host.hostname.unwrap_or_else(|| host.alias.clone()),
                user: host.user,
                port: host.port,
                key_path: host.identity_file,
                tags: vec!["ssh-config".to_string()],
                ..Default::default()
            })
            .collect())
    }
}

/// One concrete `Host` alias from an OpenSSH config file.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ParsedHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
}

/// Parse the subset of ssh_config(5) that maps onto a connection.
///
/// Follows OpenSSH lexing rules: keywords are case-insensitive, the value may
/// be separated by whitespace or `=`, values may be double-quoted, and the
/// first value seen for a keyword wins. A `Host` line with several patterns
/// yields one entry per concrete (non-wildcard, non-negated) alias. `Match`
/// blocks end the current `Host` block so their directives are not
/// misattributed to it.
pub(crate) fn parse_ssh_config(content: &str) -> Vec<ParsedHost> {
    let mut hosts: Vec<ParsedHost> = Vec::new();
    // Index range in `hosts` covered by the current Host block.
    let mut block: Option<std::ops::Range<usize>> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (keyword, value) = match line.find(|c: char| c.is_whitespace() || c == '=') {
            Some(i) => {
                let rest = line[i..].trim_start();
                let rest = rest.strip_prefix('=').unwrap_or(rest).trim();
                (&line[..i], rest)
            }
            None => (line, ""),
        };
        let keyword = keyword.to_ascii_lowercase();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);

        match keyword.as_str() {
            "host" => {
                let start = hosts.len();
                hosts.extend(
                    value
                        .split_whitespace()
                        .filter(|p| !p.starts_with('!') && !p.contains(['*', '?']))
                        .map(|p| ParsedHost {
                            alias: p.to_string(),
                            ..Default::default()
                        }),
                );
                block = Some(start..hosts.len());
            }
            "match" => block = None,
            _ => {
                let Some(range) = block.clone() else {
                    continue;
                };
                for host in &mut hosts[range] {
                    match keyword.as_str() {
                        "hostname" => {
                            host.hostname.get_or_insert_with(|| value.to_string());
                        }
                        "user" => {
                            host.user.get_or_insert_with(|| value.to_string());
                        }
                        "port" => {
                            if host.port.is_none() {
                                host.port = value.parse::<u16>().ok().filter(|&p| p != 0);
                            }
                        }
                        "identityfile" => {
                            host.identity_file.get_or_insert_with(|| value.to_string());
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_case_insensitive_keywords_and_equals() {
        let cfg = "Host web\n  Hostname web.example.com\n  user=deploy\n  PORT = 2222\n  IdentityFile \"~/.ssh/my key\"\n";
        let hosts = parse_ssh_config(cfg);
        assert_eq!(
            hosts,
            vec![ParsedHost {
                alias: "web".into(),
                hostname: Some("web.example.com".into()),
                user: Some("deploy".into()),
                port: Some(2222),
                identity_file: Some("~/.ssh/my key".into()),
            }]
        );
    }

    #[test]
    fn match_block_does_not_leak_into_previous_host() {
        let cfg = "Host a\n  HostName a.example\nMatch host b\n  User root\n";
        let hosts = parse_ssh_config(cfg);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].user, None);
    }

    #[test]
    fn multi_pattern_host_skips_wildcards_and_first_value_wins() {
        let cfg = "Host *\n  User global\nHost db1 db2 *.corp !bad\n  User one\n  User two\n";
        let hosts = parse_ssh_config(cfg);
        let names: Vec<_> = hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, ["db1", "db2"]);
        assert!(hosts.iter().all(|h| h.user.as_deref() == Some("one")));
    }
}
