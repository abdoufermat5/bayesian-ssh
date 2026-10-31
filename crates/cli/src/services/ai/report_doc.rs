//! Run reports as documents someone can read without bssh: text, Markdown, HTML, PDF, JSON.
//!
//! Every document is built from a [`RunReport`], so from the audit log and the change
//! records on disk. Nothing here is written by a model.

use super::report::{ReportCall, ReportChange, ReportStep, RunReport};
use super::ui;
use anyhow::Result;
use chrono::{DateTime, Local};
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Text,
    Markdown,
    Html,
    Pdf,
    Json,
}

impl ReportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Pdf => "pdf",
            Self::Json => "json",
        }
    }

    /// Name for a file picker filter.
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Markdown => "Markdown",
            Self::Html => "HTML",
            Self::Pdf => "PDF",
            Self::Json => "JSON",
        }
    }
}

impl std::str::FromStr for ReportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "txt" | "text" => Ok(Self::Text),
            "md" | "markdown" => Ok(Self::Markdown),
            "html" | "htm" => Ok(Self::Html),
            "pdf" => Ok(Self::Pdf),
            "json" => Ok(Self::Json),
            _ => Err(format!(
                "unknown report format '{s}' (use txt, md, html, pdf or json)"
            )),
        }
    }
}

/// `bssh-run-dddbee19-20261024-1723.pdf`.
pub fn file_name(report: &RunReport, format: ReportFormat) -> String {
    let when = report
        .started
        .as_deref()
        .and_then(local)
        .map(|t| format!("-{}", t.format("%Y%m%d-%H%M")))
        .unwrap_or_default();
    let id: String = ui::short_id(&report.run_id)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    format!("bssh-run-{id}{when}.{}", format.extension())
}

/// The report as the bytes of a file in `format`.
pub fn render(
    report: &RunReport,
    format: ReportFormat,
    generated: DateTime<Local>,
) -> Result<Vec<u8>> {
    if format == ReportFormat::Json {
        let mut s = report.to_json()?;
        s.push('\n');
        return Ok(s.into_bytes());
    }
    let doc = build(report, generated);
    Ok(match format {
        ReportFormat::Text => to_text(&doc).into_bytes(),
        ReportFormat::Markdown => to_markdown(&doc).into_bytes(),
        ReportFormat::Html => to_html(&doc).into_bytes(),
        ReportFormat::Pdf => to_pdf(&doc, report, generated),
        ReportFormat::Json => unreachable!(),
    })
}

// ---------------------------------------------------------------- document model

struct Col {
    name: &'static str,
    /// Free text that may be long: own line in narrow text layouts.
    long: bool,
    /// A command or path: monospace in Markdown and HTML.
    code: bool,
}

const fn col(name: &'static str) -> Col {
    Col {
        name,
        long: false,
        code: false,
    }
}

const fn long(name: &'static str) -> Col {
    Col {
        name,
        long: true,
        code: false,
    }
}

const fn code(name: &'static str) -> Col {
    Col {
        name,
        long: true,
        code: true,
    }
}

enum Block {
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

struct Doc {
    title: String,
    blocks: Vec<Block>,
}

fn local(ts: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|t| t.with_timezone(&Local))
}

/// `2026-10-24 17:23:00 +02:00`; unparseable input as it is.
fn when(ts: &str) -> String {
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

fn build(r: &RunReport, generated: DateTime<Local>) -> Doc {
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
    if let (Some(s), Some(e)) = (r.started.as_deref(), r.ended.as_deref()) {
        if let (Ok(s), Ok(e)) = (
            DateTime::parse_from_rfc3339(s),
            DateTime::parse_from_rfc3339(e),
        ) {
            let ms = (e - s).num_milliseconds().max(0) as u64;
            facts.push(("Duration", ui::duration_ms(ms)));
        }
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

// ---------------------------------------------------------------- text and PDF

/// Characters per line of the text and PDF layouts.
const WIDTH: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Bold,
    Normal,
}

/// Word-wrap `text` to `width` columns, breaking words longer than a line.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        let mut len = 0;
        for word in para.split(' ') {
            let mut word: Vec<char> = word.chars().collect();
            loop {
                let wl = word.len();
                let sep = usize::from(len > 0);
                if len + sep + wl <= width {
                    if sep == 1 {
                        line.push(' ');
                    }
                    line.extend(word.iter());
                    len += sep + wl;
                    break;
                }
                if len > 0 {
                    out.push(std::mem::take(&mut line));
                    len = 0;
                    continue;
                }
                // A word longer than a line.
                let rest = word.split_off(width.max(1));
                line.extend(word.iter());
                out.push(std::mem::take(&mut line));
                word = rest;
            }
        }
        out.push(line);
    }
    out
}

fn width_of(s: &str) -> usize {
    s.chars().count()
}

fn pad(s: &str, w: usize) -> String {
    let mut p = s.to_string();
    p.extend(std::iter::repeat_n(' ', w.saturating_sub(width_of(s))));
    p
}

fn text_lines(doc: &Doc) -> Vec<(Look, String)> {
    let mut out: Vec<(Look, String)> = Vec::new();
    let blank = |out: &mut Vec<(Look, String)>| {
        if out.last().is_some_and(|(_, l)| !l.is_empty()) {
            out.push((Look::Normal, String::new()));
        }
    };
    let indented = |out: &mut Vec<(Look, String)>, text: &str, indent: usize| {
        for l in wrap(text, WIDTH - indent) {
            out.push((Look::Normal, format!("{}{l}", " ".repeat(indent))));
        }
    };

    out.push((Look::Bold, doc.title.clone()));
    out.push((Look::Bold, "=".repeat(width_of(&doc.title))));
    for block in &doc.blocks {
        match block {
            Block::Heading(h) => {
                blank(&mut out);
                out.push((Look::Normal, String::new()));
                out.push((Look::Bold, h.to_uppercase()));
            }
            Block::Subheading(h) => {
                blank(&mut out);
                for l in wrap(h, WIDTH) {
                    out.push((Look::Bold, l));
                }
            }
            Block::Para(p) | Block::Note(p) => {
                blank(&mut out);
                indented(&mut out, p, 0);
            }
            Block::Facts(facts) => {
                blank(&mut out);
                let kw = facts.iter().map(|(k, _)| width_of(k)).max().unwrap_or(0) + 2;
                for (k, v) in facts {
                    let lines = wrap(v, WIDTH - kw);
                    for (i, l) in lines.iter().enumerate() {
                        let key = if i == 0 {
                            format!("{k}:")
                        } else {
                            String::new()
                        };
                        out.push((Look::Normal, format!("{}{l}", pad(&key, kw))));
                    }
                }
            }
            Block::Table { cols, rows } => {
                blank(&mut out);
                let widths: Vec<usize> = cols
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        rows.iter()
                            .map(|r| width_of(&r[i]))
                            .chain([width_of(c.name)])
                            .max()
                            .unwrap_or(0)
                    })
                    .collect();
                let total = widths.iter().sum::<usize>() + 2 * (cols.len() - 1);
                if total <= WIDTH {
                    let row_line = |cells: Vec<&str>| {
                        let last = cells.len() - 1;
                        cells
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                if i == last {
                                    c.to_string()
                                } else {
                                    pad(c, widths[i])
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("  ")
                    };
                    out.push((Look::Bold, row_line(cols.iter().map(|c| c.name).collect())));
                    out.push((
                        Look::Normal,
                        widths
                            .iter()
                            .map(|w| "-".repeat(*w))
                            .collect::<Vec<_>>()
                            .join("  "),
                    ));
                    for r in rows {
                        out.push((
                            Look::Normal,
                            row_line(r.iter().map(String::as_str).collect()),
                        ));
                    }
                } else {
                    // Too wide: short columns on one line, long ones below it.
                    for (n, r) in rows.iter().enumerate() {
                        if n > 0 {
                            out.push((Look::Normal, String::new()));
                        }
                        let head: Vec<String> = cols
                            .iter()
                            .zip(r)
                            .enumerate()
                            .filter(|(_, (c, _))| !c.long)
                            .map(|(i, (_, v))| pad(v, widths[i]))
                            .collect();
                        out.push((Look::Normal, head.join("  ").trim_end().to_string()));
                        for (c, v) in cols.iter().zip(r).filter(|(c, _)| c.long) {
                            if v != "-" {
                                indented(&mut out, &format!("{}: {v}", c.name), 4);
                            }
                        }
                    }
                }
            }
            Block::Pre { label, text } => {
                blank(&mut out);
                indented(&mut out, label, 0);
                for l in text.trim_end().lines() {
                    for w in wrap(&l.replace('\t', "    "), WIDTH - 4) {
                        out.push((Look::Normal, format!("    {w}")));
                    }
                }
            }
        }
    }
    out
}

fn to_text(doc: &Doc) -> String {
    let mut s = String::new();
    for (_, l) in text_lines(doc) {
        s.push_str(l.trim_end());
        s.push('\n');
    }
    s
}

/// One character in the PDF's WinAnsi encoding.
fn win_ansi(c: char, out: &mut Vec<u8>) {
    let b = match c {
        '(' | ')' | '\\' => {
            out.push(b'\\');
            c as u8
        }
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '€' => 0x80,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        _ => b'?',
    };
    out.push(b);
}

fn pdf_string(s: &str) -> Vec<u8> {
    let mut v = vec![b'('];
    for c in s.chars() {
        win_ansi(if c == '\t' { ' ' } else { c }, &mut v);
    }
    v.push(b')');
    v
}

/// A text-only A4 PDF in Courier, from the text layout. No dependency, no embedded font.
fn to_pdf(doc: &Doc, report: &RunReport, generated: DateTime<Local>) -> Vec<u8> {
    const PAGE_W: u32 = 595;
    const PAGE_H: u32 = 842;
    const LEFT: u32 = 42;
    const TOP: u32 = 794;
    const LEADING: u32 = 11;
    const PER_PAGE: usize = 67;

    let lines = text_lines(doc);
    let pages: Vec<&[(Look, String)]> = lines.chunks(PER_PAGE).collect();
    let n_pages = pages.len().max(1);

    // Objects: 1 catalog, 2 pages, 3 Courier, 4 Courier-Bold, 5 info, then page + content pairs.
    let mut objects: Vec<Vec<u8>> = Vec::new();
    let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", 6 + 2 * i)).collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(
        format!(
            "<< /Type /Pages /Kids [{}] /Count {n_pages} >>",
            kids.join(" ")
        )
        .into_bytes(),
    );
    for font in ["Courier", "Courier-Bold"] {
        objects.push(
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{font} /Encoding /WinAnsiEncoding >>"
            )
            .into_bytes(),
        );
    }
    let mut info = b"<< /Title ".to_vec();
    info.extend(pdf_string(&doc.title));
    info.extend(b" /Producer ");
    info.extend(pdf_string(&format!("bssh {}", env!("CARGO_PKG_VERSION"))));
    info.extend(format!(" /CreationDate (D:{}) >>", generated.format("%Y%m%d%H%M%S")).into_bytes());
    objects.push(info);

    let footer = format!("bssh AI run report {}", ui::short_id(&report.run_id));
    for i in 0..n_pages {
        let mut content: Vec<u8> = Vec::new();
        content.extend(format!("BT /F1 9 Tf {LEADING} TL {LEFT} {TOP} Td\n").into_bytes());
        let mut look = Look::Normal;
        for (l, text) in pages.get(i).copied().unwrap_or_default() {
            if *l != look {
                look = *l;
                let f = if look == Look::Bold { "F2" } else { "F1" };
                content.extend(format!("/{f} 9 Tf\n").into_bytes());
            }
            content.extend(pdf_string(text.trim_end()));
            content.extend(b" Tj T*\n");
        }
        content.extend(b"ET\n");
        content.extend(format!("BT /F1 8 Tf {LEFT} 28 Td ").into_bytes());
        content.extend(pdf_string(&format!(
            "{footer} - page {} of {n_pages}",
            i + 1
        )));
        content.extend(b" Tj ET\n");

        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                7 + 2 * i
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend(content);
        stream.extend(b"\nendstream");
        objects.push(stream);
    }

    let mut pdf = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend(format!("{} 0 obj\n", i + 1).into_bytes());
        pdf.extend(body);
        pdf.extend(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
    for o in offsets {
        pdf.extend(format!("{o:010} 00000 n \n").into_bytes());
    }
    pdf.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .into_bytes(),
    );
    pdf
}

// ---------------------------------------------------------------- Markdown

fn md_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' | '~' => {
                o.push('\\');
                o.push(c);
            }
            '\n' => o.push(' '),
            _ => o.push(c),
        }
    }
    o
}

/// Inline code that survives backticks in `s`; `|` stays escaped for tables.
fn md_code(s: &str) -> String {
    let longest = s.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let body = s.replace('\n', " ").replace('|', "\\|");
    let pad = if body.starts_with('`') || body.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{body}{pad}{fence}")
}

fn to_markdown(doc: &Doc) -> String {
    let mut s = format!("# {}\n", md_escape(&doc.title));
    for block in &doc.blocks {
        s.push('\n');
        match block {
            Block::Heading(h) => {
                let _ = writeln!(s, "## {}", md_escape(h));
            }
            Block::Subheading(h) => {
                let _ = writeln!(s, "### {}", md_escape(h));
            }
            Block::Para(p) => {
                let _ = writeln!(s, "{}", md_escape(p));
            }
            Block::Note(p) => {
                let _ = writeln!(s, "_{}_", md_escape(p));
            }
            Block::Facts(facts) => {
                for (k, v) in facts {
                    let _ = writeln!(s, "- **{k}:** {}", md_escape(v));
                }
            }
            Block::Table { cols, rows } => {
                let head: Vec<&str> = cols.iter().map(|c| c.name).collect();
                let _ = writeln!(s, "| {} |", head.join(" | "));
                let _ = writeln!(s, "|{}", "---|".repeat(cols.len()));
                for r in rows {
                    let cells: Vec<String> = cols
                        .iter()
                        .zip(r)
                        .map(|(c, v)| {
                            if c.code && v != "-" {
                                md_code(v)
                            } else {
                                md_escape(v)
                            }
                        })
                        .collect();
                    let _ = writeln!(s, "| {} |", cells.join(" | "));
                }
            }
            Block::Pre { label, text } => {
                let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                let _ = writeln!(s, "{}\n", md_escape(label));
                let _ = writeln!(s, "{fence}text\n{}\n{fence}", text.trim_end());
            }
        }
    }
    s
}

// ---------------------------------------------------------------- HTML

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

const CSS: &str = "\
:root{color-scheme:light;--fg:#1f2328;--muted:#59636e;--border:#d1d9e0;--soft:#f6f8fa}\
*{box-sizing:border-box}\
body{margin:0;font:14px/1.5 system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;color:var(--fg);background:#fff}\
main{max-width:1040px;margin:0 auto;padding:32px 24px 48px}\
h1{font-size:22px;margin:0 0 16px}\
h2{font-size:16px;margin:28px 0 8px;padding-bottom:4px;border-bottom:1px solid var(--border)}\
h3{font-size:14px;margin:20px 0 6px}\
dl{display:grid;grid-template-columns:max-content 1fr;gap:4px 16px;margin:0}\
dt{color:var(--muted)}dd{margin:0;overflow-wrap:anywhere}\
p{margin:6px 0}\
.note{color:var(--muted);font-size:12px;margin-top:32px}\
table{border-collapse:collapse;width:100%;margin:8px 0;font-size:13px}\
th,td{border:1px solid var(--border);padding:4px 8px;text-align:left;vertical-align:top}\
th{background:var(--soft);font-weight:600}\
td{overflow-wrap:anywhere}\
.nw{white-space:nowrap;overflow-wrap:normal}\
code,pre{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12px}\
pre{background:var(--soft);border:1px solid var(--border);padding:8px;white-space:pre-wrap;overflow-wrap:anywhere}\
.label{color:var(--muted);margin-top:10px}\
@media print{main{padding:0;max-width:none}h2{break-after:avoid}tr,pre{break-inside:avoid}}\
@page{size:A4;margin:16mm}";

/// Short columns (times, tools, hosts) stay on one line; long ones wrap.
fn nowrap(c: &Col) -> &'static str {
    if c.long {
        ""
    } else {
        " class=\"nw\""
    }
}

fn to_html(doc: &Doc) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
         <meta name=\"generator\" content=\"bssh {}\">\n<title>{}</title>\n<style>{CSS}</style>\n\
         </head>\n<body>\n<main>\n<h1>{}</h1>\n",
        env!("CARGO_PKG_VERSION"),
        esc(&doc.title),
        esc(&doc.title)
    );
    for block in &doc.blocks {
        match block {
            Block::Heading(h) => {
                let _ = writeln!(s, "<h2>{}</h2>", esc(h));
            }
            Block::Subheading(h) => {
                let _ = writeln!(s, "<h3>{}</h3>", esc(h));
            }
            Block::Para(p) => {
                let _ = writeln!(s, "<p>{}</p>", esc(p));
            }
            Block::Note(p) => {
                let _ = writeln!(s, "<p class=\"note\">{}</p>", esc(p));
            }
            Block::Facts(facts) => {
                s.push_str("<dl>\n");
                for (k, v) in facts {
                    let _ = writeln!(s, "<dt>{}</dt><dd>{}</dd>", esc(k), esc(v));
                }
                s.push_str("</dl>\n");
            }
            Block::Table { cols, rows } => {
                s.push_str("<table>\n<thead><tr>");
                for c in cols {
                    let _ = write!(s, "<th{}>{}</th>", nowrap(c), esc(c.name));
                }
                s.push_str("</tr></thead>\n<tbody>\n");
                for r in rows {
                    s.push_str("<tr>");
                    for (c, v) in cols.iter().zip(r) {
                        if c.code && v != "-" {
                            let _ = write!(s, "<td><code>{}</code></td>", esc(v));
                        } else {
                            let _ = write!(s, "<td{}>{}</td>", nowrap(c), esc(v));
                        }
                    }
                    s.push_str("</tr>\n");
                }
                s.push_str("</tbody>\n</table>\n");
            }
            Block::Pre { label, text } => {
                let _ = writeln!(
                    s,
                    "<p class=\"label\">{}</p>\n<pre>{}</pre>",
                    esc(label),
                    esc(text.trim_end())
                );
            }
        }
    }
    s.push_str("</main>\n</body>\n</html>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::report::{ReportApproval, ReportCall};
    use chrono::TimeZone;

    fn report() -> RunReport {
        RunReport {
            run_id: "dddbee19-f151-4ee0-8473-93924f35e1d4".into(),
            client: Some("opencode 2.1.2".into()),
            on_behalf_of: Some("ana".into()),
            policy: Some("default".into()),
            policy_hash: Some("abcdef0123".into()),
            started: Some("2026-10-24T17:23:00Z".into()),
            ended: Some("2026-10-24T17:25:10Z".into()),
            end_reason: Some("client left".into()),
            hosts: vec!["web-1".into()],
            calls: vec![
                ReportCall {
                    seq: 1,
                    ts: "2026-10-24T17:23:58Z".into(),
                    call_id: "c1".into(),
                    tool: "run_readonly".into(),
                    host: Some("web-1".into()),
                    command: Some("cat /etc/x | grep `a` <script>alert(1)</script> (y)".into()),
                    path: None,
                    decision: "deny".into(),
                    reason: "writes in a read-only call".into(),
                    exit_code: None,
                    output_sha256: None,
                    redactions: vec![],
                },
                ReportCall {
                    seq: 2,
                    ts: "2026-10-24T17:24:00Z".into(),
                    call_id: "c2".into(),
                    tool: "run_readonly".into(),
                    host: Some("web-1".into()),
                    command: Some(format!("du -xah /var/www {}", "x".repeat(150))),
                    path: None,
                    decision: "allow".into(),
                    reason: "read".into(),
                    exit_code: Some(0),
                    output_sha256: None,
                    redactions: vec![],
                },
            ],
            approvals: vec![ReportApproval {
                seq: 3,
                ts: "2026-10-24T17:24:44Z".into(),
                request_id: "req1".into(),
                outcome: "approved".into(),
                by: Some("ana".into()),
                channel: Some("desktop".into()),
                reason: None,
            }],
            changes: vec![ReportChange {
                seq: 4,
                ts: "2026-10-24T17:24:47Z".into(),
                change_id: "change-696de2f2".into(),
                plan_hash: "486c2f5a".into(),
                applied: false,
                undo_point: None,
                title: Some("Restart nginx".into()),
                reason: Some("nginx holds a deleted log".into()),
                approved_by: Some("ana".into()),
                approved_channel: Some("desktop".into()),
                approved_at: Some("2026-10-24T17:24:44Z".into()),
                steps_ok: Some((0, 1)),
                checks_ok: None,
                undone: false,
                steps: vec![
                    ReportStep {
                        phase: "step".into(),
                        id: "step-1".into(),
                        host: Some("web-1".into()),
                        action: "restart nginx".into(),
                        ok: Some(false),
                        exit_code: Some(1),
                        error: Some("unit failed".into()),
                        output: Some("Job for nginx.service failed.".into()),
                    },
                    ReportStep {
                        phase: "verify".into(),
                        id: "step-2".into(),
                        host: Some("web-1".into()),
                        action: "systemctl is-active nginx".into(),
                        ok: None,
                        exit_code: None,
                        error: None,
                        output: None,
                    },
                ],
            }],
            denials: vec!["run_readonly cat: writes in a read-only call".into()],
            redactions: vec![],
            circuit_breaker: vec![],
            recordings: vec![],
        }
    }

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap()
    }

    fn text(f: ReportFormat) -> String {
        String::from_utf8(render(&report(), f, now()).unwrap()).unwrap()
    }

    #[test]
    fn text_tells_what_happened_to_each_step() {
        let t = text(ReportFormat::Text);
        assert!(t.contains("Stopped at a failed step: Restart nginx"), "{t}");
        assert!(t.contains("The agent says: nginx holds a deleted log"));
        assert!(t.contains("failed, exit 1: unit failed"));
        assert!(t.contains("not run"));
        assert!(t.contains("Job for nginx.service failed."));
        assert!(
            t.contains("2 calls: 1 allowed by the policy, 0 needed a person's approval, 1 denied")
        );
        assert!(t.lines().all(|l| l.chars().count() <= WIDTH), "{t}");
        // The 150-character command is kept whole, across lines.
        assert!(t.matches('x').count() >= 150);
        assert!(!t.contains("Some(") && !t.contains("None"));
    }

    #[test]
    fn markdown_keeps_hostile_commands_inside_their_cell() {
        let md = text(ReportFormat::Markdown);
        let row = md
            .lines()
            .find(|l| l.contains("/etc/x"))
            .expect("denied call row");
        // Unescaped pipes are only the cell separators.
        let seps = row.replace("\\|", "").matches('|').count();
        assert_eq!(seps, 6, "{row}");
        assert!(row.contains("``cat /etc/x \\| grep `a`"), "{row}");
    }

    #[test]
    fn html_escapes_remote_and_agent_text() {
        let h = text(ReportFormat::Html);
        assert!(!h.contains("<script>alert"));
        assert!(h.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(h.starts_with("<!doctype html>"));
    }

    #[test]
    fn pdf_xref_points_at_every_object() {
        let pdf = render(&report(), ReportFormat::Pdf, now()).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        // Offsets are in bytes: the binary comment line must not be decoded.
        let tail_at = pdf.windows(10).rposition(|w| w == b"startxref\n").unwrap() + 10;
        let tail = std::str::from_utf8(&pdf[tail_at..]).unwrap();
        let start: usize = tail.lines().next().unwrap().parse().unwrap();
        let xref = std::str::from_utf8(&pdf[start..]).unwrap();
        assert!(xref.starts_with("xref\n"));
        let offsets: Vec<usize> = xref
            .lines()
            .skip(3)
            .take_while(|l| l.ends_with(" n "))
            .map(|l| l[..10].parse().unwrap())
            .collect();
        assert!(offsets.len() >= 7);
        let s = String::from_utf8_lossy(&pdf);
        for (i, o) in offsets.iter().enumerate() {
            let head = format!("{} 0 obj", i + 1);
            assert_eq!(&pdf[*o..o + head.len()], head.as_bytes());
        }
        // Parentheses in commands are escaped inside PDF strings.
        assert!(s.contains("\\(y\\)"));
    }

    #[test]
    fn long_runs_spill_onto_more_pdf_pages() {
        let mut r = report();
        let c = r.calls[1].clone();
        r.calls = (0..200).map(|_| c.clone()).collect();
        let pdf =
            String::from_utf8_lossy(&render(&r, ReportFormat::Pdf, now()).unwrap()).into_owned();
        let pages = pdf.matches("/Type /Page ").count();
        assert!(pages > 1);
        assert!(pdf.contains(&format!("/Count {pages}")));
        assert!(pdf.contains(&format!("page {pages} of {pages}")));
    }

    #[test]
    fn formats_parse_and_name_files() {
        assert_eq!("MD".parse::<ReportFormat>(), Ok(ReportFormat::Markdown));
        assert!("docx".parse::<ReportFormat>().is_err());
        let name = file_name(&report(), ReportFormat::Pdf);
        assert!(name.starts_with("bssh-run-dddbee19-2026102"), "{name}");
        assert!(name.ends_with(".pdf"));
    }
}
