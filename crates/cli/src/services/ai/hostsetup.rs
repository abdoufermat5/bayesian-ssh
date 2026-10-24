//! bssh ai setup / doctor / ca trust: scripts for the host, running them as root, and the
//! per-host checks behind `bssh ai doctor`.
//!
//!
//! bssh needs root on a host for exactly two things, both shown before they run:
//! creating the `bssh-agent` user ([`agent_user_script`]) and making sshd trust the bssh CA
//! for that account ([`trust_script`]). No firewall rules and no sudoers files, ever:
//! [`sudoers_suggestions`] and [`EGRESS_NOTE`] only tell the admin what to add.

use super::jail::{self, shell_quote};
use super::types::{
    Enforcement, ExecLimits, ExecMode, ExecResult, Executor, JailKind, Policy, Profile,
};
use crate::models::Connection;
use crate::services::transport::SubprocessTransport;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::io::IsTerminal;
use std::process::Stdio;
use std::time::Duration;

pub const AGENT_USER: &str = "bssh-agent";
/// The agent user's home: a system account, outside `/home` (often `noexec`).
pub const AGENT_HOME: &str = "/var/lib/bssh-agent";
/// Backups for `edit_file`, relative to the agent user's home.
pub const BACKUP_DIR: &str = "backups";
/// Where the CA public key lands on a host that had no `TrustedUserCAKeys` yet.
pub const HOST_CA_FILE: &str = "/etc/ssh/bssh_ai_ca.pub";
/// Per-user principals files: `<dir>/<user>` lists the principals allowed to log in as it.
pub const HOST_PRINCIPALS_DIR: &str = "/etc/ssh/bssh_ai_principals";
const SSH_DIR: &str = "/etc/ssh";

/// `sudo` wants a password and this run has no terminal for it.
pub const EXIT_SUDO_PASSWORD: i32 = 77;
/// Not root, and no `sudo` on the host.
pub const EXIT_NO_SUDO: i32 = 78;

/// Printed by `doctor`. bssh doesn't touch firewalls (Q6).
pub const EGRESS_NOTE: &str =
    "Restrict outbound traffic of the bssh-agent user on your external firewall \
(or host firewall) if you can. The read-only jail has no network and certificate mode only \
runs approved commands, but a firewall rule is the last line against exfiltration. \
bssh does not change firewalls.";

// ------------------------------------------------------------------ scripts

/// The script `bssh ai setup` runs as root. Idempotent: it creates the `bssh-agent` system
/// user (home, `/bin/sh`, no password, no sudo), adds it to `systemd-journal` and `adm`
/// when those groups exist, and creates `~/.local/bin` and the backup directory.
pub fn agent_user_script() -> String {
    agent_user_script_in(AGENT_HOME)
}

fn agent_user_script_in(home: &str) -> String {
    r#"set -eu
U=bssh-agent
if id "$U" >/dev/null 2>&1; then
  echo "user $U already exists"
else
  if command -v useradd >/dev/null 2>&1; then
    useradd --system --create-home --home-dir @HOME@ --shell /bin/sh --comment 'bssh AI agent (low privilege)' "$U"
    # '*' blocks password logins but, unlike a locked '!', lets sshd accept keys and certificates.
    usermod -p '*' "$U"
  elif command -v adduser >/dev/null 2>&1; then
    adduser -S -D -h @HOME@ -s /bin/sh "$U"
  else
    echo 'neither useradd nor adduser found: create the user bssh-agent by hand' >&2
    exit 3
  fi
  echo "created user $U"
fi
for g in systemd-journal adm; do
  if getent group "$g" >/dev/null 2>&1; then
    if command -v usermod >/dev/null 2>&1; then usermod -a -G "$g" "$U"; else addgroup "$U" "$g"; fi
    echo "$U is in group $g"
  fi
done
H=$(getent passwd "$U" | cut -d: -f6)
G=$(id -gn "$U")
mkdir -p "$H/.local/bin" "$H/backups"
chown "$U:$G" "$H" "$H/.local" "$H/.local/bin" "$H/backups"
chmod 0700 "$H" "$H/.local" "$H/.local/bin" "$H/backups"
echo "ready: $H"
"#
    .replace("@HOME@", &shell_quote(home))
}

/// The script `bssh ai ca trust` runs as root: put the CA public key where sshd reads
/// `TrustedUserCAKeys`, map the principal `bssh-agent` to the account `bssh-agent` only,
/// check with `sshd -t` (rolling back on failure) and reload sshd.
///
/// A host that already trusts another CA keeps its `TrustedUserCAKeys` file: our key is
/// appended there. The principal mapping is a `Match User bssh-agent` block at the end of
/// `sshd_config`, so it can never reach other accounts or leak into later directives.
pub fn trust_script(ca_public_key: &str) -> Result<String> {
    trust_script_in(ca_public_key, SSH_DIR)
}

fn trust_script_in(ca_public_key: &str, ssh_dir: &str) -> Result<String> {
    let line = ca_public_key.trim();
    let mut parts = line.split_whitespace();
    let (Some(kind), Some(body)) = (parts.next(), parts.next()) else {
        bail!("not an OpenSSH public key line");
    };
    let safe = |s: &str, extra: &str| {
        s.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
    };
    if !kind.starts_with("ssh-") || !safe(kind, "-") || !safe(body, "+/=") || line.contains('\n') {
        bail!("not a plain OpenSSH public key line");
    }
    // Keep the key and its comment, drop nothing else: the line is quoted anyway.
    Ok(r#"set -eu
CA_LINE=@LINE@
BODY=@BODY@
SSH_DIR=@SSH_DIR@
CFG=$SSH_DIR/sshd_config
DROPIN_DIR=$SSH_DIR/sshd_config.d
DROPIN=$DROPIN_DIR/50-bssh-ai.conf
CA_FILE=$SSH_DIR/bssh_ai_ca.pub
PRINC_DIR=$SSH_DIR/bssh_ai_principals
SSHD=$(command -v sshd 2>/dev/null || echo /usr/sbin/sshd)
BAK=$(mktemp -d)
HAD_CA_FILE=0; [ -e "$CA_FILE" ] && HAD_CA_FILE=1
HAD_DROPIN=0; [ -e "$DROPIN" ] && HAD_DROPIN=1
cp -p "$CFG" "$BAK/sshd_config"
[ "$HAD_CA_FILE" = 1 ] && cp -p "$CA_FILE" "$BAK/ca_file"
[ "$HAD_DROPIN" = 1 ] && cp -p "$DROPIN" "$BAK/dropin"

rollback() {
  echo 'sshd rejected the new configuration: restoring the previous one' >&2
  cp -p "$BAK/sshd_config" "$CFG"
  if [ "$HAD_DROPIN" = 1 ]; then cp -p "$BAK/dropin" "$DROPIN"; else rm -f "$DROPIN"; fi
  if [ "$HAD_CA_FILE" = 1 ]; then cp -p "$BAK/ca_file" "$CA_FILE"; else rm -f "$CA_FILE"; fi
}

# A host that already trusts a CA keeps its TrustedUserCAKeys file; ours joins it.
EXISTING=$("$SSHD" -T 2>/dev/null | awk '$1=="trustedusercakeys"{print $2; exit}' || true)
EXTERNAL=0
if [ -n "$EXISTING" ] && [ "$EXISTING" != none ]; then
  EXTERNAL=1
  CA_FILE=$EXISTING
  HAD_CA_FILE=1
  cp -p "$CA_FILE" "$BAK/ca_file"
fi
if ! grep -qsF "$BODY" "$CA_FILE"; then
  printf '%s\n' "$CA_LINE" >> "$CA_FILE"
fi
if [ "$EXTERNAL" = 0 ]; then
  chmod 0644 "$CA_FILE"
fi

mkdir -p "$PRINC_DIR"
printf 'bssh-agent\n' > "$PRINC_DIR/bssh-agent"
chmod 0755 "$PRINC_DIR"
chmod 0644 "$PRINC_DIR/bssh-agent"

mkdir -p "$DROPIN_DIR"
if [ "$EXTERNAL" = 0 ]; then
  printf '%s\n' '# Managed by bssh ai ca trust.' "TrustedUserCAKeys $CA_FILE" > "$DROPIN"
  chmod 0644 "$DROPIN"
fi
if [ "$EXTERNAL" = 0 ] && ! grep -qiE '^[[:space:]]*Include[[:space:]].*sshd_config\.d' "$CFG"; then
  cp -p "$CFG" "$CFG.bssh-new"
  { printf 'Include %s/*.conf\n' "$DROPIN_DIR"; cat "$CFG"; } > "$CFG.bssh-new"
  mv "$CFG.bssh-new" "$CFG"
fi
if ! grep -qF '# bssh-ai begin' "$CFG"; then
  {
    printf '\n# bssh-ai begin (bssh ai ca trust): principal bssh-agent may log in as bssh-agent only\n'
    printf 'Match User bssh-agent\n'
    printf '    AuthorizedPrincipalsFile %s/%%u\n' "$PRINC_DIR"
    printf '# bssh-ai end\n'
  } >> "$CFG"
fi
command -v restorecon >/dev/null 2>&1 && restorecon -R "$CA_FILE" "$PRINC_DIR" >/dev/null 2>&1 || true

if ! "$SSHD" -t; then
  rollback
  exit 1
fi
if systemctl reload sshd 2>/dev/null || systemctl reload ssh 2>/dev/null \
  || service sshd reload 2>/dev/null || service ssh reload 2>/dev/null; then
  echo 'sshd reloaded'
else
  echo 'sshd config is valid but could not be reloaded: reload sshd yourself' >&2
  exit 2
fi
echo "trusted CA key in $CA_FILE"
"#
    .replace("@LINE@", &shell_quote(line))
    .replace("@BODY@", &shell_quote(body))
    .replace("@SSH_DIR@", &shell_quote(ssh_dir)))
}

// ------------------------------------------------------------- running as root

/// One shell word that runs `script` as root: directly when we are root, else through
/// `sudo`. With `interactive == false` it never waits for a password: it exits with
/// [`EXIT_SUDO_PASSWORD`] instead. The script is a positional argument, quoted once.
pub fn root_command(script: &str, interactive: bool) -> String {
    const WRAPPER: &str = r#"if [ "$(id -u)" -eq 0 ]; then exec sh -c "$1"; fi
command -v sudo >/dev/null 2>&1 || { echo 'bssh: not root and sudo is not installed' >&2; exit 78; }
if [ "$2" = n ]; then
  sudo -n true 2>/dev/null || { echo 'bssh: sudo needs a password' >&2; exit 77; }
  exec sudo -n sh -c "$1"
fi
exec sudo sh -c "$1"
"#;
    format!(
        "sh -c {} bssh-root {} {}",
        shell_quote(WRAPPER),
        shell_quote(script),
        if interactive { "i" } else { "n" }
    )
}

/// Result of [`run_root_script`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRun {
    pub exit_code: i32,
    /// Captured output; empty when the run was interactive (it went to the terminal).
    pub output: String,
    pub interactive: bool,
}

/// Run `script` as root on `conn`: first without a password prompt (root login or
/// passwordless sudo) through `exec`; if sudo asks for a password and `allow_interactive`
/// is set and a terminal is attached, once more over `ssh -tt` so the human types it.
pub async fn run_root_script(
    conn: &Connection,
    exec: &dyn Executor,
    script: &str,
    allow_interactive: bool,
) -> Result<RootRun> {
    let limits = ExecLimits {
        timeout: Duration::from_secs(180),
        max_output_bytes: 64 * 1024,
    };
    let res = exec
        .exec(conn, &root_command(script, false), ExecMode::Plain, limits)
        .await?;
    if res.exit_code != Some(EXIT_SUDO_PASSWORD) {
        return Ok(RootRun {
            exit_code: res.exit_code.unwrap_or(-1),
            output: combined_output(&res),
            interactive: false,
        });
    }
    if !allow_interactive || !std::io::stdin().is_terminal() {
        bail!(
            "sudo on {} needs a password and there is no terminal to ask on. \
             Run this command in a terminal, or allow passwordless sudo for this one run.",
            conn.name
        );
    }
    let code = run_root_script_interactive(conn, script).await?;
    Ok(RootRun {
        exit_code: code,
        output: String::new(),
        interactive: true,
    })
}

async fn run_root_script_interactive(conn: &Connection, script: &str) -> Result<i32> {
    if conn.use_kerberos && conn.bastion.is_some() {
        bail!("interactive sudo through a Kerberos bastion is not supported: log in and run the script by hand");
    }
    let mut argv = SubprocessTransport::build_shell_argv(conn);
    argv.push(root_command(script, true));
    let (program, args) = argv.split_first().context("empty ssh command")?;
    let status = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .with_context(|| format!("could not start {program}"))?;
    Ok(status.code().unwrap_or(-1))
}

fn combined_output(res: &ExecResult) -> String {
    let mut out = res.stdout.clone();
    if !res.stderr.trim().is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&res.stderr);
    }
    out
}

// ---------------------------------------------------------------- host checks

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
            report.error = Some(format!(
                "test command failed (exit {:?}): {}",
                r.exit_code,
                r.stderr.trim()
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
    if let Ok(r) = run_plain(conn, exec, &format!("sh -c {passwd}")).await {
        if r.exit_code == Some(0) {
            if let Some((home, groups)) = parse_agent_user(&r.stdout) {
                report.agent_user = true;
                report.agent_home = Some(home);
                report.agent_groups = groups;
            }
        }
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

    if let Some(key) = ca_public_key {
        if let Some(body) = crate::services::ai::cert::key_body(key) {
            let check = shell_quote(&ca_check_script(body));
            if let Ok(r) = run_plain(conn, exec, &format!("sh -c {check}")).await {
                report.ca_trusted = Some(r.stdout.contains("bssh-ca-trusted"));
                report.principals_mapped = Some(r.stdout.contains("bssh-principals-ok"));
            }
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
fn ca_check_script(body: &str) -> String {
    format!(
        "f=$(cat {SSH_DIR}/sshd_config {SSH_DIR}/sshd_config.d/*.conf 2>/dev/null \
         | awk 'tolower($1)==\"trustedusercakeys\"{{print $2; exit}}'); \
         if [ -n \"$f\" ] && grep -qsF {body} \"$f\"; then echo bssh-ca-trusted; else echo bssh-ca-untrusted; fi; \
         if grep -qsx {AGENT_USER} {HOST_PRINCIPALS_DIR}/{AGENT_USER}; then echo bssh-principals-ok; fi",
        body = shell_quote(body)
    )
}

/// `getent passwd` line, then `id -nG` line -> (home, groups).
fn parse_agent_user(out: &str) -> Option<(String, Vec<String>)> {
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

// ------------------------------------------------------------------ suggestions

/// A line the admin may add to sudoers, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SudoersSuggestion {
    pub line: String,
    pub why: String,
}

/// Config directories whose services a policy may change, with the service name and an
/// optional config test (all with fixed arguments: no wildcards).
const SERVICE_HINTS: &[(&str, &str, Option<&str>)] = &[
    ("/etc/nginx", "nginx", Some("/usr/sbin/nginx -t")),
    (
        "/etc/apache2",
        "apache2",
        Some("/usr/sbin/apache2ctl configtest"),
    ),
    ("/etc/httpd", "httpd", Some("/usr/sbin/httpd -t")),
    (
        "/etc/haproxy",
        "haproxy",
        Some("/usr/sbin/haproxy -c -f /etc/haproxy/haproxy.cfg"),
    ),
    ("/etc/postgresql", "postgresql", None),
    ("/etc/redis", "redis-server", None),
    ("/etc/prometheus", "prometheus", None),
    ("/etc/grafana", "grafana-server", None),
];

/// Narrow sudoers lines for the services behind the policy's write paths. Never `ALL`:
/// each command is spelled out with its arguments. Nothing is installed by bssh.
pub fn sudoers_suggestions(policy: &Policy) -> Vec<SudoersSuggestion> {
    if policy.profile == Profile::Observe && policy.write_paths.is_empty() {
        return Vec::new();
    }
    let roots: Vec<String> = policy
        .write_paths
        .iter()
        .filter_map(|p| jail::read_root(p))
        .collect();
    let mut out = Vec::new();
    for (dir, service, test) in SERVICE_HINTS {
        if !roots.iter().any(|r| {
            r == dir
                || r.starts_with(&format!("{dir}/"))
                || (r != "/" && dir.starts_with(&format!("{r}/")))
        }) {
            continue;
        }
        let mut cmds = vec![
            format!("/usr/bin/systemctl reload {service}"),
            format!("/usr/bin/systemctl restart {service}"),
        ];
        if let Some(t) = test {
            cmds.push((*t).to_string());
        }
        out.push(SudoersSuggestion {
            line: format!("{AGENT_USER} ALL=(root) NOPASSWD: {}", cmds.join(", ")),
            why: format!("the policy may change files under {dir}: allow reloading {service} and testing its config"),
        });
    }
    if roots
        .iter()
        .any(|r| r == "/etc/systemd/system" || r.starts_with("/etc/systemd/system/"))
    {
        out.push(SudoersSuggestion {
            line: format!("{AGENT_USER} ALL=(root) NOPASSWD: /usr/bin/systemctl daemon-reload"),
            why: "the policy may change unit files under /etc/systemd/system".into(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::Path;
    use std::process::Command;

    fn conn() -> Connection {
        Connection::new(
            "web".into(),
            "web.test".into(),
            "ops".into(),
            22,
            None,
            None,
            false,
            None,
        )
    }

    fn ca_line() -> String {
        // Any valid-looking key line; the script only treats it as quoted text.
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHgGJ3W9P2bVx2w0n6sYhR3m7f1QzY9Y6c2uPq8dJvLk bssh-ai-ca@laptop"
            .to_string()
    }

    fn sh_syntax_ok(script: &str) {
        let mut child = Command::new("sh")
            .arg("-n")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn fake_bin(dir: &Path, name: &str, body: &str) {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn run_script(script: &str, bin: &Path, cwd: &Path) -> (i32, String, String) {
        run_script_with_path(script, &format!("{}:/usr/bin:/bin", bin.display()), cwd)
    }

    fn run_script_with_path(script: &str, path: &str, cwd: &Path) -> (i32, String, String) {
        let out = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .current_dir(cwd)
            .env("PATH", path)
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    // ---- setup script ----

    #[test]
    fn agent_script_has_the_documented_effects_and_nothing_more() {
        let s = agent_user_script();
        sh_syntax_ok(&s);
        assert!(s.contains("useradd --system --create-home"));
        assert!(s.contains("--shell /bin/sh"));
        assert!(s.contains(&shell_quote(AGENT_HOME)));
        assert!(s.contains("usermod -p '*'"));
        assert!(s.contains("systemd-journal adm"));
        assert!(s.contains("$H/.local/bin") && s.contains("$H/backups"));
        for forbidden in [
            "sudoers",
            "visudo",
            "iptables",
            "nft ",
            "ufw",
            "firewall-cmd",
            "passwd -d",
        ] {
            assert!(!s.contains(forbidden), "setup must not touch {forbidden}");
        }
    }

    #[test]
    fn agent_script_runs_against_fake_tools() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let home = tmp.path().join("home");
        let log = tmp.path().join("calls.log");
        let l = log.display();
        fake_bin(
            &bin,
            "id",
            "case \"$1\" in -gn) echo bssh-agent;; *) exit 1;; esac",
        );
        fake_bin(
            &bin,
            "useradd",
            &format!("echo \"useradd $*\" >> {l}; mkdir -p \"$4\""),
        );
        fake_bin(&bin, "usermod", &format!("echo \"usermod $*\" >> {l}"));
        fake_bin(
            &bin,
            "getent",
            &format!(
                "if [ \"$1\" = group ]; then [ \"$2\" = systemd-journal ]; \
                 else echo 'bssh-agent:x:998:998::{}:/bin/sh'; fi",
                home.display()
            ),
        );
        fake_bin(&bin, "chown", &format!("echo \"chown $*\" >> {l}"));
        let (code, out, err) = run_script(
            &agent_user_script_in(&home.display().to_string()),
            &bin,
            tmp.path(),
        );
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("created user bssh-agent"));
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(calls.contains(&format!(
            "useradd --system --create-home --home-dir {}",
            home.display()
        )));
        assert!(calls.contains("usermod -p * bssh-agent"));
        assert!(calls.contains("usermod -a -G systemd-journal bssh-agent"));
        assert!(
            !calls.contains("usermod -a -G adm"),
            "adm does not exist on this fake host"
        );
        assert!(home.join(".local/bin").is_dir() && home.join("backups").is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&home), 0o700);
            assert_eq!(mode(&home.join(".local/bin")), 0o700);
        }
    }

    #[test]
    fn agent_script_leaves_an_existing_user_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let log = tmp.path().join("calls.log");
        let l = log.display();
        fake_bin(
            &bin,
            "id",
            "case \"$1\" in -gn) echo bssh-agent;; *) exit 0;; esac",
        );
        fake_bin(&bin, "useradd", &format!("echo useradd >> {l}"));
        fake_bin(&bin, "usermod", &format!("echo \"usermod $*\" >> {l}"));
        fake_bin(
            &bin,
            "getent",
            &format!(
                "if [ \"$1\" = group ]; then exit 1; else echo 'bssh-agent:x:998:998::{}:/bin/sh'; fi",
                home.display()
            ),
        );
        fake_bin(&bin, "chown", "true");
        let (code, out, err) = run_script(
            &agent_user_script_in(&home.display().to_string()),
            &bin,
            tmp.path(),
        );
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("already exists"));
        assert!(!log.exists(), "no useradd/usermod when nothing is missing");
        assert!(home.join(".local/bin").is_dir());
    }

    // ---- trust script ----

    #[test]
    fn trust_script_rejects_anything_but_a_plain_key_line() {
        assert!(trust_script("").is_err());
        assert!(trust_script("ssh-ed25519").is_err());
        assert!(trust_script("ssh-ed25519 AAAA'; rm -rf / #").is_err());
        assert!(trust_script("ssh-ed25519 AAAA\nssh-ed25519 BBBB").is_err());
        assert!(trust_script("echo AAAA").is_err());
        let ok = trust_script(&ca_line()).unwrap();
        sh_syntax_ok(&ok);
        assert!(ok.contains("TrustedUserCAKeys"));
        assert!(ok.contains("AuthorizedPrincipalsFile"));
        assert!(ok.contains("Match User bssh-agent"));
        assert!(ok.contains("\"$SSHD\" -t"));
        assert!(ok.contains("systemctl reload sshd"));
    }

    /// A scratch `/etc/ssh` plus fake `sshd` and `systemctl`.
    struct SshBox {
        tmp: tempfile::TempDir,
    }

    impl SshBox {
        fn new(sshd_t_ok: bool, trusted_ca: Option<&str>, config: &str) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir(tmp.path().join("bin")).unwrap();
            std::fs::create_dir(tmp.path().join("ssh")).unwrap();
            std::fs::write(tmp.path().join("ssh/sshd_config"), config).unwrap();
            let t = match trusted_ca {
                Some(path) => format!("echo 'trustedusercakeys {path}'"),
                None => "echo 'trustedusercakeys none'".to_string(),
            };
            fake_bin(
                &tmp.path().join("bin"),
                "sshd",
                &format!(
                    "if [ \"$1\" = -T ]; then echo 'port 22'; {t}; exit 0; fi\n\
                     {}",
                    if sshd_t_ok {
                        "exit 0"
                    } else {
                        "echo 'bad config' >&2; exit 255"
                    }
                ),
            );
            fake_bin(
                &tmp.path().join("bin"),
                "systemctl",
                &format!(
                    "echo \"systemctl $*\" >> {}/systemctl.log",
                    tmp.path().display()
                ),
            );
            Self { tmp }
        }

        fn ssh(&self) -> String {
            self.tmp.path().join("ssh").display().to_string()
        }

        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.tmp.path().join("ssh").join(rel)).unwrap_or_default()
        }

        fn run(&self) -> (i32, String, String) {
            let script = trust_script_in(&ca_line(), &self.ssh()).unwrap();
            run_script(&script, &self.tmp.path().join("bin"), self.tmp.path())
        }
    }

    #[test]
    fn trust_installs_the_ca_principals_and_reloads() {
        let b = SshBox::new(true, None, "Port 22\nPermitRootLogin no\n");
        let (code, out, err) = b.run();
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("sshd reloaded"));
        assert_eq!(b.read("bssh_ai_ca.pub"), format!("{}\n", ca_line()));
        assert_eq!(b.read("bssh_ai_principals/bssh-agent"), "bssh-agent\n");
        let drop_in = b.read("sshd_config.d/50-bssh-ai.conf");
        assert!(drop_in.contains(&format!("TrustedUserCAKeys {}/bssh_ai_ca.pub", b.ssh())));
        let cfg = b.read("sshd_config");
        assert!(
            cfg.starts_with(&format!("Include {}/sshd_config.d/*.conf\n", b.ssh())),
            "{cfg}"
        );
        assert!(
            cfg.contains("Port 22\nPermitRootLogin no\n"),
            "existing settings are kept"
        );
        let block = cfg.split("# bssh-ai begin").nth(1).expect("our block");
        assert!(block.contains("Match User bssh-agent"));
        assert!(block.contains(&format!(
            "AuthorizedPrincipalsFile {}/bssh_ai_principals/%u",
            b.ssh()
        )));
        assert!(
            cfg.trim_end().ends_with("# bssh-ai end"),
            "the Match block is the last thing in sshd_config"
        );
        let reload = std::fs::read_to_string(b.tmp.path().join("systemctl.log")).unwrap();
        assert!(reload.contains("systemctl reload sshd"));
    }

    #[test]
    fn trust_is_idempotent() {
        let b = SshBox::new(
            true,
            None,
            "Include /etc/ssh/sshd_config.d/*.conf\nPort 22\n",
        );
        assert_eq!(b.run().0, 0);
        let cfg1 = b.read("sshd_config");
        let ca1 = b.read("bssh_ai_ca.pub");
        assert_eq!(b.run().0, 0);
        assert_eq!(
            b.read("sshd_config"),
            cfg1,
            "no duplicate blocks or includes"
        );
        assert_eq!(b.read("bssh_ai_ca.pub"), ca1, "key is not appended twice");
        assert_eq!(
            cfg1.matches("Include").count(),
            1,
            "an existing Include is reused"
        );
    }

    #[test]
    fn trust_rolls_back_when_sshd_rejects_the_config() {
        let original = "Port 22\n";
        let b = SshBox::new(false, None, original);
        let (code, _, err) = b.run();
        assert_eq!(code, 1);
        assert!(err.contains("restoring the previous"), "{err}");
        assert_eq!(b.read("sshd_config"), original);
        assert!(!b.tmp.path().join("ssh/bssh_ai_ca.pub").exists());
        assert!(!b
            .tmp
            .path()
            .join("ssh/sshd_config.d/50-bssh-ai.conf")
            .exists());
        assert!(
            !b.tmp.path().join("systemctl.log").exists(),
            "no reload after a failed check"
        );
    }

    #[test]
    fn trust_joins_an_existing_ca_file_instead_of_replacing_it() {
        let tmp_ca = tempfile::tempdir().unwrap();
        let company = tmp_ca.path().join("company_ca.pub");
        std::fs::write(&company, "ssh-ed25519 AAAAcompanyca company\n").unwrap();
        let b = SshBox::new(true, Some(&company.display().to_string()), "Port 22\n");
        let (code, _, err) = b.run();
        assert_eq!(code, 0, "{err}");
        let ca_file = std::fs::read_to_string(&company).unwrap();
        assert!(ca_file.starts_with("ssh-ed25519 AAAAcompanyca company\n"));
        assert!(ca_file.contains(&ca_line()));
        assert!(
            !b.ssh().is_empty()
                && !b
                    .tmp
                    .path()
                    .join("ssh/sshd_config.d/50-bssh-ai.conf")
                    .exists(),
            "no second TrustedUserCAKeys directive"
        );
        assert!(b.read("sshd_config").contains("Match User bssh-agent"));
    }

    #[test]
    fn failed_check_restores_a_shared_ca_file() {
        let tmp_ca = tempfile::tempdir().unwrap();
        let company = tmp_ca.path().join("company_ca.pub");
        std::fs::write(&company, "ssh-ed25519 AAAAcompanyca company\n").unwrap();
        let b = SshBox::new(false, Some(&company.display().to_string()), "Port 22\n");
        assert_eq!(b.run().0, 1);
        assert_eq!(
            std::fs::read_to_string(&company).unwrap(),
            "ssh-ed25519 AAAAcompanyca company\n"
        );
        assert_eq!(b.read("sshd_config"), "Port 22\n");
    }

    // ---- root wrapper ----

    #[test]
    fn root_command_passes_the_script_as_one_quoted_argument() {
        for script in ["id", "echo 'a'; $(x) `y`", "x\ny", "'; touch PWNED; '"] {
            for interactive in [false, true] {
                let line = root_command(script, interactive);
                sh_syntax_ok(&line);
                assert!(line.starts_with("sh -c '"));
                assert!(line.ends_with(if interactive { " i" } else { " n" }));
                assert!(line.contains(&shell_quote(script)));
            }
        }
    }

    #[test]
    fn root_command_runs_directly_as_root_and_asks_sudo_otherwise() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let log = tmp.path().join("sudo.log");
        // Not root, passwordless sudo available.
        fake_bin(&bin, "id", "echo 1000");
        fake_bin(
            &bin,
            "sudo",
            &format!(
                "echo \"sudo $*\" >> {}\nif [ \"$2\" = true ]; then exit 0; fi\nshift\nexec \"$@\"",
                log.display()
            ),
        );
        let (code, out, _) = run_script(&root_command("echo script-ran", false), &bin, tmp.path());
        assert_eq!(code, 0);
        assert!(out.contains("script-ran"), "{out}");
        assert!(std::fs::read_to_string(&log).unwrap().contains("sudo -n"));

        // Not root, sudo wants a password.
        fake_bin(&bin, "sudo", "exit 1");
        let (code, _, err) = run_script(&root_command("echo nope", false), &bin, tmp.path());
        assert_eq!(code, EXIT_SUDO_PASSWORD);
        assert!(err.contains("password"));

        // No sudo at all.
        std::fs::remove_file(bin.join("sudo")).unwrap();
        // Only the fake `id` and a `sh` are on PATH: the host's own sudo must stay out of sight.
        std::os::unix::fs::symlink("/bin/sh", bin.join("sh")).unwrap();
        let (code, _, _) = run_script_with_path(
            &root_command("echo nope", false),
            &bin.display().to_string(),
            tmp.path(),
        );
        assert_eq!(code, EXIT_NO_SUDO);

        // Root: sudo is never touched.
        fake_bin(&bin, "id", "echo 0");
        let (code, out, _) = run_script(&root_command("echo as-root", false), &bin, tmp.path());
        assert_eq!((code, out.trim()), (0, "as-root"));
    }

    // ---- fake executor, host checks ----

    #[derive(Clone, Default)]
    struct Host {
        reachable: bool,
        agent: bool,
        helper: bool,
        ca_trusted: bool,
        principals: bool,
        landlock: bool,
    }

    #[async_trait]
    impl Executor for Host {
        async fn exec(
            &self,
            _: &Connection,
            command: &str,
            mode: ExecMode,
            _: ExecLimits,
        ) -> Result<ExecResult> {
            assert_eq!(mode, ExecMode::Plain);
            if !self.reachable {
                bail!("connection refused");
            }
            let ok = |s: &str| ExecResult {
                stdout: s.to_string(),
                stderr: String::new(),
                exit_code: Some(0),
                truncated: false,
                timed_out: false,
                duration_ms: 1,
            };
            let fail = |code: i32| ExecResult {
                stdout: String::new(),
                stderr: "nope".into(),
                exit_code: Some(code),
                truncated: false,
                timed_out: false,
                duration_ms: 1,
            };
            if command.contains("echo bssh-ok") {
                Ok(ok("bssh-ok\n"))
            } else if command.contains("getent passwd") {
                if self.agent {
                    Ok(ok("bssh-agent:x:998:998::/var/lib/bssh-agent:/bin/sh\nbssh-agent systemd-journal adm\n"))
                } else {
                    Ok(fail(2))
                }
            } else if command.contains("bssh-yes") {
                Ok(ok(if self.helper { "bssh-yes\n" } else { "" }))
            } else if command.contains("bssh-ca-trusted") {
                let mut out = String::from(if self.ca_trusted {
                    "bssh-ca-trusted\n"
                } else {
                    "bssh-ca-untrusted\n"
                });
                if self.principals {
                    out.push_str("bssh-principals-ok\n");
                }
                Ok(ok(&out))
            } else if command.contains("setpriv") && self.landlock {
                Ok(ok("bssh-jail-ok\nbssh-jail-readonly\n"))
            } else {
                Ok(fail(126))
            }
        }
    }

    #[tokio::test]
    async fn report_for_a_fully_prepared_host() {
        let host = Host {
            reachable: true,
            agent: true,
            ca_trusted: true,
            principals: true,
            landlock: true,
            ..Host::default()
        };
        let r = check_host(&conn(), &host, Some(&ca_line())).await;
        assert!(r.reachable && r.agent_user);
        assert_eq!(r.agent_home.as_deref(), Some("/var/lib/bssh-agent"));
        assert_eq!(r.agent_groups, ["bssh-agent", "systemd-journal", "adm"]);
        assert_eq!(r.jail, JailKind::Landlock);
        assert_eq!(r.ca_trusted, Some(true));
        assert_eq!(r.principals_mapped, Some(true));
        assert_eq!(r.level(), "certificate-enforced");
        assert_eq!(
            r.enforcement,
            Enforcement {
                jail: JailKind::Landlock,
                certificates: true,
                agent_user: true
            }
        );
    }

    #[tokio::test]
    async fn report_for_a_bare_host_is_client_enforced() {
        let host = Host {
            reachable: true,
            ..Host::default()
        };
        let r = check_host(&conn(), &host, None).await;
        assert!(r.reachable && !r.agent_user);
        assert_eq!(r.jail, JailKind::None);
        assert_eq!(r.ca_trusted, None, "no local CA, nothing to compare");
        assert_eq!(r.level(), "client-enforced");
        assert_eq!(r.enforcement, Enforcement::default());
    }

    #[tokio::test]
    async fn untrusted_ca_means_no_certificate_enforcement() {
        let host = Host {
            reachable: true,
            agent: true,
            landlock: true,
            ..Host::default()
        };
        let r = check_host(&conn(), &host, Some(&ca_line())).await;
        assert_eq!(r.ca_trusted, Some(false));
        assert!(!r.enforcement.certificates);
        assert_eq!(r.level(), "jail-enforced");
    }

    #[tokio::test]
    async fn unreachable_host_is_reported_not_raised() {
        let r = check_host(&conn(), &Host::default(), Some(&ca_line())).await;
        assert!(!r.reachable);
        assert!(r
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("connection refused"));
    }

    #[test]
    fn agent_user_output_parsing() {
        assert_eq!(
            parse_agent_user(
                "bssh-agent:x:998:998:c:/var/lib/bssh-agent:/bin/sh\nbssh-agent adm\n"
            ),
            Some((
                "/var/lib/bssh-agent".into(),
                vec!["bssh-agent".into(), "adm".into()]
            ))
        );
        assert_eq!(parse_agent_user("other:x:1:1::/home/o:/bin/sh\n"), None);
        assert_eq!(parse_agent_user("garbage"), None);
        assert_eq!(parse_agent_user(""), None);
    }

    #[test]
    fn ca_check_script_quotes_the_key_and_parses() {
        sh_syntax_ok(&ca_check_script("AAAAC3+/="));
        let s = ca_check_script("AAA'; touch PWNED; '");
        sh_syntax_ok(&s);
        assert!(!s.contains("; touch PWNED; ;"));
    }

    // ---- sudoers suggestions ----

    fn policy(profile: Profile, writes: &[&str]) -> Policy {
        Policy {
            profile,
            write_paths: writes.iter().map(|s| s.to_string()).collect(),
            ..Policy::default()
        }
    }

    #[test]
    fn sudoers_lines_are_narrow_and_follow_the_write_paths() {
        let s = sudoers_suggestions(&policy(
            Profile::Operate,
            &["/etc/nginx/**", "/etc/systemd/system/**"],
        ));
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[0].line,
            "bssh-agent ALL=(root) NOPASSWD: /usr/bin/systemctl reload nginx, \
             /usr/bin/systemctl restart nginx, /usr/sbin/nginx -t"
        );
        assert_eq!(
            s[1].line,
            "bssh-agent ALL=(root) NOPASSWD: /usr/bin/systemctl daemon-reload"
        );
        for sug in &s {
            assert!(
                !sug.line.contains("ALL=(ALL)") && !sug.line.contains(": ALL"),
                "{}",
                sug.line
            );
            assert!(!sug.line.contains('*'));
            assert!(!sug.why.is_empty());
        }
    }

    #[test]
    fn observe_policies_and_unknown_paths_suggest_nothing() {
        assert!(sudoers_suggestions(&policy(Profile::Observe, &[])).is_empty());
        assert!(sudoers_suggestions(&policy(Profile::Operate, &[])).is_empty());
        assert!(sudoers_suggestions(&policy(Profile::Operate, &["/srv/app/**"])).is_empty());
        assert!(sudoers_suggestions(&policy(Profile::Operate, &["**/*.conf"])).is_empty());
    }

    #[test]
    fn egress_note_does_not_promise_firewall_changes() {
        assert!(EGRESS_NOTE.contains("does not change firewalls"));
    }
}
