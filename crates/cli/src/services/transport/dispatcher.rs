use super::russh_impl::RusshTransport;
use super::subprocess_impl::SubprocessTransport;
use super::types::{SshTransport, TransportError};
use crate::config::AppConfig;
use crate::models::Connection;

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
    if cfg.transport.force_subprocess || conn.use_kerberos || conn.bastion.is_some() {
        TransportKind::Subprocess
    } else {
        TransportKind::Native
    }
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
            pick_kind(&simple_conn(), &simple_cfg()),
            TransportKind::Native
        );
    }

    #[test]
    fn kerberos_forces_subprocess() {
        let mut conn = simple_conn();
        conn.use_kerberos = true;
        assert_eq!(pick_kind(&conn, &simple_cfg()), TransportKind::Subprocess);
    }

    #[test]
    fn bastion_forces_subprocess() {
        let mut conn = simple_conn();
        conn.bastion = Some("bastion.example.com".into());
        assert_eq!(pick_kind(&conn, &simple_cfg()), TransportKind::Subprocess);
    }

    #[test]
    fn force_subprocess_flag_wins() {
        let mut cfg = simple_cfg();
        cfg.transport.force_subprocess = true;
        assert_eq!(pick_kind(&simple_conn(), &cfg), TransportKind::Subprocess);
    }
}
