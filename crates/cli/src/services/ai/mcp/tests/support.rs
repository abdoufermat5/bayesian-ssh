//! Shared helpers for the handler tests.

use crate::services::ai::mcp::context::ToolError;
use crate::services::ai::mcp::testkit::{allow_all, conn, kit, policy, Kit};
use crate::services::ai::mcp::tools;
use serde_json::Value;

pub(super) async fn call(k: &Kit, tool: &str, args: Value) -> Result<Value, ToolError> {
    tools::call(&k.ctx, tool, args).await
}

pub(super) fn hosts() -> Vec<crate::models::Connection> {
    vec![
        conn("web-1", &["prod", "web"]),
        conn("web-2", &["prod", "web"]),
        conn("vault-1", &["hidden"]),
    ]
}

pub(super) fn std_kit() -> Kit {
    kit(policy(), allow_all(), hosts())
}

pub(super) const CONF: &str = "listen 80;\nproxy_pass http://127.0.0.1:8080;\n";
pub(super) const PATCH: &str = "--- a/app.conf\n+++ b/app.conf\n@@ -1,2 +1,2 @@\n listen 80;\n-proxy_pass http://127.0.0.1:8080;\n+proxy_pass http://127.0.0.1:8081;\n";
pub(super) const PATCHED: &str = "listen 80;\nproxy_pass http://127.0.0.1:8081;\n";
pub(super) const CONF_PATH: &str = "/etc/nginx/conf.d/app.conf";
