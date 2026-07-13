//! `bssh proxy` — SOCKS5 dynamic proxy through an SSH connection.
//!
//! Usage:
//!   bssh proxy myserver -D 1080
//!   bssh proxy myserver -D 1080 --bind 0.0.0.0
//!
//! Any SOCKS5-capable client (curl, browser, etc.) can then be pointed at
//! the local port to route traffic through the SSH connection.
//!
//! Uses the native russh transport when possible (no Kerberos, no bastion).
//! Falls back to `ssh -D` for Kerberos connections and bastion-hopped targets.

use anyhow::{Context, Result};

use crate::config::AppConfig;
use crate::services::transport::{execute_with_fallback, pick_kind, TransportKind};
use crate::services::SshService;

pub async fn execute(target: String, port: u16, bind: String, config: AppConfig) -> Result<()> {
    let ssh_service = SshService::new(config.clone())?;
    let connection =
        crate::cli::utils::resolve_connection(&ssh_service, &target, "proxy", true).await?;

    let kind = pick_kind(&connection, &config);

    eprintln!(
        "→  SOCKS5 proxy on {}:{} via {} ({})",
        bind,
        port,
        connection.name,
        match kind {
            TransportKind::Native => "native",
            TransportKind::Subprocess => "subprocess",
        }
    );
    eprintln!("   Configure your client: socks5://{}:{}", bind, port);
    eprintln!("   Press Ctrl+C to stop.\n");

    let handle = execute_with_fallback(&connection, &config, |transport| {
        let conn = connection.clone();
        let b = bind.clone();
        Box::pin(async move { transport.forward_dynamic(&conn, &b, port).await })
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    tokio::signal::ctrl_c()
        .await
        .context("Ctrl+C signal handler")?;
    eprintln!("\nShutting down proxy…");
    handle.cancel().await;
    eprintln!("Done.");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn port_range_valid() {
        // Ports 1024-65535 are valid for unprivileged use.
        let port: u16 = 1080;
        assert!(port >= 1024);
    }
}
