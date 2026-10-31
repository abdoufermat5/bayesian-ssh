//! The client's progress token for the tool call being served.

use rmcp::model::ProgressToken;
use std::future::Future;

tokio::task_local! {
    /// The client's progress token for the tool call being served, when it sent one.
    static PROGRESS_TOKEN: ProgressToken;
}

/// The progress token of the MCP call currently running, if the client supplied one.
pub(crate) fn current_progress_token() -> Option<ProgressToken> {
    PROGRESS_TOKEN.try_with(|t| t.clone()).ok()
}

/// Run `fut` with `token` as the current call's progress token (also across spawned tasks).
pub(crate) async fn with_progress_token<F: Future>(
    token: Option<ProgressToken>,
    fut: F,
) -> F::Output {
    match token {
        Some(token) => PROGRESS_TOKEN.scope(token, fut).await,
        None => fut.await,
    }
}
