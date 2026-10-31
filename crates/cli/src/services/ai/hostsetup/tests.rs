use super::check::{ca_check_script, parse_agent_user};
use super::scripts::{agent_user_script_in, trust_script_in};
use super::*;
use crate::models::Connection;
use crate::services::ai::jail::shell_quote;
use crate::services::ai::types::{
    Enforcement, ExecLimits, ExecMode, ExecResult, Executor, JailKind, Policy, Profile,
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

fn conn() -> Connection {
    Connection::new("web".into(), "web.test".into(), "ops".into(), 22)
}

fn ca_line() -> String {
    // Any valid-looking key line; the script only treats it as quoted text.
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHgGJ3W9P2bVx2w0n6sYhR3m7f1QzY9Y6c2uPq8dJvLk bssh-ai-ca@laptop"
        .to_string()
}

fn sh_syntax_ok(script: &str) {
    let mut child = Command::new("sh")
        .arg("-n")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn fake_bin(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn run_script(script: &str, bin: &Path, cwd: &Path) -> (i32, String, String) {
    run_script_with_path(script, &format!("{}:/usr/bin:/bin", bin.display()), cwd)
}

fn run_script_with_path(script: &str, path: &str, cwd: &Path) -> (i32, String, String) {
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .current_dir(cwd)
        .env("PATH", path)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

// ---- setup script ----

#[test]
fn agent_script_has_the_documented_effects_and_nothing_more() {
    let s = agent_user_script();
    sh_syntax_ok(&s);
    assert!(s.contains("useradd --system --create-home"));
    assert!(s.contains("--shell /bin/sh"));
    assert!(s.contains(&shell_quote(AGENT_HOME)));
    assert!(s.contains("usermod -p '*'"));
    assert!(s.contains("systemd-journal adm"));
    assert!(s.contains("$H/.local/bin") && s.contains("$H/backups"));
    for forbidden in [
        "sudoers",
        "visudo",
        "iptables",
        "nft ",
        "ufw",
        "firewall-cmd",
        "passwd -d",
    ] {
        assert!(!s.contains(forbidden), "setup must not touch {forbidden}");
    }
}

#[test]
fn agent_script_runs_against_fake_tools() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let home = tmp.path().join("home");
    let log = tmp.path().join("calls.log");
    let l = log.display();
    fake_bin(
        &bin,
        "id",
        "case \"$1\" in -gn) echo bssh-agent;; *) exit 1;; esac",
    );
    fake_bin(
        &bin,
        "useradd",
        &format!("echo \"useradd $*\" >> {l}; mkdir -p \"$4\""),
    );
    fake_bin(&bin, "usermod", &format!("echo \"usermod $*\" >> {l}"));
    fake_bin(
        &bin,
        "getent",
        &format!(
            "if [ \"$1\" = group ]; then [ \"$2\" = systemd-journal ]; \
             else echo 'bssh-agent:x:998:998::{}:/bin/sh'; fi",
            home.display()
        ),
    );
    fake_bin(&bin, "chown", &format!("echo \"chown $*\" >> {l}"));
    let (code, out, err) = run_script(
        &agent_user_script_in(&home.display().to_string()),
        &bin,
        tmp.path(),
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("created user bssh-agent"));
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.contains(&format!(
        "useradd --system --create-home --home-dir {}",
        home.display()
    )));
    assert!(calls.contains("usermod -p * bssh-agent"));
    assert!(calls.contains("usermod -a -G systemd-journal bssh-agent"));
    assert!(
        !calls.contains("usermod -a -G adm"),
        "adm does not exist on this fake host"
    );
    assert!(home.join(".local/bin").is_dir() && home.join("backups").is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&home), 0o700);
        assert_eq!(mode(&home.join(".local/bin")), 0o700);
    }
}

#[test]
fn agent_script_leaves_an_existing_user_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let log = tmp.path().join("calls.log");
    let l = log.display();
    fake_bin(
        &bin,
        "id",
        "case \"$1\" in -gn) echo bssh-agent;; *) exit 0;; esac",
    );
    fake_bin(&bin, "useradd", &format!("echo useradd >> {l}"));
    fake_bin(&bin, "usermod", &format!("echo \"usermod $*\" >> {l}"));
    fake_bin(
        &bin,
        "getent",
        &format!(
            "if [ \"$1\" = group ]; then exit 1; else echo 'bssh-agent:x:998:998::{}:/bin/sh'; fi",
            home.display()
        ),
    );
    fake_bin(&bin, "chown", "true");
    let (code, out, err) = run_script(
        &agent_user_script_in(&home.display().to_string()),
        &bin,
        tmp.path(),
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("already exists"));
    assert!(!log.exists(), "no useradd/usermod when nothing is missing");
    assert!(home.join(".local/bin").is_dir());
}

// ---- trust script ----

#[test]
fn trust_script_rejects_anything_but_a_plain_key_line() {
    assert!(trust_script("").is_err());
    assert!(trust_script("ssh-ed25519").is_err());
    assert!(trust_script("ssh-ed25519 AAAA'; rm -rf / #").is_err());
    assert!(trust_script("ssh-ed25519 AAAA\nssh-ed25519 BBBB").is_err());
    assert!(trust_script("echo AAAA").is_err());
    let ok = trust_script(&ca_line()).unwrap();
    sh_syntax_ok(&ok);
    assert!(ok.contains("TrustedUserCAKeys"));
    assert!(ok.contains("AuthorizedPrincipalsFile"));
    assert!(ok.contains("Match User bssh-agent"));
    assert!(ok.contains("\"$SSHD\" -t"));
    assert!(ok.contains("systemctl reload sshd"));
}

/// A scratch `/etc/ssh` plus fake `sshd` and `systemctl`.
struct SshBox {
    tmp: tempfile::TempDir,
}

impl SshBox {
    fn new(sshd_t_ok: bool, trusted_ca: Option<&str>, config: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("bin")).unwrap();
        std::fs::create_dir(tmp.path().join("ssh")).unwrap();
        std::fs::write(tmp.path().join("ssh/sshd_config"), config).unwrap();
        let t = match trusted_ca {
            Some(path) => format!("echo 'trustedusercakeys {path}'"),
            None => "echo 'trustedusercakeys none'".to_string(),
        };
        fake_bin(
            &tmp.path().join("bin"),
            "sshd",
            &format!(
                "if [ \"$1\" = -T ]; then echo 'port 22'; {t}; exit 0; fi\n\
                 {}",
                if sshd_t_ok {
                    "exit 0"
                } else {
                    "echo 'bad config' >&2; exit 255"
                }
            ),
        );
        fake_bin(
            &tmp.path().join("bin"),
            "systemctl",
            &format!(
                "echo \"systemctl $*\" >> {}/systemctl.log",
                tmp.path().display()
            ),
        );
        Self { tmp }
    }

    fn ssh(&self) -> String {
        self.tmp.path().join("ssh").display().to_string()
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.tmp.path().join("ssh").join(rel)).unwrap_or_default()
    }

    fn run(&self) -> (i32, String, String) {
        let script = trust_script_in(&ca_line(), &self.ssh()).unwrap();
        run_script(&script, &self.tmp.path().join("bin"), self.tmp.path())
    }
}

#[test]
fn trust_installs_the_ca_principals_and_reloads() {
    let b = SshBox::new(true, None, "Port 22\nPermitRootLogin no\n");
    let (code, out, err) = b.run();
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("sshd reloaded"));
    assert_eq!(b.read("bssh_ai_ca.pub"), format!("{}\n", ca_line()));
    assert_eq!(b.read("bssh_ai_principals/bssh-agent"), "bssh-agent\n");
    let drop_in = b.read("sshd_config.d/50-bssh-ai.conf");
    assert!(drop_in.contains(&format!("TrustedUserCAKeys {}/bssh_ai_ca.pub", b.ssh())));
    let cfg = b.read("sshd_config");
    assert!(
        cfg.starts_with(&format!("Include {}/sshd_config.d/*.conf\n", b.ssh())),
        "{cfg}"
    );
    assert!(
        cfg.contains("Port 22\nPermitRootLogin no\n"),
        "existing settings are kept"
    );
    let block = cfg.split("# bssh-ai begin").nth(1).expect("our block");
    assert!(block.contains("Match User bssh-agent"));
    assert!(block.contains(&format!(
        "AuthorizedPrincipalsFile {}/bssh_ai_principals/%u",
        b.ssh()
    )));
    assert!(
        cfg.trim_end().ends_with("# bssh-ai end"),
        "the Match block is the last thing in sshd_config"
    );
    let reload = std::fs::read_to_string(b.tmp.path().join("systemctl.log")).unwrap();
    assert!(reload.contains("systemctl reload sshd"));
}

#[test]
fn trust_is_idempotent() {
    let b = SshBox::new(
        true,
        None,
        "Include /etc/ssh/sshd_config.d/*.conf\nPort 22\n",
    );
    assert_eq!(b.run().0, 0);
    let cfg1 = b.read("sshd_config");
    let ca1 = b.read("bssh_ai_ca.pub");
    assert_eq!(b.run().0, 0);
    assert_eq!(
        b.read("sshd_config"),
        cfg1,
        "no duplicate blocks or includes"
    );
    assert_eq!(b.read("bssh_ai_ca.pub"), ca1, "key is not appended twice");
    assert_eq!(
        cfg1.matches("Include").count(),
        1,
        "an existing Include is reused"
    );
}

#[test]
fn trust_rolls_back_when_sshd_rejects_the_config() {
    let original = "Port 22\n";
    let b = SshBox::new(false, None, original);
    let (code, _, err) = b.run();
    assert_eq!(code, 1);
    assert!(err.contains("restoring the previous"), "{err}");
    assert_eq!(b.read("sshd_config"), original);
    assert!(!b.tmp.path().join("ssh/bssh_ai_ca.pub").exists());
    assert!(!b
        .tmp
        .path()
        .join("ssh/sshd_config.d/50-bssh-ai.conf")
        .exists());
    assert!(
        !b.tmp.path().join("systemctl.log").exists(),
        "no reload after a failed check"
    );
}

#[test]
fn trust_joins_an_existing_ca_file_instead_of_replacing_it() {
    let tmp_ca = tempfile::tempdir().unwrap();
    let company = tmp_ca.path().join("company_ca.pub");
    std::fs::write(&company, "ssh-ed25519 AAAAcompanyca company\n").unwrap();
    let b = SshBox::new(true, Some(&company.display().to_string()), "Port 22\n");
    let (code, _, err) = b.run();
    assert_eq!(code, 0, "{err}");
    let ca_file = std::fs::read_to_string(&company).unwrap();
    assert!(ca_file.starts_with("ssh-ed25519 AAAAcompanyca company\n"));
    assert!(ca_file.contains(&ca_line()));
    assert!(
        !b.ssh().is_empty()
            && !b
                .tmp
                .path()
                .join("ssh/sshd_config.d/50-bssh-ai.conf")
                .exists(),
        "no second TrustedUserCAKeys directive"
    );
    assert!(b.read("sshd_config").contains("Match User bssh-agent"));
}

#[test]
fn failed_check_restores_a_shared_ca_file() {
    let tmp_ca = tempfile::tempdir().unwrap();
    let company = tmp_ca.path().join("company_ca.pub");
    std::fs::write(&company, "ssh-ed25519 AAAAcompanyca company\n").unwrap();
    let b = SshBox::new(false, Some(&company.display().to_string()), "Port 22\n");
    assert_eq!(b.run().0, 1);
    assert_eq!(
        std::fs::read_to_string(&company).unwrap(),
        "ssh-ed25519 AAAAcompanyca company\n"
    );
    assert_eq!(b.read("sshd_config"), "Port 22\n");
}

// ---- root wrapper ----

#[test]
fn root_command_passes_the_script_as_one_quoted_argument() {
    for script in ["id", "echo 'a'; $(x) `y`", "x\ny", "'; touch PWNED; '"] {
        for mode in [SudoPassword::Fail, SudoPassword::Prompt] {
            let line = root_command(script, mode);
            sh_syntax_ok(&line);
            assert!(line.starts_with("sh -c '"));
            assert!(line.ends_with(match mode {
                SudoPassword::Prompt => " i",
                SudoPassword::Fail => " n",
            }));
            assert!(line.contains(&shell_quote(script)));
        }
    }
}

#[test]
fn root_command_runs_directly_as_root_and_asks_sudo_otherwise() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let log = tmp.path().join("sudo.log");
    // Not root, passwordless sudo available.
    fake_bin(&bin, "id", "echo 1000");
    fake_bin(
        &bin,
        "sudo",
        &format!(
            "echo \"sudo $*\" >> {}\nif [ \"$2\" = true ]; then exit 0; fi\nshift\nexec \"$@\"",
            log.display()
        ),
    );
    let (code, out, _) = run_script(
        &root_command("echo script-ran", SudoPassword::Fail),
        &bin,
        tmp.path(),
    );
    assert_eq!(code, 0);
    assert!(out.contains("script-ran"), "{out}");
    assert!(std::fs::read_to_string(&log).unwrap().contains("sudo -n"));

    // Not root, sudo wants a password.
    fake_bin(&bin, "sudo", "exit 1");
    let (code, _, err) = run_script(
        &root_command("echo nope", SudoPassword::Fail),
        &bin,
        tmp.path(),
    );
    assert_eq!(code, EXIT_SUDO_PASSWORD);
    assert!(err.contains("password"));

    // No sudo at all.
    std::fs::remove_file(bin.join("sudo")).unwrap();
    // Only the fake `id` and a `sh` are on PATH: the host's own sudo must stay out of sight.
    std::os::unix::fs::symlink("/bin/sh", bin.join("sh")).unwrap();
    let (code, _, _) = run_script_with_path(
        &root_command("echo nope", SudoPassword::Fail),
        &bin.display().to_string(),
        tmp.path(),
    );
    assert_eq!(code, EXIT_NO_SUDO);

    // Root: sudo is never touched.
    fake_bin(&bin, "id", "echo 0");
    let (code, out, _) = run_script(
        &root_command("echo as-root", SudoPassword::Fail),
        &bin,
        tmp.path(),
    );
    assert_eq!((code, out.trim()), (0, "as-root"));
}

// ---- fake executor, host checks ----

#[derive(Clone, Default)]
struct Host {
    reachable: bool,
    agent: bool,
    helper: bool,
    ca_trusted: bool,
    principals: bool,
    landlock: bool,
}

#[async_trait]
impl Executor for Host {
    async fn exec(
        &self,
        _: &Connection,
        command: &str,
        mode: ExecMode,
        _: ExecLimits,
    ) -> Result<ExecResult> {
        assert_eq!(mode, ExecMode::Plain);
        if !self.reachable {
            bail!("connection refused");
        }
        let ok = |s: &str| ExecResult {
            stdout: s.to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            truncated: false,
            timed_out: false,
            duration_ms: 1,
        };
        let fail = |code: i32| ExecResult {
            stdout: String::new(),
            stderr: "nope".into(),
            exit_code: Some(code),
            truncated: false,
            timed_out: false,
            duration_ms: 1,
        };
        if command.contains("echo bssh-ok") {
            Ok(ok("bssh-ok\n"))
        } else if command.contains("getent passwd") {
            if self.agent {
                Ok(ok("bssh-agent:x:998:998::/var/lib/bssh-agent:/bin/sh\nbssh-agent systemd-journal adm\n"))
            } else {
                Ok(fail(2))
            }
        } else if command.contains("bssh-yes") {
            Ok(ok(if self.helper { "bssh-yes\n" } else { "" }))
        } else if command.contains("bssh-ca-trusted") {
            let mut out = String::from(if self.ca_trusted {
                "bssh-ca-trusted\n"
            } else {
                "bssh-ca-untrusted\n"
            });
            if self.principals {
                out.push_str("bssh-principals-ok\n");
            }
            Ok(ok(&out))
        } else if command.contains("setpriv") && self.landlock {
            Ok(ok("bssh-jail-ok\nbssh-jail-readonly\n"))
        } else {
            Ok(fail(126))
        }
    }
}

#[tokio::test]
async fn report_for_a_fully_prepared_host() {
    let host = Host {
        reachable: true,
        agent: true,
        ca_trusted: true,
        principals: true,
        landlock: true,
        ..Host::default()
    };
    let r = check_host(&conn(), &host, Some(&ca_line())).await;
    assert!(r.reachable && r.agent_user);
    assert_eq!(r.agent_home.as_deref(), Some("/var/lib/bssh-agent"));
    assert_eq!(r.agent_groups, ["bssh-agent", "systemd-journal", "adm"]);
    assert_eq!(r.jail, JailKind::Landlock);
    assert_eq!(r.ca_trusted, Some(true));
    assert_eq!(r.principals_mapped, Some(true));
    assert_eq!(r.level(), "certificate-enforced");
    assert_eq!(
        r.enforcement,
        Enforcement {
            jail: JailKind::Landlock,
            certificates: true,
            agent_user: true
        }
    );
}

#[tokio::test]
async fn report_for_a_bare_host_is_client_enforced() {
    let host = Host {
        reachable: true,
        ..Host::default()
    };
    let r = check_host(&conn(), &host, None).await;
    assert!(r.reachable && !r.agent_user);
    assert_eq!(r.jail, JailKind::None);
    assert_eq!(r.ca_trusted, None, "no local CA, nothing to compare");
    assert_eq!(r.level(), "client-enforced");
    assert_eq!(r.enforcement, Enforcement::default());
}

#[tokio::test]
async fn untrusted_ca_means_no_certificate_enforcement() {
    let host = Host {
        reachable: true,
        agent: true,
        landlock: true,
        ..Host::default()
    };
    let r = check_host(&conn(), &host, Some(&ca_line())).await;
    assert_eq!(r.ca_trusted, Some(false));
    assert!(!r.enforcement.certificates);
    assert_eq!(r.level(), "jail-enforced");
}

#[tokio::test]
async fn unreachable_host_is_reported_not_raised() {
    let r = check_host(&conn(), &Host::default(), Some(&ca_line())).await;
    assert!(!r.reachable);
    assert!(r
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("connection refused"));
}

#[test]
fn agent_user_output_parsing() {
    assert_eq!(
        parse_agent_user("bssh-agent:x:998:998:c:/var/lib/bssh-agent:/bin/sh\nbssh-agent adm\n"),
        Some((
            "/var/lib/bssh-agent".into(),
            vec!["bssh-agent".into(), "adm".into()]
        ))
    );
    assert_eq!(parse_agent_user("other:x:1:1::/home/o:/bin/sh\n"), None);
    assert_eq!(parse_agent_user("garbage"), None);
    assert_eq!(parse_agent_user(""), None);
}

#[test]
fn ca_check_script_quotes_the_key_and_parses() {
    sh_syntax_ok(&ca_check_script("AAAAC3+/="));
    let s = ca_check_script("AAA'; touch PWNED; '");
    sh_syntax_ok(&s);
    assert!(!s.contains("; touch PWNED; ;"));
}

// ---- sudoers suggestions ----

fn policy(profile: Profile, writes: &[&str]) -> Policy {
    Policy {
        profile,
        write_paths: writes.iter().map(|s| s.to_string()).collect(),
        ..Policy::default()
    }
}

#[test]
fn sudoers_lines_are_narrow_and_follow_the_write_paths() {
    let s = sudoers_suggestions(&policy(
        Profile::Operate,
        &["/etc/nginx/**", "/etc/systemd/system/**"],
    ));
    assert_eq!(s.len(), 2);
    assert_eq!(
        s[0].line,
        "bssh-agent ALL=(root) NOPASSWD: /usr/bin/systemctl reload nginx, \
         /usr/bin/systemctl restart nginx, /usr/sbin/nginx -t"
    );
    assert_eq!(
        s[1].line,
        "bssh-agent ALL=(root) NOPASSWD: /usr/bin/systemctl daemon-reload"
    );
    for sug in &s {
        assert!(
            !sug.line.contains("ALL=(ALL)") && !sug.line.contains(": ALL"),
            "{}",
            sug.line
        );
        assert!(!sug.line.contains('*'));
        assert!(!sug.why.is_empty());
    }
}

#[test]
fn observe_policies_and_unknown_paths_suggest_nothing() {
    assert!(sudoers_suggestions(&policy(Profile::Observe, &[])).is_empty());
    assert!(sudoers_suggestions(&policy(Profile::Operate, &[])).is_empty());
    assert!(sudoers_suggestions(&policy(Profile::Operate, &["/srv/app/**"])).is_empty());
    assert!(sudoers_suggestions(&policy(Profile::Operate, &["**/*.conf"])).is_empty());
}

#[test]
fn egress_note_does_not_promise_firewall_changes() {
    assert!(EGRESS_NOTE.contains("does not change firewalls"));
}
