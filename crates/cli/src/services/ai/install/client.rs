//! The AI clients bssh can install into: config paths and detection.

use anyhow::{Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Name of our entry in every client config.
pub const SERVER_NAME: &str = "bssh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    ClaudeCode,
    Codex,
    Cursor,
    Vscode,
    ClaudeDesktop,
    Omp,
}

impl Client {
    pub const ALL: [Client; 6] = [
        Client::ClaudeCode,
        Client::Codex,
        Client::Cursor,
        Client::Vscode,
        Client::ClaudeDesktop,
        Client::Omp,
    ];

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "claude-code" | "claude" | "claudecode" => Client::ClaudeCode,
            "codex" => Client::Codex,
            "cursor" => Client::Cursor,
            "vscode" | "vs-code" | "code" => Client::Vscode,
            "claude-desktop" | "claudedesktop" => Client::ClaudeDesktop,
            "omp" | "oh-my-pi" => Client::Omp,
            other => bail!(
                "unknown client '{other}' (use claude-code, codex, cursor, vscode, claude-desktop or omp)"
            ),
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "claude-code",
            Client::Codex => "codex",
            Client::Cursor => "cursor",
            Client::Vscode => "vscode",
            Client::ClaudeDesktop => "claude-desktop",
            Client::Omp => "omp",
        }
    }

    /// Top-level JSON key holding the servers (JSON clients).
    pub(super) fn servers_key(self) -> &'static str {
        match self {
            Client::Vscode => "servers",
            _ => "mcpServers",
        }
    }
}

fn appdata(home: &Path) -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Roaming"))
}

/// Where the client keeps its user-level MCP config.
pub fn config_path(client: Client, home: &Path) -> PathBuf {
    match client {
        Client::ClaudeCode => home.join(".claude.json"),
        Client::Codex => home.join(".codex").join("config.toml"),
        Client::Cursor => home.join(".cursor").join("mcp.json"),
        Client::Vscode => {
            if cfg!(target_os = "macos") {
                home.join("Library/Application Support/Code/User/mcp.json")
            } else if cfg!(windows) {
                appdata(home).join("Code").join("User").join("mcp.json")
            } else {
                home.join(".config/Code/User/mcp.json")
            }
        }
        Client::ClaudeDesktop => {
            if cfg!(target_os = "macos") {
                home.join("Library/Application Support/Claude/claude_desktop_config.json")
            } else if cfg!(windows) {
                appdata(home)
                    .join("Claude")
                    .join("claude_desktop_config.json")
            } else {
                home.join(".config/Claude/claude_desktop_config.json")
            }
        }
        Client::Omp => home.join(".omp").join("agent").join("mcp.json"),
    }
}

/// Claude Code user settings (hooks live here).
pub fn claude_settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

// ---------------------------------------------------------------- detection

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedClient {
    pub client: Client,
    pub config_path: PathBuf,
    /// The config file or the client's config directory exists.
    pub present: bool,
    /// The config already has a `bssh` MCP entry.
    pub configured: bool,
}

/// Clients found under the given home.
pub fn detected_clients_in(home: &Path) -> Vec<DetectedClient> {
    Client::ALL
        .iter()
        .map(|&client| {
            let path = config_path(client, home);
            let text = std::fs::read_to_string(&path).ok();
            let present = text.is_some() || path.parent().map(|p| p.is_dir()).unwrap_or(false);
            let configured = text
                .as_deref()
                .map(|t| has_entry(client, t))
                .unwrap_or(false);
            DetectedClient {
                client,
                config_path: path,
                present,
                configured,
            }
        })
        .collect()
}

/// Clients found for the current user (for `bssh ai doctor`).
pub fn detected_clients() -> Vec<DetectedClient> {
    dirs::home_dir()
        .map(|h| detected_clients_in(&h))
        .unwrap_or_default()
}

fn has_entry(client: Client, text: &str) -> bool {
    if client == Client::Codex {
        return text
            .parse::<toml_edit::DocumentMut>()
            .ok()
            .and_then(|d| d.get("mcp_servers")?.get(SERVER_NAME).map(|_| ()))
            .is_some();
    }
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.get(client.servers_key())?.get(SERVER_NAME).map(|_| ()))
        .is_some()
}
