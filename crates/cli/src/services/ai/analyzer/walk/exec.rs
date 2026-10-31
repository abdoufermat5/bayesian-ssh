//! Wrappers that run their arguments through something else: `xargs`, `watch`, shells.

use crate::services::ai::analyzer::rules::Arg;

use super::opts::{Spec, parse_opts};
use super::state::{Ctx, Walker};

impl Walker {
    pub(super) fn xargs(&mut self, text: &str, rest: &[Arg], ctx: &Ctx) {
        let mut i = 0;
        let mut repl: Option<String> = None;
        while i < rest.len() {
            let t = rest[i].text.as_str();
            if t == "--" {
                i += 1;
                break;
            }
            if !t.starts_with('-') || t == "-" {
                break;
            }
            i += 1;
            if let Some(long) = t.strip_prefix("--") {
                let (name, value) = match long.split_once('=') {
                    Some((n, v)) => (n, Some(v)),
                    None => (long, None),
                };
                match name {
                    "null" | "no-run-if-empty" | "verbose" | "interactive" | "open-tty"
                    | "exit" | "show-limits" | "help" | "version" => {}
                    "arg-file" | "delimiter" | "max-args" | "max-chars" | "max-procs"
                    | "process-slot-var" => {
                        if value.is_none() {
                            i += 1;
                        }
                    }
                    "replace" => repl = Some(value.unwrap_or("{}").to_string()),
                    "eof" | "max-lines" => {}
                    _ => {
                        return self.leaf_unknown(
                            text,
                            "xargs",
                            rest,
                            ctx,
                            format!("xargs: unknown option --{name}"),
                        );
                    }
                }
                continue;
            }
            let chars: Vec<char> = t[1..].chars().collect();
            for (k, ch) in chars.iter().enumerate() {
                let tail: String = chars[k + 1..].iter().collect();
                match ch {
                    '0' | 'r' | 't' | 'p' | 'o' | 'x' | 'h' | 'V' => {}
                    'I' | 'E' | 'L' | 'n' | 'P' | 's' | 'a' | 'd' => {
                        let v = if tail.is_empty() {
                            let v = rest.get(i).map(|a| a.text.clone()).unwrap_or_default();
                            i += 1;
                            v
                        } else {
                            tail
                        };
                        if *ch == 'I' {
                            repl = Some(v);
                        }
                        break;
                    }
                    'i' => {
                        repl = Some(if tail.is_empty() { "{}".into() } else { tail });
                        break;
                    }
                    'l' | 'e' => break,
                    _ => {
                        return self.leaf_unknown(
                            text,
                            "xargs",
                            rest,
                            ctx,
                            format!("xargs: unknown option -{ch}"),
                        );
                    }
                }
            }
        }
        let mut inner: Vec<Arg> = rest[i.min(rest.len())..]
            .iter()
            .map(|a| match &repl {
                Some(r) if !r.is_empty() && a.text.contains(r.as_str()) => {
                    Arg::placeholder(&a.text)
                }
                _ => a.clone(),
            })
            .collect();
        if inner.is_empty() {
            inner.push(Arg::lit("echo"));
        }
        if repl.is_none() {
            inner.push(Arg::placeholder("<xargs input>"));
        }
        self.command(inner, &ctx.wrapped("xargs"));
    }

    pub(super) fn watch(&mut self, text: &str, rest: &[Arg], ctx: &Ctx) {
        let spec = Spec {
            flags: "bcdegpqrtwxhv",
            with_arg: "n",
            long: &[
                ("--interval", true),
                ("--differences", false),
                ("--beep", false),
                ("--color", false),
                ("--no-color", false),
                ("--errexit", false),
                ("--chgexit", false),
                ("--exec", false),
                ("--precise", false),
                ("--no-title", false),
                ("--no-wrap", false),
                ("--help", false),
                ("--version", false),
                ("--equexit", true),
            ],
            numeric: false,
        };
        let opts = match parse_opts(rest, &spec) {
            Ok(o) => o,
            Err(e) => return self.leaf_unknown(text, "watch", rest, ctx, format!("watch: {e}")),
        };
        let inner = &rest[opts.rest..];
        if inner.is_empty() {
            return self.leaf_unknown(text, "watch", rest, ctx, "watch without a command".into());
        }
        let joined = inner
            .iter()
            .map(|a| a.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        self.script(&joined, &ctx.wrapped("watch"));
    }

    pub(super) fn shell(&mut self, text: &str, prog: &str, rest: &[Arg], ctx: &Ctx) {
        let mut c_flag = false;
        let mut s_flag = false;
        let mut i = 0;
        while i < rest.len() {
            let t = rest[i].text.as_str();
            if t == "--" {
                i += 1;
                break;
            }
            if let Some(long) = t.strip_prefix("--") {
                match long.split('=').next().unwrap_or(long) {
                    "login" | "norc" | "noprofile" | "posix" | "restricted" | "verbose"
                    | "noediting" | "debugger" | "dump-strings" | "dump-po-strings" | "version"
                    | "help" => i += 1,
                    "rcfile" | "init-file" => i += 2,
                    other => {
                        return self.leaf_unknown(
                            text,
                            prog,
                            rest,
                            ctx,
                            format!("{prog}: unknown option --{other}"),
                        );
                    }
                }
            } else if (t.starts_with('-') || t.starts_with('+')) && t.len() > 1 {
                let mut skip_value = false;
                for ch in t[1..].chars() {
                    match ch {
                        'c' if t.starts_with('-') => c_flag = true,
                        's' if t.starts_with('-') => s_flag = true,
                        'o' | 'O' => skip_value = true,
                        'a' | 'b' | 'e' | 'f' | 'h' | 'i' | 'l' | 'm' | 'n' | 'p' | 'u' | 'v'
                        | 'x' | 'B' | 'C' | 'E' | 'H' | 'P' | 'T' | 'r' | 'k' | 'D' | 'I' | 'L' => {
                        }
                        _ => {
                            return self.leaf_unknown(
                                text,
                                prog,
                                rest,
                                ctx,
                                format!("{prog}: unknown option {t}"),
                            );
                        }
                    }
                }
                i += 1;
                if skip_value {
                    i += 1;
                }
            } else {
                break;
            }
        }
        if c_flag {
            let Some(script) = rest.get(i) else {
                return self.leaf_unknown(
                    text,
                    prog,
                    rest,
                    ctx,
                    format!("{prog} -c without a string"),
                );
            };
            if script.dynamic {
                return self.leaf_unknown(
                    text,
                    prog,
                    rest,
                    ctx,
                    format!("{prog} -c string only known at run time"),
                );
            }
            let s = script.text.clone();
            return self.script(&s, &ctx.wrapped(&format!("{prog} -c")));
        }
        let reason = if i < rest.len() && !s_flag {
            format!("{prog} runs a script file")
        } else {
            format!("{prog} reads commands from its input")
        };
        self.leaf_unknown(text, prog, rest, ctx, reason);
    }
}
