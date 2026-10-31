//! The surface a card or a listing is laid out on: a width to fit into, plus the colours
//! and glyphs to draw with.

use crate::services::ai::ui::{self, Style, Tone};

/// A width to fit into, with the style to draw with.
#[derive(Debug, Clone, Copy)]
pub(super) struct Canvas<'a> {
    pub(super) style: &'a Style,
    pub(super) width: usize,
}

impl<'a> Canvas<'a> {
    pub(super) fn new(style: &'a Style, width: usize) -> Self {
        Self { style, width }
    }

    /// Left text and right text on one line, the right one against column `width`.
    pub(super) fn split_line(&self, left: &str, right: &str) -> String {
        if right.is_empty() {
            return left.to_string();
        }
        let gap = self
            .width
            .saturating_sub(ui::display_width(left) + ui::display_width(right))
            .max(2);
        format!("{left}{}{right}", " ".repeat(gap))
    }

    /// `  STEP 1 · web-1                       write`
    pub(super) fn block_header(&self, label: &str, host: &str, risk: &str) -> String {
        self.split_line(
            &format!(
                "  {} {} {}",
                self.style.label(label),
                self.style.paint(Tone::Dim, "·"),
                self.style.paint(Tone::Strong, host)
            ),
            risk,
        )
    }
}
