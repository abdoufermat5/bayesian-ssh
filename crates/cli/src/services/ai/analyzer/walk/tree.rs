//! Walking the AST: nodes, redirections, simple commands and emitting results.

use crate::services::ai::analyzer::ast::{GroupKind, Node, Redir, RedirKind, SimpleCmd};
use crate::services::ai::analyzer::lexer::MAX_DEPTH;
use crate::services::ai::analyzer::parser::parse_program;
use crate::services::ai::analyzer::rules::{Arg, Class};
use crate::services::ai::types::{RiskTag, SimpleCommand};

use super::state::{Ctx, Cwd, Walker, RISKY_VARS, RISKY_VAR_PREFIXES};
use super::words::{chdir_option, harmless_target, is_block_device, word_args};

impl Walker {
    pub(super) fn note_unknown(&mut self, reason: impl Into<String>) {
        if self.unknown.is_none() {
            self.unknown = Some(reason.into());
        }
    }

    pub(super) fn node(&mut self, node: &Node, ctx: &Ctx) {
        match node {
            Node::Simple(cmd) => self.simple(cmd, ctx),
            Node::Pipeline(stages) => {
                for s in stages {
                    self.node(s, ctx);
                }
            }
            Node::Seq(items) => {
                for s in items {
                    self.node(s, ctx);
                }
            }
            Node::Group {
                kind,
                body,
                words,
                redirs,
            } => {
                let mut inner = ctx.clone();
                self.redirs(redirs, &mut inner);
                let fresh = ctx.fresh();
                for w in words {
                    self.subs(&w.subs, &fresh);
                }
                // A subshell and a function body run in their own directory;
                // `{ ...; }` and `if`/loops/`case` keep the shell's.
                let saved = self.cwd.clone();
                for s in body {
                    self.node(s, &inner);
                }
                if matches!(kind, GroupKind::Subshell | GroupKind::Function) {
                    self.cwd = saved;
                }
            }
        }
    }

    /// Commands inside expansions run on their own, without our wrappers or redirections.
    pub(super) fn subs(&mut self, subs: &[Node], fresh: &Ctx) {
        for s in subs {
            self.node(s, fresh);
        }
    }

    pub(super) fn redirs(&mut self, redirs: &[Redir], ctx: &mut Ctx) {
        let fresh = ctx.fresh();
        for r in redirs {
            self.subs(&r.target.subs, &fresh);
            self.subs(&r.subs, &fresh);
            let target = r.target.text();
            if target.starts_with("/dev/tcp/") || target.starts_with("/dev/udp/") {
                ctx.extra.insert(RiskTag::Network);
                ctx.extra.insert(RiskTag::ExfilRisk);
            }
            let writes = match r.kind {
                RedirKind::Out
                | RedirKind::Append
                | RedirKind::Clobber
                | RedirKind::ReadWrite
                | RedirKind::OutErr
                | RedirKind::AppendErr => true,
                RedirKind::DupOut => {
                    !(target == "-"
                        || (!target.is_empty() && target.chars().all(|c| c.is_ascii_digit())))
                }
                _ => false,
            };
            let reads = matches!(r.kind, RedirKind::In | RedirKind::ReadWrite);
            if harmless_target(&target) {
                continue;
            }
            if writes {
                if is_block_device(&target) {
                    ctx.extra.insert(RiskTag::Destructive);
                }
                ctx.writes.push(target.clone());
            }
            if reads {
                ctx.reads.push(target);
            }
        }
    }

    pub(super) fn simple(&mut self, cmd: &SimpleCmd, ctx: &Ctx) {
        let fresh = ctx.fresh();
        let mut ctx = ctx.clone();
        for a in &cmd.assigns {
            self.subs(&a.value.subs, &fresh);
            self.check_var(&a.name);
        }
        for w in &cmd.words {
            self.subs(&w.subs, &fresh);
        }
        self.redirs(&cmd.redirs, &mut ctx);
        // After a `cd` to a target we can't resolve, a relative operand could be anything.
        let relative = cmd.words.iter().skip(1).any(|w| {
            let t = w.text();
            !t.starts_with('-') && !t.starts_with('/')
        }) || ctx
            .writes
            .iter()
            .chain(ctx.reads.iter())
            .any(|t| !t.starts_with('/'));
        if self.cwd == Cwd::Changed && relative {
            self.note_unknown(
                "a relative path after a `cd` whose target is only known at run time",
            );
        }
        let args: Vec<Arg> = cmd.words.iter().flat_map(word_args).collect();
        if args.is_empty() {
            if !ctx.writes.is_empty() {
                let class = Class::new("redirection", &[RiskTag::Write]);
                self.emit(":", &[], &ctx, class);
            }
            return;
        }
        self.command(args, &ctx);
    }

    pub(super) fn check_var(&mut self, name: &str) {
        if RISKY_VARS.contains(&name) || RISKY_VAR_PREFIXES.iter().any(|p| name.starts_with(p)) {
            self.note_unknown(format!(
                "sets {name}, which changes what commands load or run"
            ));
        }
    }

    pub(super) fn emit(&mut self, program: &str, args: &[Arg], ctx: &Ctx, class: Class) {
        let mut tags = class.tags;
        tags.extend(ctx.extra.iter().copied());
        if !ctx.writes.is_empty() {
            tags.insert(RiskTag::Write);
        }
        if let Some(reason) = class.unknown {
            self.note_unknown(reason);
        }
        let cwd = chdir_option(args).or_else(|| match &self.cwd {
            Cwd::Dir(d) => Some(d.clone()),
            _ => None,
        });
        self.commands.push(SimpleCommand {
            program: program.to_string(),
            args: args.iter().map(|a| a.text.clone()).collect(),
            wrappers: ctx.wrappers.clone(),
            writes_to: ctx.writes.clone(),
            reads_from: ctx.reads.clone(),
            glob_args: args
                .iter()
                .filter(|a| a.glob)
                .map(|a| a.text.clone())
                .collect(),
            cwd,
            tags,
            rule: Some(class.rule),
        });
    }

    pub(super) fn script(&mut self, text: &str, ctx: &Ctx) {
        if ctx.depth + 1 > MAX_DEPTH {
            self.note_unknown("shell strings nested too deep");
            return;
        }
        match parse_program(text, ctx.depth + 1) {
            Ok(node) => {
                let mut inner = ctx.clone();
                inner.depth += 1;
                // The inner shell starts in our directory but its `cd`s don't leak back.
                let saved = self.cwd.clone();
                self.node(&node, &inner);
                self.cwd = saved;
            }
            Err(e) => self.note_unknown(format!("could not parse an inner shell string: {e}")),
        }
    }

    /// Track where relative arguments resolve: `cd /etc && cat shadow` reads `/etc/shadow`.
    /// Only a literal absolute target is followable; anything else is `Changed`, so a later
    /// relative operand makes the analysis `unknown`.
    pub(super) fn chdir(&mut self, prog: &str, args: &[Arg]) {
        let target = if prog == "popd" {
            None
        } else {
            args.iter()
                .find(|a| a.text != "--" && !a.text.starts_with('-'))
        };
        self.cwd = match target {
            Some(a) if !a.dynamic && a.text.starts_with('/') => Cwd::Dir(a.text.clone()),
            _ => Cwd::Changed,
        };
    }
}
