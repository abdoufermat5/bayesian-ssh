//! Read-only jail: Landlock (setpriv / bssh-jail helper) or bubblewrap wrappers.
//!
//!
//! `wrap` turns a command into one string that is safe to hand to `ssh host '<string>'`
//! (or to a certificate's `force-command`). Whatever shell the remote account uses
//! (bash, fish, csh), the string is a single `sh -c '<script>' bssh-jail '<command>'`:
//! the script is fixed text plus quoted paths, and the command travels as a positional
//! argument, so it is quoted exactly once and never spliced into code.
//!
//! What each jail guarantees:
//! - Landlock (`setpriv --landlock-access ...`, util-linux >= 2.40): no writes anywhere
//!   (except `/dev/null`), reads and execution only under the policy's read paths plus
//!   what programs need to start, no TCP bind/connect where the kernel's Landlock ABI is at
//!   least 4. UDP and unix sockets are not covered on older ABIs. Deny paths can't be
//!   expressed (Landlock only allows); the low-privilege user's file permissions do that.
//! - Bubblewrap: the whole filesystem read-only, no network or IPC namespace access, a
//!   private `/tmp`, deny paths masked. Reads are not limited to the read paths.
//! - The `bssh-jail` helper (`~bssh-agent/.local/bin/bssh-jail`, crate `crates/ai-jail`)
//!   does the Landlock job on hosts whose util-linux is too old.

use super::types::{ExecLimits, ExecMode, Executor, JailKind};
use crate::models::Connection;
use std::time::Duration;

/// Where the optional helper lives, relative to the agent user's `$HOME`.
pub const HELPER_REL_PATH: &str = ".local/bin/bssh-jail";

/// Exit code of the wrapper when no jail can be set up. The command did not run.
pub const EXIT_NO_JAIL: i32 = 126;

/// Read-only paths every program needs to start and run basic diagnostics.
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

/// POSIX single-quote `s`. Always quotes; an embedded `'` becomes `'\''`.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// What the jail should allow. `wrap` is `wrap_with` with no deny paths.
#[derive(Debug, Clone, Default)]
pub struct JailSpec {
    /// Policy `read_paths` (globs like `/var/log/**` are cut to their literal directory).
    pub read_paths: Vec<String>,
    /// Policy `deny_paths`. Only fully literal entries can be masked (bubblewrap).
    pub deny_paths: Vec<String>,
}

/// Wrap `command` so it runs read-only, without network, reads limited to `read_paths`.
/// Errors for `JailKind::None`.
pub fn wrap(kind: JailKind, read_paths: &[String], command: &str) -> anyhow::Result<String> {
    wrap_with(
        kind,
        &JailSpec {
            read_paths: read_paths.to_vec(),
            deny_paths: Vec::new(),
        },
        command,
    )
}

/// `wrap` with deny paths too.
pub fn wrap_with(kind: JailKind, spec: &JailSpec, command: &str) -> anyhow::Result<String> {
    if command.contains('\0') {
        anyhow::bail!("command contains a NUL byte");
    }
    let script = match kind {
        JailKind::Landlock => landlock_script(spec),
        JailKind::Bubblewrap => bubblewrap_script(spec),
        JailKind::None => anyhow::bail!("no jail on this host: nothing to wrap the command in"),
    };
    Ok(format!(
        "sh -c {} bssh-jail {}",
        shell_quote(&script),
        shell_quote(command)
    ))
}

/// The directory a read-path pattern lets the jail read: its literal leading components.
/// `/var/log/**` -> `/var/log`, `/etc/nginx/nginx.conf` -> itself, `**/x` or `rel/` -> none.
pub fn read_root(pattern: &str) -> Option<String> {
    if !pattern.starts_with('/') || pattern.contains('\0') {
        return None;
    }
    let mut root = String::new();
    for part in pattern.split('/').filter(|p| !p.is_empty()) {
        if part == ".." || part == "." {
            return None;
        }
        if part.contains(['*', '?', '[', ']', '{', '}']) {
            break;
        }
        root.push('/');
        root.push_str(part);
    }
    Some(if root.is_empty() { "/".into() } else { root })
}

/// A deny path that can be masked as is: absolute, no glob, no `..`.
fn literal_path(p: &str) -> Option<String> {
    let root = read_root(p)?;
    // `read_root` stops at the first glob: only the whole pattern counts as literal.
    let normalized: String = p
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| format!("/{s}"))
        .collect();
    (root == normalized && root != "/").then_some(root)
}

fn quoted_list(paths: &[String]) -> String {
    paths
        .iter()
        .map(|p| shell_quote(p))
        .collect::<Vec<_>>()
        .join(" ")
}

fn user_roots(spec: &JailSpec) -> Vec<String> {
    let mut roots: Vec<String> = spec
        .read_paths
        .iter()
        .filter_map(|p| read_root(p))
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

fn landlock_script(spec: &JailSpec) -> String {
    let roots = user_roots(spec);
    let mut all: Vec<String> = SYSTEM_READ_PATHS.iter().map(|s| s.to_string()).collect();
    all.extend(roots.iter().cloned());
    let all = quoted_list(&all);
    let user = quoted_list(&roots);
    let helper_args = if roots.is_empty() {
        String::new()
    } else {
        format!("for p in {user}; do set -- \"$@\" --read \"$p\"; done\n  ")
    };
    // `has X` asks setpriv whether this kernel and util-linux handle access class X. The
    // probe runs `true` *inside* the probed ruleset with no allow rules, so it may only
    // use classes `true` doesn't need (never plain `fs`: that would deny executing `true`).
    // Paths that don't exist are skipped: setpriv aborts on a rule it can't open.
    // Rights a ruleset ABI doesn't know are left out the same way (`fs:truncate`, ...).
    format!(
        r#"c=$1
has() {{ setpriv --landlock-access "$1" true >/dev/null 2>&1; }}
if has fs:make-dir; then
  set --
  for p in {all}; do
    [ -e "$p" ] && set -- "$@" --landlock-rule "path-beneath:execute,read-file,read-dir:$p"
  done
  d=read-file,write-file
  has fs:truncate && d=$d,truncate
  has fs:ioctl-dev && d=$d,ioctl-dev
  set -- "$@" --landlock-rule "path-beneath:$d:/dev/null"
  if has fs:resolve-unix; then
    for p in /run/dbus /run/systemd; do
      [ -e "$p" ] && set -- "$@" --landlock-rule "path-beneath:resolve-unix:$p"
    done
  fi
  if has net; then
    exec setpriv --landlock-access fs --landlock-access net "$@" -- sh -c "$c"
  fi
  exec setpriv --landlock-access fs "$@" -- sh -c "$c"
fi
j="${{HOME:-/nonexistent}}/{HELPER_REL_PATH}"
if [ -x "$j" ]; then
  set --
  {helper_args}exec "$j" "$@" -- "$c"
fi
echo 'bssh: no read-only jail on this host (setpriv without Landlock, no bssh-jail)' >&2
exit {EXIT_NO_JAIL}
"#
    )
}

fn bubblewrap_script(spec: &JailSpec) -> String {
    let mut deny: Vec<String> = spec
        .deny_paths
        .iter()
        .filter_map(|p| literal_path(p))
        .collect();
    deny.sort();
    deny.dedup();
    let masks = if deny.is_empty() {
        String::new()
    } else {
        format!(
            r#"for m in {}; do
  if [ -d "$m" ]; then set -- "$@" --tmpfs "$m"
  elif [ -e "$m" ]; then set -- "$@" --ro-bind /dev/null "$m"
  fi
done
"#,
            quoted_list(&deny)
        )
    };
    format!(
        r#"c=$1
set -- --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp --unshare-net --unshare-ipc --die-with-parent --new-session
{masks}exec bwrap "$@" -- sh -c "$c"
"#
    )
}

const OK_MARKER: &str = "bssh-jail-ok";
const READONLY_MARKER: &str = "bssh-jail-readonly";
const WRITABLE_MARKER: &str = "bssh-jail-writable";

/// Trial command: prints a marker, and says whether it could create a file.
/// `/var/tmp` is on the read-only side in both jails (bubblewrap only replaces `/tmp`).
fn trial_command() -> String {
    format!(
        "f=/var/tmp/.bssh-jail-probe-$$; echo {OK_MARKER}; \
         if (: > \"$f\") 2>/dev/null; then rm -f \"$f\"; echo {WRITABLE_MARKER}; \
         else echo {READONLY_MARKER}; fi"
    )
}

/// Find the best jail the host supports (runs a few probe commands via `exec`, Plain mode).
///
/// A jail counts only if the wrapped trial command ran and could not write. Transport
/// failures are errors; a jail that doesn't work is just not chosen.
pub async fn probe(conn: &Connection, exec: &dyn Executor) -> anyhow::Result<JailKind> {
    let limits = ExecLimits {
        timeout: Duration::from_secs(20),
        max_output_bytes: 4096,
    };
    for kind in [JailKind::Landlock, JailKind::Bubblewrap] {
        let wrapped = wrap(kind, &[], &trial_command())?;
        let res = exec.exec(conn, &wrapped, ExecMode::Plain, limits).await?;
        if res.exit_code == Some(0)
            && res.stdout.contains(OK_MARKER)
            && res.stdout.contains(READONLY_MARKER)
            && !res.stdout.contains(WRITABLE_MARKER)
        {
            return Ok(kind);
        }
    }
    Ok(JailKind::None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::ExecResult;
    use async_trait::async_trait;
    use std::path::Path;
    use std::process::Command;

    /// Split a command line the way a POSIX shell would, for strings made only of
    /// single-quoted and bare words (what `wrap` must produce). `None` if anything else.
    fn split_single_quoted(s: &str) -> Option<Vec<String>> {
        let mut words = Vec::new();
        let mut cur = String::new();
        let mut in_word = false;
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\'' => {
                    in_word = true;
                    loop {
                        match chars.next()? {
                            '\'' => break,
                            ch => cur.push(ch),
                        }
                    }
                }
                '\\' if chars.peek() == Some(&'\'') => {
                    // `\'` between quoted parts is a literal quote.
                    chars.next();
                    in_word = true;
                    cur.push('\'');
                }
                ' ' => {
                    if in_word {
                        words.push(std::mem::take(&mut cur));
                        in_word = false;
                    }
                }
                c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => {
                    in_word = true;
                    cur.push(c);
                }
                _ => return None,
            }
        }
        if in_word {
            words.push(cur);
        }
        Some(words)
    }

    fn adversarial() -> Vec<String> {
        [
            "",
            "id",
            "'",
            "''",
            "'; touch PWNED; '",
            "x'; touch PWNED #",
            "\"; touch PWNED; \"",
            "$(touch PWNED)",
            "`touch PWNED`",
            "a\nb\nc",
            "echo 'hi' && echo \"there\"",
            "\\'; touch PWNED; \\'",
            "'\\''",
            "a\\",
            "\\",
            "; touch PWNED",
            "| touch PWNED",
            "& touch PWNED",
            "> PWNED",
            "$HOME ${HOME} $0 $1 $@ $*",
            "~root *",
            "é ü 日本語 🦀",
            "tab\there",
            "!!",
            "-n",
            "--landlock-rule path-beneath:write-file:/",
            "'\"'\"'",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn quote_round_trips_through_a_shell_tokenizer() {
        for input in adversarial() {
            let q = shell_quote(&input);
            let words = split_single_quoted(&q).expect("only single-quoted text");
            assert_eq!(words, vec![input.clone()], "quote of {input:?}");
        }
    }

    #[test]
    fn wrapped_command_is_exactly_five_shell_words() {
        for kind in [JailKind::Landlock, JailKind::Bubblewrap] {
            for cmd in adversarial() {
                let spec = JailSpec {
                    read_paths: vec![
                        "/var/log/**".into(),
                        "/srv/it's/**".into(),
                        "/a b/**".into(),
                    ],
                    deny_paths: vec!["/etc/sha'dow".into(), "/root/.ssh".into()],
                };
                let wrapped = wrap_with(kind, &spec, &cmd).expect("wrap");
                let words = split_single_quoted(&wrapped)
                    .unwrap_or_else(|| panic!("not plain quoted words: {wrapped}"));
                assert_eq!(words.len(), 5, "{wrapped}");
                assert_eq!(&words[..2], ["sh", "-c"]);
                assert_eq!(words[3], "bssh-jail");
                assert_eq!(words[4], cmd, "the command must arrive untouched");
                assert!(!words[2].contains("PWNED"));
            }
        }
    }

    #[test]
    fn none_and_nul_are_errors() {
        assert!(wrap(JailKind::None, &[], "id").is_err());
        assert!(wrap(JailKind::Landlock, &[], "a\0b").is_err());
        assert!(wrap(JailKind::Bubblewrap, &[], "a\0b").is_err());
    }

    #[test]
    fn read_root_cuts_globs_and_refuses_relative_paths() {
        assert_eq!(read_root("/var/log/**").as_deref(), Some("/var/log"));
        assert_eq!(read_root("/etc/**").as_deref(), Some("/etc"));
        assert_eq!(read_root("/srv/app/*.conf").as_deref(), Some("/srv/app"));
        assert_eq!(
            read_root("/etc/nginx/nginx.conf").as_deref(),
            Some("/etc/nginx/nginx.conf")
        );
        assert_eq!(read_root("/var//log/").as_deref(), Some("/var/log"));
        assert_eq!(read_root("/**").as_deref(), Some("/"));
        assert_eq!(read_root("**/*.log"), None);
        assert_eq!(read_root("var/log"), None);
        assert_eq!(read_root("/var/../etc"), None);
        assert_eq!(read_root(""), None);
    }

    #[test]
    fn only_literal_deny_paths_are_masked() {
        assert_eq!(literal_path("/etc/shadow").as_deref(), Some("/etc/shadow"));
        assert_eq!(literal_path("/root/.ssh/").as_deref(), Some("/root/.ssh"));
        assert_eq!(literal_path("**/*.key"), None);
        assert_eq!(literal_path("/home/*/.ssh"), None);
        assert_eq!(literal_path("/"), None);
    }

    #[test]
    fn landlock_script_has_the_expected_shape() {
        let spec = JailSpec {
            read_paths: vec!["/var/log/**".into(), "/etc/**".into()],
            deny_paths: Vec::new(),
        };
        let script = landlock_script(&spec);
        assert!(script.contains("--landlock-access fs"));
        assert!(script.contains("--landlock-access net"));
        assert!(script.contains("path-beneath:execute,read-file,read-dir:$p"));
        assert!(script.contains("'/var/log'") && script.contains("'/etc'"));
        assert!(
            script.contains("/usr")
                && script.contains("/proc")
                && script.contains("/etc/ld.so.cache")
        );
        assert!(script.contains(HELPER_REL_PATH));
        assert!(script.contains(&format!("exit {EXIT_NO_JAIL}")));
        // Writes are never granted except to /dev/null.
        assert_eq!(script.matches("write-file").count(), 1, "{script}");
        assert!(script.contains(":/dev/null"));
    }

    #[test]
    fn bubblewrap_script_matches_the_documented_flags() {
        let script = bubblewrap_script(&JailSpec::default());
        assert!(script.contains(
            "--ro-bind / / --dev /dev --proc /proc --tmpfs /tmp --unshare-net --unshare-ipc \
             --die-with-parent --new-session"
        ));
        assert!(script.contains("exec bwrap \"$@\" -- sh -c \"$c\""));
        assert!(!script.contains("for m in"));
        let masked = bubblewrap_script(&JailSpec {
            read_paths: Vec::new(),
            deny_paths: vec!["/etc/shadow".into(), "**/*.key".into()],
        });
        assert!(masked.contains("'/etc/shadow'"));
        assert!(!masked.contains("*.key"));
    }

    // ---- run the real wrapper strings under a real `sh`, with fake jail binaries ----

    /// `bwrap` and `setpriv` stand-ins on PATH: they record their arguments, drop the
    /// jail options and exec what follows, so the shell layer runs for real.
    struct FakeBin {
        dir: tempfile::TempDir,
    }

    impl FakeBin {
        fn new(landlock_net: bool) -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let log = dir.path().join("calls.log");
            let bwrap = format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\" >> {log}.bwrap; done\n\
                 while [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
                log = log.display()
            );
            // `--landlock-access net` fails like an old kernel when !landlock_net.
            let setpriv = format!(
                "#!/bin/sh\necho \"$*\" >> {log}.setpriv\n\
                 if [ \"$#\" -eq 3 ] && [ \"$2\" = fs ]; then exit 126; fi\n\
                 if [ \"$2\" = net ] && [ \"{net}\" = no ]; then exit 1; fi\n\
                 while [ \"$1\" = --landlock-access ] || [ \"$1\" = --landlock-rule ]; do shift 2; done\n\
                 [ \"$1\" = -- ] && shift\nexec \"$@\"\n",
                log = log.display(),
                net = if landlock_net { "yes" } else { "no" }
            );
            for (name, body) in [("bwrap", bwrap), ("setpriv", setpriv)] {
                let p = dir.path().join(name);
                std::fs::write(&p, body).expect("write fake");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
                        .expect("chmod");
                }
            }
            Self { dir }
        }

        fn run(&self, line: &str, cwd: &Path) -> (String, String, Option<i32>) {
            let path = format!(
                "{}:{}",
                self.dir.path().display(),
                std::env::var("PATH").unwrap_or_default()
            );
            let out = Command::new("sh")
                .arg("-c")
                .arg(line)
                .current_dir(cwd)
                .env("PATH", path)
                .env("HOME", cwd)
                .output()
                .expect("run sh");
            (
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
                out.status.code(),
            )
        }

        fn logged(&self, which: &str) -> String {
            std::fs::read_to_string(self.dir.path().join(format!("calls.log.{which}")))
                .unwrap_or_default()
        }
    }

    #[test]
    fn adversarial_commands_arrive_intact_and_never_run_outside() {
        for kind in [JailKind::Landlock, JailKind::Bubblewrap] {
            let fake = FakeBin::new(true);
            let work = tempfile::tempdir().unwrap();
            for input in adversarial() {
                // A command that prints `input`: round-trips through both quoting layers.
                let cmd = format!("printf %s {}", shell_quote(&input));
                let wrapped = wrap(kind, &["/var/log/**".into()], &cmd).unwrap();
                let (out, err, code) = fake.run(&wrapped, work.path());
                assert_eq!(code, Some(0), "{kind:?} {input:?}: {err}");
                assert_eq!(out, input, "{kind:?}");
            }
            assert!(
                !work.path().join("PWNED").exists(),
                "{kind:?}: something ran unquoted"
            );
        }
    }

    #[test]
    fn hostile_command_text_cannot_break_out_of_the_wrapper() {
        for kind in [JailKind::Landlock, JailKind::Bubblewrap] {
            let fake = FakeBin::new(true);
            let work = tempfile::tempdir().unwrap();
            for cmd in [
                "x'; touch PWNED; echo '",
                "'; touch PWNED; '",
                "\"; touch PWNED; \"",
                "x\ntouch PWNED_NL",
            ] {
                // These are just (odd) commands for the inner `sh -c`. What matters is that
                // the *outer* shell never sees the text as code: run it with the inner shell
                // neutralised, by counting how the fake jail was invoked.
                let wrapped = wrap(kind, &[], &format!("echo {}", shell_quote(cmd))).unwrap();
                let (out, _, code) = fake.run(&wrapped, work.path());
                assert_eq!(code, Some(0));
                assert_eq!(out.trim_end_matches('\n'), cmd);
            }
            assert!(!work.path().join("PWNED").exists());
            assert!(!work.path().join("PWNED_NL").exists());
        }
    }

    #[test]
    fn hostile_paths_cannot_break_out_either() {
        let fake = FakeBin::new(true);
        let work = tempfile::tempdir().unwrap();
        let evil = "/x'; touch PWNED; echo '".to_string();
        for kind in [JailKind::Landlock, JailKind::Bubblewrap] {
            let spec = JailSpec {
                read_paths: vec![format!("{evil}/**")],
                deny_paths: vec![evil.clone()],
            };
            let wrapped = wrap_with(kind, &spec, "echo fine").unwrap();
            let (out, err, code) = fake.run(&wrapped, work.path());
            assert_eq!(code, Some(0), "{err}");
            assert_eq!(out, "fine\n");
        }
        assert!(!work.path().join("PWNED").exists());
    }

    #[test]
    fn bubblewrap_gets_the_documented_arguments() {
        let fake = FakeBin::new(true);
        let work = tempfile::tempdir().unwrap();
        let deny_dir = work.path().join("secret");
        std::fs::create_dir(&deny_dir).unwrap();
        let deny_file = work.path().join("key.pem");
        std::fs::write(&deny_file, "x").unwrap();
        let spec = JailSpec {
            read_paths: Vec::new(),
            deny_paths: vec![
                deny_dir.display().to_string(),
                deny_file.display().to_string(),
                work.path().join("missing").display().to_string(),
            ],
        };
        let wrapped = wrap_with(JailKind::Bubblewrap, &spec, "echo hi").unwrap();
        let (out, err, code) = fake.run(&wrapped, work.path());
        assert_eq!((code, out.as_str()), (Some(0), "hi\n"), "{err}");
        let args: Vec<String> = fake.logged("bwrap").lines().map(String::from).collect();
        let fixed = [
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/tmp",
            "--unshare-net",
            "--unshare-ipc",
            "--die-with-parent",
            "--new-session",
        ];
        assert_eq!(&args[..fixed.len()], fixed);
        let d = deny_dir.display().to_string();
        let f = deny_file.display().to_string();
        let tail = &args[fixed.len()..];
        assert!(
            tail.windows(2).any(|w| w == ["--tmpfs", d.as_str()]),
            "{tail:?}"
        );
        assert!(
            tail.windows(3)
                .any(|w| w == ["--ro-bind", "/dev/null", f.as_str()]),
            "{tail:?}"
        );
        assert!(!tail.iter().any(|a| a.ends_with("missing")), "{tail:?}");
    }

    #[test]
    fn landlock_adds_net_only_when_supported() {
        let work = tempfile::tempdir().unwrap();
        for net in [true, false] {
            let fake = FakeBin::new(net);
            let wrapped = wrap(JailKind::Landlock, &["/var/log/**".into()], "echo hi").unwrap();
            let (out, err, code) = fake.run(&wrapped, work.path());
            assert_eq!((code, out.as_str()), (Some(0), "hi\n"), "net={net}: {err}");
            let calls = fake.logged("setpriv");
            let real: Vec<&str> = calls
                .lines()
                .filter(|l| l.contains("landlock-rule"))
                .collect();
            assert_eq!(real.len(), 1, "{calls}");
            assert_eq!(real[0].contains("--landlock-access net"), net, "{real:?}");
            assert!(real[0].contains("--landlock-access fs"));
            assert!(
                real[0].contains("path-beneath:execute,read-file,read-dir:/usr"),
                "{real:?}"
            );
            assert!(real[0].contains(":/var/log") || !Path::new("/var/log").exists());
            assert!(real[0].ends_with("-- sh -c echo hi"), "{real:?}");
        }
    }

    #[test]
    fn landlock_without_setpriv_support_or_helper_refuses_to_run() {
        // `setpriv` that doesn't know Landlock (util-linux < 2.40) and no helper.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("setpriv");
        std::fs::write(&p, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let work = tempfile::tempdir().unwrap();
        let wrapped = wrap(JailKind::Landlock, &[], "touch SHOULD_NOT_RUN").unwrap();
        let out = Command::new("sh")
            .arg("-c")
            .arg(&wrapped)
            .current_dir(work.path())
            .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
            .env("HOME", work.path())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(EXIT_NO_JAIL));
        assert!(String::from_utf8_lossy(&out.stderr).contains("no read-only jail"));
        assert!(!work.path().join("SHOULD_NOT_RUN").exists());
    }

    #[test]
    fn landlock_falls_back_to_the_helper_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("setpriv");
        std::fs::write(&p, "#!/bin/sh\nexit 1\n").unwrap();
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let helper = bin.join("bssh-jail");
        std::fs::write(
            &helper,
            "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > \"$HOME/helper.args\"\n\
             while [ \"$1\" != -- ]; do shift; done; shift\nexec sh -c \"$1\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in [&p, &helper] {
                std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let wrapped = wrap(
            JailKind::Landlock,
            &["/var/log/**".into()],
            "echo via-helper",
        )
        .unwrap();
        let out = Command::new("sh")
            .arg("-c")
            .arg(&wrapped)
            .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
            .env("HOME", home.path())
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "via-helper\n");
        let args = std::fs::read_to_string(home.path().join("helper.args")).unwrap();
        assert_eq!(args, "--read\n/var/log\n--\necho via-helper\n");
    }

    // ---- probe ----

    /// An executor that answers jail trial commands like a host with the given jails.
    struct FakeHost {
        landlock: Trial,
        bwrap: Trial,
    }

    #[derive(Clone, Copy)]
    enum Trial {
        Missing,
        Works,
        NotEnforcing,
    }

    #[async_trait]
    impl Executor for FakeHost {
        async fn exec(
            &self,
            _conn: &Connection,
            command: &str,
            mode: ExecMode,
            _limits: ExecLimits,
        ) -> anyhow::Result<ExecResult> {
            assert_eq!(mode, ExecMode::Plain, "probes run as the connection user");
            let trial = if command.contains("exec bwrap") {
                self.bwrap
            } else if command.contains("setpriv") {
                self.landlock
            } else {
                panic!("unexpected probe command: {command}");
            };
            let (stdout, code) = match trial {
                Trial::Missing => (String::new(), 126),
                Trial::Works => (format!("{OK_MARKER}\n{READONLY_MARKER}\n"), 0),
                Trial::NotEnforcing => (format!("{OK_MARKER}\n{WRITABLE_MARKER}\n"), 0),
            };
            Ok(ExecResult {
                stdout,
                stderr: String::new(),
                exit_code: Some(code),
                truncated: false,
                timed_out: false,
                duration_ms: 1,
            })
        }
    }

    fn conn() -> Connection {
        Connection::new(
            "web".into(),
            "web.test".into(),
            "ops".into(),
            22,
            None,
            None,
            false,
            None,
        )
    }

    async fn probed(landlock: Trial, bwrap: Trial) -> JailKind {
        probe(&conn(), &FakeHost { landlock, bwrap })
            .await
            .expect("probe")
    }

    #[tokio::test]
    async fn probe_prefers_landlock_then_bubblewrap_then_none() {
        use Trial::*;
        assert_eq!(probed(Works, Works).await, JailKind::Landlock);
        assert_eq!(probed(Works, Missing).await, JailKind::Landlock);
        assert_eq!(probed(Missing, Works).await, JailKind::Bubblewrap);
        assert_eq!(probed(Missing, Missing).await, JailKind::None);
    }

    #[tokio::test]
    async fn probe_rejects_a_jail_that_does_not_enforce() {
        use Trial::*;
        assert_eq!(probed(NotEnforcing, Works).await, JailKind::Bubblewrap);
        assert_eq!(probed(NotEnforcing, NotEnforcing).await, JailKind::None);
    }

    #[tokio::test]
    async fn probe_surfaces_transport_errors() {
        struct Down;
        #[async_trait]
        impl Executor for Down {
            async fn exec(
                &self,
                _: &Connection,
                _: &str,
                _: ExecMode,
                _: ExecLimits,
            ) -> anyhow::Result<ExecResult> {
                anyhow::bail!("connection refused")
            }
        }
        let err = probe(&conn(), &Down).await.unwrap_err();
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn trial_command_is_harmless_where_the_jail_holds() {
        // Outside any jail the trial really creates and removes a file, then says so.
        let out = Command::new("sh")
            .arg("-c")
            .arg(trial_command())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains(OK_MARKER));
        assert!(text.contains(WRITABLE_MARKER) ^ text.contains(READONLY_MARKER));
    }
}
