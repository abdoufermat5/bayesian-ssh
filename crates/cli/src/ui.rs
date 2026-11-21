//! Shared CLI UI primitives for all `bssh` commands.
//!
//! Color carries meaning only (status, risk, emphasis), always paired with text
//! or a glyph. Colors use the 16 ANSI slots so they follow the user's terminal
//! theme. Output for pipes, `NO_COLOR`, and tests degrades cleanly to plain
//! text; non-UTF-8 locales fall back to ASCII glyphs and rules.

use chrono::{DateTime, Local, Utc};
use std::collections::HashSet;
use std::io::IsTerminal;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    /// Metadata: labels, ids, times.
    Dim,
    /// Titles and the program of a command.
    Strong,
    /// Allowed, done, nothing risky.
    Ok,
    /// Waiting for a person, privilege, service changes.
    Warn,
    /// Denied, destructive, failed.
    Bad,
    /// Running, network.
    Info,
    /// The agent's own words.
    Quote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Ok,
    Ask,
    Bad,
    Running,
    Waiting,
    Idle,
    Undo,
    /// Left rail of a block of commands or output.
    Rail,
    Arrow,
    Ellipsis,
    /// Separator between facts on one line.
    Dot,
}

/// How to draw: with or without color, with Unicode glyphs or ASCII.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub color: bool,
    pub unicode: bool,
}

impl Style {
    /// For stdout: color on a terminal unless `NO_COLOR` is set, Unicode
    /// unless the locale says otherwise.
    pub fn terminal() -> Self {
        let color = std::io::stdout().is_terminal() && no_color_unset();
        Style {
            color,
            unicode: locale_is_utf8(),
        }
    }

    /// For stderr: color on a terminal unless `NO_COLOR` is set, Unicode
    /// unless the locale says otherwise.
    pub fn stderr() -> Self {
        let color = std::io::stderr().is_terminal() && no_color_unset();
        Style {
            color,
            unicode: locale_is_utf8(),
        }
    }

    /// No color, Unicode glyphs: the MCP text content and tests.
    pub fn plain() -> Self {
        Style {
            color: false,
            unicode: true,
        }
    }

    pub fn paint(&self, tone: Tone, s: &str) -> String {
        if !self.color || s.is_empty() {
            return s.to_string();
        }
        let sgr = match tone {
            Tone::Plain => return s.to_string(),
            Tone::Dim => "90",
            Tone::Strong => "1",
            Tone::Ok => "32",
            Tone::Warn => "33",
            Tone::Bad => "31",
            Tone::Info => "34",
            Tone::Quote => "3",
        };
        format!("\x1b[{sgr}m{s}\x1b[0m")
    }

    pub fn glyph(&self, g: Glyph) -> &'static str {
        match (g, self.unicode) {
            (Glyph::Ok, true) => "✓",
            (Glyph::Ok, false) => "+",
            (Glyph::Ask, true) => "◆",
            (Glyph::Ask, false) => "?",
            (Glyph::Bad, true) => "✗",
            (Glyph::Bad, false) => "x",
            (Glyph::Running, true) => "●",
            (Glyph::Running, false) => "*",
            (Glyph::Waiting, true) => "◌",
            (Glyph::Waiting, false) => "~",
            (Glyph::Idle, true) => "○",
            (Glyph::Idle, false) => "o",
            (Glyph::Undo, true) => "↺",
            (Glyph::Undo, false) => "<",
            (Glyph::Rail, true) => "│",
            (Glyph::Rail, false) => "|",
            (Glyph::Arrow, true) => "→",
            (Glyph::Arrow, false) => "->",
            (Glyph::Ellipsis, true) => "…",
            (Glyph::Ellipsis, false) => "...",
            (Glyph::Dot, true) => "·",
            (Glyph::Dot, false) => "-",
        }
    }

    /// A glyph in its tone: `✓` green, `◆` amber, `✗` red.
    pub fn mark(&self, g: Glyph) -> String {
        let tone = match g {
            Glyph::Ok => Tone::Ok,
            Glyph::Ask | Glyph::Waiting => Tone::Warn,
            Glyph::Bad => Tone::Bad,
            Glyph::Running | Glyph::Undo => Tone::Info,
            _ => Tone::Dim,
        };
        self.paint(tone, self.glyph(g))
    }

    /// A section label: `WHAT THIS DOES`, dim.
    pub fn label(&self, s: &str) -> String {
        self.paint(Tone::Dim, &s.to_uppercase())
    }

    /// `a · b · c` with dim separators, skipping empty parts.
    pub fn facts(&self, parts: &[&str]) -> String {
        let sep = format!(" {} ", self.paint(Tone::Dim, self.glyph(Glyph::Dot)));
        parts
            .iter()
            .filter(|p| !p.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(&sep)
    }
}

fn no_color_unset() -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

fn locale_is_utf8() -> bool {
    for var in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(v) = std::env::var(var)
            && !v.is_empty()
        {
            let v = v.to_ascii_lowercase();
            return v.contains("utf-8") || v.contains("utf8");
        }
    }
    // No locale at all: modern terminals are UTF-8.
    true
}

// ------------------------------------------------------------ layout primitives

/// Render a clean bold heading with optional dimmed metadata (`title  meta`).
pub fn heading(style: &Style, title: &str, meta: Option<&str>) -> String {
    let main = style.paint(Tone::Strong, title);
    match meta {
        Some(m) if !m.is_empty() => format!("{main}  {}", style.paint(Tone::Dim, m)),
        _ => main,
    }
}

/// ANSI-aware, terminal-width-responsive table.
#[derive(Debug, Clone)]
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    right_aligned: HashSet<usize>,
}

impl Table {
    pub fn new(headers: &[&str]) -> Self {
        Self {
            headers: headers.iter().map(|h| (*h).to_string()).collect(),
            rows: Vec::new(),
            right_aligned: HashSet::new(),
        }
    }

    pub fn align_right(mut self, col: usize) -> Self {
        self.right_aligned.insert(col);
        self
    }

    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn render(&self, style: &Style) -> String {
        self.render_with_width(style, width())
    }

    fn render_with_width(&self, style: &Style, max_width: usize) -> String {
        const GAP: usize = 2;

        let ncols = self.headers.len();
        if ncols == 0 {
            return String::new();
        }

        let upper_headers: Vec<String> = self.headers.iter().map(|h| h.to_uppercase()).collect();
        let mut widths: Vec<usize> = upper_headers.iter().map(|h| display_width(h)).collect();

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate().take(ncols) {
                widths[i] = widths[i].max(display_width(cell));
            }
        }

        let gap_total = GAP * ncols.saturating_sub(1);
        let mut total: usize = widths.iter().sum::<usize>() + gap_total;
        while total > max_width {
            // Shrink the widest flexible column first (> 4 cols), falling back to > 1.
            let candidate = widths
                .iter()
                .enumerate()
                .filter(|(_, w)| **w > 4)
                .max_by_key(|(_, w)| **w)
                .or_else(|| {
                    widths
                        .iter()
                        .enumerate()
                        .filter(|(_, w)| **w > 1)
                        .max_by_key(|(_, w)| **w)
                });
            let Some((idx, _)) = candidate else {
                break;
            };
            widths[idx] -= 1;
            total -= 1;
        }

        let mut lines = Vec::with_capacity(self.rows.len() + 2);

        // Header line
        let header_cells: Vec<String> = (0..ncols)
            .map(|i| {
                let fitted = truncate_styled(&upper_headers[i], widths[i], style.unicode);
                let styled = style.paint(Tone::Dim, &fitted);
                align_cell(
                    &styled,
                    widths[i],
                    self.right_aligned.contains(&i),
                    i + 1 == ncols,
                )
            })
            .collect();
        lines.push(header_cells.join("  "));

        // Horizontal rule matched to actual table width
        let table_width: usize = widths.iter().sum::<usize>() + gap_total;
        let rule_char = if style.unicode { "─" } else { "-" };
        lines.push(style.paint(Tone::Dim, &rule_char.repeat(table_width)));

        // Data rows
        for row in &self.rows {
            let cells: Vec<String> = (0..ncols)
                .map(|i| {
                    let raw = row.get(i).map(String::as_str).unwrap_or("");
                    let fitted = truncate_styled(raw, widths[i], style.unicode);
                    align_cell(
                        &fitted,
                        widths[i],
                        self.right_aligned.contains(&i),
                        i + 1 == ncols,
                    )
                })
                .collect();
            lines.push(cells.join("  "));
        }

        lines.join("\n")
    }
}

fn align_cell(s: &str, width: usize, right: bool, is_last: bool) -> String {
    if right {
        pad_left(s, width)
    } else if is_last {
        s.to_string()
    } else {
        pad(s, width)
    }
}

/// Uniformly aligned key-value list with dim labels.
#[derive(Debug, Clone, Default)]
pub struct KvList {
    items: Vec<(String, String)>,
}

impl KvList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.items.push((key.into(), value.into()));
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn render(&self, style: &Style) -> String {
        if self.items.is_empty() {
            return String::new();
        }
        let key_width = self
            .items
            .iter()
            .map(|(k, _)| display_width(k))
            .max()
            .unwrap_or(0);
        self.items
            .iter()
            .map(|(k, v)| {
                let label = pad(&style.paint(Tone::Dim, k), key_width);
                format!("  {label}  {v}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ------------------------------------------------------------------ ids

/// The 8-character form of an id, used everywhere but `--json`.
/// `change-696de2f2` becomes `696de2f2`.
pub fn short_id(id: &str) -> String {
    let id = id.strip_prefix("change-").unwrap_or(id);
    id.chars().take(8).collect()
}

// ----------------------------------------------------------------- time

fn parse(ts: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|t| t.with_timezone(&Local))
}

/// Local clock time, `17:24:31`. Unparseable input comes back as it is.
pub fn clock(ts: &str) -> String {
    parse(ts)
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// Local clock time without seconds, `17:24`. Unparseable input comes back as it is.
pub fn clock_hm(ts: &str) -> String {
    parse(ts)
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// `today 17:23`, `yesterday 09:10`, `Oct 22 17:23`, `2025-12-01 08:00`.
pub fn day_clock(ts: &str, now: DateTime<Utc>) -> String {
    let Some(t) = parse(ts) else {
        return ts.to_string();
    };
    let today = now.with_timezone(&Local).date_naive();
    let day = t.date_naive();
    let hm = t.format("%H:%M");
    if day == today {
        format!("today {hm}")
    } else if today.pred_opt() == Some(day) {
        format!("yesterday {hm}")
    } else if t.format("%Y").to_string() == now.with_timezone(&Local).format("%Y").to_string() {
        format!("{} {hm}", t.format("%b %-d"))
    } else {
        format!("{} {hm}", t.format("%Y-%m-%d"))
    }
}

/// Time left as `4:41`, or `1 h 05` past an hour; `expired` once past.
pub fn expires_in(ts: &str, now: DateTime<Utc>) -> String {
    let Ok(t) = DateTime::parse_from_rfc3339(ts) else {
        return ts.to_string();
    };
    let secs = (t.with_timezone(&Utc) - now).num_seconds();
    if secs <= 0 {
        return "expired".into();
    }
    if secs >= 3600 {
        format!("{} h {:02}", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// `just now`, `40 s ago`, `2 min ago`, `3 h ago`, `5 days ago`.
pub fn ago(ts: &str, now: DateTime<Utc>) -> String {
    let Ok(t) = DateTime::parse_from_rfc3339(ts) else {
        return ts.to_string();
    };
    let secs = (now - t.with_timezone(&Utc)).num_seconds().max(0);
    match secs {
        0..=4 => "just now".into(),
        5..=59 => format!("{secs} s ago"),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// `0.2 s`, `12 s`, `1 min 50 s`, `2 h 05 min`.
pub fn duration_ms(ms: u64) -> String {
    let secs = ms / 1000;
    if ms < 10_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else if secs < 60 {
        format!("{secs} s")
    } else if secs < 3600 {
        format!("{} min {} s", secs / 60, secs % 60)
    } else {
        format!("{} h {:02} min", secs / 3600, (secs % 3600) / 60)
    }
}

// ---------------------------------------------------------------- width

/// Terminal width in columns, between 60 and 160; 100 when unknown.
pub fn width() -> usize {
    let cols = crossterm::terminal::size()
        .map(|(c, _)| c as usize)
        .ok()
        .filter(|c| *c > 0)
        .or_else(|| std::env::var("COLUMNS").ok()?.parse().ok())
        .unwrap_or(100);
    cols.clamp(60, 160)
}

/// Display width of `s`, ignoring ANSI color codes.
pub fn display_width(s: &str) -> usize {
    strip_ansi(s).width()
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Cut plain text to `max` columns, ending with `…`.
pub fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Cut possibly ANSI-styled text to `max` display columns, preserving active
/// SGR sequences and resetting at the end when cut.
fn truncate_styled(s: &str, max: usize, unicode: bool) -> String {
    if max == 0 {
        return String::new();
    }
    if display_width(s) <= max {
        return s.to_string();
    }

    let (ellipsis, ell_w) = if unicode {
        ("…", 1)
    } else if max >= 3 {
        ("...", 3)
    } else {
        (&"..."[..max], max)
    };
    let budget = max.saturating_sub(ell_w);

    let mut out = String::with_capacity(s.len());
    let mut w = 0;
    let mut saw_ansi = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            saw_ansi = true;
            out.push(c);
            if let Some(bracket) = chars.next() {
                out.push(bracket);
            }
            for seq_c in chars.by_ref() {
                out.push(seq_c);
                if seq_c.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }

        let cw = c.width().unwrap_or(0);
        if w + cw > budget {
            break;
        }
        out.push(c);
        w += cw;
    }

    out.push_str(ellipsis);
    if saw_ansi {
        out.push_str("\x1b[0m");
    }
    out
}

/// Pad (possibly colored) text with trailing spaces to `w` columns.
pub fn pad(s: &str, w: usize) -> String {
    let n = display_width(s);
    if n >= w {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(w - n))
    }
}

/// Pad (possibly colored) text with leading spaces to `w` columns.
fn pad_left(s: &str, w: usize) -> String {
    let n = display_width(s);
    if n >= w {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(w - n))
    }
}

/// Wrap plain text at spaces to `width` columns. Lines after the first start
/// with `indent` spaces; a word longer than a line is cut.
pub fn wrap(s: &str, width: usize, indent: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut avail = width.max(4);
    for word in s.split(' ') {
        let ww = word.width();
        let lw = line.width();
        if lw > 0 && lw + 1 + ww > avail {
            lines.push(std::mem::take(&mut line));
            avail = width.saturating_sub(indent).max(4);
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    let pad = " ".repeat(indent);
    lines
        .into_iter()
        .enumerate()
        .map(|(i, l)| if i == 0 { l } else { format!("{pad}{l}") })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_style_has_no_escape_codes() {
        let s = Style::plain();
        assert!(!s.color);
        assert!(s.unicode);
        assert_eq!(s.paint(Tone::Bad, "error"), "error");
        assert_eq!(s.paint(Tone::Strong, "title"), "title");
        assert_eq!(s.label("status"), "STATUS");
        assert_eq!(s.mark(Glyph::Ok), "✓");
        assert_eq!(s.facts(&["a", "", "b"]), "a · b");
        assert_eq!(
            heading(&s, "Connections", Some("3 total")),
            "Connections  3 total"
        );
        assert_eq!(heading(&s, "Connections", None), "Connections");
        assert_eq!(heading(&s, "Connections", Some("")), "Connections");
    }

    #[test]
    fn table_aligns_ansi_colored_cells_accurately() {
        let color_style = Style {
            color: true,
            unicode: true,
        };
        let mut t = Table::new(&["Name", "Status", "Latency"]).align_right(2);
        assert!(t.is_empty());

        t.row(vec![
            color_style.paint(Tone::Strong, "web-prod"),
            color_style.paint(Tone::Ok, "✓ ok"),
            "12 ms".into(),
        ]);
        t.row(vec![
            "db".into(),
            color_style.paint(Tone::Bad, "✗ failed"),
            "1200 ms".into(),
        ]);
        assert!(!t.is_empty());

        let rendered = t.render_with_width(&color_style, 80);
        let plain_lines: Vec<String> = rendered.lines().map(strip_ansi).collect();
        assert_eq!(plain_lines.len(), 4);
        assert_eq!(plain_lines[0], "NAME      STATUS    LATENCY");
        assert_eq!(plain_lines[1], "─".repeat(plain_lines[0].width()));
        assert_eq!(plain_lines[2], "web-prod  ✓ ok        12 ms");
        assert_eq!(plain_lines[3], "db        ✗ failed  1200 ms");
    }

    #[test]
    fn table_clamps_to_max_width_and_preserves_ansi_reset() {
        let color_style = Style {
            color: true,
            unicode: true,
        };
        let mut t = Table::new(&["Host", "Description"]);
        t.row(vec![
            "web-prod".into(),
            color_style.paint(
                Tone::Dim,
                "a very long description that exceeds the available width budget",
            ),
        ]);

        let rendered = t.render_with_width(&color_style, 30);
        for line in rendered.lines() {
            assert!(
                display_width(line) <= 30,
                "line exceeded 30 cols: {:?} (width {})",
                strip_ansi(line),
                display_width(line)
            );
        }
        let data_line = rendered.lines().nth(2).unwrap();
        assert!(data_line.ends_with("\x1b[0m"));
        assert!(strip_ansi(data_line).ends_with('…'));
    }

    #[test]
    fn table_ascii_fallback_uses_ascii_rule_and_ellipsis() {
        let ascii = Style {
            color: false,
            unicode: false,
        };
        let mut t = Table::new(&["Name", "Host"]);
        t.row(vec![
            "app".into(),
            "very-long-hostname.internal.example.com".into(),
        ]);

        let rendered = t.render_with_width(&ascii, 24);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].chars().all(|c| c == '-'));
        assert!(lines[2].ends_with("..."));
        assert!(lines.iter().all(|l| display_width(l) <= 24));
    }

    #[test]
    fn kv_list_pads_keys_uniformly() {
        let plain = Style::plain();
        let mut kv = KvList::new();
        assert!(kv.is_empty());
        kv.push("ID", "42");
        kv.push("Agent forwarding", "Disabled");
        kv.push("Host", "10.0.1.5:22");
        assert!(!kv.is_empty());

        let out = kv.render(&plain);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines,
            vec![
                "  ID                42",
                "  Agent forwarding  Disabled",
                "  Host              10.0.1.5:22",
            ]
        );

        let colored = Style {
            color: true,
            unicode: true,
        };
        let colored_lines: Vec<String> = kv.render(&colored).lines().map(strip_ansi).collect();
        assert_eq!(colored_lines, lines);
    }

    #[test]
    fn ids_and_durations_read_like_people_write_them() {
        assert_eq!(short_id("change-696de2f2"), "696de2f2");
        assert_eq!(short_id("dddbee19-f151-4ee0-8473-93924f35e1d4"), "dddbee19");
        assert_eq!(duration_ms(182), "0.2 s");
        assert_eq!(duration_ms(12_000), "12 s");
        assert_eq!(duration_ms(110_000), "1 min 50 s");
    }

    #[test]
    fn expiry_and_age_are_relative() {
        let now = DateTime::parse_from_rfc3339("2026-10-24T17:24:50Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(expires_in("2026-10-24T17:29:31Z", now), "4:41");
        assert_eq!(expires_in("2026-10-24T17:00:00Z", now), "expired");
        assert_eq!(ago("2026-10-24T17:22:50Z", now), "2 min ago");
    }

    #[test]
    fn wrap_and_truncate_respect_width() {
        assert_eq!(truncate("abcdefghij", 6), "abcde…");
        assert_eq!(truncate("服务器生产环境", 3), "服…");
        assert_eq!(truncate("服务器生产环境", 5), "服务…");
        assert_eq!(truncate("abc", 0), "");
        let lines = wrap("one two three four five six", 10, 2);
        assert!(lines.iter().all(|l| l.width() <= 10), "{lines:?}");
        assert!(lines[1].starts_with("  "));
    }

    #[test]
    fn clock_hm_formats_hours_and_minutes() {
        let ts = "2026-10-24T17:24:31Z";
        assert_eq!(&clock(ts)[..5], &clock_hm(ts));
    }
}
