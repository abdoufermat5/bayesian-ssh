//! The shell scripts shown to the admin before they run as root.

use crate::services::ai::jail::shell_quote;
use anyhow::{bail, Result};

pub const AGENT_USER: &str = "bssh-agent";
/// The agent user's home: a system account, outside `/home` (often `noexec`).
pub const AGENT_HOME: &str = "/var/lib/bssh-agent";

/// First `max` chars of `s`, with an ellipsis when truncated. Used so a bastion
/// MOTD never becomes the whole doctor error line.
pub(super) fn truncate_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
/// Backups for `edit_file`, relative to the agent user's home.
pub const BACKUP_DIR: &str = "backups";
/// Where the CA public key lands on a host that had no `TrustedUserCAKeys` yet.
pub const HOST_CA_FILE: &str = "/etc/ssh/bssh_ai_ca.pub";
/// Per-user principals files: `<dir>/<user>` lists the principals allowed to log in as it.
pub const HOST_PRINCIPALS_DIR: &str = "/etc/ssh/bssh_ai_principals";
pub(super) const SSH_DIR: &str = "/etc/ssh";

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

pub(super) fn agent_user_script_in(home: &str) -> String {
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

pub(super) fn trust_script_in(ca_public_key: &str, ssh_dir: &str) -> Result<String> {
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
