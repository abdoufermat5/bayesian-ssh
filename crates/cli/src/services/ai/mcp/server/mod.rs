//! `BsshMcp`: the rmcp `ServerHandler`, and the MCP side of approvals (elicitation).

mod describe;
mod elicitor;
mod handler;
pub(super) mod instructions;
mod progress;
mod render;

pub use describe::describe_request;
pub use elicitor::{Confirm, McpElicitor};
pub use handler::{build_server, BsshMcp};
pub(crate) use progress::{current_progress_token, with_progress_token};
pub use render::{render_tool_error, render_tool_result, to_result};
