//! MCP server (stdio; transport-agnostic so W8 can add streamable HTTP).
//!
//! Owner: W4. Stub from the scaffold; replace freely.

use crate::config::AppConfig;

/// Run the MCP server on stdio for policy `policy` until the client disconnects.
pub async fn serve_stdio(config: AppConfig, policy: String) -> anyhow::Result<()> {
    let _ = (config, policy);
    anyhow::bail!("mcp::serve_stdio: not implemented yet")
}
