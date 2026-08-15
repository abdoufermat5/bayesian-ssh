use crate::tui::models::{AppMode, Tab};
use crate::tui::state::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

impl App {
    pub(crate) fn open_tunnel_launch(&mut self) {
        let conn = if self.active_tab == Tab::Connections {
            self.filtered_connections.get(self.selected_index).cloned()
        } else {
            self.selected_connection.clone()
        };
        self.tunnel_target = conn;
        self.tunnel_input.clear();
        self.tunnel_launch_kind = crate::tui::models::TunnelKind::Local;
        self.mode = AppMode::TunnelLaunch;
    }

    pub(crate) fn open_socks5_launch(&mut self) {
        let conn = if self.active_tab == Tab::Connections {
            self.filtered_connections.get(self.selected_index).cloned()
        } else {
            self.selected_connection.clone()
        };
        self.tunnel_target = conn;
        self.tunnel_input.clear();
        self.tunnel_launch_kind = crate::tui::models::TunnelKind::Socks5;
        self.mode = AppMode::TunnelLaunch;
    }

    // ─── TunnelLaunch mode ───────────────────────────────────────────

    pub(crate) fn handle_tunnel_launch_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.tunnel_input.clear();
                self.tunnel_target = None;
                self.set_status("Tunnel launch cancelled");
            }
            KeyCode::Enter => {
                let spec = self.tunnel_input.trim().to_string();
                if spec.is_empty() {
                    let hint = match self.tunnel_launch_kind {
                        crate::tui::models::TunnelKind::Local => "Enter a -L spec first",
                        crate::tui::models::TunnelKind::Socks5 => {
                            "Enter a port (or addr:port) first"
                        }
                    };
                    self.set_status(hint);
                    return Ok(());
                }
                match self.tunnel_launch_kind {
                    crate::tui::models::TunnelKind::Local => {
                        match parse_forward_spec(&spec) {
                            Ok((bind_host, bind_port, remote_host, remote_port)) => {
                                let conn = match self.tunnel_target.clone() {
                                    Some(c) => c,
                                    None => {
                                        self.set_status("No target connection selected");
                                        return Ok(());
                                    }
                                };
                                self.set_status(format!(
                                    "Starting tunnel {}:{} → {}:{} via {}…",
                                    bind_host, bind_port, remote_host, remote_port, conn.name
                                ));
                                self.spawn_tunnel(
                                    conn,
                                    bind_host,
                                    bind_port,
                                    remote_host,
                                    remote_port,
                                );
                                self.mode = AppMode::Normal;
                                self.tunnel_input.clear();
                                self.tunnel_target = None;
                                // Auto-switch to Tunnels tab
                                self.active_tab = Tab::Tunnels;
                            }
                            Err(e) => {
                                self.set_status(format!("Invalid spec: {e}"));
                            }
                        }
                    }
                    crate::tui::models::TunnelKind::Socks5 => match parse_proxy_spec(&spec) {
                        Ok((bind_host, bind_port)) => {
                            let conn = match self.tunnel_target.clone() {
                                Some(c) => c,
                                None => {
                                    self.set_status("No target connection selected");
                                    return Ok(());
                                }
                            };
                            self.set_status(format!(
                                "Starting SOCKS5 proxy on {}:{} via {}…",
                                bind_host, bind_port, conn.name
                            ));
                            self.spawn_proxy(conn, bind_host, bind_port);
                            self.mode = AppMode::Normal;
                            self.tunnel_input.clear();
                            self.tunnel_target = None;
                            self.active_tab = Tab::Tunnels;
                        }
                        ok_err => {
                            if let Err(e) = ok_err {
                                self.set_status(format!("Invalid spec: {e}"));
                            }
                        }
                    },
                }
            }
            KeyCode::Backspace => {
                self.tunnel_input.pop();
            }
            KeyCode::Char(c) => {
                self.tunnel_input.push(c);
            }
            _ => {}
        }
        Ok(())
    }
}

/// Parse `[bind_addr:]bind_port:remote_host:remote_port` into its four parts.
fn parse_forward_spec(spec: &str) -> anyhow::Result<(String, u16, String, u16)> {
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    match parts.as_slice() {
        [bp, rh, rp] => {
            let bind_port: u16 = bp
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid bind port '{bp}'"))?;
            let remote_port: u16 = rp
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid remote port '{rp}'"))?;
            Ok((
                "127.0.0.1".to_string(),
                bind_port,
                rh.to_string(),
                remote_port,
            ))
        }
        [ba, bp, rh, rp] => {
            let bind_port: u16 = bp
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid bind port '{bp}'"))?;
            let remote_port: u16 = rp
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid remote port '{rp}'"))?;
            Ok((ba.to_string(), bind_port, rh.to_string(), remote_port))
        }
        _ => anyhow::bail!("expected [bind_addr:]bind_port:remote_host:remote_port, got '{spec}'"),
    }
}

/// Parse a SOCKS5 proxy spec: `port` or `bind_addr:port`.
fn parse_proxy_spec(spec: &str) -> anyhow::Result<(String, u16)> {
    // Try plain port number first.
    if let Ok(port) = spec.parse::<u16>() {
        return Ok(("127.0.0.1".to_string(), port));
    }
    // Otherwise expect bind_addr:port.
    match spec.rsplitn(2, ':').collect::<Vec<_>>().as_slice() {
        [port_str, addr] => {
            let port: u16 = port_str
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid port '{port_str}'"))?;
            Ok((addr.to_string(), port))
        }
        _ => anyhow::bail!("expected port or addr:port, got '{spec}'"),
    }
}
