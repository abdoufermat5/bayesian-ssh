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

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
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
}

impl Client {
    pub const ALL: [Client; 5] = [
        Client::ClaudeCode,
        Client::Codex,
        Client::Cursor,
        Client::Vscode,
        Client::ClaudeDesktop,
    ];

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "claude-code" | "claude" | "claudecode" => Client::ClaudeCode,
            "codex" => Client::Codex,
            "cursor" => Client::Cursor,
            "vscode" | "vs-code" | "code" => Client::Vscode,
            "claude-desktop" | "claudedesktop" => Client::ClaudeDesktop,
            other => bail!(
                "unknown client '{other}' (use claude-code, codex, cursor, vscode or claude-desktop)"
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
        }
    }

    /// Top-level JSON key holding the servers (JSON clients).
    fn servers_key(self) -> &'static str {
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

// ---------------------------------------------------------------- merging

fn mcp_args(policy: &str) -> Vec<String> {
    vec![
        "ai".into(),
        "mcp".into(),
        "--policy".into(),
        policy.to_string(),
    ]
}

fn json_entry(client: Client, bin: &Path, policy: &str) -> Value {
    let mut o = Map::new();
    if matches!(client, Client::ClaudeCode | Client::Vscode) {
        o.insert("type".into(), json!("stdio"));
    }
    o.insert("command".into(), json!(bin.to_string_lossy()));
    o.insert("args".into(), json!(mcp_args(policy)));
    Value::Object(o)
}

fn parse_json_object(before: Option<&str>, path: &Path) -> Result<Map<String, Value>> {
    match before {
        None => Ok(Map::new()),
        Some(t) if t.trim().is_empty() => Ok(Map::new()),
        Some(t) => match serde_json::from_str::<Value>(t) {
            Ok(Value::Object(m)) => Ok(m),
            Ok(_) => bail!("{} is not a JSON object; not touching it", path.display()),
            Err(e) => bail!(
                "{} is not plain JSON ({e}); not touching it. Add the entry by hand or remove comments",
                path.display()
            ),
        },
    }
}

fn pretty(m: Map<String, Value>) -> Result<String> {
    let mut s = serde_json::to_string_pretty(&Value::Object(m))?;
    s.push('\n');
    Ok(s)
}

/// New text of a JSON MCP config, or None when nothing changes.
pub fn merge_json_mcp(
    client: Client,
    path: &Path,
    before: Option<&str>,
    entry: Option<Value>,
) -> Result<Option<String>> {
    let mut root = parse_json_object(before, path)?;
    let key = client.servers_key();
    let old = root.clone();
    match entry {
        Some(e) => {
            let servers = root
                .entry(key.to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            let servers = servers
                .as_object_mut()
                .ok_or_else(|| anyhow!("'{key}' in {} is not an object", path.display()))?;
            servers.insert(SERVER_NAME.into(), e);
        }
        None => {
            if let Some(Value::Object(servers)) = root.get_mut(key) {
                servers.remove(SERVER_NAME);
                if servers.is_empty() {
                    root.remove(key);
                }
            }
        }
    }
    if root == old && before.is_some() {
        return Ok(None);
    }
    if root == old && before.is_none() && root.is_empty() {
        return Ok(None);
    }
    Ok(Some(pretty(root)?))
}

/// New text of `~/.codex/config.toml`, or None when nothing changes. Keeps comments and order.
pub fn merge_codex(before: Option<&str>, entry: Option<(&Path, &str)>) -> Result<Option<String>> {
    let mut doc: toml_edit::DocumentMut = before
        .unwrap_or("")
        .parse()
        .context("~/.codex/config.toml is not valid TOML; not touching it")?;
    let old = doc.to_string();
    match entry {
        Some((bin, policy)) => {
            let servers = doc
                .entry("mcp_servers")
                .or_insert_with(|| {
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    toml_edit::Item::Table(t)
                })
                .as_table_mut()
                .ok_or_else(|| anyhow!("'mcp_servers' is not a table"))?;
            let mut t = toml_edit::Table::new();
            t["command"] = toml_edit::value(bin.to_string_lossy().to_string());
            let mut args = toml_edit::Array::new();
            for a in mcp_args(policy) {
                args.push(a);
            }
            t["args"] = toml_edit::value(args);
            servers.insert(SERVER_NAME, toml_edit::Item::Table(t));
        }
        None => {
            if let Some(s) = doc.get_mut("mcp_servers").and_then(|i| i.as_table_mut()) {
                s.remove(SERVER_NAME);
                if s.is_empty() {
                    doc.remove("mcp_servers");
                }
            }
        }
    }
    let new = doc.to_string();
    Ok((new != old).then_some(new))
}

/// Shell-quote a path for a hook `command` string.
fn sh_quote(p: &str) -> String {
    if p.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+@:".contains(c))
    {
        p.to_string()
    } else {
        format!("'{}'", p.replace('\'', "'\\''"))
    }
}

/// Add or remove our Bash PreToolUse hook in Claude Code's settings.json.
pub fn merge_guard_settings(
    path: &Path,
    before: Option<&str>,
    script: &Path,
    install: bool,
) -> Result<Option<String>> {
    let mut root = parse_json_object(before, path)?;
    let old = root.clone();
    let cmd = sh_quote(&script.to_string_lossy());
    let is_ours = |h: &Value| h.get("command").and_then(|c| c.as_str()) == Some(cmd.as_str());

    let hooks = root
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow!("'hooks' in {} is not an object", path.display()))?;
    let pre = hooks
        .entry("PreToolUse".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let pre = pre
        .as_array_mut()
        .ok_or_else(|| anyhow!("'hooks.PreToolUse' is not an array"))?;

    // Drop our entry everywhere first; re-add below on install (keeps it idempotent).
    for group in pre.iter_mut() {
        if let Some(hs) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
            hs.retain(|h| !is_ours(h));
        }
    }
    pre.retain(|g| {
        g.get("hooks")
            .and_then(|h| h.as_array())
            .map(|h| !h.is_empty())
            .unwrap_or(true)
    });
    if install {
        pre.push(json!({
            "matcher": "Bash",
            "hooks": [{"type": "command", "command": cmd}],
        }));
    }
    if pre.is_empty() {
        hooks.remove("PreToolUse");
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }
    if root == old {
        return Ok(None);
    }
    Ok(Some(pretty(root)?))
}

// ---------------------------------------------------------------- guard script

/// The POSIX sh hook. `hosts_file` is a snapshot used only when `bssh list` yields nothing.
///
/// It blocks `ssh`/`scp`/`sftp`/`rsync` to a host bssh manages, and every `bssh` or
/// `bayesian-ssh` invocation that reaches a host or decides approvals: only read-only
/// subcommands pass. A bare name (`bssh web1`) is blocked too, so the hook does not depend
/// on the parser rejecting it today.
pub fn guard_script(bssh_bin: &Path, hosts_file: &Path) -> String {
    let q = |p: &Path| p.to_string_lossy().replace('\'', "'\\''");
    format!(
        r#"#!/bin/sh
# bssh AI guard: Claude Code PreToolUse hook for the Bash tool.
# Generated by `bssh ai install claude-code --guard`; do not edit (re-run the command).
# Input: JSON on stdin (tool_input.command). Exit 2 + stderr blocks the call, exit 0 allows it.
# Blocked: ssh/scp/sftp/rsync to a host bssh manages, and any bssh invocation that reaches
# a host or decides approvals (only read-only subcommands pass).
BSSH_BIN='{bin}'
HOSTS_FILE='{hosts}'

input=$(cat)
if command -v jq >/dev/null 2>&1; then
  cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // empty' 2>/dev/null)
else
  cmd=$(printf '%s' "$input" | sed -n -E 's/.*"command"[[:space:]]*:[[:space:]]*"(([^"\\]|\\.)*)".*/\1/p' | head -n 1)
fi
[ -n "$cmd" ] || exit 0

# Cheap exit for the common case: no remote-shell program and no bssh at all.
case "$cmd" in
  *ssh*|*scp*|*sftp*|*rsync*|*bssh*) ;;
  *) exit 0 ;;
esac

# Hosts known to bssh: names, hosts and bastions from `bssh list --detailed`.
live=$("$BSSH_BIN" list -d 2>/dev/null)
known=$( {{
  printf '%s\n' "$live" | awk '$1 == "┌─" {{ print $2 }}'
  printf '%s\n' "$live" | sed -n 's/.*@\([^@:[:space:]]*\):[0-9][0-9]*.*/\1/p'
}} )
if [ -z "$known" ] && [ -r "$HOSTS_FILE" ]; then
  known=$(cat "$HOSTS_FILE")
fi

set -f
printf '%s\n' "$cmd" | tr ';|&()`' '\n\n\n\n\n\n' | while IFS= read -r seg; do
  # shellcheck disable=SC2086
  set -- $seg

  # bssh / bayesian-ssh reach hosts and decide approvals, so the agent's shell may only
  # run read-only subcommands. Everything else is blocked, including a bare name
  # (`bssh web1`) and every `ai` subcommand that approves, grants, revokes or stops.
  # A bssh word counts in command position: first in the segment, or after a wrapper
  # (`sudo`, `env`, `timeout`, ...) and its leading VAR=value assignments.
  wrapped=0
  for w in "$@"; do
    case "$w" in *=*) continue ;; esac
    case "${{w##*/}}" in
      sudo|doas|env|command|exec|nohup|time|nice|ionice|setsid|stdbuf|timeout|xargs) wrapped=1 ;;
    esac
    break
  done
  found=0
  pos=0
  while [ $# -gt 0 ]; do
    case "${{1##*/}}" in
      bssh|bayesian-ssh)
        if [ "$pos" = 0 ] || [ "$wrapped" = 1 ]; then
          found=1
          shift
        fi
        break ;;
    esac
    pos=$((pos + 1))
    shift
  done
  if [ "$found" = 1 ]; then
    first=
    second=
    while [ $# -gt 0 ]; do
      w=$1
      shift
      case "$w" in
        --env) [ $# -gt 0 ] && shift; continue ;;
        --env=*) continue ;;
        --) if [ -z "$first" ] && [ $# -gt 0 ]; then first=$1; fi; break ;;
        -*) continue ;;
      esac
      if [ -z "$first" ]; then
        first=$w
      elif [ -z "$second" ]; then
        second=$w
      fi
    done
    ok=1
    case "$first" in
      '') ;;
      help|completions|list|show|why|history|stats|doctor|ping|groups|export) ;;
      ai)
        case "$second" in
          ''|help|explain|watch|log|status|doctor|policy) ;;
          *) ok=0 ;;
        esac ;;
      key) case "$second" in ''|help|list) ;; *) ok=0 ;; esac ;;
      env) case "$second" in ''|help|list) ;; *) ok=0 ;; esac ;;
      *) ok=0 ;;
    esac
    if [ "$ok" = 0 ]; then
      echo "bssh guard: 'bssh $first${{second:+ $second}}' is blocked. Use the bssh MCP tools instead: list_hosts, host_info, observe, read_file, run_readonly, and propose_change/apply_change for changes. Approvals are the human's: they answer in their client or the desktop app, an agent can't approve, grant or revoke for itself." >&2
      exit 2
    fi
  fi

  # Raw ssh/scp/sftp/rsync to a host bssh manages.
  [ -n "$known" ] || continue
  set -- $seg
  remote=0
  for w in "$@"; do
    case "${{w##*/}}" in ssh|scp|sftp|rsync|ssh-copy-id) remote=1 ;; esac
  done
  [ "$remote" = 1 ] || continue
  for w in "$@"; do
    t=$(printf '%s' "$w" | tr -d "'\"")
    t=${{t#*@}}
    t=${{t%%:*}}
    [ -n "$t" ] || continue
    if printf '%s\n' "$known" | grep -Fxq -- "$t"; then
      echo "bssh guard: raw ssh/scp/sftp/rsync to '$t' is blocked. '$t' is managed by bssh: use the bssh MCP tools instead (list_hosts, host_info, observe, read_file, run_readonly, propose_change/apply_change)." >&2
      exit 2
    fi
  done
done
# The loop is the last command of the pipe, so $? is its status (2 when it blocked).
exit "$?"
"#,
        bin = q(bssh_bin),
        hosts = q(hosts_file),
    )
}

// ---------------------------------------------------------------- applying

/// A file change: path, old content (None = new file), new content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: String,
}

/// Small diff for `--dry-run`: common head and tail are dropped, the middle is shown.
pub fn diff(before: Option<&str>, after: &str) -> String {
    let a: Vec<&str> = before.unwrap_or("").lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut out = String::new();
    let ctx_start = head.saturating_sub(2);
    for l in &a[ctx_start..head] {
        out.push_str(&format!("  {l}\n"));
    }
    for l in &a[head..a.len() - tail] {
        out.push_str(&format!("- {l}\n"));
    }
    for l in &b[head..b.len() - tail] {
        out.push_str(&format!("+ {l}\n"));
    }
    for l in b[b.len() - tail..].iter().take(2) {
        out.push_str(&format!("  {l}\n"));
    }
    out
}

/// Back up the old file (if any) and write the new content atomically.
pub fn apply(change: &FileChange) -> Result<Option<PathBuf>> {
    let path = &change.path;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let mut backup = None;
    if path.exists() {
        let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("config");
        let b = dir.join(format!("{name}.bssh-bak-{stamp}"));
        std::fs::copy(path, &b).with_context(|| format!("back up {}", path.display()))?;
        backup = Some(b);
    }
    let tmp = dir.join(format!(
        ".{}.bssh-tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("config"),
        std::process::id()
    ));
    std::fs::write(&tmp, &change.after)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o777)
            .unwrap_or(0o600);
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("write {}", path.display()))?;
    Ok(backup)
}

fn read_opt(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

/// Options of one `bssh ai install` run, with the environment injected (tests use a temp HOME).
pub struct InstallOptions<'a> {
    pub client: Client,
    pub policy: &'a str,
    pub guard: bool,
    pub dry_run: bool,
    pub uninstall: bool,
    pub home: &'a Path,
    pub bssh_bin: &'a Path,
    pub ai_dir: &'a Path,
    /// Connection names and hosts, written to the guard's fallback snapshot.
    pub known_hosts: &'a [String],
}

/// Run an install; returns the human-readable report (also what `--dry-run` prints).
pub fn run_install(o: &InstallOptions) -> Result<String> {
    let mut out = String::new();
    let mut changes: Vec<FileChange> = Vec::new();
    let mut note = |s: String| {
        out.push_str(&s);
        out.push('\n');
    };

    // 1. MCP entry.
    let path = config_path(o.client, o.home);
    let before = read_opt(&path)?;
    let new = if o.client == Client::Codex {
        let entry = (!o.uninstall).then_some((o.bssh_bin, o.policy));
        merge_codex(before.as_deref(), entry)?
    } else {
        let entry = (!o.uninstall).then(|| json_entry(o.client, o.bssh_bin, o.policy));
        merge_json_mcp(o.client, &path, before.as_deref(), entry)?
    };
    match new {
        Some(after) => changes.push(FileChange {
            path: path.clone(),
            before,
            after,
        }),
        None => note(format!(
            "{}: {} already {} (nothing to change)",
            o.client.name(),
            path.display(),
            if o.uninstall {
                "without bssh"
            } else {
                "up to date"
            }
        )),
    }

    // 2. Guard.
    let guard_dir = o.ai_dir.join("guard");
    let script = guard_dir.join("claude-pretooluse.sh");
    let hosts_file = guard_dir.join("hosts.txt");
    let mut write_script = false;
    if o.guard {
        if o.client != Client::ClaudeCode {
            note(format!(
                "guard: only Claude Code is wired automatically. For {} point a PreToolUse hook on shell commands at {} (same stdin JSON, exit 2 blocks); run `bssh ai install claude-code --guard` once to create the script.",
                o.client.name(),
                script.display()
            ));
        } else {
            let sp = claude_settings_path(o.home);
            let sb = read_opt(&sp)?;
            if let Some(after) = merge_guard_settings(&sp, sb.as_deref(), &script, !o.uninstall)? {
                changes.push(FileChange {
                    path: sp,
                    before: sb,
                    after,
                });
            } else {
                note(format!("guard: {} already up to date", sp.display()));
            }
            write_script = !o.uninstall;
        }
    }

    // 3. Show or write.
    for c in &changes {
        if o.dry_run {
            note(format!(
                "--- {}\n{}",
                c.path.display(),
                diff(c.before.as_deref(), &c.after)
            ));
        }
    }
    if o.dry_run {
        if write_script {
            note(format!("would write guard script {}", script.display()));
        }
        return Ok(out);
    }
    for c in &changes {
        let backup = apply(c)?;
        let verb = if o.uninstall {
            "removed bssh from"
        } else {
            "updated"
        };
        note(format!("{verb} {}", c.path.display()));
        if let Some(b) = backup {
            note(format!("  backup: {}", b.display()));
        }
    }
    if write_script {
        std::fs::create_dir_all(&guard_dir)?;
        std::fs::write(&script, guard_script(o.bssh_bin, &hosts_file))?;
        std::fs::write(&hosts_file, o.known_hosts.join("\n") + "\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        }
        note(format!("wrote guard script {}", script.display()));
    } else if o.guard && o.uninstall {
        for f in [&script, &hosts_file] {
            if f.exists() {
                std::fs::remove_file(f)?;
            }
        }
        note("removed guard script".to_string());
    }
    if !o.uninstall && changes.is_empty() && !write_script {
        note("bssh is already configured.".to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts<'a>(
        client: Client,
        home: &'a Path,
        ai: &'a Path,
        bin: &'a Path,
        hosts: &'a [String],
    ) -> InstallOptions<'a> {
        InstallOptions {
            client,
            policy: "default",
            guard: false,
            dry_run: false,
            uninstall: false,
            home,
            bssh_bin: bin,
            ai_dir: ai,
            known_hosts: hosts,
        }
    }

    #[test]
    fn claude_code_merges_without_clobbering_and_is_idempotent() {
        let h = tempfile::tempdir().unwrap();
        let ai = h.path().join("ai");
        let bin = PathBuf::from("/usr/local/bin/bssh");
        let p = config_path(Client::ClaudeCode, h.path());
        std::fs::write(
            &p,
            r#"{"numStartups": 3, "mcpServers": {"other": {"command": "x"}}, "projects": {"/a": {"k": 1}}}"#,
        )
        .unwrap();
        let o = opts(Client::ClaudeCode, h.path(), &ai, &bin, &[]);
        let r = run_install(&o).unwrap();
        assert!(r.contains("backup"));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["numStartups"], 3);
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["projects"]["/a"]["k"], 1);
        let e = &v["mcpServers"]["bssh"];
        assert_eq!(e["type"], "stdio");
        assert_eq!(e["command"], "/usr/local/bin/bssh");
        assert_eq!(e["args"], json!(["ai", "mcp", "--policy", "default"]));
        // Backup holds the original.
        let baks: Vec<_> = std::fs::read_dir(h.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("bssh-bak"))
            .collect();
        assert_eq!(baks.len(), 1);
        assert!(std::fs::read_to_string(baks[0].path())
            .unwrap()
            .contains("numStartups"));
        // Second run: no change, no new backup.
        let r2 = run_install(&o).unwrap();
        assert!(r2.contains("nothing to change"), "{r2}");
        // Uninstall removes only ours.
        let mut u = opts(Client::ClaudeCode, h.path(), &ai, &bin, &[]);
        u.uninstall = true;
        run_install(&u).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(v["mcpServers"].get("bssh").is_none());
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
    }

    #[test]
    fn dry_run_prints_diff_and_writes_nothing() {
        let h = tempfile::tempdir().unwrap();
        let ai = h.path().join("ai");
        let bin = PathBuf::from("/b/bssh");
        let mut o = opts(Client::Cursor, h.path(), &ai, &bin, &[]);
        o.dry_run = true;
        let r = run_install(&o).unwrap();
        assert!(r.contains("+ ") && r.contains("mcpServers"), "{r}");
        assert!(!config_path(Client::Cursor, h.path()).exists());
    }

    #[test]
    fn vscode_cursor_desktop_shapes() {
        let h = tempfile::tempdir().unwrap();
        let ai = h.path().join("ai");
        let bin = PathBuf::from("/b/bssh");
        for c in [Client::Vscode, Client::Cursor, Client::ClaudeDesktop] {
            run_install(&opts(c, h.path(), &ai, &bin, &[])).unwrap();
            let v: Value =
                serde_json::from_str(&std::fs::read_to_string(config_path(c, h.path())).unwrap())
                    .unwrap();
            let e = &v[c.servers_key()]["bssh"];
            assert_eq!(e["command"], "/b/bssh", "{c:?}");
            assert_eq!(e.get("type").is_some(), c == Client::Vscode);
        }
    }

    #[test]
    fn invalid_json_is_not_touched() {
        let h = tempfile::tempdir().unwrap();
        let ai = h.path().join("ai");
        let bin = PathBuf::from("/b/bssh");
        let p = config_path(Client::Vscode, h.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{ // comment\n}").unwrap();
        assert!(run_install(&opts(Client::Vscode, h.path(), &ai, &bin, &[])).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ // comment\n}");
    }

    #[test]
    fn codex_keeps_comments_and_is_idempotent() {
        let h = tempfile::tempdir().unwrap();
        let ai = h.path().join("ai");
        let bin = PathBuf::from("/b/bssh");
        let p = config_path(Client::Codex, h.path());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            "# my config\nmodel = \"x\"\n\n[mcp_servers.other]\ncommand = \"o\"\n",
        )
        .unwrap();
        let o = opts(Client::Codex, h.path(), &ai, &bin, &[]);
        run_install(&o).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains("# my config") && t.contains("[mcp_servers.other]"));
        let d: toml::Table = t.parse().unwrap();
        assert_eq!(
            d["mcp_servers"]["bssh"]["command"].as_str(),
            Some("/b/bssh")
        );
        assert_eq!(
            d["mcp_servers"]["bssh"]["args"].as_array().unwrap().len(),
            4
        );
        assert!(run_install(&o).unwrap().contains("nothing to change"));
        let mut u = opts(Client::Codex, h.path(), &ai, &bin, &[]);
        u.uninstall = true;
        run_install(&u).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(!t.contains("bssh") && t.contains("[mcp_servers.other]"));
    }

    #[test]
    fn guard_registers_hook_idempotently_and_uninstalls() {
        let h = tempfile::tempdir().unwrap();
        let ai = h.path().join("ai");
        let bin = PathBuf::from("/b/bssh");
        let sp = claude_settings_path(h.path());
        std::fs::create_dir_all(sp.parent().unwrap()).unwrap();
        std::fs::write(
            &sp,
            r#"{"theme":"dark","hooks":{"PreToolUse":[{"matcher":"Edit","hooks":[{"type":"command","command":"/x/y.sh"}]}]}}"#,
        )
        .unwrap();
        let hosts = vec!["web1".to_string()];
        let mut o = opts(Client::ClaudeCode, h.path(), &ai, &bin, &hosts);
        o.guard = true;
        run_install(&o).unwrap();
        run_install(&o).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&sp).unwrap()).unwrap();
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2, "{pre:?}");
        assert_eq!(pre[1]["matcher"], "Bash");
        let script = ai.join("guard/claude-pretooluse.sh");
        assert_eq!(
            pre[1]["hooks"][0]["command"].as_str().unwrap(),
            script.to_string_lossy()
        );
        assert!(script.exists());
        o.uninstall = true;
        run_install(&o).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&sp).unwrap()).unwrap();
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(v["theme"], "dark");
        assert!(!script.exists());
    }

    #[test]
    fn detects_clients() {
        let h = tempfile::tempdir().unwrap();
        let ai = h.path().join("ai");
        let bin = PathBuf::from("/b/bssh");
        run_install(&opts(Client::Cursor, h.path(), &ai, &bin, &[])).unwrap();
        let d = detected_clients_in(h.path());
        let cursor = d.iter().find(|c| c.client == Client::Cursor).unwrap();
        assert!(cursor.present && cursor.configured);
        let codex = d.iter().find(|c| c.client == Client::Codex).unwrap();
        assert!(!codex.present && !codex.configured);
    }

    #[cfg(unix)]
    mod script {
        use super::*;
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};

        fn setup(live: &str, snapshot: Option<&str>) -> (tempfile::TempDir, PathBuf) {
            let d = tempfile::tempdir().unwrap();
            let fake = d.path().join("bssh");
            std::fs::write(&fake, format!("#!/bin/sh\ncat <<'EOF'\n{live}\nEOF\n")).unwrap();
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
            let hosts = d.path().join("hosts.txt");
            if let Some(s) = snapshot {
                std::fs::write(&hosts, s).unwrap();
            }
            let script = d.path().join("guard.sh");
            std::fs::write(&script, guard_script(&fake, &hosts)).unwrap();
            (d, script)
        }

        fn run(script: &Path, command: &str) -> (i32, String) {
            let input =
                json!({"tool_name": "Bash", "tool_input": {"command": command}}).to_string();
            let mut ch = Command::new("sh")
                .arg(script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            ch.stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            let o = ch.wait_with_output().unwrap();
            (
                o.status.code().unwrap(),
                String::from_utf8_lossy(&o.stderr).to_string(),
            )
        }

        const LIVE: &str = "\n🔗 Connections\n════\n\n┌─ web1 \n│  alice@10.0.0.5:22\n└─\n\n┌─ db1 🔗\n│  bob@db.internal:2222\n│  via ops@jump.internal:22";

        #[test]
        fn blocks_known_hosts_only() {
            let (_d, s) = setup(LIVE, None);
            for c in [
                "ssh web1 uptime",
                "ssh alice@10.0.0.5",
                "scp ./f web1:/tmp/",
                "rsync -e ssh -av ./d bob@db.internal:/srv/",
                "cd /tmp && ssh -p 2222 db1 'df -h'",
                "sudo ssh jump.internal",
                "sftp web1",
            ] {
                let (code, err) = run(&s, c);
                assert_eq!(code, 2, "{c}: {err}");
                assert!(err.contains("bssh MCP tools"), "{err}");
            }
            for c in [
                "ls -la",
                "ssh other.example.com uptime",
                "git log --grep ssh",
                "echo web1",
                "cat ~/.ssh/config",
            ] {
                let (code, err) = run(&s, c);
                assert_eq!(code, 0, "{c}: {err}");
            }
        }

        #[test]
        fn falls_back_to_snapshot_and_ignores_garbage_input() {
            let (_d, s) = setup("", Some("web1\n10.0.0.5\n"));
            assert_eq!(run(&s, "ssh web1").0, 2);
            assert_eq!(run(&s, "ssh elsewhere").0, 0);
            // No known hosts at all: allow.
            let (_d2, s2) = setup("", None);
            assert_eq!(run(&s2, "ssh web1").0, 0);
            // Input without a command.
            let mut ch = Command::new("sh")
                .arg(&s)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            ch.stdin
                .take()
                .unwrap()
                .write_all(b"{\"tool_name\":\"Read\"}")
                .unwrap();
            assert_eq!(ch.wait().unwrap().code(), Some(0));
        }

        #[test]
        fn blocks_bssh_host_and_approval_commands() {
            let (_d, s) = setup(LIVE, None);
            for c in [
                "bssh connect web1",
                "bayesian-ssh connect web1",
                "bssh exec web1 -- uptime",
                "bssh run web1 -- uptime",
                "bssh upload web1 ./f /tmp/",
                "bssh download web1 /etc/passwd ./",
                "bssh forward db1 -L 5432:localhost:5432",
                "bssh tunnel db1 -L 5432:localhost:5432",
                "bssh proxy web1 -D 1080",
                "bssh socks web1 -D 1080",
                "bssh alias web1",
                "bssh web1",
                "cd /tmp && bssh web1",
                "bssh config --default-user root",
                "bssh edit web1 --port 2222",
                "bssh add evil evil.example.com",
                "bssh remove web1",
                "bssh import",
                "bssh source sync prod",
                "bssh teams sync",
                "bssh org list",
                "bssh env use prod",
                "bssh desktop",
                "bssh ai approve",
                "bssh ai approve --list",
                "echo a | bssh ai approve",
                "bssh ai grant",
                "bssh ai policy list && bssh ai approve",
                "bssh ai stop --all",
                "bssh ai install claude-code",
                "bssh ai token alice",
                "bssh key copy web1",
                "BB=1 sudo bssh connect web1",
            ] {
                let (code, err) = run(&s, c);
                assert_eq!(code, 2, "{c}: {err}");
                assert!(err.contains("bssh MCP tools"), "{c}: {err}");
            }
        }

        #[test]
        fn allows_read_only_bssh_commands() {
            let (_d, s) = setup(LIVE, None);
            for c in [
                "bssh list",
                "bssh list -d",
                "bssh --env prod list",
                "bssh show web1",
                "bssh why web1",
                "bssh history --days 7",
                "bssh stats",
                "bssh doctor",
                "bssh ping web1",
                "bssh groups",
                "bssh export --format json",
                "bssh ai explain 'rm -rf /'",
                "bssh ai log",
                "bssh ai watch",
                "bssh ai status --json",
                "bssh ai policy show default",
                "bssh key list",
                "bssh env list",
                "bssh --help",
                "bssh",
                "grep -r bssh README.md",
            ] {
                let (code, err) = run(&s, c);
                assert_eq!(code, 0, "{c}: {err}");
            }
        }

        #[test]
        fn blocks_bssh_without_known_hosts() {
            // No hosts to protect must not switch the bssh gate off: approvals and the CLI
            // stay blocked.
            let (_d, s) = setup("", None);
            assert_eq!(run(&s, "bssh connect web1").0, 2);
            assert_eq!(run(&s, "echo a | bssh ai approve").0, 2);
            assert_eq!(run(&s, "bssh list").0, 0);
        }
    }
}
