//! The body of a step or a call: command lines behind a rail, the diff of an edit, the
//! checks and undo steps, and the agent's own words.

use super::canvas::Canvas;
use super::view::View;
use super::words::action_name;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::effects;
use crate::services::ai::types::{Analyzer, RiskTag, Step, StepAction, ToolCall, ToolName};
use crate::services::ai::ui::{self, Glyph, Style, Tone};

/// Diff lines shown on the short card.
const DIFF_LINES: usize = 12;
/// Lines of "the agent says" on the short card.
const SAYS_LINES: usize = 3;

/// The text of one step or call, split for display.
pub(super) struct Body {
    /// Shell lines, or one plain sentence for services.
    lines: Vec<String>,
    /// A unified diff for edits.
    diff: Option<String>,
}

/// A named group of steps shown under the main block: the checks or the undo steps.
pub(super) struct SideSection<'a> {
    pub(super) name: &'a str,
    pub(super) steps: &'a [Step],
}

pub(super) fn step_body(step: &Step) -> Body {
    match &step.action {
        StepAction::Run { command } => Body {
            lines: effects::command_lines(command),
            diff: None,
        },
        StepAction::EditFile { path, patch } => Body {
            lines: vec![format!("edits {path}")],
            diff: Some(patch.clone()),
        },
        StepAction::Service { unit, action } => Body {
            lines: vec![format!("{} {unit}", action_name(*action))],
            diff: None,
        },
        StepAction::Undo { step } => Body {
            lines: vec![format!("undo {step}")],
            diff: None,
        },
    }
}

pub(super) fn call_body(call: &ToolCall) -> Body {
    let arg = |k: &str| call.args.get(k).and_then(|v| v.as_str());
    match call.tool {
        ToolName::EditFile => Body {
            lines: vec![format!(
                "edits {}",
                call.path.as_deref().unwrap_or("a file")
            )],
            diff: arg("patch").map(String::from),
        },
        ToolName::ReadFile | ToolName::SearchLogs => Body {
            lines: vec![match (call.tool, call.path.as_deref().or(arg("path"))) {
                (ToolName::ReadFile, Some(p)) => format!("read {p}"),
                _ => format!(
                    "search {}{}",
                    arg("source").unwrap_or("logs"),
                    arg("pattern")
                        .map(|p| format!(" for {p}"))
                        .unwrap_or_default()
                ),
            }],
            diff: None,
        },
        _ => match (&call.command, arg("unit")) {
            (Some(c), _) => Body {
                lines: effects::command_lines(c),
                diff: None,
            },
            (None, Some(unit)) => Body {
                lines: vec![format!("{} {unit}", arg("action").unwrap_or("change"))],
                diff: None,
            },
            _ => Body {
                lines: Vec::new(),
                diff: None,
            },
        },
    }
}

/// Command lines behind a dim rail, long ones wrapped with a hanging indent.
pub(super) fn rail_lines(canvas: &Canvas, body: &Body, view: View) -> Vec<String> {
    let style = canvas.style;
    let w = canvas.width;
    let rail = style.paint(Tone::Dim, style.glyph(Glyph::Rail));
    let mut out = Vec::new();
    for line in &body.lines {
        for (i, piece) in ui::wrap(line, w.saturating_sub(4), 3).iter().enumerate() {
            let piece = if i == 0 {
                strong_program(style, piece)
            } else {
                piece.clone()
            };
            out.push(format!("  {rail} {piece}"));
        }
    }
    if let Some(patch) = &body.diff {
        out.extend(
            diff_lines(canvas, patch, view)
                .into_iter()
                .map(|l| format!("  {rail} {l}")),
        );
    }
    out
}

/// The program (first word) in Strong; assignments and expansions stay plain.
fn strong_program(style: &Style, line: &str) -> String {
    let (first, rest) = line.split_once(' ').unwrap_or((line, ""));
    if first.contains('=') || first.starts_with(['$', '"', '\'']) {
        return line.to_string();
    }
    let first = style.paint(Tone::Strong, first);
    if rest.is_empty() {
        first
    } else {
        format!("{first} {rest}")
    }
}

/// `+` lines green, `-` lines red, the rest dim; capped unless the full view.
fn diff_lines(canvas: &Canvas, patch: &str, view: View) -> Vec<String> {
    let style = canvas.style;
    let all: Vec<&str> = patch.lines().collect();
    let shown = if view.is_full() {
        all.len()
    } else {
        all.len().min(DIFF_LINES)
    };
    let mut out: Vec<String> = all[..shown]
        .iter()
        .map(|l| {
            let tone = if l.starts_with('+') {
                Tone::Ok
            } else if l.starts_with('-') {
                Tone::Bad
            } else {
                Tone::Dim
            };
            style.paint(tone, l)
        })
        .collect();
    if shown < all.len() {
        out.push(style.paint(
            Tone::Dim,
            &format!(
                "{} {} more lines, v for all",
                style.glyph(Glyph::Ellipsis),
                all.len() - shown
            ),
        ));
    }
    out
}

/// `CHECKS  df -h /var/www … · 4 × test      read-only`, or one rail line per command in the
/// full view.
pub(super) fn side_steps(canvas: &Canvas, section: SideSection<'_>, view: View) -> Vec<String> {
    let style = canvas.style;
    let w = canvas.width;
    let bodies: Vec<String> = section
        .steps
        .iter()
        .flat_map(|s| step_body(s).lines)
        .collect();
    // What these steps risk, in the words used everywhere else: `read-only` when that is all.
    let mut tags: Vec<RiskTag> = Vec::new();
    for s in section.steps {
        match &s.action {
            StepAction::Run { command } => tags.extend(
                ShellAnalyzer
                    .analyze(command)
                    .tags
                    .into_iter()
                    .filter(|t| *t != RiskTag::Read),
            ),
            StepAction::Service { .. } => tags.push(RiskTag::ServiceChange),
            StepAction::EditFile { .. } | StepAction::Undo { .. } => tags.push(RiskTag::Write),
        }
    }
    let tag = if tags.is_empty() {
        style.paint(Tone::Dim, "read-only")
    } else {
        style.risks(&tags)
    };
    let label = ui::pad(&style.label(section.name), 8);
    let mut out = Vec::new();
    if view.is_full() {
        out.push(canvas.split_line(&format!("  {label}"), &tag));
        let body = Body {
            lines: bodies,
            diff: None,
        };
        out.extend(rail_lines(canvas, &body, View::Full));
        return out;
    }
    // Same program several times: `4 × test`.
    let mut groups: Vec<(String, usize, String)> = Vec::new();
    for line in &bodies {
        let prog = line.split_whitespace().next().unwrap_or("").to_string();
        match groups.iter_mut().find(|g| g.0 == prog) {
            Some(g) => g.1 += 1,
            None => groups.push((prog, 1, line.clone())),
        }
    }
    let text = groups
        .iter()
        .map(|(prog, n, first)| {
            if *n > 1 {
                format!("{n} × {prog}")
            } else {
                first.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" · ");
    let room = w.saturating_sub(2 + 8 + 2 + ui::display_width(&tag) + 2);
    out.push(canvas.split_line(&format!("  {label}{}", ui::truncate(&text, room)), &tag));
    out
}

/// `THE AGENT SAYS`, the quoted reason, the hint to read the rest.
pub(super) fn agent_says(canvas: &Canvas, reason: &str, view: View) -> Vec<String> {
    let style = canvas.style;
    let w = canvas.width;
    let full = view.is_full();
    let mut out = vec![canvas.split_line(
        &format!("  {}", style.label("the agent says")),
        &style.paint(Tone::Dim, "not checked by bssh"),
    )];
    let paragraphs: Vec<String> = if full {
        reason
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    } else {
        vec![reason.split_whitespace().collect::<Vec<_>>().join(" ")]
    };
    let last = paragraphs.len().saturating_sub(1);
    let mut lines: Vec<String> = Vec::new();
    for (i, p) in paragraphs.iter().enumerate() {
        let p = format!(
            "{}{p}{}",
            if i == 0 { "“" } else { "" },
            if i == last { "”" } else { "" }
        );
        lines.extend(ui::wrap(&p, w.saturating_sub(4), 1));
    }
    let cut = !full && lines.len() > SAYS_LINES;
    if cut {
        lines.truncate(SAYS_LINES);
        if let Some(l) = lines.last_mut() {
            l.push_str(" …");
        }
    }
    out.extend(
        lines
            .iter()
            .map(|l| format!("  {}", style.paint(Tone::Quote, l))),
    );
    if cut {
        out.push(format!("  {}", style.paint(Tone::Dim, "v for all of it")));
    }
    out
}
