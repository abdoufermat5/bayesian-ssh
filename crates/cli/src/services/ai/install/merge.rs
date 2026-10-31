//! Merging the bssh entry into each client's config file.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Map, Value};
use std::path::Path;

use super::client::{Client, SERVER_NAME};

fn mcp_args(policy: &str, environment: &str) -> Vec<String> {
    vec![
        "ai".into(),
        "mcp".into(),
        "--policy".into(),
        policy.to_string(),
        "--env".into(),
        environment.to_string(),
    ]
}

pub(super) fn json_entry(client: Client, bin: &Path, policy: &str, environment: &str) -> Value {
    let mut o = Map::new();
    if matches!(client, Client::ClaudeCode | Client::Vscode | Client::Omp) {
        o.insert("type".into(), json!("stdio"));
    }
    o.insert("command".into(), json!(bin.to_string_lossy()));
    o.insert("args".into(), json!(mcp_args(policy, environment)));
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
///
/// `entry` carries the binary, the policy, the environment and the approval timeout. Codex
/// gives up on a tool call after `tool_timeout_sec` (60 s by default), which is shorter than
/// an approval can wait, so the entry asks for the approval window plus 30 s.
pub fn merge_codex(
    before: Option<&str>,
    entry: Option<(&Path, &str, &str, u64)>,
) -> Result<Option<String>> {
    let mut doc: toml_edit::DocumentMut = before
        .unwrap_or("")
        .parse()
        .context("~/.codex/config.toml is not valid TOML; not touching it")?;
    let old = doc.to_string();
    match entry {
        Some((bin, policy, environment, approval_timeout_secs)) => {
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
            for a in mcp_args(policy, environment) {
                args.push(a);
            }
            t["args"] = toml_edit::value(args);
            t["tool_timeout_sec"] =
                toml_edit::value(approval_timeout_secs.saturating_add(30) as i64);
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
