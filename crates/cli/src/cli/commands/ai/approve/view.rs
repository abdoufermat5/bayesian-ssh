//! How much of an approval to show.

/// The card's depth: the short view, or the full text behind `v`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Short,
    Full,
}

impl View {
    /// Whether the limits on diffs, checks and "the agent says" are lifted.
    pub(super) fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }

    /// What pressing `v` does from here.
    pub(super) fn other_label(self) -> &'static str {
        match self {
            Self::Short => "full text",
            Self::Full => "short view",
        }
    }
}
