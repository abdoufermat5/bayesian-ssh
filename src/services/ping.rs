//! Lightweight TCP-level ping for SSH host reachability checks.
//!
//! Uses `tokio::net::TcpStream::connect` with a timeout to test whether
//! a host:port is reachable.  This avoids spawning an external `ssh`
//! process and is suitable for background checks in the TUI.

use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Result of a TCP ping attempt.
#[derive(Debug, Clone)]
pub enum PingResult {
    /// Host is reachable; includes the round-trip time.
    Reachable(Duration),
    /// Host did not respond within the timeout or the connection was refused.
    Unreachable,
}

/// Attempt a TCP connection to `host:port` with the given `timeout_secs`.
///
/// For connections that go through a bastion host, callers should ping the
/// bastion (`bastion_host:22`) rather than the final target since the
/// target is typically not directly reachable.
pub async fn tcp_ping(host: &str, port: u16, timeout_secs: u64) -> PingResult {
    let addr = format!("{}:{}", host, port);
    let start = Instant::now();

    match timeout(Duration::from_secs(timeout_secs), TcpStream::connect(&addr)).await {
        Ok(Ok(_stream)) => PingResult::Reachable(start.elapsed()),
        _ => PingResult::Unreachable,
    }
}
