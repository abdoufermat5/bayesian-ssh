use super::russh_impl::RusshTransport;
use super::subprocess_impl::SubprocessTransport;
use super::types::{SshTransport, TransportError};
use crate::config::AppConfig;
use crate::models::Connection;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Kind of transport that should be used for a given connection.
///
/// The dispatcher is a pure function so it can be tested in isolation.
/// Actual `Arc<dyn SshTransport>` construction is done by callers that
/// have access to the concrete impl types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Native,
    Subprocess,
}

pub fn pick_kind(conn: &Connection, cfg: &AppConfig) -> TransportKind {
    pick_kind_with(conn, cfg, ssh_config_uses_proxy(&conn.host))
}

/// [`pick_kind`] with the `ProxyJump`/`ProxyCommand` answer passed in, so tests do not
/// depend on the machine's `~/.ssh/config`.
pub(crate) fn pick_kind_with(
    conn: &Connection,
    cfg: &AppConfig,
    ssh_config_has_proxy: bool,
) -> TransportKind {
    // The built-in transport cannot forward the ssh-agent, and it reads neither
    // `bastion` nor `~/.ssh/config`: any of those means the system ssh must connect.
    if cfg.transport.force_subprocess
        || conn.use_kerberos
        || conn.forward_agent
        || conn.bastion.is_some()
        || ssh_config_has_proxy
    {
        TransportKind::Subprocess
    } else {
        TransportKind::Native
    }
}

/// Does `ssh -G <host>` report a `ProxyJump` or `ProxyCommand` for this host?
///
/// The built-in transport dials `host:port` itself and ignores `~/.ssh/config`, so a host
/// that is only reachable through a configured jump would time out. `ssh -G` only prints
/// configuration: it opens nothing. Answers are cached per host for the life of the process.
fn ssh_config_uses_proxy(host: &str) -> bool {
    static CACHE: LazyLock<Mutex<HashMap<String, bool>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    if let Ok(map) = CACHE.lock() {
        if let Some(found) = map.get(host) {
            return *found;
        }
    }
    let found = std::process::Command::new("ssh")
        .arg("-G")
        .arg("--")
        .arg(host)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .is_some_and(|out| ssh_g_output_uses_proxy(&String::from_utf8_lossy(&out.stdout)));
    if let Ok(mut map) = CACHE.lock() {
        map.insert(host.to_string(), found);
    }
    found
}

/// Read the `proxyjump` / `proxycommand` keys out of `ssh -G` output. `none` (the
/// default) and an absent key mean "no proxy".
pub(crate) fn ssh_g_output_uses_proxy(output: &str) -> bool {
    output.lines().any(|line| {
        let mut words = line.split_whitespace();
        let key = words.next().unwrap_or_default().to_ascii_lowercase();
        if key != "proxyjump" && key != "proxycommand" {
            return false;
        }
        !matches!(words.next(), None | Some("none"))
    })
}

/// Helper to execute an SSH operation with fallback handling.
pub async fn execute_with_fallback<F, T>(
    conn: &Connection,
    cfg: &AppConfig,
    f: F,
) -> Result<T, TransportError>
where
    F: for<'a> Fn(
        &'a dyn SshTransport,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<T, TransportError>> + Send + 'a>,
    >,
{
    let kind = pick_kind(conn, cfg);
    match kind {
        TransportKind::Native => {
            let native = RusshTransport::new(cfg.clone());
            match f(&native).await {
                Err(TransportError::Fallback(e)) => {
                    tracing::warn!("Native transport fallback ({e}), retrying with subprocess");
                    let sub = SubprocessTransport::new(cfg.clone());
                    f(&sub).await
                }
                other => other,
            }
        }
        TransportKind::Subprocess => {
            let sub = SubprocessTransport::new(cfg.clone());
            f(&sub).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::models::Connection;

    fn simple_conn() -> Connection {
        Connection::new(
            "x".into(),
            "host".into(),
            "user".into(),
            22,
            None,
            None,
            false,
            None,
        )
    }

    fn simple_cfg() -> AppConfig {
        let mut c = AppConfig::default_for_env("default");
        c.transport.force_subprocess = false;
        c
    }

    #[test]
    fn simple_connection_picks_native() {
        assert_eq!(
            pick_kind_with(&simple_conn(), &simple_cfg(), false),
            TransportKind::Native
        );
    }

    #[test]
    fn kerberos_forces_subprocess() {
        let mut conn = simple_conn();
        conn.use_kerberos = true;
        assert_eq!(
            pick_kind_with(&conn, &simple_cfg(), false),
            TransportKind::Subprocess
        );
    }

    #[test]
    fn bastion_forces_subprocess() {
        let mut conn = simple_conn();
        conn.bastion = Some("bastion.example.com".into());
        assert_eq!(
            pick_kind_with(&conn, &simple_cfg(), false),
            TransportKind::Subprocess
        );
    }

    #[test]
    fn agent_forwarding_forces_subprocess() {
        let mut conn = simple_conn();
        conn.forward_agent = true;
        assert_eq!(
            pick_kind_with(&conn, &simple_cfg(), false),
            TransportKind::Subprocess
        );
    }

    #[test]
    fn force_subprocess_flag_wins() {
        let mut cfg = simple_cfg();
        cfg.transport.force_subprocess = true;
        assert_eq!(
            pick_kind_with(&simple_conn(), &cfg, false),
            TransportKind::Subprocess
        );
    }

    #[test]
    fn a_host_with_a_configured_jump_goes_through_the_system_ssh() {
        // The built-in transport ignores ~/.ssh/config and would dial the host directly.
        assert_eq!(
            pick_kind_with(&simple_conn(), &simple_cfg(), true),
            TransportKind::Subprocess
        );
    }

    #[test]
    fn ssh_g_output_proxy_detection() {
        // What `ssh -G` prints for a host with no jump: both keys are `none`.
        assert!(!ssh_g_output_uses_proxy(
            "hostname web-1\nuser alice\nproxyjump none\nproxycommand none\n"
        ));
        assert!(!ssh_g_output_uses_proxy("hostname web-1\n"));
        assert!(ssh_g_output_uses_proxy(
            "hostname web-1\nproxyjump bastion.example.com\nproxycommand none\n"
        ));
        assert!(ssh_g_output_uses_proxy(
            "proxyjump alice@bastion.example.com:2222\n"
        ));
        assert!(ssh_g_output_uses_proxy(
            "proxycommand ssh -W %h:%p bastion.example.com\n"
        ));
        // Other keys that merely contain the words do not count.
        assert!(!ssh_g_output_uses_proxy(
            "hostname proxyjump.example\nproxycommand none\n"
        ));
    }
}
