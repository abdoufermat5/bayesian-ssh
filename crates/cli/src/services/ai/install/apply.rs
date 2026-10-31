//! Writing the changes (with a diff and a backup) and the install flow.

use crate::services::ai::types::Profile;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::client::{Client, claude_settings_path, config_path};
use super::guard::guard_script;
use super::merge::{json_entry, merge_codex, merge_guard_settings, merge_json_mcp};

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
    /// The policy's profile, for the summary printed after installing.
    pub profile: Profile,
    /// Environment written into the agent's entry, so it cannot follow an active-env switch.
    pub environment: &'a str,
    /// `ai.approval_timeout_secs`; Codex's tool timeout is derived from it.
    pub approval_timeout_secs: u64,
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
        let entry = (!o.uninstall).then_some((
            o.bssh_bin,
            o.policy,
            o.environment,
            o.approval_timeout_secs,
        ));
        merge_codex(before.as_deref(), entry)?
    } else {
        let entry =
            (!o.uninstall).then(|| json_entry(o.client, o.bssh_bin, o.policy, o.environment));
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
    if !o.uninstall {
        out.push_str(&next_steps(o));
    }
    Ok(out)
}

/// What to do after writing the config: restart the client, where approvals happen, what the
/// policy allows.
fn next_steps(o: &InstallOptions) -> String {
    let allows = match o.profile {
        Profile::Observe => "observe: the agent can look at hosts but cannot change anything",
        Profile::Operate => "operate: reads run freely, every change waits for your approval",
        Profile::Oncall => {
            "oncall: reads run freely, changes wait for approval (usually a remote approver)"
        }
    };
    let mut out = format!(
        "\nNext:\n  \
         1. Restart {client} so it starts the new bssh server.\n  \
         2. Approve what the agent asks to run: `bssh ai watch` in another terminal, or the \
         desktop app (`bssh desktop`).\n  \
         3. Check the hosts you want it to use with `bssh ai doctor`.\n\n\
         Policy '{policy}' in environment '{env}' is {allows}.\n",
        client = o.client.name(),
        policy = o.policy,
        env = o.environment,
    );
    if o.profile == Profile::Observe {
        out.push_str(&format!(
            "\nTo let it change hosts, run `bssh ai policy set {policy} --profile operate \
             --all-hosts` (or `--tag <TAG>` / `--host <NAME>` for a narrower scope), then \
             restart {client}.\n",
            policy = o.policy,
            client = o.client.name(),
        ));
    }
    out
}
