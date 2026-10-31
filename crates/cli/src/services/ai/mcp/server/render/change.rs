//! Text renderings of change-plan tools and approval envelopes.

use super::result::render_tool_result;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::effects;
use crate::services::ai::mcp::util::fence_untrusted;
use crate::services::ai::types::Analyzer;
use crate::services::ai::ui;
use serde_json::Value;

pub(super) fn steps_to_effects(steps: &[Value]) -> effects::Effects {
    use effects::{Effect, Effects, Verb};
    let mut combined = Effects::default();
    // What the change does: its steps, not the checks (`verify-*`) or the undo (`rollback-*`),
    // so the agent reads the same summary as the approver.
    for step in steps
        .iter()
        .filter(|s| s["id"].as_str().is_none_or(|id| id.starts_with("step-")))
    {
        let kind = step["kind"].as_str().unwrap_or("");
        match kind {
            "run" => {
                if let Some(cmd) = step["detail"]["command"].as_str() {
                    let a = ShellAnalyzer.analyze(cmd);
                    let e = effects::effects_for(cmd, &a);
                    combined.items.extend(e.items);
                    combined.deletes |= e.deletes;
                    combined.sudo |= e.sudo;
                    combined.network |= e.network;
                    if combined.not_understood.is_none() {
                        combined.not_understood = e.not_understood;
                    }
                }
            }
            "edit_file" => {
                let path = step["detail"]["path"].as_str().unwrap_or("file");
                combined.items.push(Effect {
                    verb: Verb::Edits,
                    targets: vec![path.to_string()],
                    dest: None,
                });
            }
            "service" => {
                let unit = step["detail"]["unit"].as_str().unwrap_or("service");
                let action = step["detail"]["action"].as_str().unwrap_or("restart");
                let verb = match action {
                    "start" => Verb::Starts,
                    "stop" => Verb::Stops,
                    "reload" => Verb::Reloads,
                    _ => Verb::Restarts,
                };
                combined.items.push(Effect {
                    verb,
                    targets: vec![unit.to_string()],
                    dest: None,
                });
            }
            "undo" => {
                let restores = step["detail"]["restores"].as_str().unwrap_or("step");
                combined.items.push(Effect {
                    verb: Verb::Runs,
                    targets: vec![format!("undo {restores}")],
                    dest: None,
                });
            }
            _ => {}
        }
    }
    combined
}

pub(super) fn render_propose(v: &Value) -> String {
    let change_id = v["change_id"].as_str().unwrap_or("-");
    let short_change = ui::short_id(change_id);
    let state = v["state"].as_str().unwrap_or("approved");
    let approved_by = v["approved_by"].as_str().unwrap_or("policy");

    let steps = v["steps"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let eff = steps_to_effects(steps);
    let eff_summary = effects::summary(&eff);

    if state == "waiting" {
        let req_id = v["request_id"].as_str().unwrap_or("");
        let short_req = ui::short_id(req_id);
        let expires = v["expires_at"].as_str().unwrap_or("");
        let expires_str = if !expires.is_empty() {
            format!(", expires {}", ui::clock(expires))
        } else {
            String::new()
        };
        format!(
            "Waiting for approval. Change {short_change} was sent to the person running bssh (request {short_req}{expires_str}).\n{eff_summary}\nCall status to check if it has been approved."
        )
    } else {
        format!(
            "Change {short_change} approved by {approved_by}.\n{eff_summary}\nCall apply_change with change_id '{change_id}' to run it."
        )
    }
}

pub(super) fn render_apply(v: &Value, tag: &str) -> String {
    let change_id = v["change_id"].as_str().unwrap_or("-");
    let status = v["status"].as_str().unwrap_or("applied");

    let steps = v["steps"].as_array();
    let steps_total = steps.map(|s| s.len()).unwrap_or(0);
    let steps_ok = steps
        .map(|s| {
            s.iter()
                .filter(|x| x["ok"].as_bool().unwrap_or(false))
                .count()
        })
        .unwrap_or(0);

    let verify = v["verify"].as_array();
    let verify_total = verify.map(|v| v.len()).unwrap_or(0);
    let verify_ok = verify
        .map(|v| {
            v.iter()
                .filter(|x| x["ok"].as_bool().unwrap_or(false))
                .count()
        })
        .unwrap_or(0);

    let host = steps
        .and_then(|s| s.iter().find_map(|x| x["host"].as_str()))
        .or_else(|| verify.and_then(|v| v.iter().find_map(|x| x["host"].as_str())))
        .unwrap_or("-");

    let status_line = match status {
        "applied" => {
            if verify_total > 0 {
                format!(
                    "Applied on {host} · step {steps_ok}/{steps_total} ok · checks {verify_ok}/{verify_total} ok"
                )
            } else {
                format!("Applied on {host} · step {steps_ok}/{steps_total} ok")
            }
        }
        "applied_verify_failed" => {
            format!(
                "Applied on {host} with verification failure · step {steps_ok}/{steps_total} ok · checks {verify_ok}/{verify_total} ok"
            )
        }
        _ => {
            let failed = v["failed_step"].as_str().unwrap_or("unknown");
            format!("Failed on {host} at step {failed}")
        }
    };

    let mut out = status_line;

    if let Some(v_arr) = verify {
        for chk in v_arr {
            let chk_out = chk["untrusted_output"].as_str().unwrap_or("");
            if !chk_out.is_empty() {
                out.push('\n');
                out.push_str(&fence_untrusted(host, chk_out, tag));
            }
        }
    }

    out.push('\n');
    out.push_str(&format!(
        "To roll back, call undo with change_id '{change_id}'."
    ));
    out
}

pub(super) fn render_edit_file(v: &Value) -> String {
    let change_id = v["change_id"].as_str().unwrap_or("-");
    let host = v["host"].as_str().unwrap_or("-");
    let path = v["path"].as_str().unwrap_or("");
    format!(
        "Applied edit on {host} · {path}\nTo roll back, call undo with change_id '{change_id}'."
    )
}

pub(super) fn render_undo(v: &Value, tag: &str) -> String {
    let change_id = v["change_id"].as_str().unwrap_or("-");
    let short_change = ui::short_id(change_id);
    let status = v["status"].as_str().unwrap_or("undone");

    let mut out = if status == "undone" {
        format!("Undone change {short_change} · all rollback steps ok.\nNothing left to undo.")
    } else {
        let failed = v["failed_step"].as_str().unwrap_or("unknown");
        format!("Undo failed for change {short_change} at step {failed}.")
    };

    if let Some(steps) = v["steps"].as_array() {
        for step in steps {
            let step_out = step["untrusted_output"].as_str().unwrap_or("");
            if !step_out.is_empty() {
                let host = step["host"].as_str().unwrap_or("-");
                out.push('\n');
                out.push_str(&fence_untrusted(host, step_out, tag));
            }
        }
    }

    out
}

pub(super) fn render_access(v: &Value) -> String {
    let approved_by = v["approved_by"].as_str().unwrap_or("-");
    let exp_secs = v["expires_in_secs"].as_u64().unwrap_or(0);
    let dur = ui::duration_ms(exp_secs * 1000);
    format!(
        "Access granted by {approved_by} · expires in {dur}.\nRetry the call that needed access."
    )
}

pub(super) fn render_pending(v: &Value) -> String {
    let id = v["request_id"].as_str().unwrap_or("-");
    let secs = v["expires_in_secs"].as_u64().unwrap_or(0);
    let tool = v["tool"].as_str().unwrap_or("this call");
    format!(
        "{tool} is waiting for approval · request {} · expires in {secs} s.\n\
         Approve it in the bssh desktop app or with `bssh ai watch`, then call wait_approval \
         with request_id '{id}'. Do not resend the original call.",
        ui::short_id(id)
    )
}

pub(super) fn render_wait_approval(v: &Value) -> String {
    if v["status"].as_str() == Some("pending") {
        return render_pending(v);
    }
    let tool = v["tool"].as_str().unwrap_or("");
    format!(
        "Approved and finished.\n{}",
        render_tool_result(tool, &v["result"])
    )
}
