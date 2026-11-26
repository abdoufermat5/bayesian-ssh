//! Runs the real helper against the real kernel. Skipped where Landlock is unavailable
//! (old kernel, container profile): the helper then refuses to run, exit code 126.

#![cfg(target_os = "linux")]

use std::process::{Command, Output};

fn jail(args: &[&str], cmd: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bssh-jail"))
        .args(args)
        .arg("--")
        .arg(cmd)
        .output()
        .expect("run bssh-jail")
}

fn landlock_works() -> bool {
    let out = jail(&[], "true");
    if out.status.code() == Some(126) {
        eprintln!("skipping: {}", String::from_utf8_lossy(&out.stderr));
        return false;
    }
    true
}

#[test]
fn runs_commands_and_blocks_writes() {
    if !landlock_works() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("created");
    let out = jail(
        &[],
        &format!(
            "echo hello; if (: > '{}') 2>/dev/null; then echo wrote; else echo blocked; fi",
            target.display()
        ),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello\nblocked\n");
    assert!(!target.exists(), "the jail must not allow creating files");
}

#[test]
fn reads_only_under_the_allowed_paths() {
    if !landlock_works() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let allowed = dir.path().join("allowed");
    let secret = dir.path().join("secret");
    std::fs::create_dir(&allowed).unwrap();
    std::fs::create_dir(&secret).unwrap();
    std::fs::write(allowed.join("f"), "visible").unwrap();
    std::fs::write(secret.join("f"), "hidden").unwrap();
    let cmd = format!(
        "cat '{}' 2>/dev/null; cat '{}' 2>/dev/null; true",
        allowed.join("f").display(),
        secret.join("f").display()
    );
    let out = jail(&["--read", allowed.to_str().unwrap()], &cmd);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "visible");
}

#[test]
fn dev_null_stays_writable_and_exit_codes_pass_through() {
    if !landlock_works() {
        return;
    }
    let out = jail(&[], "echo noise > /dev/null; exit 7");
    assert_eq!(out.status.code(), Some(7));
    assert!(out.stdout.is_empty());
}

/// Does this kernel have Landlock network rules (ABI 4)?
fn kernel_has_net_rules() -> bool {
    use landlock::{AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr};
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessNet::ConnectTcp)
        .is_ok()
}

#[test]
fn tcp_connections_are_refused_where_the_kernel_can() {
    if !landlock_works() || !kernel_has_net_rules() {
        eprintln!("skipping: no Landlock network rules on this kernel");
        return;
    }
    if Command::new("bash").arg("-c").arg("true").output().is_err() {
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let probe = format!(
        "bash -c 'exec 3<>/dev/tcp/127.0.0.1/{port}' 2>/dev/null && echo connected || echo refused"
    );
    // Control: the same connect works outside the jail, so a refusal is the jail's doing.
    let control = Command::new("sh")
        .arg("-c")
        .arg(&probe)
        .output()
        .expect("sh");
    assert_eq!(String::from_utf8_lossy(&control.stdout).trim(), "connected");
    let out = jail(&[], &probe);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "refused");
}

#[test]
fn rejects_invalid_read_paths() {
    for bad in ["relative/dir", "./local", "/var/../etc", "/", "//", "/."] {
        let out = jail(&["--read", bad], "echo should-not-run");
        assert_eq!(
            out.status.code(),
            Some(2),
            "expected exit 2 for --read {bad:?}, got {:?} (stderr: {})",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty(), "command ran for --read {bad:?}");
    }
}

#[test]
fn clears_parent_environment_and_sets_allowlist() {
    if !landlock_works() {
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_bssh-jail"))
        .env("BSSH_TEST_SECRET", "super-secret-value")
        .env("AWS_SECRET_ACCESS_KEY", "fake-aws-secret")
        .args([
            "--",
            "printf '%s|%s|%s|%s|%s' \"$BSSH_TEST_SECRET\" \"$AWS_SECRET_ACCESS_KEY\" \"$PATH\" \"$HOME\" \"$LC_ALL\"",
        ])
        .output()
        .expect("run bssh-jail");
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "||/usr/bin:/bin|/nonexistent|C"
    );
}

#[test]
fn prevents_path_hijacking() {
    use std::os::unix::fs::PermissionsExt;

    if !landlock_works() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let fake_sh = dir.path().join("sh");
    std::fs::write(&fake_sh, "#!/bin/sh\necho HIJACKED_SH\n").unwrap();
    std::fs::set_permissions(&fake_sh, std::fs::Permissions::from_mode(0o755)).unwrap();

    let fake_echo = dir.path().join("echo");
    std::fs::write(&fake_echo, "#!/bin/sh\n/bin/echo HIJACKED_ECHO\n").unwrap();
    std::fs::set_permissions(&fake_echo, std::fs::Permissions::from_mode(0o755)).unwrap();

    let hijacked_path = format!("{}:/usr/bin:/bin", dir.path().display());
    let out = Command::new(env!("CARGO_BIN_EXE_bssh-jail"))
        .env("PATH", hijacked_path)
        .args(["--read", dir.path().to_str().unwrap(), "--", "echo ok"])
        .output()
        .expect("run bssh-jail");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
}

#[test]
fn extra_read_does_not_grant_execute() {
    use std::os::unix::fs::PermissionsExt;

    if !landlock_works() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("run.sh");
    std::fs::write(&script, "#!/bin/sh\necho ran\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let cmd = format!(
        "cat '{}' >/dev/null && echo readable; '{}' 2>/dev/null && echo executed || echo noexec",
        script.display(),
        script.display()
    );
    let out = jail(&["--read", dir.path().to_str().unwrap()], &cmd);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "readable\nnoexec\n");
}

#[test]
fn closes_inherited_non_stdio_fds() {
    if !landlock_works() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, "leaked-fd-content").unwrap();

    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg("exec 3<\"$1\"; exec \"$2\" -- 'cat <&3 2>/dev/null || echo closed'")
        .arg("--")
        .arg(&secret)
        .arg(env!("CARGO_BIN_EXE_bssh-jail"))
        .output()
        .expect("run sh -> bssh-jail");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "closed");
}
