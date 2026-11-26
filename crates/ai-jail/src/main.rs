//! `bssh-jail [--read PATH]... -- COMMAND`
//!
//! Runs `sh -c COMMAND` under a Landlock ruleset: no writes anywhere (except `/dev/null`),
//! read and execute only under the system paths programs need plus every `--read PATH`,
//! no TCP bind or connect where the kernel's Landlock ABI supports it. It is the fallback
//! of `bssh ai` on hosts whose util-linux has no `setpriv --landlock-access` (< 2.40).
//! It lives in `~bssh-agent/.local/bin`, so it needs no root.
//!
//! If the kernel can't enforce the ruleset, the command does not run (exit 126).

/// Keep in sync with `SYSTEM_READ_PATHS` in `crates/cli/src/services/ai/jail.rs`.
const SYSTEM_READ_PATHS: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/ld.so.conf.d",
    "/etc/passwd",
    "/etc/group",
    "/etc/nsswitch.conf",
    "/etc/localtime",
    "/etc/os-release",
    "/etc/hostname",
    "/proc",
    "/sys",
    "/dev/urandom",
    "/dev/random",
    "/dev/zero",
];

/// Exit code when the jail can't be enforced. The command did not run.
const EXIT_NO_JAIL: i32 = 126;

#[derive(Debug, PartialEq, Eq)]
struct Args {
    allow_partial: bool,
    read: Vec<String>,
    command: String,
}

fn validate_read_path(raw: &str) -> Result<(), String> {
    use std::path::{Component, Path};

    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(format!("--read path must be absolute: {raw:?}"));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("--read path must not contain `..`: {raw:?}"));
    }
    if !path.components().any(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("--read path must not be `/`: {raw:?}"));
    }
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut it = args.into_iter();
    let mut allow_partial = false;
    let mut read = Vec::new();
    loop {
        match it.next().as_deref() {
            Some("--allow-partial") => allow_partial = true,
            Some("--read") => {
                let path = it.next().ok_or("--read needs a path")?;
                validate_read_path(&path)?;
                read.push(path);
            }
            Some("--") => break,
            Some(other) => return Err(format!("unexpected argument {other:?}")),
            None => return Err("missing `-- COMMAND`".into()),
        }
    }
    let command = it.next().ok_or("missing command after `--`")?;
    if it.next().is_some() {
        return Err("exactly one command string is expected after `--`".into());
    }
    Ok(Args {
        allow_partial,
        read,
        command,
    })
}

#[cfg(target_os = "linux")]
fn close_inherited_fds() {
    const F_SETFD: std::ffi::c_int = 2;
    const FD_CLOEXEC: std::ffi::c_int = 1;

    // SAFETY: `fcntl` is a standard POSIX/Linux libc symbol linked by `std`.
    unsafe extern "C" {
        fn fcntl(fd: std::ffi::c_int, cmd: std::ffi::c_int, ...) -> std::ffi::c_int;
    }

    let Ok(entries) = std::fs::read_dir("/proc/self/fd") else {
        return;
    };
    for entry in entries.flatten() {
        let Some(fd) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<std::ffi::c_int>().ok())
        else {
            continue;
        };
        if fd >= 3 {
            // SAFETY: Setting `FD_CLOEXEC` via `F_SETFD` on an open fd is idempotent and
            // safe even for `read_dir`'s own fd; the kernel closes all marked fds on `exec()`.
            unsafe {
                fcntl(fd, F_SETFD, FD_CLOEXEC);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    use std::os::unix::process::CommandExt;

    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "bssh-jail: {e}\nusage: bssh-jail [--allow-partial] [--read PATH]... -- COMMAND"
            );
            std::process::exit(2);
        }
    };
    if let Err(e) = jail(&args.read, args.allow_partial) {
        eprintln!("bssh-jail: {e}");
        std::process::exit(EXIT_NO_JAIL);
    }
    close_inherited_fds();
    let err = std::process::Command::new("/bin/sh")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent")
        .env("LC_ALL", "C")
        .arg("-c")
        .arg(&args.command)
        .exec();
    eprintln!("bssh-jail: cannot run /bin/sh: {err}");
    std::process::exit(127);
}

#[cfg(target_os = "linux")]
fn jail(extra_read: &[String], allow_partial: bool) -> Result<(), String> {
    use landlock::{
        ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
        RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope,
    };

    // The newest ABI we know; best-effort drops rights an older kernel doesn't have
    // (scopes need ABI 6, network rules need ABI 4, truncate ABI 3, ...), and the
    // status check below fails closed on partial enforcement unless `--allow-partial`
    // was explicitly requested.
    let abi = ABI::V6;
    let system_read = AccessFs::from_read(abi); // execute, read file, read dir
    let extra_read_rights = AccessFs::ReadFile | AccessFs::ReadDir;
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(abi))
        .and_then(|r| r.handle_access(AccessNet::from_all(abi)))
        .and_then(|r| r.scope(Scope::AbstractUnixSocket | Scope::Signal))
        .and_then(|r| r.create())
        .map_err(|e| format!("landlock is not available: {e}"))?;

    for path in SYSTEM_READ_PATHS {
        // A path that doesn't exist on this host grants nothing.
        let Ok(fd) = PathFd::new(path) else { continue };
        ruleset = ruleset
            .add_rule(PathBeneath::new(fd, system_read))
            .map_err(|e| format!("cannot allow reading {path}: {e}"))?;
    }
    for path in extra_read {
        let Ok(fd) = PathFd::new(path) else { continue };
        ruleset = ruleset
            .add_rule(PathBeneath::new(fd, extra_read_rights))
            .map_err(|e| format!("cannot allow reading {path}: {e}"))?;
    }
    if let Ok(fd) = PathFd::new("/dev/null") {
        let rights = AccessFs::ReadFile | AccessFs::WriteFile | AccessFs::Truncate;
        ruleset = ruleset
            .add_rule(PathBeneath::new(fd, rights))
            .map_err(|e| format!("cannot allow /dev/null: {e}"))?;
    }

    let status = ruleset
        .restrict_self()
        .map_err(|e| format!("cannot enforce the jail: {e}"))?;
    match status.ruleset {
        RulesetStatus::FullyEnforced => Ok(()),
        RulesetStatus::PartiallyEnforced if allow_partial => Ok(()),
        RulesetStatus::PartiallyEnforced => {
            Err("kernel only partially enforces Landlock (pass --allow-partial to allow)".into())
        }
        RulesetStatus::NotEnforced => Err("this kernel does not enforce Landlock".into()),
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    let _ = (SYSTEM_READ_PATHS, parse_args(std::env::args().skip(1)));
    eprintln!("bssh-jail: Landlock needs Linux");
    std::process::exit(EXIT_NO_JAIL);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_reads_and_command() {
        assert_eq!(
            p(&["--read", "/var/log", "--read", "/etc", "--", "ls -l"]),
            Ok(Args {
                allow_partial: false,
                read: vec!["/var/log".into(), "/etc".into()],
                command: "ls -l".into()
            })
        );
        assert_eq!(
            p(&["--allow-partial", "--read", "/var/log", "--", "true"]),
            Ok(Args {
                allow_partial: true,
                read: vec!["/var/log".into()],
                command: "true".into()
            })
        );
        assert_eq!(
            p(&["--", "--read x"]).unwrap().command,
            "--read x",
            "everything after -- is the command"
        );
    }

    #[test]
    fn refuses_malformed_calls() {
        assert!(p(&[]).is_err());
        assert!(p(&["--read"]).is_err());
        assert!(p(&["--read", "/x"]).is_err());
        assert!(p(&["--"]).is_err());
        assert!(p(&["--", "a", "b"]).is_err());
        assert!(p(&["-x", "--", "a"]).is_err());
    }

    #[test]
    fn refuses_unsafe_read_paths() {
        assert!(p(&["--read", "relative/path", "--", "true"]).is_err());
        assert!(p(&["--read", "./etc", "--", "true"]).is_err());
        assert!(p(&["--read", "/var/../etc", "--", "true"]).is_err());
        assert!(p(&["--read", "/..", "--", "true"]).is_err());
        assert!(p(&["--read", "/", "--", "true"]).is_err());
        assert!(p(&["--read", "//", "--", "true"]).is_err());
        assert!(p(&["--read", "/.", "--", "true"]).is_err());
    }
}
