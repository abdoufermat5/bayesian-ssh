//! In-memory plan store, change records and their on-disk persistence.

use crate::services::ai::ai_dir;
use crate::services::ai::mcp::context::RunContext;
use crate::services::ai::types::ChangePlan;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::warn;

// ---------------------------------------------------------------- store

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    Approved,
    Applying,
    Applied,
    Failed,
    Undone,
}

/// A file edit that can be undone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditBackup {
    pub step_id: String,
    pub host: String,
    pub path: String,
    /// Original content on this machine (`<ai_dir>/backups/<run>/<change>/<step>.orig`).
    pub local_backup: String,
    /// Copy on the host.
    pub remote_backup: String,
    pub original_sha256: String,
    pub new_sha256: String,
    pub mode: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub id: String,
    pub host: Option<String>,
    pub kind: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub output_sha256: Option<String>,
    /// Remote text, redacted and cut. Untrusted.
    pub untrusted_output: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub steps: Vec<StepRecord>,
    pub verify: Vec<StepRecord>,
    pub edits: Vec<EditBackup>,
    pub undo: Vec<StepRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanEntry {
    pub plan: ChangePlan,
    pub approved_hash: String,
    pub approved_by: String,
    pub state: PlanState,
    pub record: ChangeRecord,
}

/// Plans of one run (in memory; the change record is also written to `<ai_dir>/changes/`).
#[derive(Default)]
pub struct PlanStore {
    inner: Mutex<HashMap<String, PlanEntry>>,
}

impl PlanStore {
    pub fn insert(&self, entry: PlanEntry) {
        if let Ok(mut m) = self.inner.lock() {
            m.insert(entry.plan.change_id.clone(), entry);
        }
    }

    pub fn get(&self, change_id: &str) -> Option<PlanEntry> {
        self.inner.lock().ok()?.get(change_id).cloned()
    }

    pub fn update<R>(&self, change_id: &str, f: impl FnOnce(&mut PlanEntry) -> R) -> Option<R> {
        let mut m = self.inner.lock().ok()?;
        m.get_mut(change_id).map(f)
    }

    /// (change id, state, title) of every plan in this run.
    pub fn list(&self) -> Vec<(String, PlanState, String)> {
        let Ok(m) = self.inner.lock() else {
            return Vec::new();
        };
        let mut v: Vec<_> = m
            .values()
            .map(|e| (e.plan.change_id.clone(), e.state, e.plan.title.clone()))
            .collect();
        v.sort();
        v
    }

    /// Test hook: change the stored plan after approval.
    #[cfg(test)]
    pub fn tamper_title(&self, change_id: &str, title: &str) {
        self.update(change_id, |e| e.plan.title = title.to_string());
    }
}

pub(super) fn backups_dir(ctx: &RunContext, change_id: &str) -> PathBuf {
    ai_dir(&ctx.config)
        .join("backups")
        .join(&ctx.run_id)
        .join(change_id)
}

pub(super) fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    std::fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Best effort: the change record (no file contents) next to the backups.
pub(super) fn persist_record(ctx: &RunContext, change_id: &str) {
    let Some(entry) = ctx.plans.get(change_id) else {
        return;
    };
    let path = ai_dir(&ctx.config)
        .join("changes")
        .join(&ctx.run_id)
        .join(format!("{change_id}.json"));
    let body = json!({
        "change_id": change_id,
        "run_id": ctx.run_id,
        "plan": entry.plan,
        "plan_hash": entry.approved_hash,
        "approved_by": entry.approved_by,
        "state": entry.state,
        "record": entry.record,
    });
    match serde_json::to_vec_pretty(&body) {
        Ok(b) => {
            if let Err(e) = write_private(&path, &b) {
                warn!("change record {}: {e}", path.display());
            }
        }
        Err(e) => warn!("change record: {e}"),
    }
}
