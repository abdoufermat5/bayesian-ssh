use crate::services::transport::types::ForwardHandle;
use chrono::{DateTime, Utc};

/// The type of an active tunnel managed by the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelKind {
    /// Local port-forward (`-L`): static destination.
    Local,
    /// Dynamic SOCKS5 proxy (`-D`): destination per-request.
    Socks5,
}

impl TunnelKind {
    /// Single-character label shown in the tunnel table.
    pub fn tag(self) -> &'static str {
        match self {
            TunnelKind::Local => "L",
            TunnelKind::Socks5 => "D",
        }
    }
}

/// A live port-forward tunnel managed by the TUI.
pub struct TunnelEntry {
    /// Human-readable id (sequential, for display)
    pub id: usize,
    /// Local or SOCKS5 tunnel type
    pub kind: TunnelKind,
    /// Name of the connection being forwarded
    pub connection_name: String,
    /// Local bind address
    pub bind_host: String,
    /// Local bind port
    pub bind_port: u16,
    /// Remote target host (empty string for SOCKS5 — dynamic)
    pub remote_host: String,
    /// Remote target port (0 for SOCKS5 — dynamic)
    pub remote_port: u16,
    /// When the tunnel was started
    pub started_at: DateTime<Utc>,
    /// Owned handle — present while the tunnel is alive, consumed on cancel
    pub handle: Option<ForwardHandle>,
}

impl TunnelEntry {
    pub fn local_spec(&self) -> String {
        format!("{}:{}", self.bind_host, self.bind_port)
    }

    pub fn remote_spec(&self) -> String {
        match self.kind {
            TunnelKind::Local => format!("{}:{}", self.remote_host, self.remote_port),
            TunnelKind::Socks5 => "SOCKS5".to_string(),
        }
    }
}

/// Messages sent from async tunnel-start tasks back to the event loop.
pub enum TunnelMsg {
    /// Tunnel started successfully.
    Started {
        kind: TunnelKind,
        connection_name: String,
        bind_host: String,
        bind_port: u16,
        remote_host: String,
        remote_port: u16,
        handle: crate::services::transport::types::ForwardHandle,
    },
    /// Tunnel failed to start.
    Failed { spec: String, error: String },
}
