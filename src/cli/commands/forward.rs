//! `bssh forward` — SSH local port-forward tunnel.
//!
//! Usage:
//!   bssh forward myserver -L 8080:internal.host:80
//!   bssh forward myserver -L 0.0.0.0:5432:db.internal:5432
//!
//! Spec format (same as OpenSSH -L):  [bind_addr:]bind_port:remote_host:remote_port

use anyhow::{bail, Context, Result};

use crate::config::AppConfig;
use crate::services::transport::{execute_with_fallback, pick_kind, TransportKind};
use crate::services::SshService;

pub async fn execute(target: String, local: String, config: AppConfig) -> Result<()> {
    let (bind_host, bind_port, remote_host, remote_port) =
        parse_local_spec(&local).context("invalid -L spec")?;

    let ssh_service = SshService::new(config.clone())?;
    let connection =
        crate::cli::utils::resolve_connection(&ssh_service, &target, "forward", true).await?;

    let kind = pick_kind(&connection, &config);

    eprintln!(
        "→  Forwarding {}:{} → {}:{} via {} ({})",
        bind_host,
        bind_port,
        remote_host,
        remote_port,
        connection.name,
        match kind {
            TransportKind::Native => "native",
            TransportKind::Subprocess => "subprocess",
        }
    );
    eprintln!("   Press Ctrl+C to stop.\n");

    let handle = execute_with_fallback(&connection, &config, |transport| {
        let conn = connection.clone();
        let bh = bind_host.clone();
        let rh = remote_host.clone();
        Box::pin(async move {
            transport
                .forward_local(&conn, &bh, bind_port, &rh, remote_port)
                .await
        })
    })
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Block until Ctrl+C, then cleanly shut down.
    tokio::signal::ctrl_c().await.context("Ctrl+C handler")?;
    eprintln!("\nShutting down tunnel…");
    handle.cancel().await;
    eprintln!("Done.");
    Ok(())
}

/// Parse `[bind_addr:]bind_port:remote_host:remote_port` into its four parts.
/// Defaults the bind address to `127.0.0.1` when omitted.
fn parse_local_spec(spec: &str) -> Result<(String, u16, String, u16)> {
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    match parts.as_slice() {
        [bind_port_s, remote_host, remote_port_s] => {
            let bind_port: u16 = bind_port_s
                .parse()
                .with_context(|| format!("invalid bind port '{bind_port_s}'"))?;
            let remote_port: u16 = remote_port_s
                .parse()
                .with_context(|| format!("invalid remote port '{remote_port_s}'"))?;
            Ok((
                "127.0.0.1".to_string(),
                bind_port,
                remote_host.to_string(),
                remote_port,
            ))
        }
        [bind_addr, bind_port_s, remote_host, remote_port_s] => {
            let bind_port: u16 = bind_port_s
                .parse()
                .with_context(|| format!("invalid bind port '{bind_port_s}'"))?;
            let remote_port: u16 = remote_port_s
                .parse()
                .with_context(|| format!("invalid remote port '{remote_port_s}'"))?;
            Ok((
                bind_addr.to_string(),
                bind_port,
                remote_host.to_string(),
                remote_port,
            ))
        }
        _ => bail!("expected [bind_addr:]bind_port:remote_host:remote_port, got '{spec}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_local_spec;

    #[test]
    fn three_part_defaults_loopback() {
        let (bh, bp, rh, rp) = parse_local_spec("8080:db.internal:5432").unwrap();
        assert_eq!(bh, "127.0.0.1");
        assert_eq!(bp, 8080);
        assert_eq!(rh, "db.internal");
        assert_eq!(rp, 5432);
    }

    #[test]
    fn four_part_custom_bind() {
        let (bh, bp, rh, rp) = parse_local_spec("0.0.0.0:3306:mysql.lan:3306").unwrap();
        assert_eq!(bh, "0.0.0.0");
        assert_eq!(bp, 3306);
        assert_eq!(rh, "mysql.lan");
        assert_eq!(rp, 3306);
    }

    #[test]
    fn invalid_spec_errors() {
        assert!(parse_local_spec("8080").is_err());
        assert!(parse_local_spec("bad:port:host:80").is_err());
    }
}
