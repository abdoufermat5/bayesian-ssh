//! One feed line per event, before and after layout.

use crate::services::ai::types::{AiEvent, ApprovalOutcome, Decision, Tier};
use crate::services::ai::ui::{self, Glyph, Style, Tone};
use chrono::{DateTime, Utc};

use super::context::{CallInfo, FeedContext};
use super::words::{call_text, count, is_read, kind_word, oneline, CLOCK_W, HOST_W, KIND_W};

/// One line before it is laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedLine {
    /// Local clock time.
    pub time: String,
    pub mark: Glyph,
    pub kind: String,
    pub host: String,
    pub text: String,
    /// Right-aligned.
    pub meta: String,
    pub meta_tone: Tone,
}

impl FeedLine {
    pub(super) fn new(time: &str, mark: Glyph, kind: &str) -> Self {
        Self {
            time: time.to_string(),
            mark,
            kind: kind.to_string(),
            host: String::new(),
            text: String::new(),
            meta: String::new(),
            meta_tone: Tone::Dim,
        }
    }

    pub(super) fn host(mut self, h: Option<&str>) -> Self {
        self.host = h.map(oneline).unwrap_or_default();
        self
    }

    pub(super) fn text(mut self, t: impl Into<String>) -> Self {
        self.text = t.into();
        self
    }

    pub(super) fn meta(mut self, tone: Tone, m: impl Into<String>) -> Self {
        self.meta_tone = tone;
        self.meta = m.into();
        self
    }

    /// Lay out in `width` columns; the text gives way to the meta.
    pub fn render(&self, style: &Style, width: usize) -> String {
        let mut left = format!(
            "{}  {} ",
            style.paint(Tone::Dim, &ui::pad(&self.time, CLOCK_W)),
            style.mark(self.mark)
        );
        let mut used = CLOCK_W + 2 + 2;
        if !self.kind.is_empty() {
            left.push_str(&ui::pad(&self.kind, KIND_W));
            used += KIND_W;
        }
        if !self.host.is_empty() {
            let w = HOST_W.max(ui::display_width(&self.host) + 2);
            left.push_str(&ui::pad(&self.host, w));
            used += w;
        }
        // The meta wins over the text but never runs past the line. A short meta may squeeze
        // the text to 8 columns; a long one (a denial's reason) leaves the command its share.
        let keep = ui::display_width(&self.text).clamp(8, 30).min(width / 5);
        let meta_max = if ui::display_width(&self.meta) <= 40 {
            width.saturating_sub(used + 8 + 2)
        } else {
            width.saturating_sub(used + keep + 2)
        }
        .max(10);
        let meta = if ui::display_width(&self.meta) > meta_max {
            ui::truncate(&self.meta, meta_max)
        } else {
            self.meta.clone()
        };
        let meta_w = ui::display_width(&meta);
        let gap = if meta_w > 0 { 2 } else { 0 };
        let avail = width.saturating_sub(used + meta_w + gap).max(8);
        let text = ui::truncate(&self.text, avail);
        let text_w = ui::display_width(&text);
        left.push_str(&text);
        used += text_w;
        if meta_w == 0 {
            return left;
        }
        let fill = width.saturating_sub(used + meta_w).max(gap);
        format!(
            "{left}{}{}",
            " ".repeat(fill),
            style.paint(self.meta_tone, &meta)
        )
    }
}

fn clock_of(now: DateTime<Utc>) -> String {
    ui::clock(&now.to_rfc3339())
}

/// The run an event belongs to.
pub fn event_run_id(ev: &AiEvent) -> &str {
    match ev {
        AiEvent::RunStarted { run_id, .. }
        | AiEvent::Approval { run_id, .. }
        | AiEvent::Result { run_id, .. }
        | AiEvent::Change { run_id, .. }
        | AiEvent::Paused { run_id, .. }
        | AiEvent::Resumed { run_id, .. }
        | AiEvent::RunEnded { run_id, .. } => run_id,
        AiEvent::Call { call, .. } => &call.run_id,
    }
}

/// The line for an event, or `None` when it adds nothing (a successful result, a run start).
/// Updates `ctx` as it goes, so feed events in order.
pub fn event_feed(ev: &AiEvent, ctx: &mut FeedContext, now: DateTime<Utc>) -> Option<FeedLine> {
    let time = clock_of(now);
    ctx.last_read = false;
    match ev {
        AiEvent::RunStarted { run_id, client, .. } => {
            ctx.note_run(run_id, client);
            None
        }
        AiEvent::Call { call, decision } => {
            let text = call_text(call);
            ctx.calls.insert(
                call.call_id.clone(),
                CallInfo {
                    tool: call.tool,
                    host: call.host.as_deref().map(oneline),
                    text: text.clone(),
                },
            );
            let stats = ctx.runs.entry(call.run_id.clone()).or_default();
            stats.calls += 1;
            let line = |g| FeedLine::new(&time, g, kind_word(call.tool));
            match decision {
                Decision::Allow { reason } => {
                    let l = line(Glyph::Ok).host(call.host.as_deref()).text(text);
                    if reason.starts_with("step of approved plan") {
                        Some(l.meta(Tone::Dim, "approved plan"))
                    } else if is_read(call.tool) {
                        ctx.held.push((call.call_id.clone(), l, now));
                        None
                    } else {
                        Some(l)
                    }
                }
                Decision::Deny { reason } => {
                    stats.denied += 1;
                    let why = oneline(reason);
                    let why = why.trim_end_matches('.');
                    Some(
                        line(Glyph::Bad)
                            .host(call.host.as_deref())
                            .text(text)
                            .meta(Tone::Bad, format!("denied  {why}")),
                    )
                }
                // The approval request that follows is the line for an out-of-band ask.
                Decision::Ask { tier, .. } if *tier != Tier::InClient && !ctx.show_asks => None,
                Decision::Ask { tier, .. } => {
                    Some(line(Glyph::Ask).host(call.host.as_deref()).text(text).meta(
                        Tone::Warn,
                        if *tier == Tier::InClient {
                            "waiting in the agent's client"
                        } else {
                            "asked a person"
                        },
                    ))
                }
            }
        }
        AiEvent::Approval {
            request_id,
            outcome,
            ..
        } => {
            let label = ctx
                .requests
                .get(request_id)
                .cloned()
                .unwrap_or_else(|| format!("request {}", ui::short_id(request_id)));
            Some(match outcome {
                ApprovalOutcome::Approved { by, grant, .. } => {
                    let grant = if grant.is_some() {
                        " and allowed it for a while"
                    } else {
                        ""
                    };
                    FeedLine::new(&time, Glyph::Ok, "")
                        .text(format!("approved by {}{grant}", oneline(by)))
                        .meta(Tone::Dim, label)
                }
                ApprovalOutcome::Denied { by, reason } => {
                    let why = reason
                        .as_deref()
                        .map(|r| format!(": {}", oneline(r)))
                        .unwrap_or_default();
                    FeedLine::new(&time, Glyph::Bad, "")
                        .text(format!("denied by {}{why}", oneline(by)))
                        .meta(Tone::Dim, label)
                }
                ApprovalOutcome::Expired => FeedLine::new(&time, Glyph::Waiting, "")
                    .text("request expired, nothing ran")
                    .meta(Tone::Dim, label),
            })
        }
        AiEvent::Result {
            call_id,
            exit_code,
            redactions,
            duration_ms,
            jailed,
            ..
        } => {
            if let Some(i) = ctx.held.iter().position(|(id, ..)| id == call_id) {
                let (_, mut l, _) = ctx.held.remove(i);
                let mut meta: Vec<String> = Vec::new();
                if let Some(ms) = duration_ms {
                    meta.push(ui::duration_ms(*ms));
                }
                if *jailed == Some(true) {
                    meta.push("jail".into());
                }
                let hidden: usize = redactions.iter().map(|r| r.count).sum();
                if hidden > 0 {
                    meta.push(format!("{hidden} redacted"));
                }
                return Some(match exit_code {
                    Some(n) if *n != 0 => {
                        l.mark = Glyph::Bad;
                        meta.insert(0, format!("exit {n}"));
                        l.meta(Tone::Bad, meta.join(" · "))
                    }
                    _ => {
                        ctx.last_read = true;
                        l.meta(Tone::Dim, meta.join(" · "))
                    }
                });
            }
            let info = ctx.calls.get(call_id).cloned();
            let host = info.as_ref().and_then(|i| i.host.clone());
            let text = info.as_ref().map(|i| i.text.clone()).unwrap_or_default();
            let kind = info.as_ref().map_or("call", |i| kind_word(i.tool));
            match exit_code {
                Some(n) if *n != 0 => Some(
                    FeedLine::new(&time, Glyph::Bad, kind)
                        .host(host.as_deref())
                        .text(text)
                        .meta(Tone::Bad, format!("exit {n}")),
                ),
                _ => {
                    let n: usize = redactions.iter().map(|r| r.count).sum();
                    (n > 0).then(|| {
                        FeedLine::new(&time, Glyph::Ask, "redacted")
                            .host(host.as_deref())
                            .text(format!(
                                "{} hidden from the output",
                                count(n, "value", "values")
                            ))
                    })
                }
            }
        }
        AiEvent::Change {
            run_id,
            change_id,
            plan_hash,
            applied,
            undo_point,
        } => {
            let id = ui::short_id(change_id);
            let host = ctx.plan_hosts.get(plan_hash).cloned();
            if *applied {
                ctx.runs.entry(run_id.clone()).or_default().changes += 1;
                let mut l = FeedLine::new(&time, Glyph::Ok, "applied")
                    .host(host.as_deref())
                    .text(format!("change {id}"));
                if undo_point.is_some() {
                    l = l.meta(Tone::Dim, format!("undo with bssh ai undo {id}"));
                }
                Some(l)
            } else {
                // A step failed: the plan stopped there, and the steps before it ran.
                let meta = match undo_point {
                    Some(_) => format!("earlier steps stay done · undo with bssh ai undo {id}"),
                    None => "earlier steps stay done".to_string(),
                };
                Some(
                    FeedLine::new(&time, Glyph::Bad, "stopped")
                        .host(host.as_deref())
                        .text(format!("change {id} at a failed step"))
                        .meta(Tone::Bad, meta),
                )
            }
        }
        AiEvent::Paused { reason, .. } => Some(
            FeedLine::new(&time, Glyph::Bad, "")
                .text(format!("agent paused: {}", oneline(reason)))
                .meta(Tone::Bad, "circuit breaker"),
        ),
        AiEvent::Resumed { by, .. } => Some(
            FeedLine::new(&time, Glyph::Ok, "")
                .text(format!("agent resumed by {by}"))
                .meta(Tone::Ok, "circuit breaker"),
        ),
        AiEvent::RunEnded { run_id, reason } => {
            let stats = ctx.runs.get(run_id).cloned().unwrap_or_default();
            let client = ctx.client(run_id);
            let how = if reason.to_lowercase().contains("stop") {
                "stopped"
            } else {
                "disconnected"
            };
            Some(
                FeedLine::new(&time, Glyph::Idle, "")
                    .text(format!(
                        "{client} {how} · {} · {} denied · {}",
                        count(stats.calls, "call", "calls"),
                        stats.denied,
                        count(stats.changes, "change", "changes"),
                    ))
                    .meta(
                        Tone::Dim,
                        format!("report: bssh ai log {} --report", ui::short_id(run_id)),
                    ),
            )
        }
    }
}

/// [`event_feed`] laid out for a terminal.
pub fn event_line(
    style: &Style,
    ev: &AiEvent,
    ctx: &mut FeedContext,
    now: DateTime<Utc>,
) -> Option<String> {
    let width = ctx.width;
    event_feed(ev, ctx, now).map(|l| l.render(style, width))
}
