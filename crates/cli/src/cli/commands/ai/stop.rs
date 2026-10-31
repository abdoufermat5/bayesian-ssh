//! `bssh ai stop`.
//!
//! Sends `Stop` to one run (or all of them) over the control socket. The run ends and its
//! socket disappears; certificates stop being issued, so exposure is at most one cert TTL.

use crate::cli::parser_ai::AiArgsStop;
use crate::config::AppConfig;
use crate::services::ai::ai_dir;
use crate::services::ai::control_client::{ControlClient, connect_all};
use crate::services::ai::ui::{self, Glyph, Style};
use anyhow::{Result, bail};

/// Stop one run, or every run, over its control socket.
///
/// # Errors
/// Returns an error when no run was stopped.
pub async fn execute(args: AiArgsStop, config: AppConfig) -> Result<()> {
    let mut clients = connect_all(&ai_dir(&config), "bssh-cli").await;
    if clients.is_empty() {
        println!("{}", render_no_runs());
        return Ok(());
    }
    let ids: Vec<String> = clients.iter().map(|c| c.hello.run_id.clone()).collect();
    let selection = if args.all {
        RunSelection::All
    } else {
        RunSelection::One(args.run.as_deref())
    };
    let chosen = select_runs(&ids, selection)?;
    let mut stopped = 0;
    let style = Style::terminal();
    for idx in chosen {
        let c: &mut ControlClient = &mut clients[idx];
        match c.stop("stopped with `bssh ai stop`").await {
            Ok(()) => {
                println!(
                    "{}",
                    render_stopped(&style, &c.hello.run_id, &c.hello.client)
                );
                stopped += 1;
            }
            Err(e) => eprintln!(
                "{}",
                render_stop_error(&style, &c.hello.run_id, &e.to_string())
            ),
        }
    }
    if stopped == 0 {
        bail!("no run was stopped");
    }
    Ok(())
}

pub fn render_no_runs() -> &'static str {
    "No agents are running."
}

pub fn render_stopped(style: &Style, run_id: &str, client: &str) -> String {
    format!(
        "{} Stopped run {} ({}).",
        style.mark(Glyph::Ok),
        ui::short_id(run_id),
        client
    )
}

pub fn render_stop_error(style: &Style, run_id: &str, err: &str) -> String {
    format!(
        "{} Could not stop run {}: {err}",
        style.mark(Glyph::Bad),
        ui::short_id(run_id)
    )
}

/// Which live runs a command acts on.
pub(crate) enum RunSelection<'a> {
    /// Every live run (`--all`).
    All,
    /// A run id or unique prefix, or `None` for the only run.
    One(Option<&'a str>),
}

/// Indices of the live runs to act on: `--all`, a run id (or unique prefix), or the only one.
///
/// # Errors
/// Returns an error when the name matches no run or several runs, or when several
/// runs are active and none was named.
pub(crate) fn select_runs(ids: &[String], selection: RunSelection<'_>) -> Result<Vec<usize>> {
    let run = match selection {
        RunSelection::All => return Ok((0..ids.len()).collect()),
        RunSelection::One(run) => run,
    };
    match run {
        Some(r) => {
            if let Some(i) = ids.iter().position(|id| id == r) {
                return Ok(vec![i]);
            }
            let hits: Vec<usize> = ids
                .iter()
                .enumerate()
                .filter(|(_, id)| id.starts_with(r))
                .map(|(i, _)| i)
                .collect();
            match hits.as_slice() {
                [one] => Ok(vec![*one]),
                [] => {
                    let active: Vec<_> = ids.iter().map(|id| ui::short_id(id)).collect();
                    bail!("no active run '{r}' (active: {})", active.join(", "))
                }
                _ => bail!("'{r}' matches several runs; give more characters"),
            }
        }
        None if ids.len() == 1 => Ok(vec![0]),
        None => {
            let active: Vec<_> = ids.iter().map(|id| ui::short_id(id)).collect();
            bail!(
                "{} runs are active ({}); name one or pass --all",
                ids.len(),
                active.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<String> {
        vec!["run-aaa".into(), "run-abb".into(), "other".into()]
    }

    #[test]
    fn all_selects_everything() {
        assert_eq!(
            select_runs(&ids(), RunSelection::All).unwrap(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn exact_and_prefix() {
        assert_eq!(
            select_runs(&ids(), RunSelection::One(Some("run-abb"))).unwrap(),
            vec![1]
        );
        assert_eq!(
            select_runs(&ids(), RunSelection::One(Some("oth"))).unwrap(),
            vec![2]
        );
        assert!(select_runs(&ids(), RunSelection::One(Some("run-a"))).is_err());
        assert!(select_runs(&ids(), RunSelection::One(Some("zzz"))).is_err());
    }

    #[test]
    fn single_run_needs_no_argument() {
        assert_eq!(
            select_runs(&["only".into()], RunSelection::One(None)).unwrap(),
            vec![0]
        );
        assert!(select_runs(&ids(), RunSelection::One(None)).is_err());
    }

    #[test]
    fn render_stop_messages() {
        let s = Style::plain();
        assert_eq!(
            render_stopped(&s, "dddbee19-f151-4ee0-8473-93924f35e1d4", "opencode 2.1.2"),
            "✓ Stopped run dddbee19 (opencode 2.1.2)."
        );
        assert_eq!(
            render_stop_error(
                &s,
                "dddbee19-f151-4ee0-8473-93924f35e1d4",
                "connection reset"
            ),
            "✗ Could not stop run dddbee19: connection reset"
        );
        assert_eq!(render_no_runs(), "No agents are running.");
    }
}
