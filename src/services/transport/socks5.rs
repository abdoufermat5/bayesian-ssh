//! Minimal SOCKS5 handshake for the dynamic proxy.
//!
//! Only implements:
//!   - SOCKS5 version negotiation with NO_AUTH (method 0x00)
//!   - CONNECT command (0x01)
//!   - ATYP: IPv4 (0x01), domain (0x03), IPv6 (0x04)
//!
//! Returns the (target_host, target_port) pair so the caller can open a
//! `direct-tcpip` SSH channel and begin proxying.

use anyhow::{bail, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Perform a SOCKS5 handshake on `stream`.
///
/// After returning `Ok((host, port))` the stream is positioned at the start of
/// the application data — the caller should proxy it directly to `host:port`.
pub async fn handshake(stream: &mut TcpStream) -> Result<(String, u16)> {
    // ── Method negotiation ────────────────────────────────────────────────────
    let ver = stream.read_u8().await?;
    if ver != 0x05 {
        bail!("not SOCKS5 (VER={ver:#04x})");
    }
    let nmethods = stream.read_u8().await? as usize;
    let mut methods = vec![0u8; nmethods];
    stream.read_exact(&mut methods).await?;

    if !methods.contains(&0x00) {
        // Tell the client there is no acceptable method and hang up.
        stream.write_all(&[0x05, 0xFF]).await?;
        bail!("client requires authentication; only NO_AUTH (0x00) is supported");
    }
    // Select NO_AUTH.
    stream.write_all(&[0x05, 0x00]).await?;

    // ── Command ───────────────────────────────────────────────────────────────
    let mut hdr = [0u8; 4]; // VER CMD RSV ATYP
    stream.read_exact(&mut hdr).await?;
    if hdr[0] != 0x05 {
        bail!("bad request version ({:#04x})", hdr[0]);
    }
    if hdr[1] != 0x01 {
        // CMD=CONNECT only; reject everything else with "command not supported".
        send_reply(stream, 0x07).await?;
        bail!("only CONNECT (0x01) is supported, got CMD={:#04x}", hdr[1]);
    }

    // ── Address ───────────────────────────────────────────────────────────────
    let (host, port) = match hdr[3] {
        0x01 => {
            // IPv4 — 4 bytes
            let mut addr = [0u8; 4];
            stream.read_exact(&mut addr).await?;
            let port = stream.read_u16().await?;
            (
                format!("{}.{}.{}.{}", addr[0], addr[1], addr[2], addr[3]),
                port,
            )
        }
        0x03 => {
            // Domain name — 1-byte length prefix
            let len = stream.read_u8().await? as usize;
            let mut name = vec![0u8; len];
            stream.read_exact(&mut name).await?;
            let port = stream.read_u16().await?;
            let host = String::from_utf8(name)
                .map_err(|_| anyhow::anyhow!("non-UTF-8 domain name"))?;
            (host, port)
        }
        0x04 => {
            // IPv6 — 16 bytes
            let mut addr = [0u8; 16];
            stream.read_exact(&mut addr).await?;
            let port = stream.read_u16().await?;
            let ipv6 = std::net::Ipv6Addr::from(addr);
            (ipv6.to_string(), port)
        }
        t => {
            send_reply(stream, 0x08).await?; // address type not supported
            bail!("unsupported ATYP={t:#04x}");
        }
    };

    // ── Success reply ─────────────────────────────────────────────────────────
    // VER=5 REP=0(success) RSV=0 ATYP=1(IPv4) BND.ADDR=0.0.0.0 BND.PORT=0
    send_reply(stream, 0x00).await?;

    Ok((host, port))
}

/// Send a SOCKS5 reply with the given REP byte. Uses an all-zeros IPv4 BND address.
async fn send_reply(stream: &mut TcpStream, rep: u8) -> std::io::Result<()> {
    stream
        .write_all(&[0x05, rep, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::{TcpListener, TcpStream};

    async fn make_loopback_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = TcpStream::connect(addr).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (client, server)
    }

    #[tokio::test]
    async fn test_socks5_ipv4_connect() {
        let (mut client, mut server) = make_loopback_pair().await;

        tokio::spawn(async move {
            let result = handshake(&mut server).await.unwrap();
            assert_eq!(result.0, "93.184.216.34");
            assert_eq!(result.1, 80);
        });

        // Method negotiation: SOCKS5, 1 method, NO_AUTH
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        // Read method selection response
        let mut resp = [0u8; 2];
        client.read_exact(&mut resp).await.unwrap();
        assert_eq!(resp, [0x05, 0x00]);

        // CONNECT to 93.184.216.34:80 (IPv4)
        client
            .write_all(&[0x05, 0x01, 0x00, 0x01, 93, 184, 216, 34, 0, 80])
            .await
            .unwrap();
        // Read reply
        let mut reply = [0u8; 10];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], 0x00); // success
    }

    #[tokio::test]
    async fn test_socks5_domain_connect() {
        let (mut client, mut server) = make_loopback_pair().await;

        tokio::spawn(async move {
            let result = handshake(&mut server).await.unwrap();
            assert_eq!(result.0, "example.com");
            assert_eq!(result.1, 443);
        });

        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut resp = [0u8; 2];
        client.read_exact(&mut resp).await.unwrap();

        // CONNECT to example.com:443 (domain)
        let domain = b"example.com";
        let mut req: Vec<u8> = vec![0x05, 0x01, 0x00, 0x03, domain.len() as u8];
        req.extend_from_slice(domain);
        req.push(0x01); // port hi
        req.push(0xBB); // port lo = 443
        client.write_all(&req).await.unwrap();
        let mut reply = [0u8; 10];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], 0x00);
    }

    #[tokio::test]
    async fn test_socks5_rejects_non_connect() {
        let (mut client, mut server) = make_loopback_pair().await;

        tokio::spawn(async move {
            let _ = handshake(&mut server).await; // should return Err
        });

        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        let mut resp = [0u8; 2];
        client.read_exact(&mut resp).await.unwrap();

        // BIND command (0x02) instead of CONNECT
        client
            .write_all(&[0x05, 0x02, 0x00, 0x01, 93, 184, 216, 34, 0, 80])
            .await
            .unwrap();
        let mut reply = [0u8; 10];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], 0x07); // command not supported
    }
}
