use crate::config::AppConfig;
use crate::database::Database;
use crate::models::Connection;
use crate::tui::models::{AppMode, ConfirmAction, EditState, FilesPromptKind, PendingAction, Tab};
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

    // ─── Files prompt mode (upload / mkdir / rename) ─────────────────

    pub(crate) fn handle_files_prompt_mode(
        &mut self,
        key: KeyEvent,
        kind: FilesPromptKind,
    ) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.files_prompt_input.clear();
                self.set_status("Cancelled");
            }
            KeyCode::Enter => {
                let input = self.files_prompt_input.trim().to_string();
                if input.is_empty() {
                    self.set_status("Please enter a value first");
                    return Ok(());
                }
                self.mode = AppMode::Normal;
                self.files_prompt_input.clear();
                match kind {
                    FilesPromptKind::Upload => {
                        let local = std::path::Path::new(&input);
                        if local.is_dir() {
                            self.set_status(format!("Uploading directory '{input}'…"));
                            self.files_upload_dir(input);
                        } else {
                            self.set_status(format!("Uploading '{input}'…"));
                            self.files_upload(input);
                        }
                    }
                    FilesPromptKind::Download {
                        remote_path,
                        is_dir,
                    } => {
                        if is_dir {
                            self.set_status(format!("Downloading directory → '{input}'…"));
                            self.files_download_dir_to(remote_path, input);
                        } else {
                            self.set_status(format!("Downloading → '{input}'…"));
                            self.files_download_to(remote_path, input);
                        }
                    }
                    FilesPromptKind::Mkdir => {
                        self.set_status(format!("Creating directory '{input}'…"));
                        self.files_mkdir(input);
                    }
                    FilesPromptKind::Rename { .. } => {
                        self.set_status(format!("Renaming → '{input}'…"));
                        self.files_rename(input);
                    }
                }
            }
            KeyCode::Backspace => {
                self.files_prompt_input.pop();
            }
            KeyCode::Char(c) => {
                self.files_prompt_input.push(c);
            }
            _ => {}
        }
        Ok(())
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

    // ─── Search mode ───────────────────────────────────────────────────────────────

    pub(crate) fn handle_search_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                if self.active_tab == Tab::Connections {
                    self.search_query.clear();
                    self.apply_filter();
                    self.apply_sort();
                } else if self.active_tab == Tab::History {
                    self.history_filter.clear();
                    self.refresh_history()?;
                }
                self.set_status("Search cancelled");
            }
            KeyCode::Enter => {
                self.mode = AppMode::Normal;
                if self.active_tab == Tab::Connections {
                    self.apply_sort();
                    self.set_status(format!(
                        "Found {} connections",
                        self.filtered_connections.len()
                    ));
                } else if self.active_tab == Tab::History {
                    self.refresh_history()?;
                    self.set_status(format!("Found {} entries", self.history_entries.len()));
                }
            }
            KeyCode::Backspace => {
                if self.active_tab == Tab::Connections {
                    self.search_query.pop();
                    self.apply_filter();
                } else if self.active_tab == Tab::History {
                    self.history_filter.pop();
                    let _ = self.refresh_history();
                }
            }
            KeyCode::Char(c) => {
                if self.active_tab == Tab::Connections {
                    self.search_query.push(c);
                    self.apply_filter();
                } else if self.active_tab == Tab::History {
                    self.history_filter.push(c);
                    let _ = self.refresh_history();
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Help mode ───────────────────────────────────────────────────

    pub(crate) fn handle_help_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter => {
                self.mode = AppMode::Normal;
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Detail mode ─────────────────────────────────────────────────

    pub(crate) fn handle_detail_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('s') => {
                self.mode = AppMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection_down(),
            KeyCode::Enter => {
                if !self.filtered_connections.is_empty() {
                    self.selected_connection =
                        Some(self.filtered_connections[self.selected_index].clone());
                    self.pending_action = Some(PendingAction::Connect);
                    self.should_quit = true;
                }
            }
            KeyCode::Char('e') => {
                self.enter_edit_mode();
            }
            KeyCode::Char('p') => {
                self.mode = AppMode::CommandPreview;
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Edit / Add mode ─────────────────────────────────────────────

    pub(crate) fn handle_edit_mode(&mut self, key: KeyEvent) -> Result<()> {
        // Handle config tab editing (environment creation) separately
        if self.active_tab == Tab::Config {
            return self.handle_config_edit_mode(key);
        }

        match key.code {
            KeyCode::Esc => {
                self.edit_state = None;
                self.mode = AppMode::Normal;
                self.set_status("Edit cancelled");
            }
            KeyCode::Enter => {
                if let Some(ref mut edit) = self.edit_state {
                    edit.apply_field();

                    // Validate before saving
                    if let Err(msg) = edit.validate() {
                        self.set_status(msg);
                        return Ok(());
                    }

                    let conn = edit.connection.clone();
                    let is_new = edit.is_new;
                    let db = Database::new(&self.config)?;

                    if is_new {
                        db.add_connection(&conn)?;
                        self.set_status(format!("Created connection: {}", conn.name));
                    } else {
                        db.update_connection(&conn)?;
                        self.set_status(format!("Saved connection: {}", conn.name));
                    }
                }
                self.edit_state = None;
                self.refresh_connections()?;
                self.mode = AppMode::Normal;
            }
            KeyCode::Tab | KeyCode::Down => {
                if let Some(ref mut edit) = self.edit_state {
                    edit.apply_field();
                    edit.field_index = (edit.field_index + 1) % EditState::FIELD_COUNT;
                    edit.load_field();
                }
            }
            KeyCode::BackTab | KeyCode::Up => {
                if let Some(ref mut edit) = self.edit_state {
                    edit.apply_field();
                    edit.field_index = if edit.field_index == 0 {
                        EditState::FIELD_COUNT - 1
                    } else {
                        edit.field_index - 1
                    };
                    edit.load_field();
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut edit) = self.edit_state {
                    edit.field_value.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut edit) = self.edit_state {
                    // For Kerberos field, toggle on any key press
                    if edit.field_index == 7 {
                        edit.connection.use_kerberos = !edit.connection.use_kerberos;
                        edit.load_field();
                    } else {
                        edit.field_value.push(c);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn handle_config_edit_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.config_editing = None;
                self.mode = AppMode::Normal;
                self.set_status("Cancelled");
            }
            KeyCode::Enter => {
                if let Some(ref edit) = self.config_editing {
                    let name = edit.field_value.trim().to_string();
                    if !name.is_empty() {
                        let env_dir = dirs::config_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
                            .join("bayesian-ssh")
                            .join("environments")
                            .join(&name);

                        if env_dir.exists() {
                            self.set_status(format!("Environment '{}' already exists", name));
                        } else {
                            std::fs::create_dir_all(&env_dir)?;
                            let config = AppConfig::default_for_env(&name);
                            config.save()?;
                            self.set_status(format!("Created environment: {}", name));
                            self.refresh_environments();
                        }
                    }
                }
                self.config_editing = None;
                self.mode = AppMode::Normal;
            }
            KeyCode::Backspace => {
                if let Some(ref mut edit) = self.config_editing {
                    edit.field_value.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut edit) = self.config_editing {
                    edit.field_value.push(c);
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Confirm mode ────────────────────────────────────────────────

    pub(crate) fn handle_confirm_mode(
        &mut self,
        key: KeyEvent,
        action: ConfirmAction,
    ) -> Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                match action {
                    ConfirmAction::Delete(idx) => {
                        if self.active_tab == Tab::Config {
                            // Delete environment
                            if let Some(env_name) = self.env_list.get(idx).cloned() {
                                let env_dir = dirs::config_dir()
                                    .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
                                    .join("bayesian-ssh")
                                    .join("environments")
                                    .join(&env_name);
                                if env_dir.exists() {
                                    std::fs::remove_dir_all(&env_dir)?;
                                    self.set_status(format!("Deleted environment: {}", env_name));
                                    self.refresh_environments();
                                }
                            }
                        } else if idx < self.filtered_connections.len() {
                            let conn = &self.filtered_connections[idx];
                            let db = Database::new(&self.config)?;
                            if db.remove_connection(&conn.name)? {
                                self.set_status(format!("Deleted connection: {}", conn.name));
                                self.refresh_connections()?;
                            }
                        }
                    }
                    ConfirmAction::BatchDelete => {
                        let db = Database::new(&self.config)?;
                        let mut deleted = 0;
                        // Sort indices in reverse to avoid shifting issues
                        let mut indices: Vec<usize> =
                            self.multi_select.selected.iter().copied().collect();
                        indices.sort_unstable_by(|a, b| b.cmp(a));
                        for idx in indices {
                            if idx < self.filtered_connections.len() {
                                let conn = &self.filtered_connections[idx];
                                if db.remove_connection(&conn.name)? {
                                    deleted += 1;
                                }
                            }
                        }
                        self.multi_select.clear();
                        self.refresh_connections()?;
                        self.set_status(format!("Deleted {} connections", deleted));
                    }
                    ConfirmAction::StopTunnel(idx) => {
                        self.stop_tunnel(idx);
                        self.set_status("Tunnel stopped");
                    }
                    ConfirmAction::DeleteFile(path) => {
                        self.files_do_delete(path);
                    }
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.set_status("Action cancelled");
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Quick connect mode ──────────────────────────────────────────

    pub(crate) fn handle_quick_connect_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.quick_connect_input.clear();
                self.mode = AppMode::Normal;
                self.set_status("Quick connect cancelled");
            }
            KeyCode::Enter => {
                let input = self.quick_connect_input.trim().to_string();
                if !input.is_empty() {
                    if let Some(conn) = Self::parse_quick_connect(&input, &self.config) {
                        self.selected_connection = Some(conn);
                        self.pending_action = Some(PendingAction::Connect);
                        self.should_quit = true;
                    } else {
                        self.set_status("Invalid format. Use: [user@]host[:port]");
                    }
                }
                if !self.should_quit {
                    self.quick_connect_input.clear();
                    self.mode = AppMode::Normal;
                }
            }
            KeyCode::Backspace => {
                self.quick_connect_input.pop();
            }
            KeyCode::Char(c) => {
                self.quick_connect_input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Command preview mode ────────────────────────────────────────

    pub(crate) fn handle_command_preview_mode(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('p') | KeyCode::Char('q') => {
                self.mode = AppMode::Normal;
            }
            KeyCode::Enter => {
                // Connect directly from preview
                if !self.filtered_connections.is_empty() {
                    self.selected_connection =
                        Some(self.filtered_connections[self.selected_index].clone());
                    self.pending_action = Some(PendingAction::Connect);
                    self.should_quit = true;
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ─── Helpers ─────────────────────────────────────────────────────

    pub fn enter_edit_mode(&mut self) {
        if !self.filtered_connections.is_empty() {
            let conn = self.filtered_connections[self.selected_index].clone();
            let original_name = conn.name.clone();
            let mut edit = EditState {
                connection: conn,
                original_name,
                is_new: false,
                field_index: 0,
                field_value: String::new(),
            };
            edit.load_field();
            self.edit_state = Some(edit);
            self.mode = AppMode::Edit;
        }
    }

    pub fn enter_add_mode(&mut self) {
        let conn = Connection::new(
            String::new(),
            String::new(),
            self.config.default_user.clone(),
            self.config.default_port,
            self.config.default_bastion.clone(),
            self.config.default_bastion_user.clone(),
            self.config.use_kerberos_by_default,
            None,
        );
        let mut edit = EditState {
            connection: conn,
            original_name: String::new(),
            is_new: true,
            field_index: 0,
            field_value: String::new(),
        };
        edit.load_field();
        self.edit_state = Some(edit);
        self.mode = AppMode::Add;
    }

    /// Parse a quick connect string: [user@]host[:port]
    fn parse_quick_connect(input: &str, config: &AppConfig) -> Option<Connection> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }

        let (user, rest) = if let Some(at_pos) = input.find('@') {
            let user = &input[..at_pos];
            let rest = &input[at_pos + 1..];
            (user.to_string(), rest)
        } else {
            (config.default_user.clone(), input)
        };

        let (host, port) = if let Some(colon_pos) = rest.rfind(':') {
            let host = &rest[..colon_pos];
            let port_str = &rest[colon_pos + 1..];
            match port_str.parse::<u16>() {
                Ok(p) => (host.to_string(), p),
                Err(_) => (rest.to_string(), config.default_port),
            }
        } else {
            (rest.to_string(), config.default_port)
        };

        if host.is_empty() {
            return None;
        }

        let name = format!("quick-{}", host);
        Some(Connection::new(
            name, host, user, port, None, None, false, None,
        ))
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
