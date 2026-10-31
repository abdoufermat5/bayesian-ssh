//! bssh AI: the desktop side of the run sockets.
//!
//! Every `bssh ai mcp` process binds `<ai_dir>/runs/<run_id>.sock` (see
//! `services::ai::ipc`). A background task lists that directory, connects to
//! each socket (Hello, Subscribe, ListPending) and keeps a live picture of the
//! runs in [`AiHub`]. The frontend gets three Tauri events:
//!
//! * `ai://runs`: the full list of [`RunSummary`], after any change;
//! * `ai://event`: one [`StoredEvent`] of a run (`{ run_id, seq, at, event }`);
//! * `ai://request`: a new approval request (`{ run_id, request }`), which also
//!   raises a native notification and brings the main window forward.
//!
//! The approval dialog is the trusted channel (design 6.3, tier 2): the agent's
//! process can't reach it, and it shows the exact command, not a summary.

pub mod api;
mod hub;
mod sink;
#[cfg(unix)]
mod socket;
#[cfg(all(test, unix))]
mod tests;
mod types;

pub use api::start;
