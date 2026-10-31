//! Folding runs of allowed reads into one line.

use crate::services::ai::ui::{Glyph, Style, Tone};

use super::words::{CLOCK_W, SHOWN_READS};

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
