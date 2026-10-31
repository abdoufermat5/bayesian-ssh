//! Small conversions shared by the tool handlers.

use super::outputs::RedactionOut;
use crate::services::ai::mcp::context::failed;
use crate::services::ai::mcp::context::{ToolError, ToolResult};
use crate::services::ai::types::Redaction;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

pub(super) fn args<T: DeserializeOwned>(v: Value) -> Result<T, ToolError> {
    serde_json::from_value(v).map_err(|e| ToolError::Invalid(e.to_string()))
}

/// `exit 1`, or `no exit status` when the process never reported one.
pub(super) fn exit_text(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("exit {c}"),
        None => "no exit status".into(),
    }
}

pub(super) fn reds(r: &[Redaction]) -> Vec<RedactionOut> {
    r.iter()
        .map(|x| RedactionOut {
            kind: x.kind.clone(),
            count: x.count,
        })
        .collect()
}

pub(super) fn to_value<T: Serialize>(t: T) -> ToolResult {
    serde_json::to_value(t).map_err(failed)
}
