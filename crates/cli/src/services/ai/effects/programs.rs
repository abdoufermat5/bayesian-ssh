//! Effects of the file and text programs bssh recognises by name.

use crate::services::ai::types::{RiskTag, SimpleCommand};

use super::analyze::{find_roots_of, has_flag, operands, push, value_of, write_to};
use super::model::{Effects, Verb};
use super::system::{container_effect, kill_effect, package_effect, systemctl_effect, unit_action};

/// Handles programs bssh knows; false means "not recognised".
pub(super) fn known(c: &SimpleCommand, program: &str, out: &mut Effects) -> bool {
    let args = &c.args[..];
    match program {
        "mkdir" => {
            let t = operands(args, &["-m", "--mode", "-Z", "--context"]);
            push(out, Verb::CreatesDirs, t, None);
        }
        "install" => {
            let t = operands(
                args,
                &[
                    "-m",
                    "--mode",
                    "-o",
                    "--owner",
                    "-g",
                    "--group",
                    "-S",
                    "--suffix",
                    "-Z",
                    "--context",
                    "-t",
                    "--target-directory",
                ],
            );
            if has_flag(args, 'd', "--directory") {
                push(out, Verb::CreatesDirs, t, None);
            } else if let Some(dest) = value_of(args, "-t", "--target-directory") {
                push(out, Verb::Copies, t, Some(dest));
            } else if t.len() >= 2 {
                let (dest, src) = t.split_last().expect("len >= 2");
                push(out, Verb::Copies, src.to_vec(), Some(dest.clone()));
            } else {
                return false;
            }
        }
        "touch" => {
            let t = operands(args, &["-d", "--date", "-t", "-r", "--reference"]);
            push(out, Verb::Creates, t, None);
        }
        "ln" => {
            let t = operands(args, &["-t", "--target-directory", "-S", "--suffix"]);
            if t.is_empty() {
                return false;
            }
            // `ln -s TARGET LINK`; with one operand the link lands in `.`.
            let (link, source) = if t.len() == 1 {
                (t[0].clone(), None)
            } else {
                let (link, rest) = t.split_last().expect("not empty");
                (link.clone(), Some(rest.join(" ")))
            };
            push(out, Verb::Creates, vec![link], source);
        }
        "mv" | "cp" => {
            let verb = if program == "mv" {
                Verb::Moves
            } else {
                Verb::Copies
            };
            let mut t = operands(
                args,
                &[
                    "-t",
                    "--target-directory",
                    "-S",
                    "--suffix",
                    "-Z",
                    "--context",
                ],
            );
            if let Some(dest) = value_of(args, "-t", "--target-directory") {
                push(out, verb, t, Some(dest));
            } else if t.len() >= 2 {
                let dest = t.pop();
                push(out, verb, t, dest);
            } else {
                return false;
            }
        }
        "rsync" => {
            let mut t = operands(
                args,
                &[
                    "-e",
                    "--rsh",
                    "--exclude",
                    "--include",
                    "--exclude-from",
                    "--include-from",
                    "-f",
                    "--filter",
                    "--port",
                    "--bwlimit",
                    "--chmod",
                    "--chown",
                    "--rsync-path",
                    "--files-from",
                    "--log-file",
                    "-T",
                    "--temp-dir",
                    "--backup-dir",
                    "--timeout",
                    "--max-size",
                    "--min-size",
                    "--link-dest",
                    "--compare-dest",
                ],
            );
            if t.len() < 2 {
                return false;
            }
            let dest = t.pop();
            if args.iter().any(|a| a.starts_with("--remove-source-files")) {
                push(out, Verb::Deletes, t.clone(), None);
            }
            if args
                .iter()
                .any(|a| a.starts_with("--delete") || a == "--del" || a == "--remove-sent-files")
            {
                let at = dest.clone().unwrap_or_default();
                push(
                    out,
                    Verb::Deletes,
                    vec![format!("files in {at} missing from the source")],
                    None,
                );
            }
            push(out, Verb::Copies, t, dest);
        }
        "rm" | "rmdir" | "unlink" => {
            let t = operands(args, &[]);
            push(out, Verb::Deletes, t, None);
        }
        "shred" => {
            let t = operands(
                args,
                &["-n", "--iterations", "-s", "--size", "--random-source"],
            );
            let verb = if has_flag(args, 'u', "--remove") {
                Verb::Deletes
            } else {
                Verb::Edits
            };
            push(out, verb, t, None);
        }
        "truncate" => {
            let t = operands(
                args,
                &["-s", "--size", "-r", "--reference", "-o", "--io-blocks"],
            );
            push(out, Verb::Edits, t, None);
        }
        "find" => return find_effect(c, out),
        "sed" => return sed_effect(args, out),
        "tee" => {
            let t = operands(args, &[]);
            if t.is_empty() {
                return false;
            }
            write_to(out, &t);
        }
        "dd" => {
            let Some(of) = args.iter().find_map(|a| a.strip_prefix("of=")) else {
                return false;
            };
            write_to(out, &[of.to_string()]);
        }
        "chmod" | "chown" | "chgrp" => {
            // `chmod -x f`: a leading `-x` is a mode, not an option.
            let known_opts = |a: &str| {
                a.starts_with("--")
                    || (a.len() > 1
                        && a.starts_with('-')
                        && a[1..].chars().all(|ch| "RvcfhHLP".contains(ch)))
            };
            let rest: Vec<String> = args.iter().filter(|a| !known_opts(a)).cloned().collect();
            let by_ref = args.iter().any(|a| a.starts_with("--reference"));
            let skip = usize::from(!by_ref);
            push(
                out,
                Verb::Changes,
                rest.into_iter().skip(skip).collect(),
                None,
            );
        }
        "systemctl" => return systemctl_effect(args, out),
        "service" => {
            let ops = operands(args, &[]);
            let (Some(unit), Some(action)) = (ops.first(), ops.get(1)) else {
                return false;
            };
            return unit_action(out, action, std::slice::from_ref(unit));
        }
        p if c.program.contains("/init.d/") => {
            let ops = operands(args, &[]);
            let Some(action) = ops.first() else {
                return false;
            };
            return unit_action(out, action, &[p.to_string()]);
        }
        "nginx" => {
            let signal = args
                .iter()
                .position(|a| a == "-s")
                .and_then(|i| args.get(i + 1))
                .map(String::as_str);
            match signal {
                Some("reload" | "reopen") => push(out, Verb::Reloads, vec!["nginx".into()], None),
                Some("stop" | "quit") => push(out, Verb::Stops, vec!["nginx".into()], None),
                // `nginx -t` only tests the configuration.
                None if args.iter().any(|a| a == "-t" || a == "-T") => {}
                _ => return false,
            }
        }
        "reboot" => push(out, Verb::Restarts, vec!["the host".into()], None),
        "poweroff" | "halt" => push(out, Verb::Stops, vec!["the host".into()], None),
        "shutdown" => {
            let verb = if has_flag(args, 'r', "--reboot") {
                Verb::Restarts
            } else {
                Verb::Stops
            };
            push(out, verb, vec!["the host".into()], None);
        }
        "kill" | "pkill" | "killall" => return kill_effect(program, args, out),
        "useradd" | "adduser" | "usermod" | "userdel" | "deluser" | "groupadd" | "addgroup"
        | "groupmod" | "groupdel" | "delgroup" | "passwd" | "chpasswd" => {
            let kind = if program.contains("group") {
                "group"
            } else {
                "user"
            };
            let ops = operands(args, &[]);
            let name = ops.last().cloned().unwrap_or_default();
            let target = if name.is_empty() {
                format!("{kind}s")
            } else {
                format!("{kind} {name}")
            };
            if program.contains("del") {
                out.deletes = true;
            }
            push(out, Verb::Changes, vec![target], None);
        }
        "crontab" => {
            if has_flag(args, 'r', "--remove") && !args.iter().any(|a| a == "-l") {
                push(out, Verb::Deletes, vec!["the crontab".into()], None);
            } else if args.iter().any(|a| a == "-e") {
                push(out, Verb::Edits, vec!["the crontab".into()], None);
            } else if args.iter().any(|a| a == "-l") {
                return false;
            } else if !operands(args, &["-u"]).is_empty() {
                push(out, Verb::Edits, vec!["the crontab".into()], None);
            } else {
                return false;
            }
        }
        "docker" | "podman" => return container_effect(args, out),
        "git" => return git_effect(args, out),
        "mount" => {
            let t = operands(
                args,
                &[
                    "-t",
                    "--types",
                    "-o",
                    "--options",
                    "-L",
                    "-U",
                    "--source",
                    "--target",
                ],
            );
            if has_flag(args, 'a', "--all") {
                push(
                    out,
                    Verb::Mounts,
                    vec!["all filesystems in fstab".into()],
                    None,
                );
            } else if let Some(point) = t.last() {
                push(out, Verb::Mounts, vec![point.clone()], None);
            } else {
                return false;
            }
        }
        "umount" => {
            let t = operands(args, &["-t", "--types", "-O"]);
            if has_flag(args, 'a', "--all") {
                push(out, Verb::Unmounts, vec!["all filesystems".into()], None);
            } else if t.is_empty() {
                return false;
            } else {
                push(out, Verb::Unmounts, t, None);
            }
        }
        _ => return package_effect(program, args, out),
    }
    true
}

fn find_effect(c: &SimpleCommand, out: &mut Effects) -> bool {
    let args = &c.args;
    let roots = find_roots_of(args);
    if args.iter().any(|a| a == "-delete") {
        push(
            out,
            Verb::Deletes,
            vec![format!("files found under {roots}")],
            None,
        );
        return true;
    }
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if matches!(a.as_str(), "-fprint" | "-fprint0" | "-fprintf" | "-fls") {
            files.extend(it.next().cloned());
        }
    }
    if !files.is_empty() {
        write_to(out, &files);
    }
    let runs_others = args
        .iter()
        .any(|a| matches!(a.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"));
    // The commands behind -exec are listed on their own; plain find reads.
    !files.is_empty() || runs_others || c.tags.iter().all(|t| *t == RiskTag::Read)
}

fn sed_effect(args: &[String], out: &mut Effects) -> bool {
    let mut in_place = false;
    let mut script_given = false;
    for a in args {
        if a.starts_with("--in-place") {
            in_place = true;
        } else if a.starts_with("--expression") || a.starts_with("--file") {
            script_given = true;
        } else if a.starts_with('-') && !a.starts_with("--") {
            for ch in a.chars().skip(1) {
                match ch {
                    'i' => {
                        in_place = true;
                        break;
                    }
                    'e' | 'f' => {
                        script_given = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    if !in_place {
        return false;
    }
    let mut files = operands(
        args,
        &["-e", "-f", "-l", "--expression", "--file", "--line-length"],
    );
    if !script_given && !files.is_empty() {
        files.remove(0);
    }
    if files.is_empty() {
        return false;
    }
    write_to(out, &files);
    true
}

pub(super) fn nonempty(v: Vec<String>, fallback: &str) -> Vec<String> {
    if v.is_empty() {
        vec![fallback.to_string()]
    } else {
        v
    }
}

fn git_effect(args: &[String], out: &mut Effects) -> bool {
    let ops = operands(args, &["-C", "-c", "--git-dir", "--work-tree"]);
    match ops.first().map(String::as_str) {
        Some("clean") => {
            push(out, Verb::Deletes, vec!["untracked files".into()], None);
            true
        }
        Some("reset") if args.iter().any(|a| a == "--hard") => {
            push(out, Verb::Deletes, vec!["uncommitted changes".into()], None);
            true
        }
        _ => false,
    }
}
