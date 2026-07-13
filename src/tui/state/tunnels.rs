use crate::models::Connection;
use crate::services::transport::types::SshTransport;
use crate::services::transport::{pick_kind, RusshTransport, SubprocessTransport, TransportKind};
use crate::tui::models::{TunnelEntry, TunnelMsg};
use crate::tui::state::App;

impl App {
    /// Drain completed tunnel-start results and update `tunnels` list.
    pub fn drain_tunnel_results(&mut self) {
        while let Ok(msg) = self.tunnel_rx.try_recv() {
            match msg {
                TunnelMsg::Started {
                    kind,
                    connection_name,
                    bind_host,
                    bind_port,
                    remote_host,
                    remote_port,
                    handle,
                } => {
                    let id = self.tunnels.len() + 1;
                    let status = match kind {
                        crate::tui::models::TunnelKind::Local => format!(
                            "Tunnel #{id}: {}:{} → {}:{} active",
                            bind_host, bind_port, remote_host, remote_port
                        ),
                        crate::tui::models::TunnelKind::Socks5 => {
                            format!("Proxy #{id}: SOCKS5 on {}:{} active", bind_host, bind_port)
                        }
                    };
                    self.set_status(status);
                    self.tunnels.push(TunnelEntry {
                        id,
                        kind,
                        connection_name,
                        bind_host,
                        bind_port,
                        remote_host,
                        remote_port,
                        started_at: chrono::Utc::now(),
                        handle: Some(handle),
                    });
                }
                TunnelMsg::Failed { spec, error } => {
                    self.set_status(format!("Tunnel {spec} failed: {error}"));
                }
            }
        }
    }

    /// Start an async task that establishes a forward tunnel for `conn`.
    pub fn spawn_tunnel(
        &self,
        conn: Connection,
        bind_host: String,
        bind_port: u16,
        remote_host: String,
        remote_port: u16,
    ) {
        let config = self.config.clone();
        let tx = self.tunnel_tx.clone();
        let spec = format!("{bind_host}:{bind_port}:{remote_host}:{remote_port}");
        let cname = conn.name.clone();

        tokio::spawn(async move {
            let kind = pick_kind(&conn, &config);
            let result = match kind {
                TransportKind::Native => RusshTransport::new(config)
                    .forward_local(&conn, &bind_host, bind_port, &remote_host, remote_port)
                    .await
                    .map_err(|e| e.to_string()),
                TransportKind::Subprocess => SubprocessTransport::new(config)
                    .forward_local(&conn, &bind_host, bind_port, &remote_host, remote_port)
                    .await
                    .map_err(|e| e.to_string()),
            };
            match result {
                Ok(handle) => {
                    let _ = tx.send(TunnelMsg::Started {
                        kind: crate::tui::models::TunnelKind::Local,
                        connection_name: cname,
                        bind_host,
                        bind_port,
                        remote_host,
                        remote_port,
                        handle,
                    });
                }
                Err(error) => {
                    let _ = tx.send(TunnelMsg::Failed { spec, error });
                }
            }
        });
    }

    /// Start an async task that establishes a SOCKS5 dynamic proxy for `conn`.
    pub fn spawn_proxy(&self, conn: Connection, bind_host: String, bind_port: u16) {
        let config = self.config.clone();
        let tx = self.tunnel_tx.clone();
        let spec = format!("{bind_host}:{bind_port}");
        let cname = conn.name.clone();

        tokio::spawn(async move {
            let kind = pick_kind(&conn, &config);
            let result = match kind {
                TransportKind::Native => RusshTransport::new(config)
                    .forward_dynamic(&conn, &bind_host, bind_port)
                    .await
                    .map_err(|e| e.to_string()),
                TransportKind::Subprocess => SubprocessTransport::new(config)
                    .forward_dynamic(&conn, &bind_host, bind_port)
                    .await
                    .map_err(|e| e.to_string()),
            };
            match result {
                Ok(handle) => {
                    let _ = tx.send(TunnelMsg::Started {
                        kind: crate::tui::models::TunnelKind::Socks5,
                        connection_name: cname,
                        bind_host,
                        bind_port,
                        remote_host: String::new(),
                        remote_port: 0,
                        handle,
                    });
                }
                Err(error) => {
                    let _ = tx.send(TunnelMsg::Failed { spec, error });
                }
            }
        });
    }

    /// Stop the tunnel at `index` by consuming and cancelling its handle.
    pub fn stop_tunnel(&mut self, index: usize) {
        if let Some(entry) = self.tunnels.get_mut(index) {
            if let Some(handle) = entry.handle.take() {
                tokio::spawn(handle.cancel());
            }
        }
        if index < self.tunnels.len() {
            self.tunnels.remove(index);
        }
        if self.tunnel_selected >= self.tunnels.len() && !self.tunnels.is_empty() {
            self.tunnel_selected = self.tunnels.len() - 1;
        }
    }

    /// Cancel every active tunnel (called when the TUI is about to exit).
    pub async fn cancel_all_tunnels(&mut self) {
        let entries = std::mem::take(&mut self.tunnels);
        for mut entry in entries {
            if let Some(handle) = entry.handle.take() {
                handle.cancel().await;
            }
        }
    }
}
