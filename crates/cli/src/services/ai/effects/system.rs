//! Effects of service, process, container and package-manager commands.

use super::analyze::{operands, push, value_of};
use super::model::{Effects, Verb};
use super::programs::nonempty;

pub(super) fn unit_action(out: &mut Effects, action: &str, units: &[String]) -> bool {
    let verb = match action {
        "restart" | "try-restart" | "condrestart" | "force-restart" => Verb::Restarts,
        "reload" | "reload-or-restart" | "try-reload-or-restart" | "force-reload" => Verb::Reloads,
        "start" => Verb::Starts,
        "stop" | "kill" => Verb::Stops,
        "enable" | "unmask" => Verb::Enables,
        "disable" | "mask" => Verb::Disables,
        _ => return false,
    };
    push(out, verb, units.to_vec(), None);
    true
}

pub(super) fn systemctl_effect(args: &[String], out: &mut Effects) -> bool {
    let ops = operands(
        args,
        &[
            "-t",
            "--type",
            "-p",
            "--property",
            "-s",
            "--signal",
            "-H",
            "--host",
            "-M",
            "--machine",
            "-n",
            "--lines",
            "-o",
            "--output",
            "--kill-whom",
            "--root",
        ],
    );
    let Some((action, units)) = ops.split_first() else {
        return false;
    };
    match action.as_str() {
        "daemon-reload" | "daemon-reexec" => {
            push(out, Verb::Reloads, vec!["systemd".into()], None);
            true
        }
        "reboot" | "kexec" => {
            push(out, Verb::Restarts, vec!["the host".into()], None);
            true
        }
        "poweroff" | "halt" | "suspend" | "hibernate" => {
            push(out, Verb::Stops, vec!["the host".into()], None);
            true
        }
        a => {
            if !unit_action(out, a, units) {
                return false;
            }
            if args.iter().any(|x| x == "--now") {
                match a {
                    "enable" => push(out, Verb::Starts, units.to_vec(), None),
                    "disable" | "mask" => push(out, Verb::Stops, units.to_vec(), None),
                    _ => {}
                }
            }
            true
        }
    }
}

pub(super) fn kill_effect(program: &str, args: &[String], out: &mut Effects) -> bool {
    if args.iter().any(|a| a == "-l" || a == "-L" || a == "--list") {
        return false;
    }
    let signal = if program == "kill" {
        args.iter()
            .find(|a| a.starts_with('-') && a.len() > 1 && !a.starts_with("--"))
            .map(|a| a[1..].to_ascii_uppercase())
            .or_else(|| value_of(args, "-s", "--signal"))
    } else {
        args.iter()
            .find(|a| {
                a.starts_with('-')
                    && a.len() > 1
                    && !a.starts_with("--")
                    && (a[1..].chars().all(|ch| ch.is_ascii_digit())
                        || a[1..].chars().all(|ch| ch.is_ascii_uppercase()))
            })
            .map(|a| a[1..].to_string())
    };
    let signal = signal
        .map(|s| s.trim_start_matches("SIG").to_string())
        .unwrap_or_default();
    if signal == "0" {
        return false;
    }
    let value_opts: &[&str] = match program {
        "kill" => &["-s", "-n", "--signal"],
        "pkill" => &["-u", "-U", "-g", "-G", "-P", "-s", "-t", "-c", "--signal"],
        _ => &["-u", "-s", "-g", "--user", "--signal", "-y", "-o"],
    };
    let mut names: Vec<String> = Vec::new();
    // `kill -9 123`: numeric and signal-name options are not operands.
    let cleaned: Vec<String> = args
        .iter()
        .filter(|a| {
            !(a.starts_with('-')
                && a.len() > 1
                && (a[1..].chars().all(|ch| ch.is_ascii_digit())
                    || (program == "kill"
                        && a[1..]
                            .chars()
                            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()))))
        })
        .cloned()
        .collect();
    for op in operands(&cleaned, value_opts) {
        names.push(if program == "kill" {
            format!("pid {op}")
        } else {
            op
        });
    }
    if names.is_empty() {
        names.push("matching processes".into());
    }
    let verb = if signal == "HUP" || signal == "1" {
        Verb::Reloads
    } else {
        Verb::Stops
    };
    push(out, verb, names, None);
    true
}

pub(super) fn container_effect(args: &[String], out: &mut Effects) -> bool {
    let ops = operands(
        args,
        &[
            "-H",
            "--host",
            "-c",
            "--context",
            "--config",
            "-l",
            "--log-level",
        ],
    );
    let mut it = ops.iter().map(String::as_str);
    let Some(mut sub) = it.next() else {
        return false;
    };
    let mut kind = "";
    if matches!(
        sub,
        "container" | "image" | "volume" | "network" | "system" | "builder"
    ) {
        kind = sub;
        let Some(s) = it.next() else {
            return false;
        };
        sub = s;
    }
    let names: Vec<String> = it.map(str::to_string).collect();
    let label = |default: &str, names: Vec<String>| -> Vec<String> {
        let k = if kind.is_empty() || kind == "system" {
            default
        } else {
            kind
        };
        names.into_iter().map(|n| format!("{k} {n}")).collect()
    };
    match sub {
        "rm" => {
            let t = label("container", names);
            push(out, Verb::Deletes, nonempty(t, "containers"), None);
        }
        "rmi" => {
            let t = label("image", names);
            push(out, Verb::Deletes, nonempty(t, "images"), None);
        }
        "prune" => {
            let what = match kind {
                "" | "container" => "unused containers",
                "system" => "unused containers, images and networks",
                "image" => "unused images",
                "volume" => "unused volumes",
                "network" => "unused networks",
                _ => "unused data",
            };
            push(out, Verb::Deletes, vec![what.into()], None);
        }
        "stop" | "kill" => push(
            out,
            Verb::Stops,
            nonempty(label("container", names), "containers"),
            None,
        ),
        "restart" => push(
            out,
            Verb::Restarts,
            nonempty(label("container", names), "containers"),
            None,
        ),
        "start" => push(
            out,
            Verb::Starts,
            nonempty(label("container", names), "containers"),
            None,
        ),
        _ => return false,
    }
    true
}

pub(super) fn package_effect(program: &str, args: &[String], out: &mut Effects) -> bool {
    const APT: (&[&str], &[&str]) = (
        &[
            "install",
            "reinstall",
            "upgrade",
            "dist-upgrade",
            "full-upgrade",
        ],
        &["remove", "purge", "autoremove", "autopurge"],
    );
    const DNF: (&[&str], &[&str]) = (
        &[
            "install",
            "reinstall",
            "localinstall",
            "upgrade",
            "update",
            "downgrade",
            "in",
            "up",
        ],
        &["remove", "erase", "autoremove", "rm"],
    );
    const NPM: (&[&str], &[&str]) = (
        &["install", "i", "add", "ci", "update", "up", "upgrade"],
        &["uninstall", "remove", "rm", "un", "unlink", "r"],
    );
    let value_opts: &[&str] = &[
        "-o",
        "-t",
        "-c",
        "-p",
        "-X",
        "-r",
        "-i",
        "-e",
        "-f",
        "--root",
        "--installroot",
        "--prefix",
        "--registry",
        "--target",
        "--index-url",
        "--extra-index-url",
        "--requirement",
        "--constraint",
        "--editable",
        "--find-links",
        "--config",
    ];
    let mut args = args;
    let mut program = program;
    // `python -m pip install x`.
    if program.starts_with("python") {
        match args {
            [m, p, rest @ ..] if m.as_str() == "-m" && matches!(p.as_str(), "pip" | "pip3") => {
                args = rest;
                program = "pip";
            }
            _ => return false,
        }
    }
    let (installs, removes): (&[&str], &[&str]) = match program {
        "apt" | "apt-get" | "aptitude" => APT,
        "dnf" | "yum" | "microdnf" | "zypper" => DNF,
        "apk" => (&["add", "upgrade"], &["del"]),
        "pip" | "pip3" => (&["install"], &["uninstall"]),
        "npm" | "pnpm" | "yarn" => NPM,
        "gem" | "cargo" => (&["install"], &["uninstall"]),
        "snap" => (&["install", "refresh"], &["remove"]),
        "brew" => (
            &["install", "upgrade", "reinstall"],
            &["uninstall", "remove", "rm"],
        ),
        "dpkg" => {
            let t = operands(args, &["-t", "--root", "--admindir"]);
            if args.iter().any(|a| a == "-i" || a == "--install") {
                push(out, Verb::Installs, nonempty(t, "packages"), None);
            } else if args
                .iter()
                .any(|a| matches!(a.as_str(), "-r" | "-P" | "--remove" | "--purge"))
            {
                push(out, Verb::Removes, nonempty(t, "packages"), None);
            } else {
                return false;
            }
            return true;
        }
        _ => return false,
    };
    let ops = operands(args, value_opts);
    let Some((sub, pkgs)) = ops.split_first() else {
        return false;
    };
    let verb = if installs.contains(&sub.as_str()) {
        Verb::Installs
    } else if removes.contains(&sub.as_str()) {
        Verb::Removes
    } else {
        return false;
    };
    push(out, verb, pkgs.to_vec(), None);
    true
}
