//! "What this does": a plain account of a command's effects, read from the
//! analyzer's parse, never from the agent. Approval cards, `bssh ai watch` and
//! the agent's own results all show it, so a person can approve a one-line
//! script without reading it character by character.
//!
//! It is shown as fact, so it must never under-report: anything that is not
//! read-only and not recognised appears as `runs <program>`, and `deletes` is
//! true whenever a delete happens or can't be ruled out.

mod analyze;
mod lines;
mod model;
mod programs;
mod summary;
mod system;
mod vars;

pub use analyze::{effects, effects_for};
pub use lines::command_lines;
pub use model::{Effect, Effects, Verb};
pub use summary::summary;

#[cfg(test)]
mod tests;
