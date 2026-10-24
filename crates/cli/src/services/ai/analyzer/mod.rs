//! Shell command analyzer: POSIX AST, wrapper unwrapping, rule database, risk tags.
//!
//! `ShellAnalyzer::analyze` parses the command into an AST (own tokenizer and parser, pure
//! Rust), walks every simple command, peels wrappers (`sudo`, `env`, `sh -c`, `xargs`,
//! `find -exec`...), and tags each one from a rule table. Anything it can't follow is
//! `unknown`, never `read`.
//!
//! The analyzer reduces prompts and warns early. It is not the security boundary: the host
//! enforcement is (design 6.1 and 7).

mod ast;
mod deny;
mod lexer;
mod parser;
mod rules;
mod special;
#[cfg(test)]
mod tests;
mod walk;

use std::collections::BTreeSet;

use super::types::{Analysis, Analyzer, RiskTag};

pub use deny::matches_deny;

/// Commands longer than this are not analyzed (design 6.1).
pub const MAX_COMMAND_CHARS: usize = 10_000;

#[derive(Debug, Default, Clone)]
pub struct ShellAnalyzer;

impl ShellAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl Analyzer for ShellAnalyzer {
    fn analyze(&self, command: &str) -> Analysis {
        analyze_command(command)
    }
}

fn unknown(reason: impl Into<String>) -> Analysis {
    Analysis {
        commands: Vec::new(),
        tags: [RiskTag::Unknown].into_iter().collect(),
        unknown_reason: Some(reason.into()),
    }
}

fn analyze_command(command: &str) -> Analysis {
    if command.trim().is_empty() {
        return unknown("empty command");
    }
    // `len` is bytes; the char count only matters near the limit.
    if command.len() > MAX_COMMAND_CHARS && command.chars().count() > MAX_COMMAND_CHARS {
        return unknown(format!(
            "command longer than {MAX_COMMAND_CHARS} characters"
        ));
    }
    let root = match parser::parse_program(command, 0) {
        Ok(root) => root,
        Err(e) => return unknown(format!("could not parse the command: {e}")),
    };
    let walked = walk::walk(&root);
    let mut tags: BTreeSet<RiskTag> = walked
        .commands
        .iter()
        .flat_map(|c| c.tags.iter().copied())
        .collect();
    if walked.unknown.is_some() {
        tags.insert(RiskTag::Unknown);
    }
    Analysis {
        commands: walked.commands,
        tags,
        unknown_reason: walked.unknown,
    }
}
