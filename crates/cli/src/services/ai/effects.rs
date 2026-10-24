//! "What this does": a plain account of a command's effects, read from the
//! analyzer's parse, never from the agent. Approval cards, `bssh ai watch` and
//! the agent's own results all show it, so a person can approve a one-line
//! script without reading it character by character.

use crate::services::ai::types::{Analysis, RiskTag};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verb {
    Creates,
    Moves,
    Copies,
    Deletes,
    Edits,
    /// Permissions or ownership.
    Changes,
    Restarts,
    Reloads,
    Starts,
    Stops,
    Enables,
    Disables,
    Installs,
    /// Anything bssh can't put a better word on.
    Runs,
}

impl Verb {
    pub fn word(self) -> &'static str {
        match self {
            Verb::Creates => "creates",
            Verb::Moves => "moves",
            Verb::Copies => "copies",
            Verb::Deletes => "deletes",
            Verb::Edits => "edits",
            Verb::Changes => "changes",
            Verb::Restarts => "restarts",
            Verb::Reloads => "reloads",
            Verb::Starts => "starts",
            Verb::Stops => "stops",
            Verb::Enables => "enables",
            Verb::Disables => "disables",
            Verb::Installs => "installs",
            Verb::Runs => "runs",
        }
    }
}

/// One thing a command does: `moves /var/www/releases/2026-08-14 → /var/tmp/x/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    pub verb: Verb,
    /// Paths, units or packages, as written.
    pub targets: Vec<String>,
    /// Where moves and copies go.
    pub dest: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effects {
    /// In command order; read-only commands are left out.
    pub items: Vec<Effect>,
    pub deletes: bool,
    pub sudo: bool,
    pub network: bool,
    /// Set when the analyzer couldn't parse everything: the summary says so.
    pub not_understood: Option<String>,
}

/// Effects of one analyzed command.
pub fn effects(analysis: &Analysis) -> Effects {
    let mut out = Effects {
        not_understood: analysis.unknown_reason.clone(),
        network: analysis.tags.contains(&RiskTag::Network),
        ..Default::default()
    };
    for c in &analysis.commands {
        if c.wrappers.iter().any(|w| w == "sudo" || w == "doas") {
            out.sudo = true;
        }
        let program = c.program.rsplit('/').next().unwrap_or(&c.program);
        let operands: Vec<String> = c
            .args
            .iter()
            .filter(|a| !a.starts_with('-'))
            .cloned()
            .collect();
        let effect = match program {
            "mkdir" => Some(Effect {
                verb: Verb::Creates,
                targets: operands,
                dest: None,
            }),
            "install" if c.args.iter().any(|a| a == "-d") => Some(Effect {
                verb: Verb::Creates,
                targets: operands
                    .into_iter()
                    .filter(|a| !a.chars().all(|ch| ch.is_ascii_digit()))
                    .collect(),
                dest: None,
            }),
            "mv" | "cp" | "rsync" if operands.len() >= 2 => {
                let mut targets = operands;
                let dest = targets.pop();
                Some(Effect {
                    verb: if program == "mv" {
                        Verb::Moves
                    } else {
                        Verb::Copies
                    },
                    targets,
                    dest,
                })
            }
            "rm" | "rmdir" | "unlink" | "shred" => Some(Effect {
                verb: Verb::Deletes,
                targets: operands,
                dest: None,
            }),
            "chmod" | "chown" | "chgrp" => Some(Effect {
                verb: Verb::Changes,
                targets: operands.into_iter().skip(1).collect(),
                dest: None,
            }),
            "systemctl" => systemctl_effect(&operands),
            "apt" | "apt-get" | "dnf" | "yum" | "apk"
                if operands
                    .first()
                    .is_some_and(|a| a == "install" || a == "add") =>
            {
                Some(Effect {
                    verb: Verb::Installs,
                    targets: operands.into_iter().skip(1).collect(),
                    dest: None,
                })
            }
            _ if !c.writes_to.is_empty() => Some(Effect {
                verb: Verb::Edits,
                targets: c.writes_to.clone(),
                dest: None,
            }),
            _ if c.tags.iter().all(|t| *t == RiskTag::Read) => None,
            _ => Some(Effect {
                verb: Verb::Runs,
                targets: vec![program.to_string()],
                dest: None,
            }),
        };
        if let Some(e) = effect {
            if e.verb == Verb::Deletes {
                out.deletes = true;
            }
            out.items.push(e);
        }
    }
    out
}

fn systemctl_effect(operands: &[String]) -> Option<Effect> {
    let (action, units) = operands.split_first()?;
    let verb = match action.as_str() {
        "restart" | "try-restart" => Verb::Restarts,
        "reload" | "reload-or-restart" => Verb::Reloads,
        "start" => Verb::Starts,
        "stop" | "kill" => Verb::Stops,
        "enable" => Verb::Enables,
        "disable" | "mask" => Verb::Disables,
        _ => return None,
    };
    Some(Effect {
        verb,
        targets: units.to_vec(),
        dest: None,
    })
}

/// One line: `creates 2 paths · moves 4 paths · deletes nothing · sudo no`.
pub fn summary(e: &Effects) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut verbs: Vec<Verb> = e.items.iter().map(|i| i.verb).collect();
    verbs.sort();
    verbs.dedup();
    for v in verbs {
        let n: usize = e
            .items
            .iter()
            .filter(|i| i.verb == v)
            .map(|i| i.targets.len().max(1))
            .sum();
        parts.push(match v {
            Verb::Restarts
            | Verb::Reloads
            | Verb::Starts
            | Verb::Stops
            | Verb::Enables
            | Verb::Disables
            | Verb::Installs
            | Verb::Runs => {
                let names: Vec<&str> = e
                    .items
                    .iter()
                    .filter(|i| i.verb == v)
                    .flat_map(|i| i.targets.iter().map(String::as_str))
                    .collect();
                format!("{} {}", v.word(), names.join(", "))
            }
            _ => format!("{} {n} {}", v.word(), if n == 1 { "path" } else { "paths" }),
        });
    }
    if !e.deletes {
        parts.push("deletes nothing".into());
    }
    parts.push(format!("sudo {}", if e.sudo { "yes" } else { "no" }));
    if e.network {
        parts.push("uses the network".into());
    }
    if e.not_understood.is_some() {
        parts.push("part of it not understood".into());
    }
    parts.join(" · ")
}

/// A shell line split into the commands a person reads, one per line: split
/// at `;`, `&&`, `||` and newlines outside quotes, with `set -e…` boilerplate
/// left out. Display only; the exact text is what runs.
pub fn command_lines(command: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c);
            }
            (Some(_), '\\') => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                cur.push(c);
            }
            (None, ';' | '\n') => lines.push(std::mem::take(&mut cur)),
            (None, '&') if chars.peek() == Some(&'&') => {
                chars.next();
                lines.push(std::mem::take(&mut cur));
            }
            (None, '|') if chars.peek() == Some(&'|') => {
                chars.next();
                lines.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    lines.push(cur);
    lines
        .into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !is_boilerplate(l))
        .collect()
}

fn is_boilerplate(line: &str) -> bool {
    let mut words = line.split_whitespace();
    words.next() == Some("set") && words.all(|w| w.starts_with('-') || w == "pipefail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::analyzer::ShellAnalyzer;
    use crate::services::ai::types::Analyzer;

    const PLAN: &str = "set -euo pipefail; A=/var/tmp/x; install -d -m 0750 \"$A/releases\"; \
        mv /var/www/releases/2026-08-14 /var/www/releases/2026-09-02 \"$A/releases/\"; echo done";

    #[test]
    fn a_cleanup_script_reads_as_creates_and_moves() {
        let a = ShellAnalyzer.analyze(PLAN);
        let e = effects(&a);
        assert!(!e.deletes && !e.sudo);
        let verbs: Vec<Verb> = e.items.iter().map(|i| i.verb).collect();
        assert_eq!(verbs, vec![Verb::Creates, Verb::Moves]);
        assert_eq!(
            summary(&e),
            "creates 1 path · moves 2 paths · deletes nothing · sudo no"
        );
    }

    #[test]
    fn lines_split_where_the_shell_splits() {
        assert_eq!(
            command_lines(PLAN),
            vec![
                "A=/var/tmp/x",
                "install -d -m 0750 \"$A/releases\"",
                "mv /var/www/releases/2026-08-14 /var/www/releases/2026-09-02 \"$A/releases/\"",
                "echo done",
            ]
        );
        assert_eq!(
            command_lines("echo 'a; b' && ls"),
            vec!["echo 'a; b'", "ls"]
        );
    }
}
