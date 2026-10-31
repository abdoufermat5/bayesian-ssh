//! One-line account of a command's effects.

use super::model::{Effect, Effects, Verb};

fn is_path(t: &str) -> bool {
    !t.contains(char::is_whitespace)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn unique<'a>(names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    for n in names {
        if !seen.contains(&n) {
            seen.push(n);
        }
    }
    seen
}

/// One line: `creates 2 directories · moves 4 paths · deletes nothing · sudo no`.
pub fn summary(e: &Effects) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut verbs: Vec<Verb> = e
        .items
        .iter()
        .map(|i| match i.verb {
            Verb::CreatesDirs => Verb::Creates,
            v => v,
        })
        .collect();
    verbs.sort();
    verbs.dedup();
    for v in verbs {
        let items: Vec<&Effect> = e
            .items
            .iter()
            .filter(|i| match v {
                Verb::Creates => matches!(i.verb, Verb::Creates | Verb::CreatesDirs),
                _ => i.verb == v,
            })
            .collect();
        let targets: Vec<&str> = items
            .iter()
            .flat_map(|i| i.targets.iter().map(String::as_str))
            .collect();
        let counted = matches!(
            v,
            Verb::Creates
                | Verb::Moves
                | Verb::Copies
                | Verb::Deletes
                | Verb::Edits
                | Verb::Changes
        );
        let phrase = if counted {
            let paths = targets.iter().filter(|t| is_path(t)).count();
            let dirs = items.iter().all(|i| i.verb == Verb::CreatesDirs);
            let mut bits = Vec::new();
            // An effect without operands (`rm` alone) still counts once.
            let n = if targets.is_empty() {
                items.len()
            } else {
                paths
            };
            if n > 0 {
                bits.push(if dirs {
                    plural(n, "directory", "directories")
                } else {
                    plural(n, "path", "paths")
                });
            }
            bits.extend(
                unique(targets.iter().copied().filter(|t| !is_path(t)))
                    .into_iter()
                    .map(str::to_string),
            );
            bits.join(", ")
        } else if targets.is_empty() {
            match v {
                Verb::Installs | Verb::Removes => "packages".into(),
                Verb::Runs => "a command".into(),
                _ => "a service".into(),
            }
        } else {
            unique(targets.iter().copied()).join(", ")
        };
        let word = if v == Verb::WritesDevice {
            "writes device"
        } else {
            v.word()
        };
        parts.push(format!("{word} {phrase}"));
    }
    let lists_deletes = e
        .items
        .iter()
        .any(|i| matches!(i.verb, Verb::Deletes | Verb::Removes));
    if !e.deletes {
        parts.push("deletes nothing".into());
    } else if !lists_deletes {
        parts.push("may delete data".into());
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
