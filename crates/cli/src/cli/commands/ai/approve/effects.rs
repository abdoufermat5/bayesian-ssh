//! What a plan or call does on each host, as bssh read it: verbs, targets, deletes, sudo,
//! network, and the per-verb rows of the card.

use super::canvas::Canvas;
use super::view::View;
use super::words::{action_name, service_verb};
use crate::services::ai::effects::{self, Effect, Effects, Verb};
use crate::services::ai::types::{Analysis, Step, StepAction, ToolCall, ToolName};
use crate::services::ai::ui::{self, Glyph, Style, Tone};

/// Targets per verb shown on the short card.
const TARGETS_PER_VERB: usize = 8;

/// What happens on one host, as bssh read it.
pub(super) struct HostBlock {
    host: String,
    effects: Effects,
    /// (path, patch) for edits.
    diffs: Vec<(String, String)>,
}

/// One host's contribution to the effect list.
struct Piece<'a> {
    host: &'a str,
    effects: Effects,
    diff: Option<(String, String)>,
}

/// A "what this does" section: its label and the hosts it covers.
pub(super) struct Section<'a> {
    pub(super) label: &'a str,
    pub(super) blocks: &'a [HostBlock],
}

/// Whether a flag on a host block applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Switch {
    On,
    Off,
}

impl Switch {
    fn of(flag: bool) -> Self {
        if flag {
            Self::On
        } else {
            Self::Off
        }
    }
}

/// `yes` in the flag's own tone, `no` as allowed.
fn yes_no(style: &Style, switch: Switch, yes_tone: Tone) -> String {
    match switch {
        Switch::On => style.paint(yes_tone, "yes"),
        Switch::Off => style.paint(Tone::Ok, "no"),
    }
}

fn merge(into: &mut Effects, e: Effects) {
    into.items.extend(e.items);
    into.deletes |= e.deletes;
    into.sudo |= e.sudo;
    into.network |= e.network;
    if into.not_understood.is_none() {
        into.not_understood = e.not_understood;
    }
}

fn single_effect(verb: Verb, target: &str) -> Effect {
    Effect {
        verb,
        targets: vec![target.to_string()],
        dest: None,
    }
}

fn run_effects(command: &str, analysis: Option<&Analysis>) -> Effects {
    match analysis {
        Some(a) => effects::effects_for(command, a),
        None => Effects {
            not_understood: Some("not analyzed".into()),
            ..Default::default()
        },
    }
}

fn add_block(blocks: &mut Vec<HostBlock>, piece: Piece<'_>) {
    let at = match blocks.iter().position(|b| b.host == piece.host) {
        Some(i) => i,
        None => {
            blocks.push(HostBlock {
                host: piece.host.to_string(),
                effects: Effects::default(),
                diffs: Vec::new(),
            });
            blocks.len() - 1
        }
    };
    merge(&mut blocks[at].effects, piece.effects);
    blocks[at].diffs.extend(piece.diff);
}

pub(super) fn plan_blocks<'a>(
    steps: impl Iterator<Item = (&'a Step, Option<&'a Analysis>)>,
) -> Vec<HostBlock> {
    let mut blocks = Vec::new();
    for (step, analysis) in steps {
        let host = step.host.as_deref().unwrap_or("-");
        let (e, diff) = match &step.action {
            StepAction::Run { command } => (run_effects(command, analysis), None),
            StepAction::EditFile { path, patch } => (
                Effects {
                    items: vec![single_effect(Verb::Edits, path)],
                    ..Default::default()
                },
                Some((path.clone(), patch.clone())),
            ),
            StepAction::Service { unit, action } => (
                Effects {
                    items: vec![single_effect(
                        service_verb(action_name(*action)).unwrap_or(Verb::Runs),
                        unit,
                    )],
                    sudo: true,
                    ..Default::default()
                },
                None,
            ),
            StepAction::Undo { step } => (
                Effects {
                    items: vec![single_effect(Verb::Runs, &format!("undo of {step}"))],
                    ..Default::default()
                },
                None,
            ),
        };
        add_block(
            &mut blocks,
            Piece {
                host,
                effects: e,
                diff,
            },
        );
    }
    blocks
}

pub(super) fn call_block(call: &ToolCall, analysis: Option<&Analysis>) -> HostBlock {
    let host = call.host.as_deref().unwrap_or("-");
    let arg = |k: &str| call.args.get(k).and_then(|v| v.as_str());
    let (e, diff) = match call.tool {
        ToolName::EditFile => {
            let path = call.path.clone().unwrap_or_default();
            (
                Effects {
                    items: vec![single_effect(Verb::Edits, &path)],
                    ..Default::default()
                },
                arg("patch").map(|p| (path, p.to_string())),
            )
        }
        ToolName::ReadFile | ToolName::SearchLogs => (
            Effects {
                items: call
                    .path
                    .as_deref()
                    .or(arg("path"))
                    .or(arg("source"))
                    .map(|p| vec![single_effect(Verb::Reads, p)])
                    .unwrap_or_default(),
                ..Default::default()
            },
            None,
        ),
        ToolName::Service => (
            Effects {
                items: match (arg("action").and_then(service_verb), arg("unit")) {
                    (Some(v), Some(u)) => vec![single_effect(v, u)],
                    _ => Vec::new(),
                },
                sudo: true,
                ..Default::default()
            },
            None,
        ),
        _ => (
            run_effects(call.command.as_deref().unwrap_or(""), analysis),
            None,
        ),
    };
    let mut block = HostBlock {
        host: host.to_string(),
        effects: Effects::default(),
        diffs: Vec::new(),
    };
    merge(&mut block.effects, e);
    block.diffs.extend(diff);
    block
}

fn trim_dest(d: &str) -> &str {
    d.trim_end_matches(['/', '"']).trim_start_matches('"')
}

/// A long destination keeps its last two path components.
fn short_dest(d: &str) -> String {
    let trimmed = trim_dest(d);
    if ui::display_width(trimmed) <= 28 {
        return trimmed.to_string();
    }
    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [.., a, b] if parts.len() > 2 => format!("…/{a}/{b}"),
        _ => trimmed.to_string(),
    }
}

pub(super) fn what_section(canvas: &Canvas, section: Section<'_>, view: View) -> Vec<String> {
    let style = canvas.style;
    let w = canvas.width;
    let full = view.is_full();
    let dim = |s: &str| style.paint(Tone::Dim, s);
    let mut o: Vec<String> = Vec::new();
    o.push(canvas.split_line(
        &format!("  {}", style.label(section.label)),
        &dim("read by bssh, not the agent"),
    ));
    let hostw = section
        .blocks
        .iter()
        .map(|b| ui::display_width(&b.host))
        .max()
        .unwrap_or(0)
        + 2;
    let verbw = 9;
    for b in section.blocks {
        let mut first = true;
        let mut lead = || {
            if std::mem::take(&mut first) {
                ui::pad(&style.paint(Tone::Strong, &b.host), hostw)
            } else {
                " ".repeat(hostw)
            }
        };
        let mut verbs: Vec<Verb> = b.effects.items.iter().map(|i| i.verb).collect();
        verbs.sort();
        verbs.dedup();
        let mut deleted = 0;
        for v in verbs {
            let rows: Vec<(&str, Option<&str>)> = b
                .effects
                .items
                .iter()
                .filter(|i| i.verb == v)
                .flat_map(|i| i.targets.iter().map(|t| (t.as_str(), i.dest.as_deref())))
                .collect();
            if v == Verb::Deletes {
                deleted += rows.len();
            }
            let tone = if v == Verb::Deletes {
                Tone::Bad
            } else {
                Tone::Dim
            };
            let tw = rows
                .iter()
                .filter(|r| r.1.is_some())
                .map(|r| ui::display_width(r.0))
                .max()
                .unwrap_or(0)
                .min(w / 2);
            let shown = if full {
                rows.len()
            } else {
                rows.len().min(TARGETS_PER_VERB)
            };
            // Destinations in full when they all fit, else all shortened: one rule per group.
            let room = w.saturating_sub(2 + hostw + verbw + tw + 3);
            let full_dests = rows.iter().all(|(_, d)| {
                d.as_ref()
                    .is_none_or(|d| ui::display_width(trim_dest(d)) <= room)
            });
            for (n, (target, dest)) in rows.iter().take(shown).enumerate() {
                let verb = if n == 0 { v.word() } else { "" };
                let mut line = format!("  {}{}", lead(), style.paint(tone, &ui::pad(verb, verbw)));
                match dest {
                    Some(d) => {
                        let _ = std::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "{} {} {}",
                                ui::pad(&ui::truncate(target, tw), tw),
                                style.paint(Tone::Dim, style.glyph(Glyph::Arrow)),
                                if full_dests {
                                    trim_dest(d).to_string()
                                } else {
                                    short_dest(d)
                                }
                            ),
                        );
                    }
                    None => {
                        line.push_str(&ui::truncate(target, w.saturating_sub(2 + hostw + verbw)))
                    }
                }
                o.push(line);
            }
            if shown < rows.len() {
                o.push(format!(
                    "  {}{}",
                    " ".repeat(hostw + verbw),
                    dim(&format!(
                        "{} {} more, v for all",
                        style.glyph(Glyph::Ellipsis),
                        rows.len() - shown
                    ))
                ));
            }
        }
        if let Some(why) = &b.effects.not_understood {
            o.push(format!(
                "  {}{}",
                lead(),
                style.paint(Tone::Bad, &format!("part of this is not understood: {why}"))
            ));
        }
        let (del_tone, del) = if !b.effects.deletes {
            (Tone::Ok, "nothing".to_string())
        } else if deleted > 0 {
            (Tone::Bad, deleted.to_string())
        } else {
            (Tone::Bad, "yes".to_string())
        };
        o.push(format!(
            "  {}{}{}        {}  {}        {}  {}",
            lead(),
            style.paint(Tone::Dim, &ui::pad("deletes", verbw)),
            ui::pad(&style.paint(del_tone, &del), 7),
            style.paint(Tone::Dim, "sudo"),
            yes_no(style, Switch::of(b.effects.sudo), Tone::Warn),
            style.paint(Tone::Dim, "network"),
            yes_no(style, Switch::of(b.effects.network), Tone::Info),
        ));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_destinations_keep_two_components() {
        assert_eq!(short_dest("archive"), "archive");
        assert_eq!(
            short_dest("/var/tmp/var-www-cleanup-2026-10-24/releases/"),
            "…/var-www-cleanup-2026-10-24/releases"
        );
    }
}
