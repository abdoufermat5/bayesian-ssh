//! `sudo` and `su`.

use crate::services::ai::analyzer::rules::{Arg, Class};
use crate::services::ai::types::RiskTag;

use super::opts::{is_assignment, parse_opts, Spec};
use super::state::{Ctx, Walker};

impl Walker {
    pub(super) fn sudo(&mut self, text: &str, prog: &str, rest: &[Arg], ctx: &Ctx) {
        let spec = match prog {
            "doas" => Spec {
                flags: "nsL",
                with_arg: "uC",
                long: &[],
                numeric: false,
            },
            "pkexec" => Spec {
                flags: "",
                with_arg: "",
                long: &[
                    ("--user", true),
                    ("--disable-internal-agent", false),
                    ("--keep-cwd", false),
                    ("--version", false),
                    ("--help", false),
                ],
                numeric: false,
            },
            _ => Spec {
                flags: "AbEeHhiKklnPSsVvB",
                with_arg: "CDgpRrTtUu",
                long: &[
                    ("--askpass", false),
                    ("--background", false),
                    ("--preserve-env", false),
                    ("--edit", false),
                    ("--set-home", false),
                    ("--help", false),
                    ("--login", false),
                    ("--remove-timestamp", false),
                    ("--reset-timestamp", false),
                    ("--list", false),
                    ("--non-interactive", false),
                    ("--preserve-groups", false),
                    ("--stdin", false),
                    ("--shell", false),
                    ("--validate", false),
                    ("--version", false),
                    ("--user", true),
                    ("--group", true),
                    ("--host", true),
                    ("--prompt", true),
                    ("--role", true),
                    ("--type", true),
                    ("--other-user", true),
                    ("--chdir", true),
                    ("--close-from", true),
                    ("--command-timeout", true),
                    ("--chroot", true),
                ],
                numeric: false,
            },
        };
        let opts = match parse_opts(rest, &spec) {
            Ok(o) => o,
            Err(e) => return self.leaf_unknown(text, prog, rest, ctx, format!("{prog}: {e}")),
        };
        let mut inner_ctx = ctx.wrapped(prog);
        inner_ctx.extra.insert(RiskTag::Privilege);
        let mut idx = opts.rest;
        while idx < rest.len() && is_assignment(&rest[idx].text) {
            let name = rest[idx].text.split('=').next().unwrap_or("").to_string();
            self.check_var(name.trim_end_matches('+'));
            idx += 1;
        }
        let inner = &rest[idx..];
        if opts.has(&["-e", "--edit"]) {
            let class = Class::new(
                format!("{prog} (edit)"),
                &[RiskTag::Privilege, RiskTag::Write],
            );
            return self.emit(text, rest, ctx, class);
        }
        if inner.is_empty()
            && opts.has(&[
                "-l",
                "--list",
                "-v",
                "--validate",
                "-k",
                "-K",
                "--remove-timestamp",
                "--reset-timestamp",
                "-V",
                "--version",
                "-h",
                "--help",
            ])
        {
            let class = Class::new(format!("{prog} (no command)"), &[RiskTag::Privilege]);
            return self.emit(text, rest, ctx, class);
        }
        if opts.has(&["-l", "--list"]) {
            // `sudo -l cmd` only checks permission.
            let class = Class::new(format!("{prog} -l"), &[RiskTag::Privilege]);
            return self.emit(text, rest, ctx, class);
        }
        if opts.has(&["-s", "--shell", "-i", "--login"]) {
            if inner.is_empty() {
                return self.leaf_unknown(
                    text,
                    prog,
                    rest,
                    &inner_ctx,
                    format!("{prog}: opens an interactive shell"),
                );
            }
            if inner.iter().any(|a| a.dynamic) {
                return self.leaf_unknown(
                    text,
                    prog,
                    rest,
                    &inner_ctx,
                    format!("{prog}: shell command only known at run time"),
                );
            }
            let joined = inner
                .iter()
                .map(|a| a.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            return self.script(&joined, &inner_ctx);
        }
        if inner.is_empty() {
            return self.leaf_unknown(
                text,
                prog,
                rest,
                &inner_ctx,
                format!("{prog} without a command"),
            );
        }
        self.command(inner.to_vec(), &inner_ctx);
    }

    pub(super) fn su(&mut self, text: &str, rest: &[Arg], ctx: &Ctx) {
        let mut inner_ctx = ctx.wrapped("su");
        inner_ctx.extra.insert(RiskTag::Privilege);
        let mut i = 0;
        let mut cmd: Option<&Arg> = None;
        while i < rest.len() {
            let t = rest[i].text.as_str();
            i += 1;
            match t {
                "-c" | "--command" => {
                    let Some(c) = rest.get(i) else {
                        return self.leaf_unknown(
                            text,
                            "su",
                            rest,
                            &inner_ctx,
                            "su -c without a command".into(),
                        );
                    };
                    cmd = Some(c);
                    i += 1;
                }
                "-s" | "--shell" | "-g" | "--group" | "-G" | "--supp-group" | "-w" => i += 1,
                _ if t.starts_with("--command=") => {
                    let s = t["--command=".len()..].to_string();
                    return self.script(&s, &inner_ctx);
                }
                _ => {}
            }
        }
        match cmd {
            Some(c) if !c.dynamic => {
                let s = c.text.clone();
                self.script(&s, &inner_ctx)
            }
            Some(_) => self.leaf_unknown(
                text,
                "su",
                rest,
                &inner_ctx,
                "su -c string only known at run time".into(),
            ),
            None => self.leaf_unknown(
                text,
                "su",
                rest,
                &inner_ctx,
                "su opens an interactive shell".into(),
            ),
        }
    }
}
