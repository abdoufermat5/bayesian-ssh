//! Run reports, recordings of agent output, bssh ai log / replay.
//!
//! Reports are built from the audit log only, never by a model.

mod build;
mod casts;
mod model;
mod render;
mod runs;

pub use build::{build_report, resolve_run_id, run_report};
pub use casts::{cast_path, record_output, run_casts, safe_part};
pub use model::{ReportApproval, ReportCall, ReportChange, ReportStep, RunReport};
pub use render::render_report;
pub use runs::{RunSummary, list_runs, render_runs};

#[cfg(test)]
mod tests;
