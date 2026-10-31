//! Argument, classification and rule-table types.

use crate::services::ai::types::RiskTag;
use std::collections::BTreeSet;

/// One shell word after expansion, as the rules see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    pub text: String,
    /// Contains an expansion we can't resolve (`$x`, `$(...)`).
    pub dynamic: bool,
    /// Could turn into an option (or a different word entirely) at run time.
    pub flaggy: bool,
    /// The shell expands this word as a glob before the command sees it (`/etc/sha*`).
    /// A quoted `'*.log'` is not one: the command receives it verbatim.
    pub glob: bool,
}

impl Arg {
    pub fn lit(text: &str) -> Self {
        Arg {
            text: text.to_string(),
            dynamic: false,
            flaggy: false,
            glob: false,
        }
    }

    /// Stands for words that arrive later (`xargs` input, `find {}`).
    pub fn placeholder(text: &str) -> Self {
        Arg {
            text: text.to_string(),
            dynamic: true,
            flaggy: true,
            glob: false,
        }
    }
}

/// What the table says about one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    pub tags: BTreeSet<RiskTag>,
    pub rule: String,
    pub unknown: Option<String>,
}

impl Class {
    pub fn new(rule: impl Into<String>, tags: &[RiskTag]) -> Self {
        Class {
            tags: tags.iter().copied().collect(),
            rule: rule.into(),
            unknown: None,
        }
    }

    pub fn unknown(rule: impl Into<String>, reason: impl Into<String>) -> Self {
        Class {
            tags: [RiskTag::Unknown].into_iter().collect(),
            rule: rule.into(),
            unknown: Some(reason.into()),
        }
    }
}

pub struct Case {
    pub name: &'static str,
    pub sub: &'static [&'static str],
    pub has: &'static [&'static str],
    pub lacks: &'static [&'static str],
    /// Most positional (non-dash) arguments allowed after the subcommand.
    pub max_pos: Option<usize>,
    pub tags: &'static [RiskTag],
}

pub struct Rule {
    pub programs: &'static [&'static str],
    /// Options allowed before the subcommand: (name, takes a value).
    pub globals: &'static [(&'static str, bool)],
    pub cases: &'static [Case],
}
