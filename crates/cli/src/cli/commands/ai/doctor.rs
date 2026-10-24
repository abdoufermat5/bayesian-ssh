//! `bssh ai doctor`. Owner: W5.
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
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[derive(Debug, Serialize)]
struct LocalReport {
    ca_initialised: bool,
    ca_fingerprint: Option<String>,
    ca_sealed: bool,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    local: LocalReport,
    hosts: Vec<HostReport>,
    sudoers: Vec<SudoersSuggestion>,
    egress_note: &'static str,
}

pub async fn execute(args: AiArgsDoctor, config: AppConfig) -> Result<()> {
    let pol = policy::resolve(&config, &args.policy);
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
                .filter(|c| policy::host_in_scope(p, c))
                .collect(),
            Err(_) => all,
        }
    };

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
    let _ = writeln!(
        s,
        "  skip  agent client config (not checked by this build; `bssh ai install` sets it up)"
    );
    if let Some(e) = policy_error {
        let _ = writeln!(
            s,
            "  warn  policy not readable, no sudoers suggestions: {e}"
        );
    }
    for h in &report.hosts {
        let _ = writeln!(s, "\n{}  [{}]", h.host, h.level());
        if !h.reachable {
            let _ = writeln!(
                s,
                "  FAIL  unreachable: {}",
                h.error.as_deref().unwrap_or("unknown")
            );
            continue;
        }
        let _ = writeln!(s, "  ok    reachable");
        if h.agent_user {
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
                let _ = writeln!(s, "  ok    jail: {kind:?}");
            }
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
        DoctorReport {
            local: LocalReport {
                ca_initialised: true,
                ca_fingerprint: Some("SHA256:abc".into()),
                ca_sealed: false,
            },
            hosts,
            sudoers: vec![SudoersSuggestion {
                line: "bssh-agent ALL=(root) NOPASSWD: /usr/bin/systemctl reload nginx".into(),
                why: "nginx".into(),
            }],
            egress_note: hostsetup::EGRESS_NOTE,
        }
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
        assert!(text.contains("ok    jail: Bubblewrap"));
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
