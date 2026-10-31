//! Plain-words description of an approval request, as the client shows it.

use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::effects;
use crate::services::ai::mcp::tools;
use crate::services::ai::types::Analyzer;
use crate::services::ai::types::{
    Analysis, ApprovalRequest, ApprovalSubject, ChangePlan, ServiceAction, StepAction,
};
use crate::services::ai::ui;
use serde_json::Value;

/// Text shown by the client. The agent's own words are labelled as such.
fn plan_effects(plan: &ChangePlan, analyses: &[Option<Analysis>]) -> effects::Effects {
    use effects::{Effect, Effects, Verb};
    let mut combined = Effects::default();
    for (i, step) in plan.steps.iter().enumerate() {
        if let Some(Some(a)) = analyses.get(i) {
            let e = effects::effects(a);
            combined.items.extend(e.items);
            combined.deletes |= e.deletes;
            combined.sudo |= e.sudo;
            combined.network |= e.network;
            if combined.not_understood.is_none() {
                combined.not_understood = e.not_understood;
            }
        } else {
            match &step.action {
                StepAction::Run { command } => {
                    let a = ShellAnalyzer.analyze(command);
                    let e = effects::effects(&a);
                    combined.items.extend(e.items);
                    combined.deletes |= e.deletes;
                    combined.sudo |= e.sudo;
                    combined.network |= e.network;
                    if combined.not_understood.is_none() {
                        combined.not_understood = e.not_understood;
                    }
                }
                StepAction::EditFile { path, .. } => {
                    combined.items.push(Effect {
                        verb: Verb::Edits,
                        targets: vec![path.clone()],
                        dest: None,
                    });
                }
                StepAction::Service { unit, action } => {
                    let verb = match action {
                        ServiceAction::Start => Verb::Starts,
                        ServiceAction::Stop => Verb::Stops,
                        ServiceAction::Restart => Verb::Restarts,
                        ServiceAction::Reload => Verb::Reloads,
                    };
                    combined.items.push(Effect {
                        verb,
                        targets: vec![unit.clone()],
                        dest: None,
                    });
                }
                StepAction::Undo { step } => {
                    combined.items.push(Effect {
                        verb: Verb::Runs,
                        targets: vec![format!("undo {step}")],
                        dest: None,
                    });
                }
            }
        }
    }
    combined
}

/// Text shown by the client. The agent's own words are labelled as such.
pub fn describe_request(req: &ApprovalRequest) -> String {
    let what = match &req.subject {
        ApprovalSubject::Call { call, analysis } => {
            let host = match call.host.as_deref() {
                Some(h) => h.to_string(),
                // `run_many` names its hosts in the arguments (one approval for all of them).
                None => call
                    .args
                    .get("hosts")
                    .and_then(Value::as_array)
                    .map(|hosts| {
                        hosts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "-".into()),
            };
            let tool = tools::name_str(call.tool);
            let target = call
                .command
                .as_deref()
                .or(call.path.as_deref())
                .unwrap_or("");
            let eff_str = analysis
                .as_ref()
                .map(|a| {
                    let eff = effects::effects(a);
                    format!("\n{}", effects::summary(&eff))
                })
                .unwrap_or_default();
            format!("{tool} on {host}: {target}{eff_str}")
        }
        ApprovalSubject::Plan {
            plan,
            plan_hash,
            analyses,
        } => {
            let hash_short = ui::short_id(plan_hash);
            let steps_len = plan.steps.len();
            let steps_word = if steps_len == 1 { "step" } else { "steps" };
            let eff = plan_effects(plan, analyses);
            let eff_summary = effects::summary(&eff);
            format!(
                "change plan '{}' ({} {}, hash {})\n{}\nthe agent says: {}",
                plan.title, steps_len, steps_word, hash_short, eff_summary, plan.reason
            )
        }
        ApprovalSubject::Access {
            scope,
            reason,
            duration,
        } => {
            let mut parts = Vec::new();
            if !scope.tags.is_empty() {
                parts.push(format!("tags {}", scope.tags.join(", ")));
            }
            if !scope.hosts.is_empty() {
                parts.push(format!("hosts {}", scope.hosts.join(", ")));
            }
            let scope_str = if parts.is_empty() {
                "all hosts".to_string()
            } else {
                parts.join(" · ")
            };
            format!("wider access to {scope_str} for {duration}\nthe agent says: {reason}")
        }
    };
    format!("{} requests approval:\n{what}", req.client)
}
