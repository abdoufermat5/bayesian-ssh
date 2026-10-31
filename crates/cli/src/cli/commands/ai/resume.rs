//! `bssh ai resume`.
//!
//! Clears a circuit-breaker pause over the control socket. The run keeps its history,
//! approvals and grants; only the pause and its denial window are reset. An explicit
//! `bssh ai stop` is final and cannot be resumed.

use crate::cli::parser_ai::AiArgsResume;
use crate::config::AppConfig;
use crate::services::ai::ai_dir;
use crate::services::ai::control_client::connect_all;
use crate::services::ai::ui::{self, Glyph, Style};
use anyhow::{Result, bail};

/// Resume paused runs over their control sockets.
///
/// # Errors
/// Returns an error when no run was resumed.
pub async fn execute(args: AiArgsResume, config: AppConfig) -> Result<()> {
    let mut clients = connect_all(&ai_dir(&config), "bssh-cli").await;
    if clients.is_empty() {
        println!("{}", render_no_runs());
        return Ok(());
    }
    let ids: Vec<String> = clients.iter().map(|c| c.hello.run_id.clone()).collect();
    let chosen =
        super::stop::select_runs(&ids, super::stop::RunSelection::One(args.run.as_deref()))?;
    let style = Style::terminal();
    let mut resumed = 0;
    for idx in chosen {
        let c = &mut clients[idx];
        match c.resume(&whoami::username()).await {
            Ok(()) => {
                println!(
                    "{}",
                    render_resumed(&style, &c.hello.run_id, &c.hello.client)
                );
                resumed += 1;
            }
            Err(e) => eprintln!(
                "{}",
                render_resume_error(&style, &c.hello.run_id, &e.to_string())
            ),
        }
    }
    if resumed == 0 {
        bail!("no run was resumed");
    }
    Ok(())
}

pub fn render_no_runs() -> &'static str {
    "No agents are running."
}

pub fn render_resumed(style: &Style, run_id: &str, client: &str) -> String {
    format!(
        "{} Resumed run {} ({}); it can make tool calls again.",
        style.mark(Glyph::Ok),
        ui::short_id(run_id),
        client
    )
}

pub fn render_resume_error(style: &Style, run_id: &str, err: &str) -> String {
    format!(
        "{} Could not resume run {}: {err}",
        style.mark(Glyph::Bad),
        ui::short_id(run_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_resume_messages() {
        let s = Style::plain();
        assert_eq!(
            render_resumed(&s, "dddbee19-f151-4ee0-8473-93924f35e1d4", "opencode 2.1.2"),
            "✓ Resumed run dddbee19 (opencode 2.1.2); it can make tool calls again."
        );
        assert_eq!(
            render_resume_error(&s, "dddbee19-f151-4ee0-8473-93924f35e1d4", "not paused"),
            "✗ Could not resume run dddbee19: not paused"
        );
        assert_eq!(render_no_runs(), "No agents are running.");
    }
}
