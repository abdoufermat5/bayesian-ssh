//! The approval card: what the request does as bssh read it, the commands, and what the
//! agent says, on one screen.

use super::canvas::Canvas;
use super::detail::{agent_says, call_body, rail_lines, side_steps, step_body, SideSection};
use super::effects::{call_block, plan_blocks, what_section, Section};
use super::grant::QUICK_GRANT_MINUTES;
use super::view::View;
use super::words::{call_title, expiry_text, human_duration, kind_word, tier_words};
use crate::services::ai::control_client::RunHello;
use crate::services::ai::types::{ApprovalRequest, ApprovalSubject, RiskTag, Step, StepAction};
use crate::services::ai::ui::{self, Glyph, Style, Tone};
use chrono::{DateTime, Utc};

/// The approval to draw: the run it came from, the request, and the clock.
#[derive(Debug, Clone, Copy)]
pub(super) struct Card<'a> {
    pub(super) style: &'a Style,
    pub(super) run: &'a RunHello,
    pub(super) request: &'a ApprovalRequest,
    pub(super) now: DateTime<Utc>,
}

/// Everything the approver needs to decide, as text.
pub(crate) fn render_request(run: &RunHello, r: &ApprovalRequest) -> String {
    let style = Style::terminal();
    render_card(
        Card {
            style: &style,
            run,
            request: r,
            now: Utc::now(),
        },
        View::Short,
    )
}

/// The approval card, at the terminal's width.
pub(super) fn render_card(card: Card<'_>, view: View) -> String {
    render_card_w(card, view, ui::width())
}

pub(super) fn render_card_w(card: Card<'_>, view: View, w: usize) -> String {
    let Card {
        style,
        run,
        request: r,
        now,
    } = card;
    let canvas = Canvas::new(style, w);
    let dim = |s: &str| style.paint(Tone::Dim, s);
    let mut o: Vec<String> = Vec::new();

    o.push(canvas.split_line(
        &format!(
            "{} {}",
            style.mark(Glyph::Ask),
            style.paint(Tone::Strong, "Approval needed")
        ),
        &style.paint(Tone::Warn, &expiry_text(r, now)),
    ));

    let (title, kind_fact, counts) = match &r.subject {
        ApprovalSubject::Plan { plan, .. } => {
            let plural = |n: usize, one: &str, many: &str| {
                if n == 1 {
                    format!("{n} {one}")
                } else {
                    format!("{n} {many}")
                }
            };
            let mut counts = vec![plural(plan.steps.len(), "step", "steps")];
            if !plan.verify.is_empty() {
                counts.push(plural(plan.verify.len(), "check", "checks"));
            }
            if !plan.rollback.is_empty() {
                counts.push(plural(plan.rollback.len(), "undo", "undos"));
            }
            (
                plan.title.clone(),
                format!("change plan {}", ui::short_id(&plan.change_id)),
                counts,
            )
        }
        ApprovalSubject::Call { call, .. } => (
            call_title(call),
            format!("{} request {}", kind_word(r), ui::short_id(&r.request_id)),
            Vec::new(),
        ),
        ApprovalSubject::Access { duration, .. } => (
            format!("Wider access for {}", human_duration(duration)),
            format!("access request {}", ui::short_id(&r.request_id)),
            Vec::new(),
        ),
    };
    o.push(format!("  {}", style.paint(Tone::Strong, &title)));
    let run_fact = format!("run {}", ui::short_id(&run.run_id));
    let mut facts: Vec<&str> = vec![&kind_fact];
    facts.extend(counts.iter().map(String::as_str));
    facts.push(&r.client);
    facts.push(&run_fact);
    o.push(format!(
        "  {}",
        dim(&ui::truncate(&facts.join(" · "), w - 2))
    ));
    o.push(String::new());

    // What bssh read.
    match &r.subject {
        ApprovalSubject::Plan { plan, analyses, .. } => {
            let blocks = plan_blocks(
                plan.steps
                    .iter()
                    .zip((0..plan.steps.len()).map(|i| analyses.get(i).and_then(|a| a.as_ref()))),
            );
            o.extend(what_section(
                &canvas,
                Section {
                    label: "what this does",
                    blocks: &blocks,
                },
                view,
            ));
        }
        ApprovalSubject::Call { call, analysis } => {
            let blocks = vec![call_block(call, analysis.as_ref())];
            o.extend(what_section(
                &canvas,
                Section {
                    label: "what this does",
                    blocks: &blocks,
                },
                view,
            ));
        }
        ApprovalSubject::Access {
            scope, duration, ..
        } => {
            o.push(canvas.split_line(
                &format!("  {}", style.label("what this allows")),
                &dim("read by bssh, not the agent"),
            ));
            if !scope.hosts.is_empty() {
                o.push(format!("  {}  {}", dim("hosts"), scope.hosts.join(", ")));
            }
            if !scope.tags.is_empty() {
                o.push(format!("  {}  {}", dim("tags "), scope.tags.join(", ")));
            }
            o.push(format!("  {}  {}", dim("for  "), human_duration(duration)));
        }
    }
    o.push(format!(
        "  {}  {}",
        dim("policy"),
        style.facts(&[&run.policy, tier_words(r.tier)])
    ));
    if !r.reason.is_empty() {
        // The policy's own words for why it asks.
        o.push(format!("  {}  {}", dim("why   "), r.reason));
    }

    // The commands.
    match &r.subject {
        ApprovalSubject::Plan { plan, analyses, .. } => {
            for (i, step) in plan.steps.iter().enumerate() {
                o.push(String::new());
                let host = step.host.as_deref().unwrap_or("-");
                let tags = analyses.get(i).and_then(|a| a.as_ref()).map(|a| &a.tags);
                o.push(canvas.block_header(
                    &format!("step {}", i + 1),
                    host,
                    &step_risk(style, step, tags),
                ));
                o.extend(rail_lines(&canvas, &step_body(step), view));
            }
            for (name, steps) in [("checks", &plan.verify), ("undo", &plan.rollback)] {
                if !steps.is_empty() {
                    o.extend(side_steps(&canvas, SideSection { name, steps }, view));
                }
            }
        }
        ApprovalSubject::Call { call, analysis } => {
            o.push(String::new());
            let host = call.host.as_deref().unwrap_or("-");
            let risk = analysis
                .as_ref()
                .map(|a| style.risks(&a.tags))
                .unwrap_or_default();
            o.push(canvas.block_header(kind_word(r), host, &risk));
            o.extend(rail_lines(&canvas, &call_body(call), view));
        }
        ApprovalSubject::Access { .. } => {}
    }

    // The agent's words.
    let reason = match &r.subject {
        ApprovalSubject::Plan { plan, .. } => plan.reason.as_str(),
        ApprovalSubject::Access { reason, .. } => reason.as_str(),
        ApprovalSubject::Call { .. } => "",
    };
    if !reason.trim().is_empty() {
        o.push(String::new());
        o.extend(agent_says(&canvas, reason, view));
    }

    // Keys.
    o.push(String::new());
    let key = |k: &str, what: &str| format!("{} {what}", style.paint(Tone::Strong, k));
    let mut keys = vec![
        key("a", "approve"),
        key("d", "deny"),
        key("v", view.other_label()),
        key("s", "skip"),
    ];
    let right = match &r.subject {
        ApprovalSubject::Plan { plan_hash, .. } => {
            dim(&format!("approves plan {} only", ui::short_id(plan_hash)))
        }
        _ => String::new(),
    };
    let grant_key = matches!(r.subject, ApprovalSubject::Call { .. }).then(|| {
        key(
            "g",
            &format!("approve and allow this for {QUICK_GRANT_MINUTES} min"),
        )
    });
    let sep = "    ";
    let base = keys.join(sep);
    match grant_key {
        Some(g) if ui::display_width(&base) + sep.len() + ui::display_width(&g) + 2 <= w => {
            keys.push(g);
            o.push(format!("  {}", keys.join(sep)));
        }
        Some(g) => {
            o.push(format!("  {base}"));
            o.push(format!("  {g}"));
        }
        None => o.push(canvas.split_line(&format!("  {base}"), &right)),
    }
    o.join("\n")
}

fn step_risk(
    style: &Style,
    step: &Step,
    tags: Option<&std::collections::BTreeSet<RiskTag>>,
) -> String {
    match (&step.action, tags) {
        (StepAction::EditFile { .. }, _) => style.risk(RiskTag::Write),
        (StepAction::Service { .. }, _) => style.risk(RiskTag::ServiceChange),
        (_, Some(t)) => style.risks(t),
        _ => String::new(),
    }
}
