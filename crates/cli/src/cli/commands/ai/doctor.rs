//! `bssh ai doctor`.
//!
//! Checks each host's enforcement (reachable, agent user, jail, CA trusted) and prints
//! suggestions only: sudoers lines from the policy's write paths and an egress note.
//! It never changes a host.

use crate::cli::parser_ai::AiArgsDoctor;
use crate::cli::utils::MAX_PARALLEL_HOSTS;
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::ai::exec::AiExecutor;
use crate::services::ai::hostsetup::{self, HostReport, SudoersSuggestion};
use crate::services::ai::types::{Executor, JailKind};
use crate::services::ai::{cert, policy};
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[derive(Debug, Serialize)]
struct LocalReport {
    ca_initialised: bool,
    ca_fingerprint: Option<String>,
    ca_sealed: bool,
}

/// How a host is reached, and what that route means for the agent.
#[derive(Debug, Default, Serialize)]
struct RouteReport {
    /// `direct`, `jump host <name>`, `Kerberos bastion <name>` or
    /// `ssh_config ProxyJump <name>`.
    route: String,
    /// Facts the user has to know about this route.
    notes: Vec<String>,
    /// Reached through a Kerberos (interactive) bastion: commands run as the user, so the
    /// `bssh-agent` user and CA trust are never used and are not worth flagging.
    kerberos_bastion: bool,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    local: LocalReport,
    /// Problems in the policy as written (ignored fields, empty scope).
    warnings: Vec<String>,
    routes: BTreeMap<String, RouteReport>,
    hosts: Vec<HostReport>,
    sudoers: Vec<SudoersSuggestion>,
    egress_note: &'static str,
}

/// Check every host the policy reaches and print the report.
///
/// # Errors
/// Returns an error when a host cannot be checked or the report cannot be serialized.
pub async fn execute(args: AiArgsDoctor, config: AppConfig) -> Result<()> {
    let pol = policy::resolve(&config, &args.policy);
    // A source we can't read fails closed: its tags don't widen the policy.
    let sources: std::collections::HashMap<_, _> = crate::database::Database::new(&config)
        .and_then(|db| db.list_sources())
        .map(|list| {
            list.into_iter()
                .map(|summary| (summary.source.id, summary.source.name))
                .collect()
        })
        .unwrap_or_default();
    let targets = if args.target.is_some() || args.tag.is_some() {
        super::setup::resolve_targets(
            args.target.as_deref(),
            args.tag.as_deref(),
            "ai doctor",
            &config,
        )
        .await?
    } else {
        // No target: every host the policy lets the agent see (all hosts if the policy
        // can't be read, so doctor still tells you something).
        let all = crate::services::SshService::new(config.clone())?
            .list_connections(None, false)
            .await?;
        match &pol {
            Ok(p) => all
                .into_iter()
                .filter(|c| {
                    let source = c
                        .source_id
                        .and_then(|id| sources.get(&id))
                        .map(String::as_str);
                    policy::host_in_scope(
                        p,
                        c,
                        policy::tags_trusted(&config.ai.trusted_tag_sources, c, source),
                    )
                })
                .collect(),
            Err(_) => all,
        }
    };

    let warnings = policy_warnings(&config, &args.policy, &targets, &sources);
    let ssh_config = ssh_config_hosts(&config);
    let ticket = kerberos_ticket();
    let routes: BTreeMap<String, RouteReport> = targets
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                route_report(c, ssh_config.as_deref(), ticket),
            )
        })
        .collect();

    let ca_pub = cert::ca_public_key(&config).ok();
    let local = LocalReport {
        ca_initialised: cert::ca_available(&config),
        ca_fingerprint: cert::ca_fingerprint(&config).ok(),
        ca_sealed: cert::ca_sealed(&config),
    };
    let exec: Arc<dyn Executor> = Arc::new(AiExecutor::new(&config));
    let hosts = check_all(targets, exec, ca_pub).await;

    let sudoers = match &pol {
        Ok(p) => hostsetup::sudoers_suggestions(p),
        Err(_) => Vec::new(),
    };
    let report = DoctorReport {
        local,
        warnings,
        routes,
        hosts,
        sudoers,
        egress_note: hostsetup::EGRESS_NOTE,
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!(
            "{}",
            render_text(&report, pol.as_ref().err().map(|e| format!("{e:#}")))
        );
    }
    let unreachable = report.hosts.iter().filter(|h| !h.reachable).count();
    if unreachable > 0 {
        bail!("{unreachable} host(s) could not be checked");
    }
    Ok(())
}

/// Policy problems that matter before anything runs: fields serde would ignore, a policy
/// that reaches no host, and tags that come from a source the policy does not trust.
fn policy_warnings(
    config: &AppConfig,
    name: &str,
    targets: &[Connection],
    sources: &std::collections::HashMap<uuid::Uuid, String>,
) -> Vec<String> {
    let mut out = policy::validation_warnings(config, name);
    for c in targets {
        let Some(source) = c.source_id.and_then(|id| sources.get(&id)) else {
            continue;
        };
        if config.ai.trusted_tag_sources.iter().any(|t| t == source) {
            continue;
        }
        let msg = format!(
            "tags on hosts from inventory source '{source}' are not trusted: they cannot widen \
             the scope or allow in-client approval; add \"{source}\" to ai.trusted_tag_sources \
             if you trust it"
        );
        if !out.contains(&msg) {
            out.push(msg);
        }
    }
    out
}

/// The `~/.ssh/config` bssh reads, parsed once. `None` when it is missing or unreadable.
fn ssh_config_hosts(
    config: &AppConfig,
) -> Option<Vec<crate::services::inventory::ssh_config::ParsedHost>> {
    let path = config.ssh_config_path.clone()?;
    let text = std::fs::read_to_string(&path).ok()?;
    Some(crate::services::inventory::ssh_config::parse_ssh_config(
        &text,
    ))
}

/// How `ssh` would reach [`Connection`], phrased for the person running `doctor`.
fn route_report(
    conn: &Connection,
    ssh_config: Option<&[crate::services::inventory::ssh_config::ParsedHost]>,
    ticket: Option<bool>,
) -> RouteReport {
    let mut notes = Vec::new();
    let route = match (&conn.bastion, conn.use_kerberos) {
        (Some(b), true) => {
            notes.push(
                "commands run as your user through the bastion; certificates and edits as \
                 bssh-agent are not available here, and file reads and edits go over the \
                 command channel"
                    .to_string(),
            );
            format!("Kerberos bastion {b}")
        }
        (Some(b), false) => format!("jump host {b}"),
        (None, _) => match ssh_config.and_then(|hosts| ssh_config_jump(hosts, conn)) {
            Some(jump) => format!("ssh_config ProxyJump {jump}"),
            None => "direct".to_string(),
        },
    };
    if conn.use_kerberos {
        match ticket {
            Some(true) => notes.push("Kerberos ticket: valid".to_string()),
            Some(false) => notes.push(
                "no Kerberos ticket: run `kinit` first; the agent cannot prompt for one"
                    .to_string(),
            ),
            None => {}
        }
    }
    let kerberos_bastion = conn.bastion.is_some() && conn.use_kerberos;
    RouteReport {
        route,
        notes,
        kerberos_bastion,
    }
}

/// The one-hop `ProxyJump` `~/.ssh/config` sets for this connection, if any.
fn ssh_config_jump(
    hosts: &[crate::services::inventory::ssh_config::ParsedHost],
    conn: &Connection,
) -> Option<String> {
    use crate::services::inventory::ssh_config::{resolve_bastion, BastionRoute};
    // The bssh name is the ssh alias for imported hosts; a hand-added name may only match
    // the HostName.
    let host = hosts.iter().find(|h| h.alias == conn.name).or_else(|| {
        hosts
            .iter()
            .find(|h| h.hostname.as_deref() == Some(&conn.host))
    })?;
    match resolve_bastion(hosts, host).route {
        BastionRoute::Hop { bastion, .. } => Some(bastion),
        _ => None,
    }
}

/// `klist -s` for any connection that uses Kerberos. `None` when there is no `klist`.
fn kerberos_ticket() -> Option<bool> {
    let status = std::process::Command::new("klist")
        .arg("-s")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) => Some(s.success()),
        Err(_) => None,
    }
}

/// Check every host with a bounded number in flight, keeping the input order.
async fn check_all(
    targets: Vec<Connection>,
    exec: Arc<dyn Executor>,
    ca_pub: Option<String>,
) -> Vec<HostReport> {
    let semaphore = Arc::new(Semaphore::new(MAX_PARALLEL_HOSTS));
    let mut set = JoinSet::new();
    let total = targets.len();
    for (i, conn) in targets.into_iter().enumerate() {
        let exec = exec.clone();
        let ca_pub = ca_pub.clone();
        let semaphore = semaphore.clone();
        set.spawn(async move {
            // The semaphore is never closed, so acquire cannot fail.
            let _permit = semaphore.acquire_owned().await.ok();
            (
                i,
                hostsetup::check_host(&conn, exec.as_ref(), ca_pub.as_deref()).await,
            )
        });
    }
    let mut out: Vec<Option<HostReport>> = (0..total).map(|_| None).collect();
    while let Some(res) = set.join_next().await {
        if let Ok((i, report)) = res {
            out[i] = Some(report);
        }
    }
    out.into_iter().flatten().collect()
}

fn render_text(report: &DoctorReport, policy_error: Option<String>) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "bssh ai doctor\n");
    let l = &report.local;
    let _ = writeln!(s, "Local");
    match (&l.ca_fingerprint, l.ca_initialised) {
        (Some(fp), true) => {
            let _ = writeln!(
                s,
                "  ok    CA {fp} ({})",
                if l.ca_sealed { "sealed" } else { "0600 file" }
            );
        }
        _ => {
            let _ = writeln!(
                s,
                "  warn  no local CA: run `bssh ai ca init` for approval-bound certificates"
            );
        }
    }
    let clients: Vec<_> = crate::services::ai::install::detected_clients()
        .into_iter()
        .filter(|c| c.present)
        .collect();
    if clients.is_empty() {
        let _ = writeln!(
            s,
            "  skip  no agent client found (Claude Code, Codex, Cursor, VS Code, Claude Desktop, omp)"
        );
    }
    for c in clients {
        let name = c.client.name();
        if c.configured {
            let _ = writeln!(s, "  ok    {name} uses bssh ({})", c.config_path.display());
        } else {
            let _ = writeln!(
                s,
                "  warn  {name} is not set up: run `bssh ai install {name}`"
            );
        }
    }
    if let Some(e) = policy_error {
        let _ = writeln!(
            s,
            "  warn  policy not readable, no sudoers suggestions: {e}"
        );
    }
    for w in &report.warnings {
        let _ = writeln!(s, "  warn  {w}");
    }
    for h in &report.hosts {
        let _ = writeln!(s, "\n{}  [{}]", h.host, h.level());
        let route = report.routes.get(&h.host);
        if let Some(r) = route {
            let _ = writeln!(s, "  route {}", r.route);
            for note in &r.notes {
                let _ = writeln!(s, "        {note}");
            }
        }
        let behind_kerberos_bastion = route.is_some_and(|r| r.kerberos_bastion);
        if !h.reachable {
            let _ = writeln!(
                s,
                "  FAIL  unreachable: {}",
                h.error.as_deref().unwrap_or("unknown")
            );
            continue;
        }
        let _ = writeln!(s, "  ok    reachable");
        if behind_kerberos_bastion {
            let _ = writeln!(
                s,
                "  skip  agent user and CA trust (not used behind a Kerberos bastion)"
            );
        } else if h.agent_user {
            let _ = writeln!(
                s,
                "  ok    agent user {} (groups: {})",
                hostsetup::AGENT_USER,
                h.agent_groups.join(" ")
            );
        } else {
            let _ = writeln!(
                s,
                "  FAIL  no user {}: run `bssh ai setup {}`",
                hostsetup::AGENT_USER,
                h.host
            );
        }
        match h.jail {
            JailKind::None => {
                let _ = writeln!(
                    s,
                    "  warn  no read-only jail (reads fall back to the analyzer allowlist): install \
                     bubblewrap or util-linux >= 2.40{}",
                    if h.jail_helper { "" } else { ", or the bssh-jail helper" }
                );
            }
            kind => {
                let _ = writeln!(s, "  ok    jail: {}", kind.as_str());
            }
        }
        if behind_kerberos_bastion {
            continue;
        }
        match (h.ca_trusted, report.local.ca_initialised) {
            (Some(true), _) if h.principals_mapped == Some(false) => {
                let _ = writeln!(
                    s,
                    "  FAIL  CA trusted but the principals file is missing: run `bssh ai ca trust {}`",
                    h.host
                );
            }
            (Some(true), _) => {
                let _ = writeln!(s, "  ok    CA trusted: per-step certificates work");
            }
            (Some(false), _) => {
                let _ = writeln!(
                    s,
                    "  warn  CA not trusted (writes are client-enforced): run `bssh ai ca trust {}`",
                    h.host
                );
            }
            (None, _) => {
                let _ = writeln!(s, "  skip  CA trust (no local CA)");
            }
        }
    }
    if !report.sudoers.is_empty() {
        let _ = writeln!(
            s,
            "\nSuggested sudoers lines (bssh installs nothing; review, then add them with `visudo -f /etc/sudoers.d/bssh-agent`):"
        );
        for sug in &report.sudoers {
            let _ = writeln!(s, "  # {}", sug.why);
            let _ = writeln!(s, "  {}", sug.line);
        }
        let _ = writeln!(
            s,
            "  # Check the binary paths with `command -v systemctl`; sudoers wants absolute paths."
        );
    }
    let _ = writeln!(s, "\nEgress: {}", report.egress_note);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::Enforcement;

    fn host(name: &str) -> HostReport {
        HostReport {
            host: name.into(),
            reachable: true,
            agent_user: true,
            agent_groups: vec!["bssh-agent".into(), "adm".into()],
            agent_home: Some("/var/lib/bssh-agent".into()),
            jail: JailKind::Bubblewrap,
            ca_trusted: Some(false),
            principals_mapped: Some(false),
            enforcement: Enforcement {
                jail: JailKind::Bubblewrap,
                certificates: false,
                agent_user: true,
            },
            ..HostReport::default()
        }
    }

    fn report(hosts: Vec<HostReport>) -> DoctorReport {
        let routes = hosts
            .iter()
            .map(|h| {
                (
                    h.host.clone(),
                    RouteReport {
                        route: "direct".into(),
                        notes: Vec::new(),
                        kerberos_bastion: false,
                    },
                )
            })
            .collect();
        DoctorReport {
            local: LocalReport {
                ca_initialised: true,
                ca_fingerprint: Some("SHA256:abc".into()),
                ca_sealed: false,
            },
            warnings: Vec::new(),
            routes,
            hosts,
            sudoers: vec![SudoersSuggestion {
                line: "bssh-agent ALL=(root) NOPASSWD: /usr/bin/systemctl reload nginx".into(),
                why: "nginx".into(),
            }],
            egress_note: hostsetup::EGRESS_NOTE,
        }
    }

    /// A connection with a Kerberos bastion and no ticket, as OVH "The Bastion" users have.
    fn bastioned(name: &str) -> Connection {
        let mut c = Connection {
            bastion: Some("bastion".into()),
            use_kerberos: true,
            ..Connection::new(name.into(), "10.0.0.9".into(), "deploy".into(), 22)
        };
        c.tags = vec!["prod".into()];
        c
    }

    #[test]
    fn route_names_each_hop_and_kerberos_bastion_limits() {
        let r = route_report(&bastioned("web-1"), None, Some(false));
        assert_eq!(r.route, "Kerberos bastion bastion");
        assert!(
            r.notes
                .iter()
                .any(|n| n.contains("commands run as your user through the bastion")),
            "{r:?}"
        );
        assert!(
            r.notes
                .iter()
                .any(|n| n.contains("certificates") && n.contains("file reads and edits")),
            "{r:?}"
        );
        assert!(r.notes.iter().any(|n| n.contains("run `kinit`")), "{r:?}");

        // A ticket that exists is reported as fine, without advice.
        let ok = route_report(&bastioned("web-1"), None, Some(true));
        assert!(ok
            .notes
            .iter()
            .any(|n| n.contains("Kerberos ticket: valid")));
        assert!(!ok.notes.iter().any(|n| n.contains("kinit")));

        // A plain jump host and a Kerberos connection with no ticket.
        let mut jump = bastioned("web-2");
        jump.use_kerberos = false;
        let j = route_report(&jump, None, Some(false));
        assert_eq!(j.route, "jump host bastion");
        assert!(j.notes.is_empty(), "{j:?}");

        let direct = Connection::new("db-1".into(), "10.0.0.10".into(), "deploy".into(), 22);
        assert_eq!(route_report(&direct, None, None).route, "direct");

        // `~/.ssh/config` ProxyJump is named when bssh stores no bastion itself.
        let cfg = crate::services::inventory::ssh_config::parse_ssh_config(
            "Host db-1\n  HostName 10.0.0.10\n  ProxyJump ops@jump-1\n",
        );
        assert_eq!(
            route_report(&direct, Some(&cfg), None).route,
            "ssh_config ProxyJump jump-1"
        );
    }

    #[test]
    fn untrusted_source_tags_are_a_warning_not_silence() {
        let mut config = AppConfig::default_for_env("test");
        let mut conn = bastioned("web-1");
        conn.source_id = Some(uuid::Uuid::new_v4());
        conn.tags = vec!["prod".into()];
        let sources: std::collections::HashMap<_, _> =
            [(conn.source_id.unwrap(), "team-vault".to_string())]
                .into_iter()
                .collect();
        let w = policy_warnings(&config, "default", std::slice::from_ref(&conn), &sources);
        assert!(
            w.iter()
                .any(|m| m.contains("team-vault") && m.contains("trusted_tag_sources")),
            "{w:?}"
        );
        // Once the source is trusted, the warning goes away.
        config.ai.trusted_tag_sources = vec!["team-vault".into()];
        let w = policy_warnings(&config, "default", std::slice::from_ref(&conn), &sources);
        assert!(!w.iter().any(|m| m.contains("team-vault")), "{w:?}");
    }

    #[test]
    fn text_report_names_each_problem_and_the_fix() {
        let mut down = host("db-1");
        down.reachable = false;
        down.error = Some("connection refused".into());
        let mut bare = host("web-2");
        bare.agent_user = false;
        bare.jail = JailKind::None;
        bare.ca_trusted = None;
        let text = render_text(&report(vec![host("web-1"), bare, down]), None);
        assert!(text.contains("web-1  [jail-enforced]"), "{text}");
        assert!(text.contains("ok    jail: bubblewrap"));
        assert!(text.contains(
            "warn  CA not trusted (writes are client-enforced): run `bssh ai ca trust web-1`"
        ));
        assert!(text.contains("FAIL  no user bssh-agent: run `bssh ai setup web-2`"));
        assert!(text.contains("no read-only jail"));
        assert!(text.contains("skip  CA trust"));
        assert!(text.contains("FAIL  unreachable: connection refused"));
        assert!(text.contains("bssh-agent ALL=(root) NOPASSWD: /usr/bin/systemctl reload nginx"));
        assert!(text.contains("Egress:"));
        assert!(text.contains("CA SHA256:abc (0600 file)"));
    }

    #[test]
    fn json_report_carries_the_same_facts() {
        let v = serde_json::to_value(report(vec![host("web-1")])).unwrap();
        assert_eq!(v["hosts"][0]["host"], "web-1");
        assert_eq!(v["hosts"][0]["jail"], "bubblewrap");
        assert_eq!(v["hosts"][0]["ca_trusted"], false);
        assert_eq!(v["local"]["ca_fingerprint"], "SHA256:abc");
        assert!(v["sudoers"][0]["line"]
            .as_str()
            .unwrap()
            .starts_with("bssh-agent ALL=(root)"));
        assert!(v["egress_note"].as_str().unwrap().contains("firewall"));
    }
}
