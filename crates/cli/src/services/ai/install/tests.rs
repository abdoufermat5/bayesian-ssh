use super::*;
use crate::services::ai::types::Profile;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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
        profile: Profile::Observe,
        environment: "default",
        approval_timeout_secs: 300,
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
    assert_eq!(
        e["args"],
        json!(["ai", "mcp", "--policy", "default", "--env", "default"])
    );
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
    for c in [
        Client::Vscode,
        Client::Cursor,
        Client::ClaudeDesktop,
        Client::Omp,
    ] {
        run_install(&opts(c, h.path(), &ai, &bin, &[])).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(config_path(c, h.path())).unwrap())
                .unwrap();
        let e = &v[c.servers_key()]["bssh"];
        assert_eq!(e["command"], "/b/bssh", "{c:?}");
        assert_eq!(
            e.get("type").is_some(),
            matches!(c, Client::Vscode | Client::Omp)
        );
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
        6
    );
    // Codex gives up on a tool call after `tool_timeout_sec`; an approval may wait
    // longer than its 60 s default.
    assert_eq!(
        d["mcp_servers"]["bssh"]["tool_timeout_sec"].as_integer(),
        Some(330)
    );
    assert!(run_install(&o).unwrap().contains("nothing to change"));
    let mut u = opts(Client::Codex, h.path(), &ai, &bin, &[]);
    u.uninstall = true;
    run_install(&u).unwrap();
    let t = std::fs::read_to_string(&p).unwrap();
    assert!(!t.contains("bssh") && t.contains("[mcp_servers.other]"));
}

#[test]
fn the_entry_is_pinned_to_the_environment_and_install_says_what_next() {
    let h = tempfile::tempdir().unwrap();
    let ai = h.path().join("ai");
    let bin = PathBuf::from("/b/bssh");
    let mut o = opts(Client::ClaudeDesktop, h.path(), &ai, &bin, &[]);
    o.environment = "prod";
    o.policy = "prod-observe";
    o.profile = Profile::Observe;
    let r = run_install(&o).unwrap();
    let v: Value = serde_json::from_str(
        &std::fs::read_to_string(config_path(Client::ClaudeDesktop, h.path())).unwrap(),
    )
    .unwrap();
    // Switching the active environment must not change what the agent reaches.
    assert_eq!(
        v["mcpServers"]["bssh"]["args"],
        json!(["ai", "mcp", "--policy", "prod-observe", "--env", "prod"])
    );
    assert!(r.contains("Restart claude-desktop"), "{r}");
    assert!(r.contains("`bssh ai watch` in another terminal"), "{r}");
    assert!(r.contains("`bssh ai doctor`"), "{r}");
    assert!(
        r.contains("Policy 'prod-observe' in environment 'prod' is observe: the agent can look at hosts but cannot change anything"),
        "{r}"
    );
    assert!(
        r.contains("`bssh ai policy set prod-observe --profile operate --all-hosts`"),
        "{r}"
    );
    // The next steps are about installing; uninstalling prints none.
    let mut u = opts(Client::ClaudeDesktop, h.path(), &ai, &bin, &[]);
    u.uninstall = true;
    let r = run_install(&u).unwrap();
    assert!(!r.contains("Next:"), "{r}");
}

#[test]
fn codex_tool_timeout_follows_the_approval_timeout() {
    let h = tempfile::tempdir().unwrap();
    let ai = h.path().join("ai");
    let bin = PathBuf::from("/b/bssh");
    let mut o = opts(Client::Codex, h.path(), &ai, &bin, &[]);
    o.approval_timeout_secs = 600;
    run_install(&o).unwrap();
    let t = std::fs::read_to_string(config_path(Client::Codex, h.path())).unwrap();
    let d: toml::Table = t.parse().unwrap();
    assert_eq!(
        d["mcp_servers"]["bssh"]["tool_timeout_sec"].as_integer(),
        Some(630)
    );
    // A shorter approval window still leaves room for the call itself.
    o.approval_timeout_secs = 0;
    run_install(&o).unwrap();
    let t = std::fs::read_to_string(config_path(Client::Codex, h.path())).unwrap();
    let d: toml::Table = t.parse().unwrap();
    assert_eq!(
        d["mcp_servers"]["bssh"]["tool_timeout_sec"].as_integer(),
        Some(30)
    );
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
    use serde_json::json;
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
        let input = json!({"tool_name": "Bash", "tool_input": {"command": command}}).to_string();
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
            // Accepting a team sync's route change is the human's call.
            "bssh confirm web1",
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
            // Widening the policy that governs the agent is the human's call.
            "bssh ai policy set default --profile operate --all-hosts",
            "bssh ai policy set default --profile operate --tag prod",
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
            "bssh ai policy list",
            "bssh ai policy test checks.txt",
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
