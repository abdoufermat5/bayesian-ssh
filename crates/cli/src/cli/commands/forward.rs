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
        crate::cli::utils::resolve_connection(&ssh_service, &target, "forward", true, &config)
            .await?;

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
/// Defaults the bind address to `127.0.0.1` when omitted. Like OpenSSH,
/// IPv6 addresses may be wrapped in brackets (`[::1]:8080:[fe80::1]:80`).
fn parse_local_spec(spec: &str) -> Result<(String, u16, String, u16)> {
    let parts = split_spec(spec);
    let (bind_addr, bind_port_s, remote_host, remote_port_s) = match parts.as_slice() {
        [bp, rh, rp] => ("127.0.0.1", *bp, *rh, *rp),
        [ba, bp, rh, rp] => (*ba, *bp, *rh, *rp),
        _ => bail!("expected [bind_addr:]bind_port:remote_host:remote_port, got '{spec}'"),
    };
    let bind_addr = unbracket(bind_addr);
    let remote_host = unbracket(remote_host);
    if bind_addr.is_empty() {
        bail!("empty bind address in '{spec}'");
    }
    if remote_host.is_empty() {
        bail!("empty remote host in '{spec}'");
    }
    let bind_port = parse_port(bind_port_s).context("invalid bind port")?;
    let remote_port = parse_port(remote_port_s).context("invalid remote port")?;
    Ok((
        bind_addr.to_string(),
        bind_port,
        remote_host.to_string(),
        remote_port,
    ))
}

/// Split on `:` except inside `[...]` (bracketed IPv6 literals).
fn split_spec(spec: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0u32;
    let mut start = 0;
    for (i, c) in spec.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                parts.push(&spec[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&spec[start..]);
    parts
}

fn unbracket(s: &str) -> &str {
    s.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(s)
}

fn parse_port(s: &str) -> Result<u16> {
    match s.parse::<u16>() {
        Ok(0) | Err(_) => bail!("'{s}' is not a port in 1-65535"),
        Ok(p) => Ok(p),
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
        assert!(parse_local_spec("8080::80").is_err());
        assert!(parse_local_spec("0:host:80").is_err());
        assert!(parse_local_spec("8080:host:0").is_err());
        assert!(parse_local_spec("a:1:host:2:3").is_err());
    }

    #[test]
    fn bracketed_ipv6() {
        let (bh, bp, rh, rp) = parse_local_spec("[::1]:8080:[fe80::1]:80").unwrap();
        assert_eq!(bh, "::1");
        assert_eq!(bp, 8080);
        assert_eq!(rh, "fe80::1");
        assert_eq!(rp, 80);
    }
}
