//! Change plans: propose, apply, verify, rollback, edit_file backups, undo.
//!
//! The approval covers `ChangePlan::hash()`. `apply` refuses any plan whose hash differs from
//! the approved one, runs exactly the approved steps, stops at the first failure and runs
//! `verify`. Multi-host plans are canaried: the first host (steps, then its verify steps)
//! must pass before the next one starts. Every edit is backed up on the host and locally
//! before it is written, so `undo` can restore it.

mod apply;
mod edit;
mod input;
mod propose;
mod store;
mod undo;

pub use apply::{apply, service_healthy};
pub use edit::{MAX_EDIT_BYTES, REMOTE_BACKUP_ROOT, do_edit, edit_file, restore_edit};
pub use input::{
    MAX_STEPS, PatchError, PlanError, ProposeInput, ServiceActionArg, StepInput, action_verb,
    apply_unified_diff, build_plan, check_patch, plan_hosts, service_command, step_host,
};
pub use propose::propose;
pub use store::{ChangeRecord, EditBackup, PlanEntry, PlanState, PlanStore, StepRecord};
pub use undo::undo;

#[cfg(test)]
mod tests;
