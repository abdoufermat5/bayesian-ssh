//! `edit_file`: fetch, patch, back up, write and read back one file.

use crate::services::ai::mcp::context::{RunContext, ToolError, ToolResult};
use crate::services::ai::mcp::util::{sh_quote, sha256_hex, short_id};
use crate::services::ai::types::{
    AiEvent, ChangePlan, ExecLimits, ExecMode, Step, StepAction, ToolCall, ToolName,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

use super::apply::exec_ok;
use super::input::{apply_unified_diff, check_patch};
use super::store::{
    ChangeRecord, EditBackup, PlanEntry, PlanState, backups_dir, persist_record, write_private,
};

/// Largest file `edit_file` will fetch and rewrite.
pub const MAX_EDIT_BYTES: usize = 2 * 1024 * 1024;
pub const REMOTE_BACKUP_ROOT: &str = "/var/tmp/bssh-agent-backups";

// ---------------------------------------------------------------- edit_file

fn short_name(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or("file")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Fetch, patch, back up (host and local), write, read back. Returns the undo information.
pub async fn do_edit(
    ctx: &Arc<RunContext>,
    conn: &crate::models::Connection,
    call: &ToolCall,
    change_id: &str,
    step_id: &str,
    path: &str,
    patch: &str,
) -> Result<EditBackup, ToolError> {
    let orig = ctx
        .deps
        .files
        .read(conn, path, 0, MAX_EDIT_BYTES)
        .await
        .map_err(|e| ToolError::Failed(format!("{}: {e:#}", conn.name)))?;
    if orig.size as usize > MAX_EDIT_BYTES {
        return Err(ToolError::Invalid(format!(
            "{path} is larger than {MAX_EDIT_BYTES} bytes; edit_file is for config files"
        )));
    }
    let text = String::from_utf8(orig.data.clone())
        .map_err(|_| ToolError::Invalid(format!("{path} is not UTF-8 text")))?;
    let patched =
        apply_unified_diff(&text, patch).map_err(|e| ToolError::Invalid(e.to_string()))?;
    let original_sha = sha256_hex(&orig.data);

    // Back up before touching anything: locally, then on the host.
    let dir = backups_dir(ctx, change_id);
    let local = dir.join(format!("{step_id}.orig"));
    write_private(&local, &orig.data)
        .map_err(|e| ToolError::Failed(format!("local backup {}: {e}", local.display())))?;
    let remote_dir = format!("{REMOTE_BACKUP_ROOT}/{}/{change_id}", ctx.run_id);
    let remote_file = format!("{remote_dir}/{step_id}-{}", short_name(path));
    let cmd = format!(
        "mkdir -p -- {} && cp -p -- {} {}",
        sh_quote(&remote_dir),
        sh_quote(path),
        sh_quote(&remote_file)
    );
    let bcall = ctx.new_call(
        ToolName::EditFile,
        Some(&conn.name),
        Some(cmd.clone()),
        Some(path.to_string()),
        json!({"backup_for": step_id}),
    );
    let ex = ctx
        .exec(
            conn,
            &bcall,
            &cmd,
            ExecMode::Plain,
            ExecLimits {
                timeout: Duration::from_secs(30),
                ..ExecLimits::default()
            },
        )
        .await?;
    if !exec_ok(&ex) {
        return Err(ToolError::Failed(format!(
            "could not back up {path} on {}; nothing was changed",
            conn.name
        )));
    }

    ctx.deps
        .files
        .write(conn, path, patched.as_bytes(), orig.mode)
        .await
        .map_err(|e| ToolError::FailedAfterRun(format!("{}: write {path}: {e:#}", conn.name)))?;
    let new_sha = sha256_hex(patched.as_bytes());
    // Read back to confirm what the host holds.
    let back = ctx
        .deps
        .files
        .read(conn, path, 0, MAX_EDIT_BYTES)
        .await
        .map_err(|e| ToolError::FailedAfterRun(format!("{}: verify {path}: {e:#}", conn.name)))?;
    if sha256_hex(&back.data) != new_sha {
        return Err(ToolError::FailedAfterRun(format!(
            "{path} on {} does not match the patched content after writing; use undo",
            conn.name
        )));
    }
    ctx.emit_result(call, None, new_sha.clone(), Vec::new(), None);
    Ok(EditBackup {
        step_id: step_id.to_string(),
        host: conn.name.clone(),
        path: path.to_string(),
        local_backup: local.display().to_string(),
        remote_backup: remote_file,
        original_sha256: original_sha,
        new_sha256: new_sha,
        mode: orig.mode,
    })
}

/// Write the saved original back.
pub async fn restore_edit(
    ctx: &Arc<RunContext>,
    conn: &crate::models::Connection,
    b: &EditBackup,
) -> Result<(), ToolError> {
    let data = std::fs::read(&b.local_backup)
        .map_err(|e| ToolError::Failed(format!("read backup {}: {e}", b.local_backup)))?;
    if sha256_hex(&data) != b.original_sha256 {
        return Err(ToolError::Failed(format!(
            "backup {} was modified; refusing to restore it",
            b.local_backup
        )));
    }
    ctx.deps
        .files
        .write(conn, &b.path, &data, b.mode)
        .await
        .map_err(|e| {
            ToolError::FailedAfterRun(format!("{}: restore {}: {e:#}", conn.name, b.path))
        })?;
    Ok(())
}

/// Standalone `edit_file`: one approved call, one undoable change.
pub async fn edit_file(ctx: &Arc<RunContext>, host: &str, path: &str, patch: &str) -> ToolResult {
    let path = ctx.clean_path(path)?;
    check_patch(patch).map_err(|e| ToolError::Invalid(e.to_string()))?;
    let change_id = format!("edit-{}", short_id());
    let call = ctx.new_call(
        ToolName::EditFile,
        Some(host),
        None,
        Some(path.clone()),
        json!({ "host": host, "path": path, "patch": patch, "change_id": change_id }),
    );
    if let Some(why) = ctx.path_denied(&path) {
        return Err(ctx.deny_call(&call, &format!("{path}: {why}")));
    }
    let conn = ctx.resolve_host(host).await?;
    let facts = ctx.facts(&conn).await;
    // Before asking: a human must never approve work the budget then refuses.
    ctx.consume_change()?;
    ctx.authorize(&call, None, &facts).await?;

    let step = Step {
        id: "step-1".into(),
        host: Some(host.to_string()),
        action: StepAction::EditFile {
            path: path.clone(),
            patch: patch.to_string(),
        },
    };
    let plan = ChangePlan {
        change_id: change_id.clone(),
        run_id: ctx.run_id.clone(),
        title: format!("edit {path} on {host}"),
        reason: String::new(),
        steps: vec![step],
        verify: vec![],
        rollback: vec![Step {
            id: "rollback-1".into(),
            host: None,
            action: StepAction::Undo {
                step: "step-1".into(),
            },
        }],
    };
    let hash = plan.hash();
    let backup = do_edit(ctx, &conn, &call, &change_id, "step-1", &path, patch).await;
    let backup = match backup {
        Ok(b) => b,
        Err(e) => {
            let ev = AiEvent::Change {
                run_id: ctx.run_id.clone(),
                change_id: change_id.clone(),
                plan_hash: hash,
                applied: false,
                undo_point: None,
            };
            let _ = ctx.emit(&ev);
            return Err(e);
        }
    };
    let undo_point = backups_dir(ctx, &change_id).display().to_string();
    ctx.plans.insert(PlanEntry {
        plan,
        approved_hash: hash.clone(),
        approved_by: "call approval".into(),
        state: PlanState::Applied,
        record: ChangeRecord {
            edits: vec![backup.clone()],
            ..Default::default()
        },
    });
    persist_record(ctx, &change_id);
    let ev = AiEvent::Change {
        run_id: ctx.run_id.clone(),
        change_id: change_id.clone(),
        plan_hash: hash,
        applied: true,
        undo_point: Some(undo_point.clone()),
    };
    if let Err(e) = ctx.emit(&ev) {
        warn!("audit Change: {e:#}");
    }
    Ok(json!({
        "change_id": change_id,
        "host": host,
        "path": path,
        "status": "applied",
        "original_sha256": backup.original_sha256,
        "new_sha256": backup.new_sha256,
        "remote_backup": backup.remote_backup,
        "undo_point": undo_point,
        "next": "call undo with this change_id to restore the original",
    }))
}
