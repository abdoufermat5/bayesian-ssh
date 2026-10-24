//! The feed: one aligned line per event, for `bssh ai watch` now and `bssh ai log <run>` later.
//!
//! Everything here is a pure function over [`AiEvent`] (no IO, no clock): the caller passes
//! the time an event happened, and a [`FeedContext`] remembers what later lines need (which
//! call a result belongs to, which request an approval answers, counts for the run-ended line).
//!
//! ```text
//! 17:23:42  ✓ read          web-1   du -xah /var/www | sort -rh | head -40
//!           … 22 more reads, all allowed
//! 17:23:58  ✗ read          web-1   touch /var/tmp/.bssh_write_test    denied  writes in a read-only call
//! ```

use super::effects::{self, Effects};
use super::types::{
    AiEvent, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Decision, Tier, ToolCall, ToolName,
};
use super::ui::{self, Glyph, Style, Tone};
use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap};

/// Width of the kind column, host column (minimum) and the clock.
const KIND_W: usize = 14;
const HOST_W: usize = 8;
const CLOCK_W: usize = 8;
/// Longest run of allowed reads shown before the rest fold.
const SHOWN_READS: usize = 3;

// ------------------------------------------------------------------ words

/// The kind of a call in words.
pub fn kind_word(tool: ToolName) -> &'static str {
    match tool {
        ToolName::ListHosts => "list hosts",
        ToolName::HostInfo => "host info",
        ToolName::Status => "status",
        ToolName::Observe => "observe",
        ToolName::ReadFile | ToolName::SearchLogs | ToolName::RunReadonly | ToolName::RunMany => {
            "read"
        }
        ToolName::Run => "run",
        ToolName::Service => "service",
        ToolName::EditFile => "edit",
        ToolName::ProposeChange => "change plan",
        ToolName::ApplyChange => "apply",
        ToolName::Undo => "undo",
        ToolName::RequestAccess => "access",
    }
}

/// Reads: what the folder may hide when allowed.
fn is_read(tool: ToolName) -> bool {
    matches!(
        tool,
        ToolName::Observe
            | ToolName::ReadFile
            | ToolName::SearchLogs
            | ToolName::RunReadonly
            | ToolName::RunMany
    )
}

/// One line of plain text: control characters (an agent can put escape codes in a command or
/// a title) and runs of white space become single spaces.
fn oneline(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a call is about, as a person would say it.
fn call_text(call: &ToolCall) -> String {
    let arg = |k: &str| call.args.get(k).and_then(|v| v.as_str());
    let text = if let Some(c) = &call.command {
        c.clone()
    } else if let Some(p) = &call.path {
        p.clone()
    } else if call.tool == ToolName::Service {
        match (arg("action"), arg("unit")) {
            (Some(a), Some(u)) => format!("{a} {u}"),
            (None, Some(u)) => u.to_string(),
            _ => String::new(),
        }
    } else if let Some(t) = arg("title") {
        t.to_string()
    } else if let Some(c) = arg("change_id") {
        format!("change {}", ui::short_id(c))
    } else {
        String::new()
    };
    oneline(&text)
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---------------------------------------------------------------- context

#[derive(Debug, Clone)]
struct CallInfo {
    tool: ToolName,
    host: Option<String>,
    text: String,
}

#[derive(Debug, Clone, Default)]
struct RunStats {
    client: String,
    calls: usize,
    denied: usize,
    changes: usize,
}

/// What later lines need to know about earlier events.
#[derive(Debug, Clone)]
pub struct FeedContext {
    /// Columns a line may use.
    pub width: usize,
    calls: HashMap<String, CallInfo>,
    /// request id -> "plan 486c2f5a" / "request 440947ca"
    requests: HashMap<String, String>,
    /// plan hash -> hosts of its steps
    plan_hosts: HashMap<String, String>,
    runs: HashMap<String, RunStats>,
    /// Allowed reads waiting for their result, which brings duration and jail.
    held: Vec<(String, FeedLine, DateTime<Utc>)>,
    /// The line just returned was an allowed read (for the folder).
    last_read: bool,
    /// Give out-of-band asks a line of their own. `watch` shows the approval card instead;
    /// a log has no cards.
    pub show_asks: bool,
}

impl Default for FeedContext {
    fn default() -> Self {
        Self::with_width(ui::width())
    }
}

impl FeedContext {
    /// Whether the line just returned by `event_feed` was an allowed read.
    pub fn last_was_read(&self) -> bool {
        self.last_read
    }

    /// Reads still waiting for their result after `max_age` (a failed call never sends
    /// one), released without timing so no call goes unshown.
    pub fn take_held(&mut self, now: DateTime<Utc>, max_age: chrono::Duration) -> Vec<FeedLine> {
        let (old, keep) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|(_, _, at)| now - *at >= max_age);
        self.held = keep;
        old.into_iter().map(|(_, l, _)| l).collect()
    }
}

impl FeedContext {
    pub fn with_width(width: usize) -> Self {
        Self {
            width,
            calls: HashMap::new(),
            requests: HashMap::new(),
            plan_hosts: HashMap::new(),
            runs: HashMap::new(),
            held: Vec::new(),
            last_read: false,
            show_asks: false,
        }
    }

    /// Remember a run's client name (from the control socket's hello or `RunStarted`).
    pub fn note_run(&mut self, run_id: &str, client: &str) {
        self.runs.entry(run_id.to_string()).or_default().client = oneline(client);
    }

    /// Remember an approval request so its outcome can name the plan.
    pub fn note_request(&mut self, req: &ApprovalRequest) {
        let label = match &req.subject {
            ApprovalSubject::Plan {
                plan_hash, plan, ..
            } => {
                self.plan_hosts
                    .insert(plan_hash.clone(), plan_hosts(plan).join(", "));
                format!("plan {}", ui::short_id(plan_hash))
            }
            _ => format!("request {}", ui::short_id(&req.request_id)),
        };
        self.requests.insert(req.request_id.clone(), label);
    }

    /// Host a call was made on.
    pub fn call_host(&self, call_id: &str) -> Option<&str> {
        self.calls.get(call_id)?.host.as_deref()
    }

    fn client(&self, run_id: &str) -> String {
        self.runs
            .get(run_id)
            .map(|r| r.client.clone())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "agent".into())
    }
}

fn plan_hosts(plan: &super::types::ChangePlan) -> Vec<String> {
    let set: BTreeSet<String> = plan
        .steps
        .iter()
        .chain(&plan.verify)
        .filter_map(|s| s.host.as_deref().map(oneline))
        .collect();
    set.into_iter().collect()
}

// ------------------------------------------------------------------- line

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
    fn new(time: &str, mark: Glyph, kind: &str) -> Self {
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

    fn host(mut self, h: Option<&str>) -> Self {
        self.host = h.map(oneline).unwrap_or_default();
        self
    }

    fn text(mut self, t: impl Into<String>) -> Self {
        self.text = t.into();
        self
    }

    fn meta(mut self, tone: Tone, m: impl Into<String>) -> Self {
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
        | AiEvent::CircuitBreaker { run_id, .. }
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
                Some(
                    FeedLine::new(&time, Glyph::Bad, "not applied")
                        .host(host.as_deref())
                        .text(format!("change {id}")),
                )
            }
        }
        AiEvent::CircuitBreaker { reason, .. } => Some(
            FeedLine::new(&time, Glyph::Bad, "")
                .text(format!("agent paused: {}", oneline(reason)))
                .meta(Tone::Bad, "circuit breaker"),
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

// ----------------------------------------------------------------- header

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
    let analyses: Vec<(&str, &super::types::Analysis)> = match &req.subject {
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
                    super::types::StepAction::Run { command } => command.as_str(),
                    _ => "",
                };
                a.as_ref().map(|a| (command, a))
            })
            .collect(),
        ApprovalSubject::Access { .. } => return None,
    };
    if analyses.is_empty() {
        return None;
    }
    let mut all = Effects::default();
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

// -------------------------------------------------------------- folding

/// Folds runs of more than three consecutive allowed reads into
/// `… N more reads, all allowed`. Denials, requests and changes never fold.
#[derive(Debug, Clone)]
pub struct Folder {
    pub enabled: bool,
    streak: usize,
    hidden: usize,
}

impl Default for Folder {
    fn default() -> Self {
        Self {
            enabled: true,
            streak: 0,
            hidden: 0,
        }
    }
}

impl Folder {
    /// Turn folding on or off; returns the new state.
    pub fn toggle(&mut self) -> bool {
        self.enabled = !self.enabled;
        self.enabled
    }

    fn fold_line(&mut self, style: &Style) -> Option<String> {
        if self.hidden == 0 {
            return None;
        }
        let n = std::mem::take(&mut self.hidden);
        let indent = " ".repeat(CLOCK_W + 2);
        Some(format!(
            "{indent}{}",
            style.paint(
                Tone::Dim,
                &format!(
                    "{} {n} more {}, all allowed",
                    style.glyph(Glyph::Ellipsis),
                    if n == 1 { "read" } else { "reads" }
                )
            )
        ))
    }

    /// The lines to print for a rendered `line`; `allowed_read` comes from
    /// `FeedContext::last_was_read`. A folded read yields nothing; the
    /// next line of another kind yields the fold note first.
    pub fn push(&mut self, style: &Style, allowed_read: bool, line: Option<String>) -> Vec<String> {
        let mut out = Vec::new();
        if allowed_read {
            self.streak += 1;
            if self.enabled && self.streak > SHOWN_READS {
                self.hidden += 1;
                return out;
            }
        } else if line.is_some() {
            self.streak = 0;
            out.extend(self.fold_line(style));
        }
        out.extend(line);
        out
    }

    /// Print the pending fold note now (the feed went quiet); later reads of the same streak
    /// fold into a new note.
    pub fn flush(&mut self, style: &Style) -> Option<String> {
        self.fold_line(style)
    }
}

// ----------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::{
        Analysis, ChangePlan, Redaction, RiskTag, SimpleCommand, Step, StepAction,
    };
    use chrono::TimeZone;

    fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        // Built from local time so the formatted clock is the same in every timezone.
        chrono::Local
            .with_ymd_and_hms(2026, 10, 24, h, m, s)
            .unwrap()
            .with_timezone(&Utc)
    }

    const RUN: &str = "dddbee19-f151-4ee0-8473-93924f35e1d4";

    fn call(id: &str, tool: ToolName, host: Option<&str>, cmd: Option<&str>) -> ToolCall {
        ToolCall {
            run_id: RUN.into(),
            call_id: id.into(),
            tool,
            host: host.map(Into::into),
            command: cmd.map(Into::into),
            path: None,
            args: serde_json::Value::Null,
        }
    }

    fn allow(c: ToolCall) -> AiEvent {
        AiEvent::Call {
            call: c,
            decision: Decision::Allow {
                reason: "read-only".into(),
            },
        }
    }

    fn deny(c: ToolCall, why: &str) -> AiEvent {
        AiEvent::Call {
            call: c,
            decision: Decision::Deny { reason: why.into() },
        }
    }

    fn ctx() -> FeedContext {
        FeedContext::with_width(78)
    }

    fn line(ev: &AiEvent, ctx: &mut FeedContext, now: DateTime<Utc>) -> Option<String> {
        event_line(&Style::plain(), ev, ctx, now)
    }

    /// An allowed read and its result, as the feed sees them: the line comes with the result.
    fn settle(ev: &AiEvent, c: &mut FeedContext, t: DateTime<Utc>) -> Option<String> {
        let first = line(ev, c, t);
        let AiEvent::Call { call, .. } = ev else {
            return first;
        };
        if first.is_some() {
            return first;
        }
        let res = AiEvent::Result {
            run_id: call.run_id.clone(),
            call_id: call.call_id.clone(),
            exit_code: Some(0),
            output_sha256: String::new(),
            redactions: Vec::new(),
            duration_ms: Some(180),
            jailed: Some(true),
        };
        line(&res, c, t)
    }

    #[test]
    fn allowed_read_waits_for_its_result_and_shows_its_timing() {
        let mut c = ctx();
        let ev = allow(call(
            "c1",
            ToolName::RunReadonly,
            Some("web-1"),
            Some("df -h /var/www"),
        ));
        assert_eq!(line(&ev, &mut c, at(17, 23, 42)), None);
        let res = AiEvent::Result {
            duration_ms: Some(180),
            jailed: Some(true),
            run_id: RUN.into(),
            call_id: "c1".into(),
            exit_code: Some(0),
            output_sha256: String::new(),
            redactions: vec![],
        };
        let l = line(&res, &mut c, at(17, 23, 43)).unwrap();
        assert!(
            l.starts_with("17:23:42  ✓ read          web-1   df -h /var/www")
                && l.ends_with("0.2 s · jail"),
            "{l}"
        );
        assert!(c.last_was_read());
    }

    #[test]
    fn failed_result_names_its_call() {
        let mut c = ctx();
        line(
            &allow(call(
                "c1",
                ToolName::RunReadonly,
                Some("web-1"),
                Some("ls /nope"),
            )),
            &mut c,
            at(17, 0, 0),
        );
        let res = AiEvent::Result {
            duration_ms: None,
            jailed: None,
            run_id: RUN.into(),
            call_id: "c1".into(),
            exit_code: Some(2),
            output_sha256: String::new(),
            redactions: vec![Redaction {
                kind: "x".into(),
                count: 1,
            }],
        };
        let l = line(&res, &mut c, at(17, 0, 1)).unwrap();
        assert!(
            l.contains("✗ read") && l.contains("web-1") && l.ends_with("exit 2 · 1 redacted"),
            "{l}"
        );
        assert_eq!(l.chars().count(), 78);
    }

    #[test]
    fn denial_says_why_on_the_right() {
        let mut c = FeedContext::with_width(100);
        let ev = deny(
            call(
                "c2",
                ToolName::RunReadonly,
                Some("web-1"),
                Some("touch /var/tmp/x"),
            ),
            "writes in a read-only call.",
        );
        let l = line(&ev, &mut c, at(17, 23, 58)).unwrap();
        assert_eq!(
            l,
            "17:23:58  ✗ read          web-1   touch /var/tmp/x                denied  writes in a read-only call"
        );
    }

    #[test]
    fn list_hosts_has_no_host_column() {
        let mut c = ctx();
        let l = line(
            &allow(call("c0", ToolName::ListHosts, None, None)),
            &mut c,
            at(17, 23, 33),
        )
        .unwrap();
        assert_eq!(l, "17:23:33  ✓ list hosts    ");
    }

    fn plan_request() -> ApprovalRequest {
        let step = |id: &str, cmd: &str| Step {
            id: id.into(),
            host: Some("web-1".into()),
            action: StepAction::Run {
                command: cmd.into(),
            },
        };
        let analysis = Analysis {
            commands: vec![SimpleCommand {
                program: "mv".into(),
                args: vec!["/var/www/a".into(), "/var/tmp/x/".into()],
                wrappers: vec![],
                writes_to: vec![],
                tags: [RiskTag::Write].into_iter().collect(),
                rule: None,
            }],
            tags: [RiskTag::Write].into_iter().collect(),
            unknown_reason: None,
        };
        let plan = ChangePlan {
            change_id: "change-696de2f2".into(),
            run_id: RUN.into(),
            title: "Free 228 MB on web-1 /var/www".into(),
            reason: "full".into(),
            steps: vec![step("step-1", "mv /var/www/a /var/tmp/x/")],
            verify: vec![step("v1", "df"), step("v2", "test -f /var/tmp/x/a")],
            rollback: vec![step("r1", "mv /var/tmp/x/a /var/www/")],
        };
        ApprovalRequest {
            request_id: "440947ca-1111".into(),
            run_id: RUN.into(),
            client: "opencode 2.1.2".into(),
            tier: Tier::OutOfBand,
            subject: ApprovalSubject::Plan {
                plan,
                plan_hash: "486c2f5a99".into(),
                analyses: vec![Some(analysis)],
            },
            created_at: at(17, 24, 31).to_rfc3339(),
            expires_at: at(17, 29, 12).to_rfc3339(),
            reason: String::new(),
        }
    }

    #[test]
    fn plan_request_card_then_approval_and_change() {
        let mut c = ctx();
        let style = Style::plain();
        let card = request_lines(&style, &plan_request(), &mut c, at(17, 24, 31), true);
        assert_eq!(card.len(), 4, "{card:?}");
        assert!(
            card[0].starts_with("17:24:31  ◆ change plan   web-1   Free 228 MB"),
            "{}",
            card[0]
        );
        assert!(card[0].ends_with("waiting for you"));
        assert!(
            card[1].contains("moves 1 path") && card[1].contains("sudo no"),
            "{}",
            card[1]
        );
        assert!(card[2].starts_with("          1 step · 2 checks · 1 undo"));
        assert!(card[2].ends_with("expires in 4:41"), "{}", card[2]);
        assert!(card[3].contains("a approve    d deny    ⏎ open the full card"));
        let quiet = request_lines(&style, &plan_request(), &mut c, at(17, 24, 31), false);
        assert_eq!(quiet.len(), 3);

        let appr = AiEvent::Approval {
            request_id: "440947ca-1111".into(),
            run_id: RUN.into(),
            outcome: ApprovalOutcome::Approved {
                by: "ana".into(),
                channel: "cli".into(),
                grant: None,
            },
        };
        assert_eq!(
            line(&appr, &mut c, at(17, 24, 44)).unwrap(),
            "17:24:44  ✓ approved by ana                                      plan 486c2f5a"
        );
        c.width = 100;
        let chg = AiEvent::Change {
            run_id: RUN.into(),
            change_id: "change-696de2f2".into(),
            plan_hash: "486c2f5a99".into(),
            applied: true,
            undo_point: Some("u".into()),
        };
        assert_eq!(
            line(&chg, &mut c, at(17, 24, 47)).unwrap(),
            "17:24:47  ✓ applied       web-1   change 696de2f2                    undo with bssh ai undo 696de2f2"
        );
    }

    #[test]
    fn denied_approval_and_expiry() {
        let mut c = ctx();
        let d = AiEvent::Approval {
            request_id: "abcdef012345".into(),
            run_id: RUN.into(),
            outcome: ApprovalOutcome::Denied {
                by: "ana".into(),
                reason: Some("too broad".into()),
            },
        };
        let l = line(&d, &mut c, at(1, 2, 3)).unwrap();
        assert!(
            l.contains("✗ denied by ana: too broad") && l.ends_with("request abcdef01"),
            "{l}"
        );
        let e = AiEvent::Approval {
            request_id: "abcdef012345".into(),
            run_id: RUN.into(),
            outcome: ApprovalOutcome::Expired,
        };
        assert!(line(&e, &mut c, at(1, 2, 4))
            .unwrap()
            .contains("request expired"));
    }

    #[test]
    fn run_ended_counts_calls_denials_and_changes() {
        let mut c = FeedContext::with_width(110);
        c.note_run(RUN, "opencode");
        line(
            &allow(call("a", ToolName::RunReadonly, Some("h"), Some("ls"))),
            &mut c,
            at(1, 0, 0),
        );
        line(
            &deny(call("b", ToolName::Run, Some("h"), Some("rm x")), "no"),
            &mut c,
            at(1, 0, 1),
        );
        line(
            &AiEvent::Change {
                run_id: RUN.into(),
                change_id: "c".into(),
                plan_hash: "h".into(),
                applied: true,
                undo_point: None,
            },
            &mut c,
            at(1, 0, 2),
        );
        let l = line(
            &AiEvent::RunEnded {
                run_id: RUN.into(),
                reason: "client disconnected".into(),
            },
            &mut c,
            at(17, 25, 10),
        )
        .unwrap();
        assert_eq!(
            l,
            "17:25:10  ○ opencode disconnected · 2 calls · 1 denied · 1 change        report: bssh ai log dddbee19 --report"
        );
    }

    #[test]
    fn header_line_has_the_run_facts_and_since_on_the_right() {
        let h = RunHeader {
            client: "opencode 2.1.2".into(),
            run_id: "dddbee19-f151".into(),
            policy: "default".into(),
            on_behalf_of: "ana".into(),
            hosts: vec!["web-1".into()],
            started_at: at(17, 23, 20).to_rfc3339(),
        };
        let l = run_header(&Style::plain(), &h, 100);
        assert!(l.starts_with(
            "● opencode 2.1.2   run dddbee19   policy default   on behalf of ana   hosts web-1"
        ));
        assert!(l.ends_with("since 17:23:20"));
        assert_eq!(l.chars().count(), 100);
    }

    #[test]
    fn long_commands_truncate_to_the_width_and_escape_codes_are_dropped() {
        let mut c = FeedContext::with_width(60);
        let cmd = format!("echo \x1b[31m{}", "x".repeat(200));
        let l = line(
            &allow(call("c", ToolName::Run, Some("web-1"), Some(&cmd))),
            &mut c,
            at(1, 2, 3),
        )
        .unwrap();
        assert_eq!(ui::display_width(&l), 60, "{l}");
        assert!(l.ends_with('…') && !l.contains('\x1b'), "{l}");
    }

    #[test]
    fn a_long_meta_is_cut_to_the_line() {
        let mut c = FeedContext::with_width(80);
        let ev = deny(
            call(
                "c",
                ToolName::RunReadonly,
                Some("web-1"),
                Some("touch /var/tmp/x"),
            ),
            &"this host has no read-only jail and the command is not read-only ".repeat(3),
        );
        let l = line(&ev, &mut c, at(1, 0, 0)).unwrap();
        assert!(ui::display_width(&l) <= 80 && l.ends_with('…'), "{l}");
    }

    #[test]
    fn meta_wins_over_text_when_both_do_not_fit() {
        let mut c = FeedContext::with_width(80);
        let ev = deny(
            call("c", ToolName::Run, Some("web-1"), Some(&"y".repeat(100))),
            "writes in a read-only call",
        );
        let l = line(&ev, &mut c, at(1, 2, 3)).unwrap();
        assert!(
            l.ends_with("denied  writes in a read-only call") && ui::display_width(&l) <= 80,
            "{l}"
        );
    }

    #[test]
    fn folder_hides_the_fourth_read_on_and_notes_how_many() {
        let style = Style::plain();
        let mut c = ctx();
        let mut f = Folder::default();
        let mut shown = Vec::new();
        for i in 0..25 {
            let ev = allow(call(
                &format!("c{i}"),
                ToolName::RunReadonly,
                Some("web-1"),
                Some("ls"),
            ));
            let l = settle(&ev, &mut c, at(1, 0, i));
            shown.extend(f.push(&style, c.last_was_read(), l));
        }
        assert_eq!(shown.len(), 3);
        let d = deny(
            call("d", ToolName::RunReadonly, Some("web-1"), Some("touch x")),
            "writes",
        );
        let l = line(&d, &mut c, at(1, 1, 0));
        let out = f.push(&style, c.last_was_read(), l);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], "          … 22 more reads, all allowed");
        assert!(out[1].contains("✗ read"));
        // A new streak starts over.
        let ev = allow(call("e", ToolName::RunReadonly, Some("web-1"), Some("ls")));
        let l = settle(&ev, &mut c, at(1, 2, 0));
        assert_eq!(f.push(&style, c.last_was_read(), l).len(), 1);
    }

    #[test]
    fn three_reads_do_not_fold_and_toggling_shows_everything() {
        let style = Style::plain();
        let mut c = ctx();
        let mut f = Folder::default();
        let mut n = 0;
        for i in 0..3 {
            let ev = allow(call(&format!("c{i}"), ToolName::ReadFile, Some("h"), None));
            let l = settle(&ev, &mut c, at(1, 0, i));
            n += f.push(&style, c.last_was_read(), l).len();
        }
        assert_eq!(n, 3);
        assert!(!f.toggle());
        let mut n = 0;
        for i in 0..10 {
            let ev = allow(call(&format!("d{i}"), ToolName::ReadFile, Some("h"), None));
            let l = settle(&ev, &mut c, at(1, 0, i));
            n += f.push(&style, c.last_was_read(), l).len();
        }
        assert_eq!(n, 10);
    }

    #[test]
    fn flush_prints_the_note_once() {
        let style = Style::plain();
        let mut c = ctx();
        let mut f = Folder::default();
        for i in 0..5 {
            let ev = allow(call(&format!("c{i}"), ToolName::Observe, Some("h"), None));
            let l = settle(&ev, &mut c, at(1, 0, i));
            f.push(&style, c.last_was_read(), l);
        }
        assert_eq!(
            f.flush(&style).unwrap(),
            "          … 2 more reads, all allowed"
        );
        assert_eq!(f.flush(&style), None);
    }

    #[test]
    fn color_marks_the_verdict_and_keeps_the_layout() {
        let color = Style {
            color: true,
            unicode: true,
        };
        let mut c = ctx();
        let ev = deny(call("c", ToolName::Run, Some("h"), Some("rm x")), "no");
        let l = event_line(&color, &ev, &mut c, at(1, 2, 3)).unwrap();
        assert!(
            l.contains("\x1b[31m✗\x1b[0m") && l.contains("\x1b[31mdenied  no\x1b[0m"),
            "{l:?}"
        );
        assert_eq!(ui::display_width(&l), 78);
    }

    #[test]
    fn ascii_fallback_has_no_unicode_marks() {
        let ascii = Style {
            color: false,
            unicode: false,
        };
        let mut c = ctx();
        let l = event_line(
            &ascii,
            &allow(call("c", ToolName::Run, Some("h"), Some("ls"))),
            &mut c,
            at(1, 2, 3),
        )
        .unwrap();
        assert!(l.contains("+ run"), "{l}");
    }
}
