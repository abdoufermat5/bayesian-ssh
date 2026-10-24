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
    read: Vec<String>,
    command: String,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut it = args.into_iter();
    let mut read = Vec::new();
    loop {
        match it.next().as_deref() {
            Some("--read") => read.push(it.next().ok_or("--read needs a path")?),
            Some("--") => break,
            Some(other) => return Err(format!("unexpected argument {other:?}")),
            None => return Err("missing `-- COMMAND`".into()),
        }
    }
    let command = it.next().ok_or("missing command after `--`")?;
    if it.next().is_some() {
        return Err("exactly one command string is expected after `--`".into());
    }
    Ok(Args { read, command })
}

#[cfg(target_os = "linux")]
fn main() {
    use std::os::unix::process::CommandExt;

    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bssh-jail: {e}\nusage: bssh-jail [--read PATH]... -- COMMAND");
            std::process::exit(2);
        }
    };
    if let Err(e) = jail(&args.read) {
        eprintln!("bssh-jail: {e}");
        std::process::exit(EXIT_NO_JAIL);
    }
    let err = std::process::Command::new("sh")
        .arg("-c")
        .arg(&args.command)
        .exec();
    eprintln!("bssh-jail: cannot run sh: {err}");
    std::process::exit(127);
}

#[cfg(target_os = "linux")]
fn jail(extra_read: &[String]) -> Result<(), String> {
    use landlock::{
        Access, AccessFs, AccessNet, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset,
        RulesetAttr, RulesetCreatedAttr, RulesetStatus, ABI,
    };

    // The newest ABI we know; best-effort drops rights an older kernel doesn't have
    // (network rules need ABI 4, truncate ABI 3, ...), the status check below decides
    // whether what remains is acceptable.
    let abi = ABI::V5;
    let read = AccessFs::from_read(abi); // execute, read file, read dir
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(abi))
        .and_then(|r| r.handle_access(AccessNet::from_all(abi)))
        .and_then(|r| r.create())
        .map_err(|e| format!("landlock is not available: {e}"))?;

    for path in SYSTEM_READ_PATHS
        .iter()
        .copied()
        .chain(extra_read.iter().map(String::as_str))
    {
        // A path that doesn't exist on this host grants nothing.
        let Ok(fd) = PathFd::new(path) else { continue };
        ruleset = ruleset
            .add_rule(PathBeneath::new(fd, read))
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
    if status.ruleset == RulesetStatus::NotEnforced {
        return Err("this kernel does not enforce Landlock".into());
    }
    Ok(())
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
                read: vec!["/var/log".into(), "/etc".into()],
                command: "ls -l".into()
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
}
