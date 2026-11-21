//! AI-specific UI helpers built on top of [`crate::ui`].
//!
//! All shared terminal primitives (`Style`, `Tone`, `Glyph`, width/time helpers,
//! `Table`, `KvList`, `heading`) live in `crate::ui` and are re-exported here so
//! existing `services::ai::ui` callers keep working seamlessly.

use crate::services::ai::types::{Decision, RiskTag};

pub use crate::ui::*;

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

impl Style {
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

/// Tool name in human words: `list_hosts` -> `list hosts`, `run_readonly` -> `read`, etc.
pub fn tool_kind_in_words(tool: &str) -> &'static str {
    match tool {
        "list_hosts" => "list hosts",
        "host_info" => "observe",
        "observe" => "observe",
        "read_file" | "search_logs" | "run_readonly" | "run_many" => "read",
        "propose_change" => "change plan",
        "apply_change" => "apply",
        "edit_file" => "edit",
        "service" => "service",
        "run" => "run",
        "undo" => "undo",
        "request_access" => "access",
        "status" => "status",
        _ => "call",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn plain_style_verdict_has_no_escape_codes() {
        let s = Style::plain();
        assert_eq!(s.verdict(Verdict::Ask), "[ask]");
        let c = Style {
            color: true,
            unicode: true,
        };
        assert_eq!(display_width(&c.verdict(Verdict::Deny)), "✗ deny".width());
    }

    #[test]
    fn risks_put_the_serious_ones_first() {
        let s = Style::plain();
        let tags = [RiskTag::Write, RiskTag::Destructive, RiskTag::Privilege];
        assert_eq!(s.risks(&tags), "destructive · privilege · write");
    }

    #[test]
    fn tool_kind_in_words_translates_known_tools() {
        assert_eq!(tool_kind_in_words("run_readonly"), "read");
        assert_eq!(tool_kind_in_words("read_file"), "read");
        assert_eq!(tool_kind_in_words("propose_change"), "change plan");
        assert_eq!(tool_kind_in_words("service"), "service");
        assert_eq!(tool_kind_in_words("request_access"), "access");
    }
}
