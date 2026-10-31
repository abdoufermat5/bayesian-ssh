//! The feed: one aligned line per event, for `bssh ai watch` now and `bssh ai log <run>` later.
//!
//! Everything here is a pure function over [`AiEvent`](crate::services::ai::types::AiEvent) (no IO, no clock): the caller passes
//! the time an event happened, and a [`FeedContext`] remembers what later lines need (which
//! call a result belongs to, which request an approval answers, counts for the run-ended line).
//!
//! ```text
//! 17:23:42  ✓ read          web-1   du -xah /var/www | sort -rh | head -40
//!           … 22 more reads, all allowed
//! 17:23:58  ✗ read          web-1   touch /var/tmp/.bssh_write_test    denied  writes in a read-only call
//! ```

mod cards;
mod context;
mod fold;
mod line;
mod words;

pub use cards::{RunHeader, request_lines, run_header};
pub use context::FeedContext;
pub use fold::Folder;
pub use line::{FeedLine, event_feed, event_line, event_run_id};
pub use words::kind_word;

#[cfg(test)]
mod tests;
