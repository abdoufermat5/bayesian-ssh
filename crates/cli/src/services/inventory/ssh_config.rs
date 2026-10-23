use super::{config_str, expand_tilde, InventoryProvider};
use crate::models::{DiscoveredHost, Source};
use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::warn;

pub struct SshConfigProvider;

impl InventoryProvider for SshConfigProvider {
    fn fetch(&self, source: &Source) -> Result<Vec<DiscoveredHost>> {
        let path = config_str(&source.config, "path")
            .map(|p| expand_tilde(&p))
            .or_else(|| dirs::home_dir().map(|h| h.join(".ssh/config")))
            .unwrap_or_else(|| PathBuf::from("~/.ssh/config"));

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read SSH config {}", path.display()))?;

        let parsed = parse_ssh_config(&content);
        Ok(parsed
            .iter()
            .map(|host| {
                let jump = resolve_bastion(&parsed, host);
                for message in &jump.warnings {
                    warn!("ssh config host '{}': {}", host.alias, message);
                }
                let (bastion, bastion_user) = match jump.route {
                    BastionRoute::Hop { bastion, user } => (Some(bastion), user),
                    _ => (None, None),
                };
                DiscoveredHost {
                    external_id: host.alias.clone(),
                    name: host.alias.clone(),
                    host: host.hostname.clone().unwrap_or_else(|| host.alias.clone()),
                    user: host.user.clone(),
                    port: host.port,
                    key_path: host.identity_file.clone(),
                    tags: vec!["ssh-config".to_string()],
                    bastion,
                    bastion_user,
                    ..Default::default()
                }
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
    /// Raw `ProxyJump` value (first one wins), e.g. `user@bastion:2222,hop2`.
    pub proxy_jump: Option<String>,
    /// Raw `ProxyCommand` value (first one wins).
    pub proxy_command: Option<String>,
}

/// How a host reaches its target, derived from `ProxyJump`/`ProxyCommand`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BastionRoute {
    /// Nothing configured: keep whatever default applies.
    Unset,
    /// Explicitly direct (`ProxyJump none`).
    Direct,
    /// Jump through one bastion. `bastion` is `host` or `host:port` (the form
    /// accepted by `ssh -J user@bastion`), `user` is the bastion login if known.
    Hop {
        bastion: String,
        user: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedBastion {
    pub route: BastionRoute,
    pub warnings: Vec<String>,
}

/// A parsed `[user@]host[:port]` jump specification.
struct Hop {
    host: String,
    user: Option<String>,
    port: Option<u16>,
}

fn parse_port(port: &str) -> Option<u16> {
    port.parse::<u16>().ok().filter(|&p| p != 0)
}

fn parse_hop(spec: &str) -> Option<Hop> {
    let spec = spec.trim();
    let spec = spec.strip_prefix("ssh://").unwrap_or(spec);
    let spec = spec.strip_suffix('/').unwrap_or(spec);
    let (user, rest) = match spec.rsplit_once('@') {
        Some((u, r)) if !u.is_empty() => (Some(u.to_string()), r),
        Some(_) => return None,
        None => (None, spec),
    };
    let (host, port) = if let Some(inner) = rest.strip_prefix('[') {
        // Bracketed IPv6 literal, optionally followed by `:port`.
        let (addr, tail) = inner.split_once(']')?;
        let port = match tail.strip_prefix(':') {
            Some(p) => Some(parse_port(p)?),
            None if tail.is_empty() => None,
            None => return None,
        };
        (format!("[{addr}]"), port)
    } else {
        match rest.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(parse_port(p)?)),
            None => (rest.to_string(), None),
        }
    };
    if host.is_empty() {
        return None;
    }
    Some(Hop { host, user, port })
}

/// Extract the jump host from `ssh [-q] -W %h:%p [user@]host`, the usual
/// ProxyCommand spelling of ProxyJump. Any other option (`-o`, `-p`, `-i`,
/// ...) or command is not mapped.
fn proxy_command_target(command: &str) -> Option<&str> {
    let mut tokens = command.split_whitespace();
    if tokens.next()? != "ssh" {
        return None;
    }
    let mut forward = false;
    let mut target = None;
    while let Some(token) = tokens.next() {
        match token {
            "-q" => {}
            "-W" => {
                if tokens.next()? != "%h:%p" {
                    return None;
                }
                forward = true;
            }
            "-W%h:%p" => forward = true,
            t if t.starts_with('-') || target.is_some() => return None,
            t => target = Some(t),
        }
    }
    target.filter(|_| forward)
}

/// Work out the bastion for `host` from its `ProxyJump` (or, failing that, a
/// `ssh -W %h:%p` `ProxyCommand`). A jump host that is itself an alias in
/// `all` is resolved to its HostName/User/Port; explicit `user@`/`:port` in
/// the jump spec win over the alias. Only one hop is supported: for a chain
/// the first hop is kept and a warning names the rest.
pub(crate) fn resolve_bastion(all: &[ParsedHost], host: &ParsedHost) -> ResolvedBastion {
    let mut warnings = Vec::new();
    let unset = |warnings| ResolvedBastion {
        route: BastionRoute::Unset,
        warnings,
    };

    let (spec, source) = match (&host.proxy_jump, &host.proxy_command) {
        (Some(pj), _) => (pj.clone(), "ProxyJump"),
        (None, Some(pc)) => match proxy_command_target(pc) {
            Some(target) => (target.to_string(), "ProxyCommand"),
            None => {
                warnings.push(format!(
                    "ProxyCommand '{pc}' is not imported (only 'ssh -W %h:%p <host>' is \
                     supported); set a bastion with 'bssh edit'"
                ));
                return unset(warnings);
            }
        },
        (None, None) => return unset(warnings),
    };

    if spec.trim().eq_ignore_ascii_case("none") {
        return ResolvedBastion {
            route: BastionRoute::Direct,
            warnings,
        };
    }

    let mut hops = spec.split(',').map(str::trim).filter(|h| !h.is_empty());
    let Some(first) = hops.next() else {
        return unset(warnings);
    };
    let rest: Vec<&str> = hops.collect();
    if !rest.is_empty() {
        warnings.push(format!(
            "{source} chain '{spec}' has several hops but only one bastion is supported: \
             using '{first}', ignoring {}",
            rest.join(", ")
        ));
    }

    let Some(hop) = parse_hop(first) else {
        warnings.push(format!(
            "{source} '{first}' could not be parsed; no bastion set"
        ));
        return unset(warnings);
    };

    let alias = all.iter().find(|h| h.alias == hop.host);
    let real_host = alias
        .and_then(|a| a.hostname.clone())
        .unwrap_or_else(|| hop.host.clone());
    let user = hop.user.or_else(|| alias.and_then(|a| a.user.clone()));
    let port = hop.port.or_else(|| alias.and_then(|a| a.port));
    let bastion = match port {
        Some(p) if p != 22 => format!("{real_host}:{p}"),
        _ => real_host,
    };

    ResolvedBastion {
        route: BastionRoute::Hop { bastion, user },
        warnings,
    }
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
                        "proxyjump" => {
                            host.proxy_jump.get_or_insert_with(|| value.to_string());
                        }
                        "proxycommand" => {
                            host.proxy_command.get_or_insert_with(|| value.to_string());
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
                ..Default::default()
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

    fn route(cfg: &str, alias: &str) -> ResolvedBastion {
        let hosts = parse_ssh_config(cfg);
        let host = hosts.iter().find(|h| h.alias == alias).unwrap();
        resolve_bastion(&hosts, host)
    }

    fn hop(bastion: &str, user: Option<&str>) -> BastionRoute {
        BastionRoute::Hop {
            bastion: bastion.into(),
            user: user.map(Into::into),
        }
    }

    #[test]
    fn proxy_jump_to_alias_resolves_hostname_user_port() {
        let cfg = "Host bastion\n  HostName bastion.example.com\n  User deploy\n  Port 2200\n\
                   Host db-prod\n  HostName db-prod\n  User deploy\n  ProxyJump bastion\n";
        let r = route(cfg, "db-prod");
        assert_eq!(r.route, hop("bastion.example.com:2200", Some("deploy")));
        assert!(r.warnings.is_empty());
        assert_eq!(route(cfg, "bastion").route, BastionRoute::Unset);
    }

    #[test]
    fn proxy_jump_alias_without_hostname_uses_alias() {
        let cfg = "Host bastion\n  User deploy\nHost db\n  ProxyJump bastion\n";
        assert_eq!(route(cfg, "db").route, hop("bastion", Some("deploy")));
    }

    #[test]
    fn proxy_jump_explicit_user_and_port() {
        let cfg = "Host db\n  ProxyJump ops@jump.example.com:2222\n";
        assert_eq!(
            route(cfg, "db").route,
            hop("jump.example.com:2222", Some("ops"))
        );
        let cfg = "Host db\n  ProxyJump jump.example.com:22\n";
        assert_eq!(route(cfg, "db").route, hop("jump.example.com", None));
    }

    #[test]
    fn proxy_jump_explicit_overrides_alias_values() {
        let cfg = "Host b\n  HostName b.example\n  User x\n  Port 2200\n\
                   Host db\n  ProxyJump y@b:2300\n";
        assert_eq!(route(cfg, "db").route, hop("b.example:2300", Some("y")));
    }

    #[test]
    fn proxy_jump_ipv6_and_url_forms() {
        let cfg = "Host db\n  ProxyJump u@[fe80::1]:2222\n";
        assert_eq!(route(cfg, "db").route, hop("[fe80::1]:2222", Some("u")));
        let cfg = "Host db\n  ProxyJump ssh://u@jump:2201\n";
        assert_eq!(route(cfg, "db").route, hop("jump:2201", Some("u")));
    }

    #[test]
    fn proxy_jump_chain_keeps_first_hop_and_warns() {
        let cfg = "Host db\n  ProxyJump a.example, u@b.example:2200\n";
        let r = route(cfg, "db");
        assert_eq!(r.route, hop("a.example", None));
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("u@b.example:2200"));
        assert!(r.warnings[0].contains("a.example"));
    }

    #[test]
    fn proxy_jump_none_is_direct() {
        let r = route("Host db\n  ProxyJump none\n", "db");
        assert_eq!(r.route, BastionRoute::Direct);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn proxy_jump_wins_over_proxy_command_and_first_value_wins() {
        let cfg = "Host db\n  ProxyJump a\n  ProxyJump b\n  ProxyCommand ssh -W %h:%p c\n";
        assert_eq!(route(cfg, "db").route, hop("a", None));
    }

    #[test]
    fn proxy_command_ssh_w_maps_to_bastion() {
        let cfg = "Host bastion\n  HostName b.example\n  User deploy\n\
                   Host db\n  ProxyCommand ssh -W %h:%p bastion\n";
        assert_eq!(route(cfg, "db").route, hop("b.example", Some("deploy")));
        let cfg = "Host db\n  ProxyCommand ssh -q -W %h:%p ops@jump\n";
        assert_eq!(route(cfg, "db").route, hop("jump", Some("ops")));
    }

    #[test]
    fn unsupported_proxy_command_warns_and_sets_nothing() {
        for cmd in [
            "nc -X 5 -x proxy:1080 %h %p",
            "ssh -o StrictHostKeyChecking=no -W %h:%p jump",
            "ssh -p 2222 -W %h:%p jump",
            "ssh jump nc %h %p",
            "ssh -W %h:%p",
        ] {
            let r = route(&format!("Host db\n  ProxyCommand {cmd}\n"), "db");
            assert_eq!(r.route, BastionRoute::Unset, "{cmd}");
            assert_eq!(r.warnings.len(), 1, "{cmd}");
            assert!(r.warnings[0].contains("not imported"));
        }
    }

    #[test]
    fn no_proxy_means_unset() {
        let r = route("Host db\n  HostName db\n", "db");
        assert_eq!(r.route, BastionRoute::Unset);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn wildcard_host_defaults_are_not_applied() {
        let cfg = "Host *\n  ProxyJump gw\nHost db\n  HostName db\n";
        assert_eq!(route(cfg, "db").route, BastionRoute::Unset);
    }

    #[test]
    fn shared_host_block_applies_proxy_jump_to_every_alias() {
        let cfg = "Host a b\n  ProxyJump gw\n";
        assert_eq!(route(cfg, "a").route, hop("gw", None));
        assert_eq!(route(cfg, "b").route, hop("gw", None));
    }
}
