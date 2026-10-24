//! `bssh ai approve`.
//!
//! Connects to every live run socket, lists the approval requests waiting for a human, shows
//! the card (what it does as bssh read it, the commands, "the agent says") and sends the
//! decision. This is the trusted channel: the agent's process can't reach it.

use crate::cli::parser_ai::AiArgsApprove;
use crate::config::AppConfig;
use crate::services::ai::ai_dir;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::control_client::{connect_all, ControlClient, RunHello};
use crate::services::ai::effects::{self, Effect, Effects, Verb};
use crate::services::ai::types::{
    Analysis, Analyzer, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Grant, RiskTag,
    ServiceAction, Step, StepAction, Tier, ToolCall, ToolName,
};
use crate::services::ai::ui::{self, Glyph, Style, Tone};
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::io::{BufRead, IsTerminal, Write};

/// Minutes a `g` keypress allows the same kind of call.
const QUICK_GRANT_MINUTES: u32 = 15;
/// Diff lines shown on the short card.
const DIFF_LINES: usize = 12;
/// Targets per verb shown on the short card.
const TARGETS_PER_VERB: usize = 8;
/// Lines of "the agent says" on the short card.
const SAYS_LINES: usize = 3;

pub async fn execute(args: AiArgsApprove, config: AppConfig) -> Result<()> {
    let dir = ai_dir(&config);
    let mut clients = connect_all(&dir, "bssh-cli").await;
    if clients.is_empty() {
        println!("No active AI runs. Start an agent with `bssh ai mcp` first.");
        return Ok(());
    }

    // (client index, request)
    let mut all: Vec<(usize, ApprovalRequest)> = Vec::new();
    for (i, c) in clients.iter_mut().enumerate() {
        match c.pending().await {
            Ok(reqs) => all.extend(reqs.into_iter().map(|r| (i, r))),
            Err(e) => eprintln!("run {}: {e}", c.hello.run_id),
        }
    }
    if all.is_empty() {
        println!("No pending approvals.");
        return Ok(());
    }

    let style = Style::terminal();
    if args.list {
        for line in summary_lines(&style, &clients, &all, Utc::now(), ui::width()) {
            println!("{line}");
        }
        return Ok(());
    }

    let idx = match &args.request_id {
        Some(id) => find_request(&all, id)?,
        None if all.len() == 1 => 0,
        None => pick(&style, &clients, &all)?,
    };
    let (ci, req) = all[idx].clone();
    let hello = clients[ci].hello.clone();
    let can_grant = build_grant(&req, 1).is_some();

    let who = format!("{}@cli", whoami::username());
    let mut full = false;
    let choice = if args.deny {
        println!("{}", render_card(&style, &hello, &req, Utc::now(), full));
        Choice::Deny
    } else {
        'ask: loop {
            println!("{}", render_card(&style, &hello, &req, Utc::now(), full));
            loop {
                match read_choice(can_grant)? {
                    Choice::Full => {
                        full = true;
                        continue 'ask;
                    }
                    Choice::Other => continue,
                    c => break 'ask c,
                }
            }
        }
    };

    if choice == Choice::Skip {
        println!("Left pending.");
        return Ok(());
    }
    let approve = matches!(choice, Choice::Approve | Choice::Grant);

    let outcome = if approve {
        let minutes = if choice == Choice::Grant {
            Some(QUICK_GRANT_MINUTES)
        } else {
            args.grant
        };
        let grant = match minutes {
            Some(mins) => {
                let g = build_grant(&req, mins);
                if g.is_none() {
                    println!("Note: grants only apply to single calls; approving without one.");
                }
                g
            }
            None => None,
        };
        ApprovalOutcome::Approved {
            by: who.clone(),
            channel: "cli".into(),
            grant,
        }
    } else {
        ApprovalOutcome::Denied {
            by: who.clone(),
            reason: args.reason.clone(),
        }
    };
    let granted = matches!(&outcome, ApprovalOutcome::Approved { grant: Some(_), .. });
    clients[ci].decide(&req.request_id, outcome).await?;
    println!("{}", result_line(&style, &req, approve, &who, granted));
    Ok(())
}

/// What the person sees after deciding.
fn result_line(
    style: &Style,
    r: &ApprovalRequest,
    approved: bool,
    who: &str,
    granted: bool,
) -> String {
    if !approved {
        return format!(
            "{} Denied. {} is told \"denied by {who}\" and nothing ran.",
            style.mark(Glyph::Bad),
            r.client
        );
    }
    if r.tier == Tier::TwoPerson {
        return format!(
            "{} Your approval is recorded. It needs one more person: anyone else who may approve.",
            style.mark(Glyph::Ask)
        );
    }
    let mut line = match &r.subject {
        ApprovalSubject::Plan { plan_hash, .. } => format!(
            "Approved plan {} for {}. It runs now; bssh ai watch follows it.",
            ui::short_id(plan_hash),
            r.client
        ),
        ApprovalSubject::Call { .. } => format!(
            "Approved for {}. It runs now; bssh ai watch follows it.",
            r.client
        ),
        ApprovalSubject::Access { duration, .. } => format!(
            "Access granted to {} for {}.",
            r.client,
            human_duration(duration)
        ),
    };
    if granted {
        line.push_str(" The same kind of call is allowed for a while.");
    }
    format!("{} {line}", style.mark(Glyph::Ok))
}

/// Find a request by exact id or unique prefix.
fn find_request(all: &[(usize, ApprovalRequest)], id: &str) -> Result<usize> {
    if let Some(i) = all.iter().position(|(_, r)| r.request_id == id) {
        return Ok(i);
    }
    let hits: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, (_, r))| r.request_id.starts_with(id))
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => bail!("no pending request '{id}'"),
        _ => bail!("'{id}' matches several requests; give more characters"),
    }
}

fn pick(
    style: &Style,
    clients: &[ControlClient],
    all: &[(usize, ApprovalRequest)],
) -> Result<usize> {
    let lines = summary_lines(
        style,
        clients,
        all,
        Utc::now(),
        ui::width().saturating_sub(4),
    );
    for (n, line) in lines.iter().enumerate() {
        println!("{:>2}  {line}", n + 1);
    }
    print!("Which one? [1-{}] ", all.len());
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    match line.trim().parse::<usize>() {
        Ok(n) if (1..=all.len()).contains(&n) => Ok(n - 1),
        _ => bail!("not a valid choice"),
    }
}

// ---------------------------------------------------------------- prompt

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Approve,
    Deny,
    /// Leave it pending (`s`, Ctrl-C, end of input).
    Skip,
    /// Show everything, then ask again.
    Full,
    /// Approve and allow the same kind of call for a while.
    Grant,
    /// Any other key: nothing happens.
    Other,
}

fn choice_for(word: &str, can_grant: bool) -> Choice {
    match word.trim().to_lowercase().as_str() {
        "a" | "approve" => Choice::Approve,
        "d" | "deny" => Choice::Deny,
        "s" | "skip" => Choice::Skip,
        "v" | "full" => Choice::Full,
        "g" | "grant" if can_grant => Choice::Grant,
        _ => Choice::Other,
    }
}

/// One key in raw mode on a terminal; a line of text when stdin is a pipe.
fn read_choice(can_grant: bool) -> Result<Choice> {
    if !std::io::stdin().is_terminal() {
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line)? == 0 {
            return Ok(Choice::Skip);
        }
        return Ok(choice_for(&line, can_grant));
    }
    struct RawMode;
    impl Drop for RawMode {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    crossterm::terminal::enable_raw_mode()?;
    let _raw = RawMode;
    loop {
        let Event::Key(k) = event::read()? else {
            continue;
        };
        if k.kind == KeyEventKind::Release {
            continue;
        }
        return Ok(match k.code {
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => Choice::Skip,
            KeyCode::Char(c) => choice_for(&c.to_string(), can_grant),
            _ => Choice::Other,
        });
    }
}

// ----------------------------------------------------------------- lines

/// `4:41` as "expires in 4:41"; "expired" once past.
fn expiry_text(r: &ApprovalRequest, now: DateTime<Utc>) -> String {
    match ui::expires_in(&r.expires_at, now).as_str() {
        "expired" => "expired".into(),
        left => format!("expires in {left}"),
    }
}

fn kind_word(r: &ApprovalRequest) -> &'static str {
    match &r.subject {
        ApprovalSubject::Plan { .. } => "plan",
        ApprovalSubject::Access { .. } => "access",
        ApprovalSubject::Call { call, .. } => match call.tool {
            ToolName::Run => "run",
            ToolName::Service => "service",
            ToolName::EditFile => "edit",
            ToolName::ApplyChange => "apply",
            ToolName::Undo => "undo",
            _ => "call",
        },
    }
}

fn request_host(r: &ApprovalRequest) -> String {
    match &r.subject {
        ApprovalSubject::Call { call, .. } => call.host.clone().unwrap_or_else(|| "-".into()),
        ApprovalSubject::Plan { plan, .. } => {
            let mut hosts: Vec<&str> = plan
                .steps
                .iter()
                .filter_map(|s| s.host.as_deref())
                .collect();
            hosts.dedup();
            match hosts.as_slice() {
                [] => "-".into(),
                [one] => (*one).to_string(),
                many => format!("{} hosts", many.len()),
            }
        }
        ApprovalSubject::Access { scope, .. } => match scope.hosts.as_slice() {
            [] => "-".into(),
            [one] => one.clone(),
            many => format!("{} hosts", many.len()),
        },
    }
}

/// What the request is, for one-line listings.
fn request_title(r: &ApprovalRequest) -> String {
    match &r.subject {
        ApprovalSubject::Plan { plan, .. } => plan.title.clone(),
        ApprovalSubject::Access { duration, .. } => {
            format!("Wider access for {}", human_duration(duration))
        }
        ApprovalSubject::Call { call, .. } => call
            .command
            .as_deref()
            .map(|c| c.split_whitespace().collect::<Vec<_>>().join(" "))
            .or_else(|| call.path.clone())
            .unwrap_or_else(|| call_title(call)),
    }
}

/// One aligned line per pending request: mark, short id, kind, host, title, client, expiry.
fn summary_lines(
    style: &Style,
    clients: &[ControlClient],
    all: &[(usize, ApprovalRequest)],
    now: DateTime<Utc>,
    w: usize,
) -> Vec<String> {
    let rows: Vec<[String; 6]> = all
        .iter()
        .map(|(ci, r)| {
            let client = if r.client.is_empty() {
                clients[*ci].hello.client.clone()
            } else {
                r.client.clone()
            };
            [
                ui::short_id(&r.request_id),
                kind_word(r).to_string(),
                request_host(r),
                request_title(r),
                client,
                expiry_text(r, now),
            ]
        })
        .collect();
    summary_rows(style, &rows, w)
}

fn summary_rows(style: &Style, rows: &[[String; 6]], w: usize) -> Vec<String> {
    let col = |i: usize| {
        rows.iter()
            .map(|r| ui::display_width(&r[i]))
            .max()
            .unwrap_or(0)
    };
    let (kindw, hostw, clientw, expw) = (col(1), col(2), col(4), col(5));
    let fixed = 2 + 8 + 2 + kindw + 2 + hostw + 2 + 2 + clientw + 2 + expw;
    let titlew = w.saturating_sub(fixed).max(10);
    rows.iter()
        .map(|r| {
            format!(
                "{} {}  {}  {}  {}  {}  {}",
                style.mark(Glyph::Ask),
                style.paint(Tone::Dim, &r[0]),
                style.paint(Tone::Dim, &ui::pad(&r[1], kindw)),
                ui::pad(&r[2], hostw),
                ui::pad(&ui::truncate(&r[3], titlew), titlew),
                style.paint(Tone::Dim, &ui::pad(&r[4], clientw)),
                style.paint(Tone::Warn, &r[5]),
            )
        })
        .collect()
}

/// `30m` becomes `30 min`, `2h` becomes `2 h`; anything else stays as written.
fn human_duration(d: &str) -> String {
    let digits: String = d.chars().take_while(char::is_ascii_digit).collect();
    let unit = &d[digits.len()..];
    let word = match unit {
        "s" => "s",
        "m" => "min",
        "h" => "h",
        "d" => "days",
        _ => return d.to_string(),
    };
    if digits.is_empty() {
        d.to_string()
    } else {
        format!("{digits} {word}")
    }
}

fn service_verb(action: &str) -> Option<Verb> {
    match action {
        "start" => Some(Verb::Starts),
        "stop" => Some(Verb::Stops),
        "restart" => Some(Verb::Restarts),
        "reload" => Some(Verb::Reloads),
        _ => None,
    }
}

fn action_name(a: ServiceAction) -> &'static str {
    match a {
        ServiceAction::Start => "start",
        ServiceAction::Stop => "stop",
        ServiceAction::Restart => "restart",
        ServiceAction::Reload => "reload",
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A single call in words: `Restart nginx on db-1`.
fn call_title(call: &ToolCall) -> String {
    let on = call
        .host
        .as_deref()
        .map(|h| format!(" on {h}"))
        .unwrap_or_default();
    match call.tool {
        ToolName::Service => {
            let unit = call.args.get("unit").and_then(|v| v.as_str());
            let action = call.args.get("action").and_then(|v| v.as_str());
            match (action, unit) {
                (Some(a), Some(u)) => format!("{} {u}{on}", capitalize(a)),
                _ => format!("Change a service{on}"),
            }
        }
        ToolName::EditFile => match &call.path {
            Some(p) => format!("Edit {p}{on}"),
            None => format!("Edit a file{on}"),
        },
        ToolName::Run => format!("Run a command{on}"),
        ToolName::Undo => format!("Undo a change{on}"),
        ToolName::ApplyChange => format!("Apply a change{on}"),
        _ => format!("Run a tool{on}"),
    }
}

fn tier_words(t: Tier) -> &'static str {
    match t {
        Tier::InClient => "confirm in the agent's client",
        Tier::OutOfBand => "one approver, outside the agent",
        Tier::TwoPerson => "two different people must approve",
    }
}

// ------------------------------------------------------------------ card

/// Everything the approver needs to decide, as text.
#[allow(dead_code)]
pub(crate) fn render_request(run: &RunHello, r: &ApprovalRequest) -> String {
    render_card(&Style::terminal(), run, r, Utc::now(), false)
}

/// The approval card. `full` lifts the limits on diffs, checks and "the agent says".
pub(crate) fn render_card(
    style: &Style,
    run: &RunHello,
    r: &ApprovalRequest,
    now: DateTime<Utc>,
    full: bool,
) -> String {
    render_card_w(style, run, r, now, full, ui::width())
}

/// Left text and right text on one line, the right one against column `w`.
fn split_line(left: &str, right: &str, w: usize) -> String {
    if right.is_empty() {
        return left.to_string();
    }
    let gap = w
        .saturating_sub(ui::display_width(left) + ui::display_width(right))
        .max(2);
    format!("{left}{}{right}", " ".repeat(gap))
}

fn render_card_w(
    style: &Style,
    run: &RunHello,
    r: &ApprovalRequest,
    now: DateTime<Utc>,
    full: bool,
    w: usize,
) -> String {
    let dim = |s: &str| style.paint(Tone::Dim, s);
    let mut o: Vec<String> = Vec::new();

    o.push(split_line(
        &format!(
            "{} {}",
            style.mark(Glyph::Ask),
            style.paint(Tone::Strong, "Approval needed")
        ),
        &style.paint(Tone::Warn, &expiry_text(r, now)),
        w,
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
            what_section(style, &mut o, "what this does", &blocks, w, full);
        }
        ApprovalSubject::Call { call, analysis } => {
            let blocks = vec![call_block(call, analysis.as_ref())];
            what_section(style, &mut o, "what this does", &blocks, w, full);
        }
        ApprovalSubject::Access {
            scope, duration, ..
        } => {
            o.push(split_line(
                &format!("  {}", style.label("what this allows")),
                &dim("read by bssh, not the agent"),
                w,
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

    // The commands.
    match &r.subject {
        ApprovalSubject::Plan { plan, analyses, .. } => {
            for (i, step) in plan.steps.iter().enumerate() {
                o.push(String::new());
                let host = step.host.as_deref().unwrap_or("-");
                let tags = analyses.get(i).and_then(|a| a.as_ref()).map(|a| &a.tags);
                o.push(block_header(
                    style,
                    &format!("step {}", i + 1),
                    host,
                    step_risk(style, step, tags),
                    w,
                ));
                o.extend(rail_lines(style, &step_body(step), w, full));
            }
            for (name, steps) in [("checks", &plan.verify), ("undo", &plan.rollback)] {
                if !steps.is_empty() {
                    o.extend(side_steps(style, name, steps, w, full));
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
            o.push(block_header(style, kind_word(r), host, risk, w));
            o.extend(rail_lines(style, &call_body(call), w, full));
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
        o.extend(agent_says(style, reason, w, full));
    }

    // Keys.
    o.push(String::new());
    let key = |k: &str, what: &str| format!("{} {what}", style.paint(Tone::Strong, k));
    let mut keys = vec![
        key("a", "approve"),
        key("d", "deny"),
        key("v", if full { "short view" } else { "full text" }),
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
        None => o.push(split_line(&format!("  {base}"), &right, w)),
    }
    o.join("\n")
}

/// `  STEP 1 · web-1                       write`
fn block_header(style: &Style, label: &str, host: &str, risk: String, w: usize) -> String {
    split_line(
        &format!(
            "  {} {} {}",
            style.label(label),
            style.paint(Tone::Dim, "·"),
            style.paint(Tone::Strong, host)
        ),
        &risk,
        w,
    )
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

/// The text of one step or call, split for display.
struct Body {
    /// Shell lines, or one plain sentence for services.
    lines: Vec<String>,
    /// A unified diff for edits.
    diff: Option<String>,
}

fn step_body(step: &Step) -> Body {
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
            lines: vec![format!("service {} {unit}", action_name(*action))],
            diff: None,
        },
        StepAction::Undo { step } => Body {
            lines: vec![format!("undo {step}")],
            diff: None,
        },
    }
}

fn call_body(call: &ToolCall) -> Body {
    let arg = |k: &str| call.args.get(k).and_then(|v| v.as_str());
    match call.tool {
        ToolName::EditFile => Body {
            lines: vec![format!(
                "edits {}",
                call.path.as_deref().unwrap_or("a file")
            )],
            diff: arg("patch").map(String::from),
        },
        _ => match (&call.command, arg("unit")) {
            (Some(c), _) => Body {
                lines: effects::command_lines(c),
                diff: None,
            },
            (None, Some(unit)) => Body {
                lines: vec![format!(
                    "service {} {unit}",
                    arg("action").unwrap_or("change")
                )],
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
fn rail_lines(style: &Style, body: &Body, w: usize, full: bool) -> Vec<String> {
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
            diff_lines(style, patch, full)
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

/// `+` lines green, `-` lines red, the rest dim; capped unless `full`.
fn diff_lines(style: &Style, patch: &str, full: bool) -> Vec<String> {
    let all: Vec<&str> = patch.lines().collect();
    let shown = if full {
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

/// `CHECKS  df -h /var/www … · 4 × test      read-only`, or one rail line per command when `full`.
fn side_steps(style: &Style, name: &str, steps: &[Step], w: usize, full: bool) -> Vec<String> {
    let bodies: Vec<String> = steps.iter().flat_map(|s| step_body(s).lines).collect();
    let read_only = steps.iter().all(|s| match &s.action {
        StepAction::Run { command } => ShellAnalyzer.analyze(command).is_read_only(),
        _ => false,
    });
    let tag = style.paint(Tone::Dim, if read_only { "read-only" } else { "write" });
    let label = ui::pad(&style.label(name), 8);
    let mut out = Vec::new();
    if full {
        out.push(split_line(&format!("  {label}"), &tag, w));
        let body = Body {
            lines: bodies,
            diff: None,
        };
        out.extend(rail_lines(style, &body, w, true));
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
    out.push(split_line(
        &format!("  {label}{}", ui::truncate(&text, room)),
        &tag,
        w,
    ));
    out
}

/// `THE AGENT SAYS`, the quoted reason, the hint to read the rest.
fn agent_says(style: &Style, reason: &str, w: usize, full: bool) -> Vec<String> {
    let mut out = vec![split_line(
        &format!("  {}", style.label("the agent says")),
        &style.paint(Tone::Dim, "not checked by bssh"),
        w,
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

// --------------------------------------------------------------- effects

/// What happens on one host, as bssh read it.
struct HostBlock {
    host: String,
    effects: Effects,
    /// (path, patch) for edits.
    diffs: Vec<(String, String)>,
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

fn run_effects(analysis: Option<&Analysis>) -> Effects {
    match analysis {
        // `effects_for(command, analysis)` replaces this call once the effects package lands.
        Some(a) => effects::effects(a),
        None => Effects {
            not_understood: Some("not analyzed".into()),
            ..Default::default()
        },
    }
}

fn add_block(blocks: &mut Vec<HostBlock>, host: &str, e: Effects, diff: Option<(String, String)>) {
    let at = match blocks.iter().position(|b| b.host == host) {
        Some(i) => i,
        None => {
            blocks.push(HostBlock {
                host: host.to_string(),
                effects: Effects::default(),
                diffs: Vec::new(),
            });
            blocks.len() - 1
        }
    };
    merge(&mut blocks[at].effects, e);
    blocks[at].diffs.extend(diff);
}

fn plan_blocks<'a>(
    steps: impl Iterator<Item = (&'a Step, Option<&'a Analysis>)>,
) -> Vec<HostBlock> {
    let mut blocks = Vec::new();
    for (step, analysis) in steps {
        let host = step.host.as_deref().unwrap_or("-");
        let (e, diff) = match &step.action {
            StepAction::Run { .. } => (run_effects(analysis), None),
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
        add_block(&mut blocks, host, e, diff);
    }
    blocks
}

fn call_block(call: &ToolCall, analysis: Option<&Analysis>) -> HostBlock {
    let host = call.host.as_deref().unwrap_or("-");
    let arg = |k: &str| call.args.get(k).and_then(|v| v.as_str());
    let mut blocks = Vec::new();
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
        _ => (run_effects(analysis), None),
    };
    add_block(&mut blocks, host, e, diff);
    blocks.remove(0)
}

/// A long destination keeps its last two path components.
fn short_dest(d: &str) -> String {
    let trimmed = d.trim_end_matches(['/', '"']).trim_start_matches('"');
    if ui::display_width(trimmed) <= 28 {
        return trimmed.to_string();
    }
    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [.., a, b] if parts.len() > 2 => format!("…/{a}/{b}"),
        _ => trimmed.to_string(),
    }
}

fn what_section(
    style: &Style,
    o: &mut Vec<String>,
    label: &str,
    blocks: &[HostBlock],
    w: usize,
    full: bool,
) {
    let dim = |s: &str| style.paint(Tone::Dim, s);
    o.push(split_line(
        &format!("  {}", style.label(label)),
        &dim("read by bssh, not the agent"),
        w,
    ));
    let hostw = blocks
        .iter()
        .map(|b| ui::display_width(&b.host))
        .max()
        .unwrap_or(0)
        + 2;
    let verbw = 9;
    for b in blocks {
        let mut first = true;
        let lead = |first: &mut bool| {
            if std::mem::take(first) {
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
            for (n, (target, dest)) in rows.iter().take(shown).enumerate() {
                let verb = if n == 0 { v.word() } else { "" };
                let mut line = format!(
                    "  {}{}",
                    lead(&mut first),
                    style.paint(tone, &ui::pad(verb, verbw))
                );
                match dest {
                    Some(d) => {
                        let _ = std::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "{} {} {}",
                                ui::pad(&ui::truncate(target, tw), tw),
                                style.paint(Tone::Dim, style.glyph(Glyph::Arrow)),
                                short_dest(d)
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
        for (path, patch) in &b.diffs {
            let _ = path;
            for l in diff_lines(style, patch, full) {
                o.push(format!("  {}{}{l}", " ".repeat(hostw), " ".repeat(verbw)));
            }
        }
        if let Some(why) = &b.effects.not_understood {
            o.push(format!(
                "  {}{}",
                lead(&mut first),
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
        let yes_no = |on: bool, yes_tone: Tone| {
            if on {
                style.paint(yes_tone, "yes")
            } else {
                style.paint(Tone::Ok, "no")
            }
        };
        o.push(format!(
            "  {}{}{}        {}  {}        {}  {}",
            lead(&mut first),
            style.paint(Tone::Dim, &ui::pad("deletes", verbw)),
            ui::pad(&style.paint(del_tone, &del), 7),
            style.paint(Tone::Dim, "sudo"),
            yes_no(b.effects.sudo, Tone::Warn),
            style.paint(Tone::Dim, "network"),
            yes_no(b.effects.network, Tone::Info),
        ));
    }
}

// ----------------------------------------------------------------- grant

/// A grant for the same kind of call, valid `minutes` from now. Only single calls qualify.
pub(crate) fn build_grant(r: &ApprovalRequest, minutes: u32) -> Option<Grant> {
    let ApprovalSubject::Call { call, .. } = &r.subject else {
        return None;
    };
    let target = match call.tool {
        ToolName::EditFile => call.path.clone(),
        ToolName::Service => call
            .args
            .get("unit")
            .and_then(|v| v.as_str())
            .map(String::from),
        _ => call.command.clone(),
    };
    Some(Grant {
        grant_id: uuid::Uuid::new_v4().to_string(),
        run_id: Some(r.run_id.clone()),
        tool: call.tool,
        hosts: call.host.clone().unwrap_or_else(|| "*".into()),
        target,
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(i64::from(minutes)))
            .to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::{ChangePlan, Scope};

    fn hello() -> RunHello {
        RunHello {
            run_id: "dddbee19-f151-4ee0-8473-93924f35e1d4".into(),
            client: "opencode".into(),
            policy: "default".into(),
            started_at: "2026-10-24T17:23:20Z".into(),
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-24T17:24:50Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn base(subject: ApprovalSubject) -> ApprovalRequest {
        ApprovalRequest {
            request_id: "440947ca-0000-0000-0000-000000000000".into(),
            run_id: "dddbee19-f151-4ee0-8473-93924f35e1d4".into(),
            client: "opencode 2.1.2".into(),
            tier: Tier::OutOfBand,
            subject,
            created_at: "2026-10-24T17:24:31Z".into(),
            expires_at: "2026-10-24T17:29:31Z".into(),
        }
    }

    fn call(tool: ToolName, command: Option<&str>) -> ToolCall {
        ToolCall {
            run_id: "run1".into(),
            call_id: "c1".into(),
            tool,
            host: Some("db-1".into()),
            command: command.map(String::from),
            path: None,
            args: serde_json::Value::Null,
        }
    }

    fn run_step(id: &str, command: &str) -> Step {
        Step {
            id: id.into(),
            host: Some("web-1".into()),
            action: StepAction::Run {
                command: command.into(),
            },
        }
    }

    fn demo_plan_request() -> ApprovalRequest {
        let script = "A=/var/tmp/var-www-cleanup-2026-10-24\n\
            install -d -m 0750 \"$A/releases\" \"$A/cache/assets\"\n\
            mv /var/www/releases/2026-08-14 /var/www/releases/2026-09-02 \
            /var/www/releases/2026-09-21 \"$A/releases/\"\n\
            echo 'archive complete'";
        let plan = ChangePlan {
            change_id: "change-696de2f2".into(),
            run_id: "dddbee19".into(),
            title: "Free 228 MB on web-1 /var/www (archive 3 old releases + stale bundle)".into(),
            reason: "/var/www is a 310 MB tmpfs at 93%. I will keep the newest release, move \
                the three older ones and the stale bundle into a dated folder under /var/tmp, \
                and check that every moved file arrived before anything else happens. Nothing \
                is deleted, so the move can be put back with the undo step."
                .into(),
            steps: vec![run_step("step-1", script)],
            verify: vec![
                run_step("step-2", "df -h /var/www"),
                run_step("step-3", "test -f /var/tmp/a/releases/2026-08-14"),
                run_step("step-4", "test -f /var/tmp/a/releases/2026-09-02"),
                run_step("step-5", "test -f /var/tmp/a/releases/2026-09-21"),
                run_step("step-6", "test -f /var/tmp/a/cache/assets/bundle-old.js"),
            ],
            rollback: vec![run_step(
                "step-7",
                "mv /var/tmp/a/releases/2026-08-14 /var/www/releases/",
            )],
        };
        let analyses = vec![Some(ShellAnalyzer.analyze(script))];
        let mut r = base(ApprovalSubject::Plan {
            plan_hash: "486c2f5a".repeat(8),
            plan,
            analyses,
        });
        r.client = "opencode 2.1.2".into();
        r
    }

    fn service_request(tier: Tier) -> ApprovalRequest {
        let mut c = call(
            ToolName::Service,
            Some("sudo -n systemctl restart -- nginx"),
        );
        c.args = serde_json::json!({"unit": "nginx", "action": "restart"});
        let mut r = base(ApprovalSubject::Call {
            call: c,
            analysis: Some(ShellAnalyzer.analyze("sudo -n systemctl restart -- nginx")),
        });
        r.tier = tier;
        r
    }

    fn plain(r: &ApprovalRequest) -> String {
        render_card_w(&Style::plain(), &hello(), r, now(), false, 86)
    }

    #[test]
    fn plan_card_follows_the_mockup() {
        let text = plain(&demo_plan_request());
        println!("{text}");
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("◆ Approval needed"));
        assert!(lines[0].ends_with("expires in 4:41"));
        assert_eq!(ui::display_width(lines[0]), 86);
        assert!(lines[1].contains("Free 228 MB on web-1"));
        assert_eq!(
            lines[2].trim(),
            "change plan 696de2f2 · 1 step · 5 checks · 1 undo · opencode 2.1.2 · run dddbee19"
        );
        assert!(text.contains("WHAT THIS DOES"));
        assert!(text.contains("read by bssh, not the agent"));
        assert!(text.contains("creates"));
        assert!(text.contains("moves"));
        assert!(text.contains("→"));
        assert!(text.contains("deletes  nothing"));
        assert!(text.contains("sudo  no"));
        assert!(text.contains("network  no"));
        assert!(text.contains("policy  default · one approver, outside the agent"));
        assert!(text.contains("STEP 1 · web-1"));
        assert!(text.contains("│ A=/var/tmp/var-www-cleanup-2026-10-24"));
        assert!(text.contains("│ install -d -m 0750"));
        assert!(text.contains("CHECKS  df -h /var/www · 4 × test"));
        assert!(text.contains("read-only"));
        assert!(text.contains("UNDO"));
        assert!(text.contains("THE AGENT SAYS"));
        assert!(text.contains("not checked by bssh"));
        assert!(text.contains("“/var/www is a 310 MB tmpfs"));
        assert!(text.contains("v for all of it"));
        let keys = lines.last().unwrap();
        assert!(keys.contains("a approve    d deny    v full text    s skip"));
        assert!(keys.ends_with("approves plan 486c2f5a only"));
        assert!(!keys.contains("g approve"));
        assert!(lines.iter().all(|l| ui::display_width(l) <= 86), "{text}");
    }

    #[test]
    fn full_view_lifts_the_limits() {
        let r = demo_plan_request();
        let short = plain(&r);
        let full = render_card_w(&Style::plain(), &hello(), &r, now(), true, 86);
        assert!(full.lines().count() > short.lines().count());
        assert!(!full.contains("v for all of it"));
        assert!(full.contains("short view"));
        assert!(full.contains("│ test -f /var/tmp/a/cache/assets/bundle-old.js"));
    }

    #[test]
    fn single_service_call_uses_the_same_frame() {
        let text = plain(&service_request(Tier::OutOfBand));
        println!("{text}");
        assert!(text
            .lines()
            .nth(1)
            .unwrap()
            .contains("Restart nginx on db-1"));
        assert!(text.contains("service request 440947ca"));
        assert!(text.contains("db-1"));
        assert!(text.contains("restarts"));
        assert!(text.contains("sudo  yes"));
        assert!(text.contains("SERVICE · db-1"));
        assert!(text.contains("│ sudo -n systemctl restart -- nginx"));
        assert!(text.contains("privilege · service"));
        assert!(!text.contains("THE AGENT SAYS"));
        let keys = text.lines().last().unwrap();
        assert!(keys.contains("g approve and allow this for 15 min"));
    }

    #[test]
    fn two_person_calls_say_so() {
        let text = plain(&service_request(Tier::TwoPerson));
        assert!(text.contains("two different people must approve"));
    }

    #[test]
    fn edit_call_shows_a_colored_diff() {
        let mut c = call(ToolName::EditFile, None);
        c.path = Some("/etc/nginx/nginx.conf".into());
        c.args = serde_json::json!({"patch": "-worker_processes 2;\n+worker_processes 4;"});
        let r = base(ApprovalSubject::Call {
            call: c,
            analysis: None,
        });
        let text = plain(&r);
        assert!(text.contains("Edit /etc/nginx/nginx.conf on db-1"));
        assert!(text.contains("edits    /etc/nginx/nginx.conf"));
        assert!(text.contains("-worker_processes 2;"));
        assert!(text.contains("+worker_processes 4;"));
        let colored = render_card_w(
            &Style {
                color: true,
                unicode: true,
            },
            &hello(),
            &r,
            now(),
            false,
            86,
        );
        assert!(colored.contains("\x1b[32m+worker_processes 4;"));
        assert!(colored.contains("\x1b[31m-worker_processes 2;"));
    }

    #[test]
    fn access_request_card() {
        let r = base(ApprovalSubject::Access {
            scope: Scope {
                tags: vec!["prod".into()],
                hosts: vec!["db-1".into()],
            },
            reason: "debug outage".into(),
            duration: "30m".into(),
        });
        let text = plain(&r);
        println!("{text}");
        assert!(text
            .lines()
            .nth(1)
            .unwrap()
            .contains("Wider access for 30 min"));
        assert!(text.contains("access request 440947ca"));
        assert!(text.contains("hosts  db-1"));
        assert!(text.contains("tags   prod"));
        assert!(text.contains("“debug outage”"));
        let keys = text.lines().last().unwrap();
        assert!(keys.contains("s skip"));
        assert!(!keys.contains("g approve"));
    }

    #[test]
    fn unparsed_commands_are_flagged() {
        let a = Analysis {
            commands: vec![],
            tags: [RiskTag::Unknown].into_iter().collect(),
            unknown_reason: Some("eval".into()),
        };
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run, Some("eval $X")),
            analysis: Some(a),
        });
        let text = plain(&r);
        assert!(text.contains("part of this is not understood: eval"));
        assert!(text.contains("not understood"));
    }

    #[test]
    fn color_output_strips_to_the_plain_output() {
        let color = Style {
            color: true,
            unicode: true,
        };
        for r in [demo_plan_request(), service_request(Tier::OutOfBand)] {
            let painted = render_card_w(&color, &hello(), &r, now(), false, 86);
            assert!(painted.contains('\x1b'));
            assert_eq!(ui::strip_ansi(&painted), plain(&r));
        }
    }

    #[test]
    fn render_request_is_a_thin_wrapper() {
        let text = render_request(&hello(), &service_request(Tier::OutOfBand));
        assert!(ui::strip_ansi(&text).contains("Approval needed"));
    }

    #[test]
    fn results_read_like_the_brief() {
        let s = Style::plain();
        let plan = demo_plan_request();
        assert_eq!(
            result_line(&s, &plan, true, "ana@cli", false),
            "✓ Approved plan 486c2f5a for opencode 2.1.2. It runs now; bssh ai watch follows it."
        );
        assert_eq!(
            result_line(&s, &plan, false, "ana@cli", false),
            "✗ Denied. opencode 2.1.2 is told \"denied by ana@cli\" and nothing ran."
        );
        let two = service_request(Tier::TwoPerson);
        assert!(result_line(&s, &two, true, "ana@cli", false)
            .contains("Your approval is recorded. It needs one more person"));
    }

    #[test]
    fn keys_map_to_choices() {
        assert_eq!(choice_for("a", false), Choice::Approve);
        assert_eq!(choice_for("D", false), Choice::Deny);
        assert_eq!(choice_for("v", false), Choice::Full);
        assert_eq!(choice_for("s", false), Choice::Skip);
        assert_eq!(choice_for("g", true), Choice::Grant);
        assert_eq!(choice_for("g", false), Choice::Other);
        assert_eq!(choice_for("", true), Choice::Other);
        assert_eq!(choice_for("x", true), Choice::Other);
    }

    #[test]
    fn listing_is_one_aligned_line_per_request() {
        let rows: Vec<[String; 6]> = [demo_plan_request(), service_request(Tier::OutOfBand)]
            .iter()
            .map(|r| {
                [
                    ui::short_id(&r.request_id),
                    kind_word(r).to_string(),
                    request_host(r),
                    request_title(r),
                    r.client.clone(),
                    expiry_text(r, now()),
                ]
            })
            .collect();
        let lines = summary_rows(&Style::plain(), &rows, 150);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| !l.contains('\n')));
        assert!(
            lines.iter().all(|l| ui::display_width(l) <= 150),
            "{lines:?}"
        );
        assert!(lines[0].starts_with("◆ 440947ca  plan"));
        assert!(lines[0].contains("web-1"));
        assert!(lines[1].contains("sudo -n systemctl restart -- nginx"));
        assert!(lines[0].ends_with("expires in 4:41"));
        // Columns line up.
        assert_eq!(lines[0].find("opencode"), lines[1].find("opencode"));
    }

    #[test]
    fn durations_are_human() {
        assert_eq!(human_duration("30m"), "30 min");
        assert_eq!(human_duration("2h"), "2 h");
        assert_eq!(human_duration("until friday"), "until friday");
    }

    #[test]
    fn long_destinations_keep_two_components() {
        assert_eq!(short_dest("archive"), "archive");
        assert_eq!(
            short_dest("/var/tmp/var-www-cleanup-2026-10-24/releases/"),
            "…/var-www-cleanup-2026-10-24/releases"
        );
    }

    #[test]
    fn grant_for_a_call_targets_the_command_path_or_unit() {
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run, Some("systemctl restart nginx")),
            analysis: None,
        });
        let g = build_grant(&r, 30).unwrap();
        assert_eq!(g.tool, ToolName::Run);
        assert_eq!(g.hosts, "db-1");
        assert_eq!(g.target.as_deref(), Some("systemctl restart nginx"));
        assert_eq!(
            g.run_id.as_deref(),
            Some("dddbee19-f151-4ee0-8473-93924f35e1d4")
        );

        let mut c = call(ToolName::EditFile, None);
        c.path = Some("/etc/nginx/a.conf".into());
        let g = build_grant(
            &base(ApprovalSubject::Call {
                call: c,
                analysis: None,
            }),
            5,
        )
        .unwrap();
        assert_eq!(g.target.as_deref(), Some("/etc/nginx/a.conf"));

        let g = build_grant(&service_request(Tier::OutOfBand), 5).unwrap();
        assert_eq!(g.target.as_deref(), Some("nginx"));
    }

    #[test]
    fn grant_expires_in_the_future_and_is_refused_for_plans() {
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run, Some("ls")),
            analysis: None,
        });
        let g = build_grant(&r, 30).unwrap();
        let exp = chrono::DateTime::parse_from_rfc3339(&g.expires_at).unwrap();
        let delta = exp.signed_duration_since(chrono::Utc::now());
        assert!(delta.num_minutes() >= 29 && delta.num_minutes() <= 30);

        assert!(build_grant(&demo_plan_request(), 30).is_none());
    }

    #[test]
    fn find_request_by_id_and_prefix() {
        let mut a = base(ApprovalSubject::Access {
            scope: Scope::default(),
            reason: "x".into(),
            duration: "1h".into(),
        });
        a.request_id = "abc111".into();
        let mut b = a.clone();
        b.request_id = "abd222".into();
        let all = vec![(0, a), (0, b)];
        assert_eq!(find_request(&all, "abc111").unwrap(), 0);
        assert_eq!(find_request(&all, "abd").unwrap(), 1);
        assert!(find_request(&all, "ab").is_err());
        assert!(find_request(&all, "zzz").is_err());
    }
}
