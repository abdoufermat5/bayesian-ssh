//! Walker state: the context a command inherits, and the directory it runs in.

use std::collections::BTreeSet;

use crate::services::ai::analyzer::ast::Node;
use crate::services::ai::types::{RiskTag, SimpleCommand};

/// Directories whose programs we trust by name. Anything else with a path is unknown:
/// `/tmp/x/ls` is not `ls`.
pub(super) const SYSTEM_DIRS: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/games",
    "/snap/bin",
    "/opt/homebrew/bin",
];

/// Variables that change what a command loads or runs.
pub(super) const RISKY_VARS: &[&str] = &[
    "PATH",
    "IFS",
    "ENV",
    "BASH_ENV",
    "PROMPT_COMMAND",
    "PS4",
    "SHELLOPTS",
    "BASHOPTS",
    "CDPATH",
    "GLOBIGNORE",
    "PAGER",
    "MANPAGER",
    "EDITOR",
    "VISUAL",
    "SYSTEMD_PAGER",
    "SYSTEMD_EDITOR",
    "SUDO_EDITOR",
    "SUDO_ASKPASS",
    "SSH_ASKPASS",
    "HOSTALIASES",
    "LOCPATH",
    "NLSPATH",
    "TERMINFO",
    "TERMCAP",
    "GCONV_PATH",
    "LESSOPEN",
    "LESSCLOSE",
    "HISTFILE",
    "HOME",
];
pub(super) const RISKY_VAR_PREFIXES: &[&str] = &[
    "LD_",
    "DYLD_",
    "GIT_",
    "PERL",
    "PYTHON",
    "RUBY",
    "NODE_",
    "JAVA_TOOL",
    "_JAVA_",
    "BASH_FUNC_",
];

#[derive(Clone, Default)]
pub(super) struct Ctx {
    /// Wrappers peeled so far, outermost first.
    pub(super) wrappers: Vec<String>,
    /// Tags every command in scope gets (`sudo` adds privilege, `/dev/tcp` adds network).
    pub(super) extra: BTreeSet<RiskTag>,
    /// Redirection targets written to, from the command and from enclosing groups.
    pub(super) writes: Vec<String>,
    /// Redirection targets read from, from the command and from enclosing groups.
    pub(super) reads: Vec<String>,
    pub(super) depth: usize,
}

impl Ctx {
    pub(super) fn fresh(&self) -> Ctx {
        Ctx {
            depth: self.depth,
            ..Ctx::default()
        }
    }

    pub(super) fn wrapped(&self, name: &str) -> Ctx {
        let mut c = self.clone();
        c.wrappers.push(name.to_string());
        c
    }
}

/// Where relative arguments of the commands we are about to walk resolve.
#[derive(Clone, Default, PartialEq, Eq)]
pub(super) enum Cwd {
    /// No `cd` on this command line: relative arguments stay relative.
    #[default]
    Untracked,
    /// A literal absolute `cd`/`pushd` target.
    Dir(String),
    /// A `cd`/`pushd` to something we can't resolve (`cd $dir`, `cd ..`, `cd -`, `popd`):
    /// relative arguments after it are not provably anything.
    Changed,
}

#[derive(Default)]
pub struct Walker {
    pub commands: Vec<SimpleCommand>,
    pub unknown: Option<String>,
    pub(super) cwd: Cwd,
}

pub fn walk(root: &Node) -> Walker {
    let mut w = Walker::default();
    w.node(root, &Ctx::default());
    w
}
