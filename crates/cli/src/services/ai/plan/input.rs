//! Plan input as the agent writes it, and the validated `ChangePlan` built from it.

use crate::services::ai::mcp::util::{normalize_path, sh_quote, valid_unit};
use crate::services::ai::types::{ChangePlan, ServiceAction, Step, StepAction, ToolName};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_STEPS: usize = 50;

// ---------------------------------------------------------------- input

/// systemd actions the `service` tool and plan steps accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceActionArg {
    Start,
    Stop,
    Restart,
    Reload,
}

impl From<ServiceActionArg> for ServiceAction {
    fn from(a: ServiceActionArg) -> Self {
        match a {
            ServiceActionArg::Start => ServiceAction::Start,
            ServiceActionArg::Stop => ServiceAction::Stop,
            ServiceActionArg::Restart => ServiceAction::Restart,
            ServiceActionArg::Reload => ServiceAction::Reload,
        }
    }
}

pub fn action_verb(a: ServiceAction) -> &'static str {
    match a {
        ServiceAction::Start => "start",
        ServiceAction::Stop => "stop",
        ServiceAction::Restart => "restart",
        ServiceAction::Reload => "reload",
    }
}

/// One step as the agent writes it: exactly one of `run`, `edit_file`, `service`, `undo`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepInput {
    /// Connection name. Required except for `undo`.
    #[serde(default)]
    pub host: Option<String>,
    /// Shell command to run.
    #[serde(default)]
    pub run: Option<String>,
    /// Absolute path of a file to edit; needs `patch`.
    #[serde(default)]
    pub edit_file: Option<String>,
    /// Unified diff for `edit_file`.
    #[serde(default)]
    pub patch: Option<String>,
    /// systemd unit; needs `action`.
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub action: Option<ServiceActionArg>,
    /// Id of an earlier edit step to restore (`step-1`). Used in `rollback`.
    #[serde(default)]
    pub undo: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposeInput {
    pub title: String,
    /// Why. Shown to the approver labelled "the agent says".
    pub reason: String,
    pub steps: Vec<StepInput>,
    /// Read-only checks run after the steps (`curl -fsS localhost/health`).
    #[serde(default)]
    pub verify: Vec<StepInput>,
    /// Steps `undo` runs after restoring edits.
    #[serde(default)]
    pub rollback: Vec<StepInput>,
}

/// Why a change plan was refused. Step errors name the step (`step-1: ...`).
#[derive(Debug, Error)]
pub enum PlanError {
    #[error("title must be 1-200 characters")]
    BadTitle,
    #[error("a plan needs at least one step")]
    NoSteps,
    #[error("too many steps (max {})", MAX_STEPS)]
    TooManySteps,
    #[error("{id}: give exactly one of run, edit_file, service, undo")]
    StepKind { id: String },
    #[error("{id}: empty command")]
    EmptyCommand { id: String },
    #[error("{id}: edit_file needs a patch")]
    MissingPatch { id: String },
    #[error("{id}: {path}: {reason}")]
    BadPath {
        id: String,
        path: String,
        reason: String,
    },
    #[error("{id}: {source}")]
    BadPatch {
        id: String,
        #[source]
        source: PatchError,
    },
    #[error("{id}: service needs an action")]
    MissingAction { id: String },
    #[error("{id}: invalid unit name '{unit}'")]
    BadUnit { id: String, unit: String },
    #[error("{id}: empty step")]
    EmptyStep { id: String },
    #[error("{id}: patch only goes with edit_file")]
    StrayPatch { id: String },
    #[error("{id}: undo takes no host")]
    UndoWithHost { id: String },
    #[error("{id}: host is required")]
    HostRequired { id: String },
    #[error("{id}: undo target '{target}' is not an edit step")]
    BadUndoTarget { id: String, target: String },
    #[error("{id}: verify steps must be run or service checks")]
    BadVerifyStep { id: String },
}

/// Why a unified diff was refused.
#[derive(Debug, Error)]
pub enum PatchError {
    #[error("patch does not parse: {0}")]
    Parse(diffy::ParsePatchError),
    #[error("patch has no hunks")]
    NoHunks,
    #[error("patch does not apply cleanly ({0})")]
    Conflict(diffy::ApplyError),
}

impl StepInput {
    fn into_step(self, id: String) -> Result<Step, PlanError> {
        let has_patch = self.patch.is_some();
        let kinds = [
            self.run.is_some(),
            self.edit_file.is_some(),
            self.service.is_some(),
            self.undo.is_some(),
        ]
        .iter()
        .filter(|b| **b)
        .count();
        if kinds != 1 {
            return Err(PlanError::StepKind { id });
        }
        let action = if let Some(command) = self.run {
            if command.trim().is_empty() {
                return Err(PlanError::EmptyCommand { id });
            }
            StepAction::Run { command }
        } else if let Some(path) = self.edit_file {
            let Some(patch) = self.patch else {
                return Err(PlanError::MissingPatch { id });
            };
            let path = match normalize_path(&path) {
                Ok(p) => p,
                Err(e) => {
                    return Err(PlanError::BadPath {
                        id,
                        path,
                        reason: e.to_string(),
                    })
                }
            };
            if let Err(source) = check_patch(&patch) {
                return Err(PlanError::BadPatch { id, source });
            }
            StepAction::EditFile { path, patch }
        } else if let Some(unit) = self.service {
            let Some(action) = self.action else {
                return Err(PlanError::MissingAction { id });
            };
            if !valid_unit(&unit) {
                return Err(PlanError::BadUnit { id, unit });
            }
            StepAction::Service {
                unit,
                action: action.into(),
            }
        } else if let Some(step) = self.undo {
            StepAction::Undo { step }
        } else {
            return Err(PlanError::EmptyStep { id });
        };
        if has_patch && !matches!(action, StepAction::EditFile { .. }) {
            return Err(PlanError::StrayPatch { id });
        }
        if matches!(action, StepAction::Undo { .. }) {
            if self.host.is_some() {
                return Err(PlanError::UndoWithHost { id });
            }
        } else if self.host.as_deref().map(str::trim).unwrap_or("").is_empty() {
            return Err(PlanError::HostRequired { id });
        }
        Ok(Step {
            id,
            host: self.host,
            action,
        })
    }
}

fn parse_patch(patch: &str) -> Result<diffy::Patch<'_, str>, PatchError> {
    let p = diffy::Patch::from_str(patch).map_err(PatchError::Parse)?;
    if p.hunks().is_empty() {
        return Err(PatchError::NoHunks);
    }
    Ok(p)
}

/// A patch must parse and touch at least one hunk.
///
/// # Errors
/// [`PatchError::Parse`] when the text is not a unified diff, [`PatchError::NoHunks`] when it
/// changes nothing.
pub fn check_patch(patch: &str) -> Result<(), PatchError> {
    parse_patch(patch).map(|_| ())
}

/// Apply a unified diff; refuses anything that does not apply cleanly.
///
/// # Errors
/// The [`check_patch`] errors, or [`PatchError::Conflict`] when a hunk does not match
/// `original`.
pub fn apply_unified_diff(original: &str, patch: &str) -> Result<String, PatchError> {
    let p = parse_patch(patch)?;
    diffy::apply(original, &p).map_err(PatchError::Conflict)
}

/// Build a plan from agent input. Ids: `step-N`, `verify-N`, `rollback-N`.
///
/// # Errors
/// [`PlanError`] naming the first step or field that is invalid.
pub fn build_plan(
    run_id: &str,
    change_id: &str,
    input: ProposeInput,
) -> Result<ChangePlan, PlanError> {
    let title = input.title.trim().to_string();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(PlanError::BadTitle);
    }
    if input.steps.is_empty() {
        return Err(PlanError::NoSteps);
    }
    if input.steps.len() + input.verify.len() + input.rollback.len() > MAX_STEPS {
        return Err(PlanError::TooManySteps);
    }
    let conv = |list: Vec<StepInput>, prefix: &str| -> Result<Vec<Step>, PlanError> {
        list.into_iter()
            .enumerate()
            .map(|(i, s)| s.into_step(format!("{prefix}-{}", i + 1)))
            .collect()
    };
    let steps = conv(input.steps, "step")?;
    let verify = conv(input.verify, "verify")?;
    let rollback = conv(input.rollback, "rollback")?;
    // Undo steps must point at an edit step of this plan.
    for s in steps.iter().chain(&rollback) {
        if let StepAction::Undo { step } = &s.action {
            let ok = steps
                .iter()
                .any(|t| &t.id == step && matches!(t.action, StepAction::EditFile { .. }));
            if !ok {
                return Err(PlanError::BadUndoTarget {
                    id: s.id.clone(),
                    target: step.clone(),
                });
            }
        }
    }
    for s in &verify {
        if matches!(
            s.action,
            StepAction::Undo { .. } | StepAction::EditFile { .. }
        ) {
            return Err(PlanError::BadVerifyStep { id: s.id.clone() });
        }
    }
    Ok(ChangePlan {
        change_id: change_id.to_string(),
        run_id: run_id.to_string(),
        title,
        reason: input.reason,
        steps,
        verify,
        rollback,
    })
}

/// Host of a step; an `undo` step runs where the edit it restores ran.
pub fn step_host(plan: &ChangePlan, step: &Step) -> Option<String> {
    if let Some(h) = &step.host {
        return Some(h.clone());
    }
    if let StepAction::Undo { step: target } = &step.action {
        return plan
            .steps
            .iter()
            .find(|s| &s.id == target)
            .and_then(|s| s.host.clone());
    }
    None
}

/// Distinct hosts of the plan's steps in order of first use (the first is the canary).
pub fn plan_hosts(plan: &ChangePlan) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for s in &plan.steps {
        if let Some(h) = step_host(plan, s) {
            if !hosts.contains(&h) {
                hosts.push(h);
            }
        }
    }
    hosts
}

/// Command a `service` step runs. `sudo -n` unless we connect as root.
pub fn service_command(unit: &str, action: ServiceAction, sudo: bool) -> String {
    format!(
        "{}systemctl {} -- {}",
        if sudo { "sudo -n " } else { "" },
        action_verb(action),
        sh_quote(unit)
    )
}

pub(super) fn step_command(step: &Step, user: &str) -> Option<String> {
    match &step.action {
        StepAction::Run { command } => Some(command.clone()),
        StepAction::Service { unit, action } => {
            Some(service_command(unit, *action, user != "root"))
        }
        _ => None,
    }
}

pub(super) fn step_kind(step: &Step) -> &'static str {
    match step.action {
        StepAction::Run { .. } => "run",
        StepAction::EditFile { .. } => "edit_file",
        StepAction::Service { .. } => "service",
        StepAction::Undo { .. } => "undo",
    }
}

pub(super) fn step_tool(step: &Step) -> ToolName {
    match step.action {
        StepAction::Run { .. } => ToolName::Run,
        StepAction::EditFile { .. } => ToolName::EditFile,
        StepAction::Service { .. } => ToolName::Service,
        StepAction::Undo { .. } => ToolName::Undo,
    }
}
