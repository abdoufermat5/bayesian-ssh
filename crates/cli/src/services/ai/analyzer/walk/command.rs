//! Dispatching one command word to its wrapper handler or the rule table.

use crate::services::ai::analyzer::rules::{Arg, Class, classify};
use crate::services::ai::analyzer::special::find_exec_commands;

use super::opts::{basename, is_assignment, trusted_path};
use super::state::{Ctx, Walker};

impl Walker {
    pub(super) fn command(&mut self, args: Vec<Arg>, ctx: &Ctx) {
        let first = &args[0];
        if first.dynamic {
            let class = Class::unknown(
                first.text.clone(),
                format!("command name '{}' is only known at run time", first.text),
            );
            self.emit(&first.text, &args[1..], ctx, class);
            return;
        }
        if ctx.wrappers.len() > 24 {
            self.note_unknown("wrappers nested too deep");
            return;
        }
        let text = first.text.clone();
        let prog = basename(&text).to_string();
        let rest = &args[1..];
        if !trusted_path(&text) {
            let class = Class::unknown(
                prog.clone(),
                format!("'{text}' runs a program outside the system directories"),
            );
            self.emit(&text, rest, ctx, class);
            return;
        }
        match prog.as_str() {
            "sudo" | "doas" | "pkexec" => self.sudo(&text, &prog, rest, ctx),
            "su" => self.su(&text, rest, ctx),
            "env" => self.env(&text, rest, ctx),
            "timeout" | "nice" | "ionice" | "nohup" | "stdbuf" | "setsid" | "time" | "command"
            | "builtin" | "exec" | "taskset" | "flock" => {
                self.simple_wrapper(&text, &prog, rest, ctx)
            }
            "busybox" if !rest.is_empty() => self.command(rest.to_vec(), &ctx.wrapped("busybox")),
            "busybox" => {
                let class = Class::unknown("busybox", "busybox without an applet");
                self.emit(&text, rest, ctx, class)
            }
            "xargs" => self.xargs(&text, rest, ctx),
            "watch" => self.watch(&text, rest, ctx),
            "cd" | "pushd" | "popd" => {
                self.chdir(&prog, rest);
                let class = classify(&prog, rest);
                self.emit(&text, rest, ctx, class);
            }
            "sh" | "bash" | "dash" | "zsh" | "ksh" | "ash" | "mksh" | "rbash" | "fish" | "csh"
            | "tcsh" => self.shell(&text, &prog, rest, ctx),
            "eval" => {
                let class = Class::unknown("eval", "eval runs a string built at run time");
                self.emit(&text, rest, ctx, class);
                if rest.iter().all(|a| !a.dynamic) && !rest.is_empty() {
                    let joined = rest
                        .iter()
                        .map(|a| a.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    self.script(&joined, &ctx.wrapped("eval"));
                }
            }
            "find" => {
                let class = classify("find", rest);
                self.emit(&text, rest, ctx, class);
                match find_exec_commands(rest) {
                    Ok(list) => {
                        for (opt, inner) in list {
                            self.command(inner, &ctx.wrapped(&format!("find {opt}")));
                        }
                    }
                    Err(e) => self.note_unknown(e),
                }
            }
            _ => {
                if matches!(
                    prog.as_str(),
                    "export" | "declare" | "typeset" | "readonly" | "local"
                ) {
                    for a in rest {
                        if is_assignment(&a.text) {
                            let name = a
                                .text
                                .split('=')
                                .next()
                                .unwrap_or("")
                                .trim_end_matches('+')
                                .to_string();
                            self.check_var(&name);
                        }
                    }
                }
                let class = classify(&prog, rest);
                self.emit(&text, rest, ctx, class);
            }
        }
    }

    pub(super) fn leaf_unknown(
        &mut self,
        text: &str,
        prog: &str,
        rest: &[Arg],
        ctx: &Ctx,
        reason: String,
    ) {
        let class = Class::unknown(prog, reason);
        self.emit(text, rest, ctx, class);
    }
}
