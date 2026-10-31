//! `bssh ai approve`'s main loop: connect, list, ask, decide — and the lines it prints
//! before and after.

use super::canvas::Canvas;
use super::card::{render_card, Card};
use super::grant::{build_grant, QUICK_GRANT_MINUTES};
use super::listing::{summary_lines, Pending};
use super::prompt::{approval_gate, find_request, pick, read_choice, Choice, GrantKey, Human};
use super::view::View;
use super::words::human_duration;
use crate::cli::parser_ai::AiArgsApprove;
use crate::config::AppConfig;
use crate::services::ai::ai_dir;
use crate::services::ai::control_client::connect_all;
use crate::services::ai::types::{ApprovalOutcome, ApprovalRequest, ApprovalSubject, Tier};
use crate::services::ai::ui::{self, Glyph, Style, Tone};
use anyhow::{bail, Result};
use chrono::Utc;

/// Ask each live run for its pending approvals, decide, and send the decision.
///
/// # Errors
///
/// Fails when the request cannot be picked (unknown id, several pending without a
/// terminal), when nobody is at a terminal to decide, and when the run refuses the decision.
pub async fn execute(args: AiArgsApprove, config: AppConfig) -> Result<()> {
    let dir = ai_dir(&config);
    let style = Style::terminal();
    // `--follow` answers one request after another. `--list` and `--deny` stay one shot: a
    // scripted run must never decide twice.
    let follow = args.follow && !args.list && !args.deny;
    let mut announced = false;
    // The "no agent" line is printed once, not once per polling round.
    let mut told_empty = false;
    // `--follow` answers the named request first, then whatever else comes in.
    let mut want = args.request_id.clone();

    loop {
        let mut clients = connect_all(&dir, "bssh-cli").await;
        if !announced {
            println!("{}", env_header(&style, &config.environment, clients.len()));
            announced = true;
        }
        if clients.is_empty() {
            if !told_empty {
                println!("{}", no_agent_message(&config));
                told_empty = true;
            }
            if follow {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
            return Ok(());
        }
        told_empty = false;

        // (client index, request)
        let mut all: Vec<(usize, ApprovalRequest)> = Vec::new();
        for (i, c) in clients.iter_mut().enumerate() {
            match c.pending().await {
                Ok(reqs) => all.extend(reqs.into_iter().map(|r| (i, r))),
                Err(e) => eprintln!("run {}: {e}", c.hello.run_id),
            }
        }
        if all.is_empty() {
            if follow {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
            println!("No pending approvals.");
            return Ok(());
        }
        let human = Human::here();
        approval_gate(&args, human)?;

        if args.list {
            let canvas = Canvas::new(&style, ui::width());
            let pending = Pending {
                clients: &clients,
                all: &all,
            };
            for line in summary_lines(&canvas, pending, Utc::now()) {
                println!("{line}");
            }
            return Ok(());
        }

        let idx = match &want {
            Some(id) => find_request(&all, id)?,
            None if all.len() == 1 => 0,
            None if human == Human::Piped => {
                bail!(
                    "give the request id: several requests are pending and picking one needs a \
                     terminal"
                )
            }
            None => {
                let canvas = Canvas::new(&style, ui::width().saturating_sub(4));
                let pending = Pending {
                    clients: &clients,
                    all: &all,
                };
                pick(&canvas, pending)?
            }
        };
        let (ci, req) = &all[idx];
        let hello = &clients[*ci].hello;
        let grant_key = GrantKey::for_request(req);

        let who = format!("{}@cli", whoami::username());
        let mut view = View::Short;
        let card = || Card {
            style: &style,
            run: hello,
            request: req,
            now: Utc::now(),
        };
        let choice = if args.deny {
            println!("{}", render_card(card(), view));
            Choice::Deny
        } else {
            'ask: loop {
                println!("{}", render_card(card(), view));
                loop {
                    match read_choice(grant_key)? {
                        Choice::Full => {
                            view = View::Full;
                            continue 'ask;
                        }
                        Choice::Other => continue,
                        c => break 'ask c,
                    }
                }
            }
        };

        if choice == Choice::Skip {
            println!("Left pending.");
            if follow {
                want = None;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
            return Ok(());
        }

        // An approve or a grant both send an approval; only a grant carries a `Grant`.
        let (outcome, ruling) = if matches!(choice, Choice::Approve | Choice::Grant) {
            let minutes = if choice == Choice::Grant {
                Some(QUICK_GRANT_MINUTES)
            } else {
                args.grant
            };
            let grant = match minutes {
                Some(mins) => {
                    let g = build_grant(req, mins);
                    if g.is_none() {
                        println!("Note: grants only apply to single calls; approving without one.");
                    }
                    g
                }
                None => None,
            };
            let granted = grant.is_some();
            (
                ApprovalOutcome::Approved {
                    by: who.clone(),
                    channel: "cli".into(),
                    grant,
                },
                Ruling::Approved { granted },
            )
        } else {
            (
                ApprovalOutcome::Denied {
                    by: who.clone(),
                    reason: args.reason.clone(),
                },
                Ruling::Denied { by: &who },
            )
        };
        clients[*ci].decide(&req.request_id, outcome).await?;
        println!("{}", result_line(&style, req, ruling));
        if !follow {
            return Ok(());
        }
        want = None;
    }
}

/// One header line naming the environment, so it is clear which runs are listed.
fn env_header(style: &Style, environment: &str, runs: usize) -> String {
    let runs = match runs {
        0 => "no run connected".to_string(),
        1 => "1 run connected".to_string(),
        n => format!("{n} runs connected"),
    };
    style.paint(Tone::Dim, &format!("environment '{environment}' · {runs}"))
}

/// What to say when no agent's control socket answers.
fn no_agent_message(config: &AppConfig) -> String {
    if cfg!(not(unix)) {
        return "Out-of-band approval needs Unix sockets and is not supported on Windows. \
                Answer approval requests in your agent's client."
            .to_string();
    }
    format!(
        "No agent is connected in environment '{}'. Start your agent (set up with `bssh ai \
         install`).",
        config.environment
    )
}

/// How the approver decided, for the line printed afterwards.
#[derive(Debug, Clone, Copy)]
enum Ruling<'a> {
    Denied {
        by: &'a str,
    },
    Approved {
        /// Whether the same kind of call is now allowed for a while.
        granted: bool,
    },
}

/// What the person sees after deciding.
fn result_line(style: &Style, r: &ApprovalRequest, ruling: Ruling<'_>) -> String {
    let granted = match ruling {
        Ruling::Denied { by } => {
            return format!(
                "{} Denied. {} is told \"denied by {by}\" and nothing ran.",
                style.mark(Glyph::Bad),
                r.client
            );
        }
        Ruling::Approved { granted } => granted,
    };
    if r.tier == Tier::TwoPerson {
        return format!(
            "{} Your approval is recorded as the first of two. Local approvals all count as one \
             person, so a second person has to approve it on the Teams relay.",
            style.mark(Glyph::Ask)
        );
    }
    let mut line = match &r.subject {
        ApprovalSubject::Plan { plan_hash, .. } => format!(
            "Approved plan {} for {}. It runs now; `bssh ai approve --follow` answers the next \
             request.",
            ui::short_id(plan_hash),
            r.client
        ),
        ApprovalSubject::Call { .. } => format!(
            "Approved for {}. It runs now; `bssh ai approve --follow` answers the next request.",
            r.client
        ),
        ApprovalSubject::Access { duration, .. } => format!(
            "Access granted to {} for {}.",
            r.client,
            human_duration(duration)
        ),
    };
    if granted {
        line.push_str(" The same kind of call is allowed for a while.");
    }
    format!("{} {line}", style.mark(Glyph::Ok))
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{demo_plan_request, service_request};
    use super::*;

    #[test]
    fn results_read_like_the_brief() {
        let s = Style::plain();
        let plan = demo_plan_request();
        assert_eq!(
            result_line(&s, &plan, Ruling::Approved { granted: false }),
            "✓ Approved plan 486c2f5a for opencode 2.1.2. It runs now; `bssh ai approve --follow` \
             answers the next request."
        );
        assert_eq!(
            result_line(&s, &plan, Ruling::Denied { by: "ana@cli" }),
            "✗ Denied. opencode 2.1.2 is told \"denied by ana@cli\" and nothing ran."
        );
        let two = service_request(Tier::TwoPerson);
        assert!(result_line(&s, &two, Ruling::Approved { granted: false })
            .contains("recorded as the first of two"));
    }
}
