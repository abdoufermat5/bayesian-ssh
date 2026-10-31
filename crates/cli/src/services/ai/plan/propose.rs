//! `propose_change`: validate, decide every step, ask for approval of the whole plan.

use crate::services::ai::mcp::context::{RunContext, ToolError, ToolResult};
use crate::services::ai::mcp::util::short_id;
use crate::services::ai::types::{
    Analysis, ApprovalSubject, ChangePlan, Decision, HostFacts, Step, StepAction, Tier, ToolCall,
    ToolName,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

use super::input::{
    ProposeInput, action_verb, build_plan, plan_hosts, step_command, step_host, step_kind,
    step_tool,
};
use super::store::{ChangeRecord, PlanEntry, PlanState};

// ---------------------------------------------------------------- propose

fn step_summary(plan: &ChangePlan, s: &Step, analysis: Option<&Analysis>) -> Value {
    let detail = match &s.action {
        StepAction::Run { command } => json!({ "command": command }),
        StepAction::EditFile { path, patch } => json!({ "path": path, "patch": patch }),
        StepAction::Service { unit, action } => {
            json!({ "unit": unit, "action": action_verb(*action) })
        }
        StepAction::Undo { step } => json!({ "restores": step }),
    };
    json!({
        "id": s.id, "host": step_host(plan, s), "kind": step_kind(s), "detail": detail,
        "tags": analysis.map(|a| a.tags.iter().collect::<Vec<_>>()),
    })
}

pub(super) fn tool_call_for_step(
    ctx: &RunContext,
    plan: &ChangePlan,
    s: &Step,
    user: &str,
) -> ToolCall {
    let (path, args) = match &s.action {
        StepAction::EditFile { path, patch } => (
            Some(path.clone()),
            json!({ "change_id": plan.change_id, "step": s.id, "path": path, "patch": patch }),
        ),
        _ => (None, json!({ "change_id": plan.change_id, "step": s.id })),
    };
    let host = step_host(plan, s);
    ctx.new_call(
        step_tool(s),
        host.as_deref(),
        step_command(s, user),
        path,
        args,
    )
}

/// `propose_change`: validate, decide every step, ask for approval of the whole plan.
pub async fn propose(ctx: &Arc<RunContext>, input: ProposeInput) -> ToolResult {
    let change_id = format!("change-{}", short_id());
    let plan = build_plan(&ctx.run_id, &change_id, input)
        .map_err(|e| ToolError::Invalid(e.to_string()))?;

    // Hosts in scope, once each.
    let mut conns: HashMap<String, crate::models::Connection> = HashMap::new();
    for s in plan.steps.iter().chain(&plan.verify).chain(&plan.rollback) {
        if let Some(h) = &s.host
            && !conns.contains_key(h)
        {
            conns.insert(h.clone(), ctx.resolve_host(h).await?);
        }
    }
    let user_of = |s: &Step| -> String {
        step_host(&plan, s)
            .and_then(|h| conns.get(&h).map(|c| c.user.clone()))
            .unwrap_or_default()
    };

    // Deny paths apply to edits.
    let first_host = plan_hosts(&plan).into_iter().next();
    let propose_call = ctx.new_call(
        ToolName::ProposeChange,
        first_host.as_deref(),
        None,
        None,
        json!({ "change_id": plan.change_id, "title": plan.title, "plan_hash": plan.hash() }),
    );
    for s in plan.steps.iter().chain(&plan.rollback) {
        if let StepAction::EditFile { path, .. } = &s.action
            && let Some(why) = ctx.path_denied(path)
        {
            return Err(ctx.deny_call(&propose_call, &format!("{}: {path}: {why}", s.id)));
        }
    }

    // Analyze every run-like step.
    let all: Vec<&Step> = plan
        .steps
        .iter()
        .chain(&plan.verify)
        .chain(&plan.rollback)
        .collect();
    let analyses: Vec<Option<Analysis>> = all
        .iter()
        .map(|s| step_command(s, &user_of(s)).map(|c| ctx.deps.analyzer.analyze(&c)))
        .collect();

    let facts0 = match first_host.as_ref().and_then(|h| conns.get(h)) {
        Some(c) => ctx.facts(c).await,
        None => HostFacts::default(),
    };

    let mut tier: Option<Tier> = None;
    let mut bump = |t: Tier| tier = Some(tier.map_or(t, |c| c.max(t)));
    match ctx.decide_only(&propose_call, None, &facts0)? {
        Decision::Allow { .. } => {}
        Decision::Deny { reason } => {
            ctx.note_denial(&reason);
            return Err(ToolError::Denied(reason));
        }
        Decision::Ask { tier, .. } => bump(tier),
    }
    // Each step must be allowed or askable; the one plan approval covers the asks.
    for (s, a) in all.iter().zip(&analyses) {
        let host_facts = match step_host(&plan, s).and_then(|h| conns.get(&h)) {
            Some(c) => ctx.facts(c).await,
            None => HostFacts::default(),
        };
        let call = tool_call_for_step(ctx, &plan, s, &user_of(s));
        match ctx.decide_only(&call, a.as_ref(), &host_facts)? {
            Decision::Allow { .. } => {}
            Decision::Deny { reason } => {
                let msg = format!("{}: {reason}", s.id);
                ctx.note_denial(&msg);
                return Err(ToolError::Denied(msg));
            }
            Decision::Ask { tier, .. } => bump(tier),
        }
    }

    let plan_hash = plan.hash();
    let approved_by = match tier {
        None => "policy".to_string(),
        Some(t) => {
            let subject = ApprovalSubject::Plan {
                plan: plan.clone(),
                plan_hash: plan_hash.clone(),
                analyses: analyses.clone(),
            };
            ctx.request_approval(t, subject, &plan.title).await?
        }
    };

    let summaries: Vec<Value> = all
        .iter()
        .zip(&analyses)
        .map(|(s, a)| step_summary(&plan, s, a.as_ref()))
        .collect();
    ctx.plans.insert(PlanEntry {
        plan: plan.clone(),
        approved_hash: plan_hash.clone(),
        approved_by: approved_by.clone(),
        state: PlanState::Approved,
        record: ChangeRecord::default(),
    });
    Ok(json!({
        "change_id": change_id,
        "plan_hash": plan_hash,
        "state": "approved",
        "approved_by": approved_by,
        "hosts": plan_hosts(&plan),
        "steps": summaries,
        "next": "call apply_change with this change_id",
    }))
}
