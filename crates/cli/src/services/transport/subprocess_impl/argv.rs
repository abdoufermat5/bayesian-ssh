//! Argv builders for `ssh`: quoting, host-key options, jump hosts and the
//! exec, shell, forward and dynamic-proxy command lines.

//! Subprocess transport — shells out to `ssh`/`scp`/`sftp`.
//!
//! This is the existing behavior extracted behind the `SshTransport` trait.

use crate::config::HostKeyPolicy;
use crate::models::Connection;
use std::path::PathBuf;
use tokio::process::Command as TokioCommand;

use super::SubprocessTransport;

/// POSIX single-quote a value for interpolation into a shell command line
/// (`sh -c`). `ssh -o ProxyCommand=...` runs its value through the shell, so
/// any unquoted user-controlled string (bastion host, key path, …) is a
/// command-injection vector. Single quotes make the value opaque to the
/// shell; embedded `'` is escaped as `'"'"'`.
pub(crate) fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if !value.contains([
        '\'', '\\', ' ', '\t', '\n', '"', '$', '`', ';', '&', '|', '<', '>', '(', ')', '*', '?',
        '[', ']', '{', '}', '!', '#', '~',
    ]) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Quote a value for a `ProxyCommand`: ssh first performs `%` token
/// expansion, then hands the string to `sh -c`, so literal `%` must be
/// doubled on top of the shell quoting.
fn proxy_quote(value: &str) -> String {
    shell_quote(value).replace('%', "%%")
}

/// Agent and CI shells often export `TERM=dumb`. Some Kerberos bastions then
/// refuse the session with a misleading "add -t" message even when `-tt` was
/// sent. Give `ssh` a real terminal type whenever ours is missing or dumb.
pub(super) fn prepare_ssh_command(program: &str) -> TokioCommand {
    let mut cmd = TokioCommand::new(program);
    let bad_term = match std::env::var_os("TERM") {
        None => true,
        Some(t) if t.is_empty() => true,
        Some(t) => t.eq_ignore_ascii_case("dumb"),
    };
    if bad_term {
        cmd.env("TERM", "xterm-256color");
    }
    cmd
}

/// Map the config policy (`strict` | `accept-new` | `off`, plus the OpenSSH
/// `yes`/`no` aliases) onto a value the OpenSSH client accepts. `strict` is
/// not an OpenSSH keyword — passing it verbatim makes `ssh` abort with
/// "unsupported option". An unrecognised value fails closed to `yes`.
pub(crate) fn ssh_strict_host_key_value(policy: &str) -> &'static str {
    match HostKeyPolicy::from_config(policy) {
        HostKeyPolicy::Strict => "yes",
        HostKeyPolicy::AcceptNew => "accept-new",
        HostKeyPolicy::Off => "no",
    }
}

/// Extra `ssh` switches for the jump-host hop (`ProxyCommand`).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct HopOptions {
    /// Never prompt on the hop: a broken bastion login fails instead of waiting for input.
    pub non_interactive: bool,
    pub connect_timeout_secs: Option<u64>,
}

/// Build the `ProxyCommand=` value for a classic jump host.
///
/// `leading_opts` are inserted right after `ssh` (e.g. TTY flags). `hop`
/// carries the non-interactive switches. Every connection field is quoted, the
/// bastion destination follows `--` so a value starting with `-` cannot become
/// an option, and the target is only passed as `%h` when it is shell-inert:
/// OpenSSH < 9.6 substitutes `%h` into the shell command unquoted.
pub(crate) fn jump_proxy_command(
    conn: &Connection,
    leading_opts: &str,
    shkc: &str,
    hop: &HopOptions,
) -> String {
    let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
    let bastion = conn.bastion.as_deref().unwrap_or_default();
    let key_flag = conn
        .key_path
        .as_deref()
        .map(|k| format!(" -i {}", proxy_quote(k)))
        .unwrap_or_default();
    let mut hop_flags = String::new();
    if hop.non_interactive {
        hop_flags.push_str(" -o BatchMode=yes");
    }
    if let Some(secs) = hop.connect_timeout_secs {
        hop_flags.push_str(&format!(" -o ConnectTimeout={secs}"));
    }
    let target = if shell_quote(&conn.host) == conn.host {
        "%h:%p".to_string()
    } else if conn.host.contains(':') {
        proxy_quote(&format!("[{}]:{}", conn.host, conn.port))
    } else {
        proxy_quote(&format!("{}:{}", conn.host, conn.port))
    };
    format!(
        "ssh{leading_opts}{hop_flags}{key_flag} -o StrictHostKeyChecking={} -W {target} -- {}@{}",
        ssh_strict_host_key_value(shkc),
        proxy_quote(bu),
        proxy_quote(bastion)
    )
}

/// Switches for a shell-style (PTY) connection driven by an agent run.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ShellExtras {
    /// Never prompt on any hop; no agent or port forwarding.
    pub non_interactive: bool,
    pub connect_timeout_secs: Option<u64>,
}

/// Extra `ssh` switches for [`SshTransport::exec_limited`].
#[derive(Debug, Default, Clone)]
pub(crate) struct ArgvExtras {
    /// `BatchMode=yes`, no agent/X11/port forwarding.
    pub non_interactive: bool,
    pub connect_timeout_secs: Option<u64>,
    /// Private key and certificate files to authenticate with (and nothing else).
    pub cert: Option<(PathBuf, PathBuf)>,
    /// Directory for `ControlMaster` sockets (run-scoped SSH connection sharing).
    pub control_dir: Option<PathBuf>,
}

impl SubprocessTransport {
    /// Build the argv for a non-interactive exec call.
    ///
    /// NOTE: this is used for direct connections and classic jump-host
    /// bastions only.  Interactive bastions (kerberos + bastion) cannot
    /// pass a remote command via the argv — use `run_interactive_exec`
    /// instead.
    pub(crate) fn build_exec_argv(conn: &Connection, command: &str, shkc: &str) -> Vec<String> {
        Self::build_exec_argv_with(conn, command, shkc, &ArgvExtras::default())
    }

    /// [`Self::build_exec_argv`] plus the AI/limited-exec switches: `BatchMode`,
    /// `ConnectTimeout`, no forwarding, and a certificate identity.
    pub(crate) fn build_exec_argv_with(
        conn: &Connection,
        command: &str,
        shkc: &str,
        extras: &ArgvExtras,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        // Non-interactive: no remote PTY. A forced TTY makes ssh print
        // "Connection to <host> closed." and merges remote stderr into stdout.
        argv.push("-T".into());
        if extras.non_interactive {
            // An agent run must not hand the user's agent or ports to the host.
            argv.push("-a".into());
            argv.push("-x".into());
            argv.push("-o".into());
            argv.push("ClearAllForwardings=yes".into());
            // Never prompt for a password, passphrase or host key.
            argv.push("-o".into());
            argv.push("BatchMode=yes".into());
        }
        if let Some(secs) = extras.connect_timeout_secs {
            argv.push("-o".into());
            argv.push(format!("ConnectTimeout={secs}"));
        }
        if conn.use_kerberos {
            if extras.non_interactive {
                // A ticket may prove who we are; it must not be forwarded to the host.
                argv.push("-o".into());
                argv.push("GSSAPIAuthentication=yes".into());
                argv.push("-o".into());
                argv.push("GSSAPIDelegateCredentials=no".into());
            } else {
                argv.push("-K".into());
            }
        }
        if let Some((key, cert)) = &extras.cert {
            // Only the per-command certificate: no other identity may be offered.
            argv.push("-o".into());
            argv.push("IdentitiesOnly=yes".into());
            argv.push("-i".into());
            argv.push(key.to_string_lossy().into_owned());
            argv.push("-o".into());
            argv.push(format!("CertificateFile={}", cert.to_string_lossy()));
        } else if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        // One ssh connection per run: a bastion handshake is paid once, not per call.
        // Not with a certificate: every certified call is a different identity.
        if extras.non_interactive && extras.cert.is_none() {
            if let Some(dir) = &extras.control_dir {
                argv.push("-o".into());
                argv.push("ControlMaster=auto".into());
                argv.push("-o".into());
                argv.push(format!("ControlPath={}", dir.join("%C").to_string_lossy()));
                argv.push("-o".into());
                argv.push("ControlPersist=60".into());
            }
        }
        argv.push("-o".into());
        argv.push(format!(
            "StrictHostKeyChecking={}",
            ssh_strict_host_key_value(shkc)
        ));

        if conn.bastion.is_some() {
            let hop = HopOptions {
                non_interactive: extras.non_interactive,
                connect_timeout_secs: extras.connect_timeout_secs,
            };
            argv.push("-o".into());
            argv.push(format!(
                "ProxyCommand={}",
                jump_proxy_command(conn, " -T", shkc, &hop)
            ));
        }
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        // `--` so a user/host beginning with `-` cannot be parsed as an option.
        argv.push("--".into());
        argv.push(format!("{}@{}", conn.user, conn.host));
        argv.push(command.to_string());
        argv
    }

    /// Build the argv for an interactive shell session.
    ///
    /// When Kerberos + bastion are both active the bastion is an *interactive*
    /// bastion: we SSH into it and pass `target_user@target` as argument.
    /// Without Kerberos the bastion is a classic jump host using ProxyCommand
    /// with forced TTY.
    ///
    /// `policy` is applied to the bastion hop. The target's host-key prompt is
    /// left to the user's ssh configuration (OpenSSH default: interactive
    /// `ask` prompt in the terminal); CLI paths that must enforce the policy
    /// on the target call `build_shell_argv_with_shkc` with enforcement on.
    pub fn build_shell_argv(conn: &Connection, policy: HostKeyPolicy) -> Vec<String> {
        Self::build_shell_argv_with_shkc(conn, policy, false)
    }

    /// `enforce_target_shkc`: also pass the policy for the target connection
    /// (CLI paths honour `transport.strict_host_key_checking`).
    pub(super) fn build_shell_argv_with_shkc(
        conn: &Connection,
        policy: HostKeyPolicy,
        enforce_target_shkc: bool,
    ) -> Vec<String> {
        Self::build_shell_argv_ext(conn, policy, enforce_target_shkc, &ShellExtras::default())
    }

    /// Shell-style argv with agent switches: `extras.non_interactive` keeps the PTY
    /// (an interactive bastion needs one) but never prompts on any hop, never forwards
    /// the agent, and does not delegate the Kerberos ticket.
    pub(super) fn build_shell_argv_ext(
        conn: &Connection,
        policy: HostKeyPolicy,
        enforce_target_shkc: bool,
        extras: &ShellExtras,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        // Force remote TTY allocation even when stdin is not a terminal (e.g. GUI background process)
        argv.push("-tt".into());
        argv.push("-o".into());
        argv.push("RequestTTY=force".into());
        // Kerberos + bastion is an interactive bastion: it SSHs to the target as
        // the user, so the agent hop must carry the same credentials a human
        // session uses (`-A` when opted in, TGT delegation for GSSAPI targets).
        let interactive_bastion = conn.use_kerberos && conn.bastion.is_some();
        if extras.non_interactive {
            if interactive_bastion && conn.forward_agent {
                argv.push("-A".into());
            } else {
                // Direct/jump-host agent runs must not hand the user's agent to the host.
                argv.push("-a".into());
                argv.push("-o".into());
                argv.push("ClearAllForwardings=yes".into());
            }
            argv.push("-x".into());
            // Fail instead of waiting for a password/OTP prompt.
            argv.push("-o".into());
            argv.push("BatchMode=yes".into());
            if let Some(secs) = extras.connect_timeout_secs {
                argv.push("-o".into());
                argv.push(format!("ConnectTimeout={secs}"));
            }
        } else if conn.forward_agent {
            // `-A` hands the local agent to the remote host: only when this
            // connection opted in. `-K` (Kerberos TGT delegation) follows
            // `use_kerberos` on its own.
            argv.push("-A".into());
        }
        if conn.use_kerberos {
            if extras.non_interactive {
                argv.push("-o".into());
                argv.push("GSSAPIAuthentication=yes".into());
                argv.push("-o".into());
                // Through an interactive bastion the *target* authenticates with
                // the delegated TGT; without it the bastion gets "Permission denied
                // (gssapi-with-mic)". Direct Kerberos logins must not delegate.
                if interactive_bastion {
                    argv.push("GSSAPIDelegateCredentials=yes".into());
                } else {
                    argv.push("GSSAPIDelegateCredentials=no".into());
                }
            } else {
                argv.push("-K".into());
            }
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        if enforce_target_shkc {
            argv.push("-o".into());
            argv.push(format!(
                "StrictHostKeyChecking={}",
                ssh_strict_host_key_value(policy.as_str())
            ));
        }

        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            if conn.use_kerberos {
                // Interactive bastion: connect to bastion, pass target as argument.
                let (bastion_host, bastion_port) = crate::models::split_bastion(bastion);
                if let Some(port) = bastion_port {
                    argv.push("-p".into());
                    argv.push(port.to_string());
                }
                argv.push("--".into());
                argv.push(format!("{bu}@{bastion_host}"));
                argv.push(format!("{}@{}", conn.user, conn.host));
            } else {
                // ProxyCommand with forced TTY allocation on the bastion connection
                let hop = HopOptions {
                    non_interactive: extras.non_interactive,
                    connect_timeout_secs: extras.connect_timeout_secs,
                };
                argv.push("-o".into());
                argv.push(format!(
                    "ProxyCommand={}",
                    jump_proxy_command(conn, " -tt -o RequestTTY=force", policy.as_str(), &hop)
                ));
                argv.push("-p".into());
                argv.push(conn.port.to_string());
                argv.push("--".into());
                argv.push(format!("{}@{}", conn.user, conn.host));
            }
        } else {
            argv.push("-p".into());
            argv.push(conn.port.to_string());
            argv.push("--".into());
            argv.push(format!("{}@{}", conn.user, conn.host));
        }
        argv
    }

    /// Build the argv for a local port-forward session (`ssh -L -N`).
    pub(crate) fn build_forward_argv(
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
        remote_host: &str,
        remote_port: u16,
        shkc: &str,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        if conn.use_kerberos {
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        argv.push("-o".into());
        argv.push(format!(
            "StrictHostKeyChecking={}",
            ssh_strict_host_key_value(shkc)
        ));
        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            argv.push("-J".into());
            argv.push(format!("{bu}@{bastion}"));
        }
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        argv.push("-L".into());
        argv.push(format!(
            "{bind_host}:{bind_port}:{remote_host}:{remote_port}"
        ));
        argv.push("-N".into());
        argv.push("--".into());
        argv.push(format!("{}@{}", conn.user, conn.host));
        argv
    }
}

impl SubprocessTransport {
    /// Build the argv for a SOCKS5 dynamic proxy session (`ssh -D -N`).
    pub(crate) fn build_dynamic_argv(
        conn: &Connection,
        bind_host: &str,
        bind_port: u16,
        shkc: &str,
    ) -> Vec<String> {
        let mut argv: Vec<String> = vec!["ssh".into()];
        if conn.use_kerberos {
            argv.push("-K".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        argv.push("-o".into());
        argv.push(format!(
            "StrictHostKeyChecking={}",
            ssh_strict_host_key_value(shkc)
        ));
        if let Some(bastion) = &conn.bastion {
            let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
            argv.push("-J".into());
            argv.push(format!("{bu}@{bastion}"));
        }
        argv.push("-p".into());
        argv.push(conn.port.to_string());
        argv.push("-D".into());
        // Use bind_host:port form only when bind_host is not loopback.
        if bind_host == "127.0.0.1" || bind_host == "localhost" {
            argv.push(format!("{bind_port}"));
        } else {
            argv.push(format!("{bind_host}:{bind_port}"));
        }
        argv.push("-N".into());
        argv.push("--".into());
        argv.push(format!("{}@{}", conn.user, conn.host));
        argv
    }
}
