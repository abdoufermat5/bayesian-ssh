//! The per-host checks behind `bssh ai doctor`.

use crate::models::Connection;
use crate::services::ai::jail::{self, shell_quote};
use crate::services::ai::types::{
    Enforcement, ExecLimits, ExecMode, ExecResult, Executor, JailKind,
};
use anyhow::Result;
use serde::Serialize;
use std::time::Duration;

use super::scripts::{AGENT_USER, HOST_PRINCIPALS_DIR, SSH_DIR, truncate_chars};

/// What `bssh ai doctor` found on one host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HostReport {
    pub host: String,
    pub reachable: bool,
    /// Why the host couldn't be checked.
    pub error: Option<String>,
    pub agent_user: bool,
    pub agent_groups: Vec<String>,
    pub agent_home: Option<String>,
    /// `bssh-jail` helper installed for the agent user.
    pub jail_helper: bool,
    pub jail: JailKind,
    /// sshd trusts the local CA. `None`: no local CA to compare with.
    pub ca_trusted: Option<bool>,
    /// The principals file maps `bssh-agent` to the account `bssh-agent`.
    pub principals_mapped: Option<bool>,
    pub enforcement: Enforcement,
}

impl HostReport {
    /// One word for how strongly the host is enforced.
    pub fn level(&self) -> &'static str {
        let e = &self.enforcement;
        if e.certificates && e.agent_user {
            "certificate-enforced"
        } else if e.jail != JailKind::None && e.agent_user {
            "jail-enforced"
        } else if e.jail != JailKind::None {
            "jail-only"
        } else {
            "client-enforced"
        }
    }
}

const PROBE_LIMITS: ExecLimits = ExecLimits {
    timeout: Duration::from_secs(20),
    max_output_bytes: 16 * 1024,
};

async fn run_plain(conn: &Connection, exec: &dyn Executor, command: &str) -> Result<ExecResult> {
    exec.exec(conn, command, ExecMode::Plain, PROBE_LIMITS)
        .await
}

/// Probe one host. Never fails: problems end up in the report.
/// `ca_public_key` is the local CA's OpenSSH line, when there is one.
pub async fn check_host(
    conn: &Connection,
    exec: &dyn Executor,
    ca_public_key: Option<&str>,
) -> HostReport {
    let mut report = HostReport {
        host: conn.name.clone(),
        ..HostReport::default()
    };
    match run_plain(conn, exec, "echo bssh-ok").await {
        Ok(r) if r.exit_code == Some(0) && r.stdout.contains("bssh-ok") => report.reachable = true,
        Ok(r) => {
            // Prefer stderr; fall back to a short stdout snippet so a bastion
            // banner dumped on the PTY does not become a multi-page error.
            let detail = {
                let err = r.stderr.trim();
                if !err.is_empty() {
                    truncate_chars(err, 160)
                } else {
                    truncate_chars(r.stdout.trim(), 160)
                }
            };
            report.error = Some(format!(
                "test command failed ({}): {}",
                match r.exit_code {
                    Some(code) => format!("exit {code}"),
                    None => "no exit status".into(),
                },
                detail
            ));
            return report;
        }
        Err(e) => {
            report.error = Some(format!("{e:#}"));
            return report;
        }
    }

    let passwd = shell_quote(&format!(
        "getent passwd {AGENT_USER} && id -nG {AGENT_USER}"
    ));
    if let Ok(r) = run_plain(conn, exec, &format!("sh -c {passwd}")).await
        && r.exit_code == Some(0)
        && let Some((home, groups)) = parse_agent_user(&r.stdout)
    {
        report.agent_user = true;
        report.agent_home = Some(home);
        report.agent_groups = groups;
    }
    if let Some(home) = &report.agent_home {
        let helper = format!("{home}/{}", jail::HELPER_REL_PATH);
        let test = shell_quote(&format!("[ -x {} ] && echo bssh-yes", shell_quote(&helper)));
        if let Ok(r) = run_plain(conn, exec, &format!("sh -c {test}")).await {
            report.jail_helper = r.stdout.contains("bssh-yes");
        }
    }

    match jail::probe(conn, exec).await {
        Ok(kind) => report.jail = kind,
        Err(e) => report.error = Some(format!("jail probe failed: {e:#}")),
    }

    if let Some(key) = ca_public_key
        && let Some(body) = crate::services::ai::cert::key_body(key)
    {
        let check = shell_quote(&ca_check_script(body));
        if let Ok(r) = run_plain(conn, exec, &format!("sh -c {check}")).await {
            report.ca_trusted = Some(r.stdout.contains("bssh-ca-trusted"));
            report.principals_mapped = Some(r.stdout.contains("bssh-principals-ok"));
        }
    }

    report.enforcement = Enforcement {
        jail: report.jail,
        certificates: report.agent_user
            && report.ca_trusted == Some(true)
            && report.principals_mapped != Some(false),
        agent_user: report.agent_user,
    };
    report
}

/// Does sshd read a `TrustedUserCAKeys` file holding this key, and is the principals file there?
pub(super) fn ca_check_script(body: &str) -> String {
    format!(
        "f=$(cat {SSH_DIR}/sshd_config {SSH_DIR}/sshd_config.d/*.conf 2>/dev/null \
         | awk 'tolower($1)==\"trustedusercakeys\"{{print $2; exit}}'); \
         if [ -n \"$f\" ] && grep -qsF {body} \"$f\"; then echo bssh-ca-trusted; else echo bssh-ca-untrusted; fi; \
         if grep -qsx {AGENT_USER} {HOST_PRINCIPALS_DIR}/{AGENT_USER}; then echo bssh-principals-ok; fi",
        body = shell_quote(body)
    )
}

/// `getent passwd` line, then `id -nG` line -> (home, groups).
pub(super) fn parse_agent_user(out: &str) -> Option<(String, Vec<String>)> {
    let mut lines = out.lines();
    let passwd = lines.next()?;
    let fields: Vec<&str> = passwd.split(':').collect();
    if fields.len() < 7 || fields[0] != AGENT_USER {
        return None;
    }
    let groups = lines
        .next()
        .map(|l| l.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    Some((fields[5].to_string(), groups))
}
