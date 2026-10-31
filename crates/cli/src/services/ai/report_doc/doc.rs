//! The format-independent document built from a `RunReport`.

use crate::services::ai::report::{ReportCall, ReportChange, ReportStep, RunReport};
use crate::services::ai::ui;
use chrono::{DateTime, Local};
use std::fmt::Write;

/// What a table column holds, which decides how each layout sets it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ColKind {
    Short,
    /// Free text that may be long: own line in narrow text layouts.
    Long,
    /// A command or path: long, and monospace in Markdown and HTML.
    Code,
}

pub(super) struct Col {
    pub(super) name: &'static str,
    kind: ColKind,
}

impl Col {
    /// Free text that may be long.
    pub(super) fn is_long(&self) -> bool {
        self.kind != ColKind::Short
    }

    /// A command or path.
    pub(super) fn is_code(&self) -> bool {
        self.kind == ColKind::Code
    }
}

const fn col(name: &'static str) -> Col {
    Col {
        name,
        kind: ColKind::Short,
    }
}

const fn long(name: &'static str) -> Col {
    Col {
        name,
        kind: ColKind::Long,
    }
}

const fn code(name: &'static str) -> Col {
    Col {
        name,
        kind: ColKind::Code,
    }
}

pub(super) enum Block {
    Heading(String),
    Subheading(String),
    Para(String),
    Note(String),
    Facts(Vec<(&'static str, String)>),
    Table {
        cols: Vec<Col>,
        rows: Vec<Vec<String>>,
    },
    Pre {
        label: String,
        text: String,
    },
}

pub(super) struct Doc {
    pub(super) title: String,
    pub(super) blocks: Vec<Block>,
}

pub(super) fn local(ts: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|t| t.with_timezone(&Local))
}

/// `2026-10-24 17:23:00 +02:00`; unparseable input as it is.
pub(super) fn when(ts: &str) -> String {
    local(ts)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S %:z").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn or_dash(s: Option<&str>) -> String {
    s.filter(|s| !s.is_empty()).unwrap_or("-").to_string()
}

fn change_state(c: &ReportChange) -> &'static str {
    if c.undone {
        "Undone"
    } else if c.applied {
        "Applied"
    } else {
        "Stopped at a failed step"
    }
}

fn step_result(s: &ReportStep) -> String {
    match s.ok {
        None => "not run".into(),
        Some(true) => "ok".into(),
        Some(false) => {
            let mut r = "failed".to_string();
            if let Some(code) = s.exit_code {
                let _ = write!(r, ", exit {code}");
            }
            if let Some(e) = s.error.as_deref().filter(|e| !e.is_empty()) {
                let _ = write!(r, ": {e}");
            }
            r
        }
    }
}

fn phase_name(phase: &str) -> &str {
    match phase {
        "step" => "step",
        "verify" => "check",
        "undo" => "undo",
        other => other,
    }
}

fn call_target(c: &ReportCall) -> String {
    or_dash(c.command.as_deref().or(c.path.as_deref()))
}

fn decision_word(d: &str) -> &str {
    match d {
        "allow" => "allowed",
        "ask" => "asked",
        "deny" => "denied",
        other => other,
    }
}

pub(super) fn build(r: &RunReport, generated: DateTime<Local>) -> Doc {
    let mut b = Vec::new();

    let mut facts = vec![("Run", r.run_id.clone())];
    facts.push(("Agent", or_dash(r.client.as_deref())));
    facts.push(("On behalf of", or_dash(r.on_behalf_of.as_deref())));
    let policy = match (r.policy.as_deref(), r.policy_hash.as_deref()) {
        (Some(p), Some(h)) if !h.is_empty() => format!("{p} (hash {})", ui::short_id(h)),
        (p, _) => or_dash(p),
    };
    facts.push(("Policy", policy));
    facts.push((
        "Started",
        r.started.as_deref().map(when).unwrap_or_else(|| "-".into()),
    ));
    facts.push((
        "Ended",
        match (r.ended.as_deref(), r.end_reason.as_deref()) {
            (Some(e), Some(why)) if !why.is_empty() => format!("{} ({why})", when(e)),
            (Some(e), _) => when(e),
            (None, _) => "no end recorded".into(),
        },
    ));
    if let (Some(s), Some(e)) = (r.started.as_deref(), r.ended.as_deref())
        && let (Ok(s), Ok(e)) = (
            DateTime::parse_from_rfc3339(s),
            DateTime::parse_from_rfc3339(e),
        )
    {
        let ms = (e - s).num_milliseconds().max(0) as u64;
        facts.push(("Duration", ui::duration_ms(ms)));
    }
    facts.push((
        "Hosts",
        if r.hosts.is_empty() {
            "none".into()
        } else {
            r.hosts.join(", ")
        },
    ));
    b.push(Block::Facts(facts));

    // Summary
    b.push(Block::Heading("Summary".into()));
    let count = |d: &str| r.calls.iter().filter(|c| c.decision == d).count();
    let denied = count("deny").max(r.denials.len());
    b.push(Block::Para(format!(
        "{}: {} allowed by the policy, {} needed a person's approval, {} denied by the policy.",
        plural(r.calls.len(), "call", "calls"),
        count("allow"),
        count("ask"),
        denied
    )));
    if !r.approvals.is_empty() {
        let outcome = |o: &str| r.approvals.iter().filter(|a| a.outcome == o).count();
        b.push(Block::Para(format!(
            "{}: {} approved, {} denied, {} expired.",
            plural(r.approvals.len(), "approval request", "approval requests"),
            outcome("approved"),
            outcome("denied"),
            outcome("expired")
        )));
    }
    b.push(Block::Para(if r.changes.is_empty() {
        "No change was made to any host.".into()
    } else {
        let applied = r.changes.iter().filter(|c| c.applied && !c.undone).count();
        let undone = r.changes.iter().filter(|c| c.undone).count();
        let failed = r.changes.iter().filter(|c| !c.applied && !c.undone).count();
        format!(
            "{}: {applied} applied, {undone} undone, {failed} stopped at a failed step.",
            plural(r.changes.len(), "change", "changes")
        )
    }));

    // Changes
    b.push(Block::Heading("Changes".into()));
    if r.changes.is_empty() {
        b.push(Block::Para("None.".into()));
    }
    for c in &r.changes {
        let title = c
            .title
            .clone()
            .unwrap_or_else(|| format!("plan {}", ui::short_id(&c.plan_hash)));
        b.push(Block::Subheading(format!("{}: {title}", change_state(c))));
        let mut f = vec![
            ("Change", c.change_id.clone()),
            ("Plan hash", c.plan_hash.clone()),
        ];
        if let Some(by) = &c.approved_by {
            let mut s = format!("by {by}");
            if let Some(ch) = c.approved_channel.as_deref().filter(|ch| *ch != "in-band") {
                let _ = write!(s, " via {ch}");
            }
            if let Some(at) = &c.approved_at {
                let _ = write!(s, " at {}", when(at));
            }
            f.push(("Approved", s));
        }
        f.push(("Recorded", when(&c.ts)));
        if let Some((ok, total)) = c.steps_ok {
            f.push(("Steps", format!("{ok} of {total} ok")));
        }
        if let Some((ok, total)) = c.checks_ok {
            f.push(("Checks", format!("{ok} of {total} ok")));
        }
        if c.undone {
            f.push(("Undo", "undone".into()));
        } else if let Some(u) = &c.undo_point {
            f.push((
                "Undo",
                format!(
                    "bssh ai undo {} (undo point {u})",
                    ui::short_id(&c.change_id)
                ),
            ));
        }
        b.push(Block::Facts(f));
        if let Some(reason) = &c.reason {
            b.push(Block::Para(format!("The agent says: {reason}")));
        }
        if !c.steps.is_empty() {
            b.push(Block::Table {
                cols: vec![
                    col("Phase"),
                    col("Step"),
                    col("Host"),
                    code("Action"),
                    long("Result"),
                ],
                rows: c
                    .steps
                    .iter()
                    .map(|s| {
                        vec![
                            phase_name(&s.phase).to_string(),
                            s.id.clone(),
                            or_dash(s.host.as_deref()),
                            s.action.clone(),
                            step_result(s),
                        ]
                    })
                    .collect(),
            });
            for s in c.steps.iter().filter(|s| s.output.is_some()) {
                b.push(Block::Pre {
                    label: format!(
                        "Output of {} {} on {} (from the host, redacted):",
                        phase_name(&s.phase),
                        s.id,
                        or_dash(s.host.as_deref())
                    ),
                    text: s.output.clone().unwrap_or_default(),
                });
            }
        }
    }

    // Denied
    b.push(Block::Heading("Denied by the policy".into()));
    let denied_calls: Vec<&ReportCall> = r.calls.iter().filter(|c| c.decision == "deny").collect();
    if !denied_calls.is_empty() {
        b.push(Block::Table {
            cols: vec![
                col("Time"),
                col("Kind"),
                col("Host"),
                code("Command or path"),
                long("Reason"),
            ],
            rows: denied_calls
                .iter()
                .map(|c| {
                    vec![
                        ui::clock(&c.ts),
                        ui::tool_kind_in_words(&c.tool).to_string(),
                        or_dash(c.host.as_deref()),
                        call_target(c),
                        c.reason.clone(),
                    ]
                })
                .collect(),
        });
    } else if !r.denials.is_empty() {
        for d in &r.denials {
            b.push(Block::Para(d.clone()));
        }
    } else {
        b.push(Block::Para("Nothing.".into()));
    }

    // Approvals
    b.push(Block::Heading("Approvals".into()));
    if r.approvals.is_empty() {
        b.push(Block::Para("No approval was requested.".into()));
    } else {
        b.push(Block::Table {
            cols: vec![
                col("Time"),
                col("Outcome"),
                col("By"),
                col("Channel"),
                col("Request"),
                long("Reason"),
            ],
            rows: r
                .approvals
                .iter()
                .map(|a| {
                    vec![
                        ui::clock(&a.ts),
                        a.outcome.clone(),
                        or_dash(a.by.as_deref()),
                        or_dash(a.channel.as_deref()),
                        ui::short_id(&a.request_id),
                        or_dash(a.reason.as_deref()),
                    ]
                })
                .collect(),
        });
    }

    // Every call
    b.push(Block::Heading("All calls".into()));
    if r.calls.is_empty() {
        b.push(Block::Para("The agent made no call.".into()));
    } else {
        b.push(Block::Table {
            cols: vec![
                col("#"),
                col("Time"),
                col("Tool"),
                col("Host"),
                col("Decision"),
                col("Exit"),
                code("Command or path"),
                long("Reason"),
            ],
            rows: r
                .calls
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    vec![
                        (i + 1).to_string(),
                        ui::clock(&c.ts),
                        c.tool.clone(),
                        or_dash(c.host.as_deref()),
                        decision_word(&c.decision).to_string(),
                        c.exit_code
                            .map(|e| e.to_string())
                            .unwrap_or_else(|| "-".into()),
                        call_target(c),
                        c.reason.clone(),
                    ]
                })
                .collect(),
        });
    }

    // Redactions, circuit breaker, recordings
    b.push(Block::Heading("Redactions".into()));
    b.push(Block::Para(if r.redactions.is_empty() {
        "Nothing was redacted.".into()
    } else {
        let parts: Vec<String> = r
            .redactions
            .iter()
            .map(|x| format!("{} {}", x.kind, x.count))
            .collect();
        format!("Redacted before the agent saw it: {}.", parts.join(", "))
    }));
    if !r.circuit_breaker.is_empty() {
        b.push(Block::Heading("Circuit breaker".into()));
        for reason in &r.circuit_breaker {
            b.push(Block::Para(format!("Paused: {reason}")));
        }
    }
    b.push(Block::Heading("Recordings".into()));
    b.push(Block::Para(if r.recordings.is_empty() {
        "No output was recorded for this run.".into()
    } else {
        format!(
            "{} of command output. Replay with: bssh ai replay {}",
            plural(r.recordings.len(), "recording", "recordings"),
            ui::short_id(&r.run_id)
        )
    }));

    let seqs = r
        .calls
        .iter()
        .map(|c| c.seq)
        .chain(r.approvals.iter().map(|a| a.seq))
        .chain(r.changes.iter().map(|c| c.seq));
    let range = match (seqs.clone().min(), seqs.max()) {
        (Some(a), Some(z)) => format!(" (entries {a} to {z})"),
        _ => String::new(),
    };
    b.push(Block::Note(format!(
        "Generated by bssh {} on {} from the local audit log{range} and the change records. \
         Built from recorded events only; no model wrote this report.",
        env!("CARGO_PKG_VERSION"),
        generated.format("%Y-%m-%d %H:%M:%S %:z")
    )));

    Doc {
        title: format!("bssh AI run report: {}", ui::short_id(&r.run_id)),
        blocks: b,
    }
}
