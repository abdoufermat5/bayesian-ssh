//! The run header and the approval-request card.

use crate::services::ai::effects::{self, Effects};
use crate::services::ai::types::{ApprovalRequest, ApprovalSubject, Tier};
use crate::services::ai::ui::{self, Glyph, Style, Tone};
use chrono::{DateTime, Utc};

use super::context::{plan_hosts, FeedContext};
use super::line::FeedLine;
use super::words::{call_text, count, kind_word, oneline, CLOCK_W};

/// What the header line of a run shows.
#[derive(Debug, Clone, Default)]
pub struct RunHeader {
    pub client: String,
    pub run_id: String,
    pub policy: String,
    pub on_behalf_of: String,
    pub hosts: Vec<String>,
    /// RFC 3339.
    pub started_at: String,
}

/// `● opencode 2.1.2   run dddbee19   policy default   on behalf of ana   hosts web-1   since 17:23:20`
pub fn run_header(style: &Style, h: &RunHeader, width: usize) -> String {
    let mut parts = vec![
        format!("{} {}", style.mark(Glyph::Running), oneline(&h.client)),
        format!("run {}", ui::short_id(&h.run_id)),
        format!("policy {}", oneline(&h.policy)),
    ];
    if !h.on_behalf_of.is_empty() {
        parts.push(format!("on behalf of {}", oneline(&h.on_behalf_of)));
    }
    if !h.hosts.is_empty() {
        parts.push(format!("hosts {}", h.hosts.join(", ")));
    }
    let since = format!("since {}", ui::clock(&h.started_at));
    let left = parts.join("   ");
    let fill = width.saturating_sub(ui::display_width(&left) + ui::display_width(&since));
    if fill < 3 {
        let room = width.saturating_sub(ui::display_width(&since) + 3);
        let cut = ui::truncate(&ui::strip_ansi(&left), room);
        return format!("{cut}   {since}");
    }
    format!("{left}{}{since}", " ".repeat(fill))
}

// ---------------------------------------------------------- request card

/// Effects of everything a request would run, merged into one.
fn request_effects(req: &ApprovalRequest) -> Option<Effects> {
    // Each analysis with the command it came from, so variables expand.
    let analyses: Vec<(&str, &crate::services::ai::types::Analysis)> = match &req.subject {
        ApprovalSubject::Call { call, analysis } => analysis
            .iter()
            .map(|a| (call.command.as_deref().unwrap_or(""), a))
            .collect(),
        ApprovalSubject::Plan { plan, analyses, .. } => plan
            .steps
            .iter()
            .zip(analyses)
            .filter_map(|(step, a)| {
                let command = match &step.action {
                    crate::services::ai::types::StepAction::Run { command } => command.as_str(),
                    _ => "",
                };
                a.as_ref().map(|a| (command, a))
            })
            .collect(),
        ApprovalSubject::Access { .. } => return None,
    };
    // File edits have no command to parse, but they are what a change does too.
    let edits: Vec<String> = match &req.subject {
        ApprovalSubject::Call { call, .. }
            if call.tool == crate::services::ai::types::ToolName::EditFile =>
        {
            call.path.iter().cloned().collect()
        }
        ApprovalSubject::Plan { plan, .. } => plan
            .steps
            .iter()
            .filter_map(|s| match &s.action {
                crate::services::ai::types::StepAction::EditFile { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    if analyses.is_empty() && edits.is_empty() {
        return None;
    }
    let mut all = Effects::default();
    if !edits.is_empty() {
        all.items.push(effects::Effect {
            verb: effects::Verb::Edits,
            targets: edits,
            dest: None,
        });
    }
    for (command, a) in analyses {
        let e = effects::effects_for(command, a);
        all.items.extend(e.items);
        all.deletes |= e.deletes;
        all.sudo |= e.sudo;
        all.network |= e.network;
        all.not_understood = all.not_understood.or(e.not_understood);
    }
    Some(all)
}

/// The compact card for an approval request: the waiting line, what it does, its size and
/// expiry, and (with `keys`) the keys that answer it.
pub fn request_lines(
    style: &Style,
    req: &ApprovalRequest,
    ctx: &mut FeedContext,
    now: DateTime<Utc>,
    keys: bool,
) -> Vec<String> {
    ctx.note_request(req);
    let time = ui::clock(&req.created_at);
    let (kind, host, text, size) = match &req.subject {
        ApprovalSubject::Call { call, .. } => (
            kind_word(call.tool),
            call.host.clone().unwrap_or_default(),
            call_text(call),
            "1 call".to_string(),
        ),
        ApprovalSubject::Plan { plan, .. } => (
            "change plan",
            plan_hosts(plan).join(", "),
            oneline(&plan.title),
            format!(
                "{} · {} · {}",
                count(plan.steps.len(), "step", "steps"),
                count(plan.verify.len(), "check", "checks"),
                count(plan.rollback.len(), "undo", "undos"),
            ),
        ),
        ApprovalSubject::Access {
            reason, duration, ..
        } => (
            "access",
            String::new(),
            format!(
                "wider access for {}: {}",
                oneline(duration),
                oneline(reason)
            ),
            "scope widens for a while".to_string(),
        ),
    };
    let waiting = if req.tier == Tier::TwoPerson {
        "waiting for two of you"
    } else {
        "waiting for you"
    };
    let width = ctx.width;
    let mut out = vec![FeedLine::new(&time, Glyph::Ask, kind)
        .host(Some(&host))
        .text(text)
        .meta(Tone::Warn, waiting)
        .render(style, width)];
    let indent = " ".repeat(CLOCK_W + 2);
    if let Some(e) = request_effects(req) {
        out.push(format!(
            "{indent}{}",
            ui::truncate(&effects::summary(&e), width.saturating_sub(indent.len()))
        ));
    }
    let expires = format!("expires in {}", ui::expires_in(&req.expires_at, now));
    let fill = width.saturating_sub(indent.len() + ui::display_width(&size) + expires.len());
    out.push(format!(
        "{indent}{size}{}{}",
        " ".repeat(fill.max(2)),
        style.paint(Tone::Dim, &expires)
    ));
    if keys {
        out.push(format!(
            "{indent}a approve    d deny    {} open the full card",
            if style.unicode { "⏎" } else { "enter" }
        ));
    }
    out
}
