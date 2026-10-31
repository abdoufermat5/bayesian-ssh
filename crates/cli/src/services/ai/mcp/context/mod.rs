//! `RunContext`: everything one agent run needs, and the guarded call pipeline.
//!
//! Pipeline for every call: ToolCall -> analyze -> decide -> audit `Call` -> approval when the
//! decision is Ask -> execute -> redact -> audit `Result` -> record output -> return.
//!
//! The context is transport-agnostic. Stdio builds one per process (`RunContext::from_config`);
//! the Teams gateway builds one per session with `RunContext::new`, passing its own
//! `ApprovalBroker` (the relay broker) and `Executor` through [`RunDeps`].

mod approval;
mod call_slot;
mod calls;
mod error;
mod execution;
mod host_access;
mod inventory;
mod lifecycle;
mod limits;
mod run_context;
mod status;

pub(crate) use call_slot::with_call_slot;
pub use call_slot::{CallPhase, CallSlot};
pub use error::{failed, ResumeError, ToolError, ToolResult};
pub use execution::merge_redactions;
pub use inventory::{DbHosts, HostSource, ScopeFn, StaticHosts};
pub use run_context::{Executed, RunContext, RunDeps};
