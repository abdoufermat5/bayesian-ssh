//! bssh ai install <client> [--guard]: MCP config and a Claude Code guard hook that keeps the
//! agent off raw `ssh`/`scp` and off the bssh CLI itself (host access and approvals belong to
//! the MCP tools).
//!
//! Formats checked against the vendors' docs:
//! - Claude Code: user-scope servers live in the top-level `mcpServers` of `~/.claude.json`
//!   as `{"type":"stdio","command":..,"args":[..]}` (code.claude.com/docs/en/mcp).
//! - Claude Code hooks: `~/.claude/settings.json` `hooks.PreToolUse[] = {matcher, hooks:[{type:"command",command}]}`;
//!   the hook gets JSON on stdin (`tool_input.command` for Bash) and blocks with exit 2 + stderr
//!   (code.claude.com/docs/en/hooks).
//! - Codex: `[mcp_servers.<name>]` with `command` and `args` in `~/.codex/config.toml`.
//! - Cursor: `~/.cursor/mcp.json`, `mcpServers`. VS Code: user `mcp.json`, `servers` with `type`.
//! - Claude Desktop: `claude_desktop_config.json`, `mcpServers`.
//! - omp (oh-my-pi): `~/.omp/agent/mcp.json`, `mcpServers` with `type`. omp ignores other
//!   clients' user configs (`~/.claude.json`, …) unless opted in, so it needs its own entry.

mod apply;
mod client;
mod guard;
mod merge;

pub use apply::{apply, diff, run_install, FileChange, InstallOptions};
pub use client::{
    claude_settings_path, config_path, detected_clients, detected_clients_in, Client,
    DetectedClient, SERVER_NAME,
};
pub use guard::guard_script;
pub use merge::{merge_codex, merge_guard_settings, merge_json_mcp};

#[cfg(test)]
mod tests;
