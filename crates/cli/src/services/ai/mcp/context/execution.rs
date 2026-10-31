//! Guarded execution: run, redact, audit the result and record the output.

use super::error::ToolError;
use super::run_context::{Executed, RunContext};
use crate::models::Connection;
use crate::services::ai::mcp::util::sha256_hex;
use crate::services::ai::report;
use crate::services::ai::types::{
    AiEvent, ExecLimits, ExecMode, ExecResult, Redaction, Redactor, ToolCall,
};
use anyhow::Result;
use serde_json::Value;
use tracing::warn;

impl RunContext {
    /// Execute, redact, audit `Result`, record output.
    pub async fn exec(
        &self,
        conn: &Connection,
        call: &ToolCall,
        command: &str,
        mode: ExecMode,
        limits: ExecLimits,
    ) -> Result<Executed, ToolError> {
        let jailed = mode.is_jailed();
        let raw = self
            .deps
            .executor
            .exec(conn, command, mode, limits)
            .await
            .map_err(|e| ToolError::Failed(format!("{}: {e:#}", conn.name)))?;
        let (stdout, r1) = self.deps.redactor.redact(&raw.stdout);
        let (stderr, r2) = self.deps.redactor.redact(&raw.stderr);
        let redactions = merge_redactions([r1, r2]);
        let result = ExecResult {
            stdout,
            stderr,
            ..raw
        };
        let digest =
            sha256_hex(format!("{}\n--stderr--\n{}", result.stdout, result.stderr).as_bytes());
        self.emit_result(
            call,
            result.exit_code,
            digest,
            redactions.clone(),
            Some((result.duration_ms, jailed)),
        );
        if let Err(e) =
            report::record_output(&self.config, &self.run_id, &call.call_id, command, &result)
        {
            warn!("record output: {e:#}");
        }
        Ok(Executed { result, redactions })
    }

    pub fn emit_result(
        &self,
        call: &ToolCall,
        exit_code: Option<i32>,
        output_sha256: String,
        redactions: Vec<Redaction>,
        timing: Option<(u64, bool)>,
    ) {
        let ev = AiEvent::Result {
            run_id: self.run_id.clone(),
            call_id: call.call_id.clone(),
            exit_code,
            output_sha256,
            redactions,
            duration_ms: timing.map(|t| t.0),
            jailed: timing.map(|t| t.1),
        };
        if let Err(e) = self.emit(&ev) {
            warn!("audit Result: {e:#}");
        }
    }

    /// Redact free text (file contents, log lines) and audit its `Result`.
    pub fn redact_text(&self, call: &ToolCall, text: &str) -> (String, Vec<Redaction>) {
        let (out, reds) = self.deps.redactor.redact(text);
        self.emit_result(call, None, sha256_hex(out.as_bytes()), reds.clone(), None);
        (out, reds)
    }

    /// Redact every string leaf of parsed remote data.
    pub fn redact_value(&self, v: &mut Value) -> Vec<Redaction> {
        let mut all = Vec::new();
        redact_leaves(self.deps.redactor.as_ref(), v, &mut all);
        merge_redactions([all])
    }
}

fn redact_leaves(r: &dyn Redactor, v: &mut Value, all: &mut Vec<Redaction>) {
    match v {
        Value::String(s) => {
            let (out, reds) = r.redact(s);
            if !reds.is_empty() {
                *s = out;
                all.extend(reds);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| redact_leaves(r, x, all)),
        Value::Object(o) => o.values_mut().for_each(|x| redact_leaves(r, x, all)),
        _ => {}
    }
}

/// Sum counts per kind.
pub fn merge_redactions<I: IntoIterator<Item = Vec<Redaction>>>(lists: I) -> Vec<Redaction> {
    let mut map: Vec<Redaction> = Vec::new();
    for r in lists.into_iter().flatten() {
        match map.iter_mut().find(|m| m.kind == r.kind) {
            Some(m) => m.count += r.count,
            None => map.push(r),
        }
    }
    map
}
