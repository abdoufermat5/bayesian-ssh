//! `env`, and the wrappers that take options and then a command (`timeout`, `nice`...).

use crate::services::ai::analyzer::rules::{Arg, Class};
use crate::services::ai::types::RiskTag;

use super::opts::{is_assignment, parse_opts, Spec};
use super::state::{Ctx, Walker};

impl Walker {
    pub(super) fn env(&mut self, text: &str, rest: &[Arg], ctx: &Ctx) {
        let spec = Spec {
            flags: "i0v",
            with_arg: "uCSP",
            long: &[
                ("--ignore-environment", false),
                ("--null", false),
                ("--unset", true),
                ("--chdir", true),
                ("--split-string", true),
                ("--debug", false),
                ("--help", false),
                ("--version", false),
            ],
            numeric: false,
        };
        let opts = match parse_opts(rest, &spec) {
            Ok(o) => o,
            Err(e) => return self.leaf_unknown(text, "env", rest, ctx, format!("env: {e}")),
        };
        if opts.has(&["-S", "--split-string", "-P"]) {
            return self.leaf_unknown(
                text,
                "env",
                rest,
                ctx,
                "env -S/-P builds the command at run time".into(),
            );
        }
        let mut idx = opts.rest;
        while idx < rest.len() && is_assignment(&rest[idx].text) {
            let name = rest[idx].text.split('=').next().unwrap_or("").to_string();
            self.check_var(name.trim_end_matches('+'));
            idx += 1;
        }
        let inner = &rest[idx..];
        if inner.is_empty() {
            let class = Class::new("env (print)", &[RiskTag::Read]);
            return self.emit(text, rest, ctx, class);
        }
        self.command(inner.to_vec(), &ctx.wrapped("env"));
    }

    /// `timeout`, `nice`, `nohup`...: options, optional leading positionals, then the command.
    pub(super) fn simple_wrapper(&mut self, text: &str, prog: &str, rest: &[Arg], ctx: &Ctx) {
        let (spec, positionals, no_command_read) = match prog {
            "timeout" => (
                Spec {
                    flags: "v",
                    with_arg: "ks",
                    long: &[
                        ("--kill-after", true),
                        ("--signal", true),
                        ("--preserve-status", false),
                        ("--foreground", false),
                        ("--verbose", false),
                    ],
                    numeric: false,
                },
                1,
                false,
            ),
            "nice" => (
                Spec {
                    flags: "",
                    with_arg: "n",
                    long: &[("--adjustment", true)],
                    numeric: true,
                },
                0,
                true,
            ),
            "ionice" => (
                Spec {
                    flags: "thp",
                    with_arg: "cnpPu",
                    long: &[
                        ("--class", true),
                        ("--classdata", true),
                        ("--pid", true),
                        ("--pgid", true),
                        ("--uid", true),
                        ("--ignore", false),
                        ("--help", false),
                    ],
                    numeric: false,
                },
                0,
                false,
            ),
            "nohup" => (
                Spec {
                    flags: "",
                    with_arg: "",
                    long: &[("--help", false), ("--version", false)],
                    numeric: false,
                },
                0,
                true,
            ),
            "stdbuf" => (
                Spec {
                    flags: "",
                    with_arg: "ioe",
                    long: &[("--input", true), ("--output", true), ("--error", true)],
                    numeric: false,
                },
                0,
                false,
            ),
            "setsid" => (
                Spec {
                    flags: "cfw",
                    with_arg: "",
                    long: &[("--ctty", false), ("--fork", false), ("--wait", false)],
                    numeric: false,
                },
                0,
                false,
            ),
            "time" => (
                Spec {
                    flags: "pv",
                    with_arg: "",
                    long: &[("--portability", false), ("--verbose", false)],
                    numeric: false,
                },
                0,
                true,
            ),
            "command" => (
                Spec {
                    flags: "pvV",
                    with_arg: "",
                    long: &[],
                    numeric: false,
                },
                0,
                true,
            ),
            "builtin" => (
                Spec {
                    flags: "",
                    with_arg: "",
                    long: &[],
                    numeric: false,
                },
                0,
                true,
            ),
            "exec" => (
                Spec {
                    flags: "cl",
                    with_arg: "a",
                    long: &[],
                    numeric: false,
                },
                0,
                true,
            ),
            "taskset" => (
                Spec {
                    flags: "acpr",
                    with_arg: "",
                    long: &[
                        ("--all-tasks", false),
                        ("--cpu-list", false),
                        ("--pid", false),
                        ("--reset-affinity", false),
                    ],
                    numeric: false,
                },
                1,
                false,
            ),
            _ => (
                // flock
                Spec {
                    flags: "sxunoFh",
                    with_arg: "wEc",
                    long: &[
                        ("--shared", false),
                        ("--exclusive", false),
                        ("--unlock", false),
                        ("--nonblock", false),
                        ("--nb", false),
                        ("--no-fork", false),
                        ("--close", false),
                        ("--verbose", false),
                        ("--timeout", true),
                        ("--wait", true),
                        ("--conflict-exit-code", true),
                        ("--command", true),
                        ("--help", false),
                    ],
                    numeric: false,
                },
                1,
                false,
            ),
        };
        let opts = match parse_opts(rest, &spec) {
            Ok(o) => o,
            Err(e) => return self.leaf_unknown(text, prog, rest, ctx, format!("{prog}: {e}")),
        };
        if prog == "command" && opts.has(&["-v", "-V"]) {
            let class = Class::new("command -v", &[RiskTag::Read]);
            return self.emit(text, rest, ctx, class);
        }
        if (prog == "ionice" && opts.has(&["-p", "-P", "-u", "--pid", "--pgid", "--uid"]))
            || (prog == "taskset" && opts.has(&["-p", "--pid"]))
        {
            let class = Class::new(
                format!("{prog} (other processes)"),
                &[RiskTag::ServiceChange],
            );
            return self.emit(text, rest, ctx, class);
        }
        if prog == "flock" {
            if let Some(cmd) = opts.value(&["-c", "--command"]) {
                let cmd = cmd.to_string();
                return self.script(&cmd, &ctx.wrapped("flock"));
            }
        }
        let idx = opts.rest + positionals;
        let inner = rest.get(idx..).unwrap_or(&[]);
        if inner.is_empty() {
            return if no_command_read {
                let class = Class::new(format!("{prog} (no command)"), &[RiskTag::Read]);
                self.emit(text, rest, ctx, class)
            } else {
                self.leaf_unknown(text, prog, rest, ctx, format!("{prog} without a command"))
            };
        }
        self.command(inner.to_vec(), &ctx.wrapped(prog));
    }
}
