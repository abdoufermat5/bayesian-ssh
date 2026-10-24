//! The look shared by every bssh AI surface: approval cards, `bssh ai watch`,
//! logs, reports, `explain`, and the text an agent's client shows.
//!
//! Color carries meaning only (decisions and risk), always next to a word or a
//! glyph. Colors are the 16 ANSI slots, so they follow the user's terminal
//! theme. Output for pipes, `NO_COLOR` and the MCP text content is plain.

use crate::services::ai::types::{Decision, RiskTag};
use chrono::{DateTime, Local, Utc};
use std::io::IsTerminal;
use unicode_width::UnicodeWidthStr;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Ask,
    Deny,
}

impl From<&Decision> for Verdict {
    fn from(d: &Decision) -> Self {
        match d {
            Decision::Allow { .. } => Verdict::Allow,
            Decision::Ask { .. } => Verdict::Ask,
            Decision::Deny { .. } => Verdict::Deny,
        }
    }
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
        let color = std::io::stdout().is_terminal()
            && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
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

    /// `✓ allow`, `◆ ask`, `✗ deny`; `[allow]` without color.
    pub fn verdict(&self, v: Verdict) -> String {
        let (g, tone, word) = match v {
            Verdict::Allow => (Glyph::Ok, Tone::Ok, "allow"),
            Verdict::Ask => (Glyph::Ask, Tone::Warn, "ask"),
            Verdict::Deny => (Glyph::Bad, Tone::Bad, "deny"),
        };
        if self.color {
            self.paint(tone, &format!("{} {word}", self.glyph(g)))
        } else {
            format!("[{word}]")
        }
    }

    /// One risk tag in plain words; only destructive and privilege draw the eye.
    pub fn risk(&self, tag: RiskTag) -> String {
        let tone = match tag {
            RiskTag::Read => Tone::Dim,
            RiskTag::Destructive | RiskTag::ExfilRisk | RiskTag::Unknown => Tone::Bad,
            RiskTag::Privilege | RiskTag::ServiceChange => Tone::Warn,
            RiskTag::Network => Tone::Info,
            RiskTag::Write => Tone::Plain,
        };
        self.paint(tone, risk_word(tag))
    }

    /// Tags joined with ` · `, most serious first.
    pub fn risks<'a>(&self, tags: impl IntoIterator<Item = &'a RiskTag>) -> String {
        let mut tags: Vec<RiskTag> = tags.into_iter().copied().collect();
        tags.sort_by_key(|t| std::cmp::Reverse(severity(*t)));
        tags.dedup();
        let sep = format!(" {} ", self.glyph(Glyph::Dot));
        tags.iter()
            .map(|t| self.risk(*t))
            .collect::<Vec<_>>()
            .join(&sep)
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

fn locale_is_utf8() -> bool {
    for var in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                let v = v.to_ascii_lowercase();
                return v.contains("utf-8") || v.contains("utf8");
            }
        }
    }
    // No locale at all: modern terminals are UTF-8.
    true
}

pub fn risk_word(tag: RiskTag) -> &'static str {
    match tag {
        RiskTag::Read => "read",
        RiskTag::Write => "write",
        RiskTag::ServiceChange => "service",
        RiskTag::Destructive => "destructive",
        RiskTag::Privilege => "privilege",
        RiskTag::Network => "network",
        RiskTag::ExfilRisk => "exfiltration risk",
        RiskTag::Unknown => "not understood",
    }
}

fn severity(tag: RiskTag) -> u8 {
    match tag {
        RiskTag::Destructive => 7,
        RiskTag::Unknown => 6,
        RiskTag::ExfilRisk => 5,
        RiskTag::Privilege => 4,
        RiskTag::ServiceChange => 3,
        RiskTag::Network => 2,
        RiskTag::Write => 1,
        RiskTag::Read => 0,
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
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Pad (possibly colored) text with spaces to `w` columns.
pub fn pad(s: &str, w: usize) -> String {
    let n = display_width(s);
    if n >= w {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(w - n))
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
        assert_eq!(s.verdict(Verdict::Ask), "[ask]");
        assert_eq!(s.paint(Tone::Bad, "x"), "x");
        let c = Style {
            color: true,
            unicode: true,
        };
        assert_eq!(display_width(&c.verdict(Verdict::Deny)), "✗ deny".width());
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
    fn risks_put_the_serious_ones_first() {
        let s = Style::plain();
        let tags = [RiskTag::Write, RiskTag::Destructive, RiskTag::Privilege];
        assert_eq!(s.risks(&tags), "destructive · privilege · write");
    }

    #[test]
    fn wrap_and_truncate_respect_width() {
        assert_eq!(truncate("abcdefghij", 6), "abcde…");
        let lines = wrap("one two three four five six", 10, 2);
        assert!(lines.iter().all(|l| l.width() <= 10), "{lines:?}");
        assert!(lines[1].starts_with("  "));
    }
}
