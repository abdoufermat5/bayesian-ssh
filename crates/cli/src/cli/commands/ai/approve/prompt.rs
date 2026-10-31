//! Picking a request and reading the decision: one key, from a terminal only.

use super::canvas::Canvas;
use super::grant::build_grant;
use super::listing::{Pending, summary_lines};
use crate::cli::parser_ai::AiArgsApprove;
use crate::services::ai::types::ApprovalRequest;
use anyhow::{Result, bail};
use chrono::Utc;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::io::{BufRead, IsTerminal, Write};

/// Find a request by exact id or unique prefix.
pub(super) fn find_request(all: &[(usize, ApprovalRequest)], id: &str) -> Result<usize> {
    if let Some(i) = all.iter().position(|(_, r)| r.request_id == id) {
        return Ok(i);
    }
    let hits: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, (_, r))| r.request_id.starts_with(id))
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => bail!("no pending request '{id}'"),
        _ => bail!("'{id}' matches several requests; give more characters"),
    }
}

pub(super) fn pick(canvas: &Canvas, pending: Pending<'_>) -> Result<usize> {
    let lines = summary_lines(canvas, pending, Utc::now());
    for (n, line) in lines.iter().enumerate() {
        println!("{:>2}  {line}", n + 1);
    }
    print!("Which one? [1-{}] ", pending.all.len());
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    match line.trim().parse::<usize>() {
        Ok(n) if (1..=pending.all.len()).contains(&n) => Ok(n - 1),
        _ => bail!("not a valid choice"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Choice {
    Approve,
    Deny,
    /// Leave it pending (`s`, Ctrl-C, end of input).
    Skip,
    /// Show everything, then ask again.
    Full,
    /// Approve and allow the same kind of call for a while.
    Grant,
    /// Any other key: nothing happens.
    Other,
}

/// Whether the `g` key offers a grant: only a single call with a host qualifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GrantKey {
    Offered,
    Unavailable,
}

impl GrantKey {
    pub(super) fn for_request(r: &ApprovalRequest) -> Self {
        if build_grant(r, 1).is_some() {
            Self::Offered
        } else {
            Self::Unavailable
        }
    }
}

fn choice_for(word: &str, grant_key: GrantKey) -> Choice {
    match word.trim().to_lowercase().as_str() {
        "a" | "approve" => Choice::Approve,
        "d" | "deny" => Choice::Deny,
        "s" | "skip" => Choice::Skip,
        "v" | "full" => Choice::Full,
        "g" | "grant" if grant_key == GrantKey::Offered => Choice::Grant,
        _ => Choice::Other,
    }
}

/// One key in raw mode on a terminal.
///
/// `bssh ai approve` decides for a person, so it reads only from a terminal: a line from a
/// pipe (`echo a | bssh ai approve`, or anything that can write the caller's stdin) must
/// never approve or grant.
pub(super) fn read_choice(grant_key: GrantKey) -> Result<Choice> {
    if Human::here() == Human::Piped {
        bail!("approvals need a human at a terminal; without one use `--deny` or the desktop app");
    }
    struct RawMode;
    impl Drop for RawMode {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    crossterm::terminal::enable_raw_mode()?;
    let _raw = RawMode;
    loop {
        let Event::Key(k) = event::read()? else {
            continue;
        };
        if k.kind == KeyEventKind::Release {
            continue;
        }
        return Ok(match k.code {
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => Choice::Skip,
            KeyCode::Char(c) => choice_for(&c.to_string(), grant_key),
            _ => Choice::Other,
        });
    }
}

/// Whether this process is talking to a person: stdin, stdout and stderr on a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Human {
    AtTerminal,
    /// At least one of the three is a pipe.
    Piped,
}

impl Human {
    pub(super) fn here() -> Self {
        let tty = std::io::stdin().is_terminal()
            && std::io::stdout().is_terminal()
            && std::io::stderr().is_terminal();
        if tty { Self::AtTerminal } else { Self::Piped }
    }
}

/// Listing and denying decide nothing for the agent, so they stay scriptable.
fn scripted_ok(args: &AiArgsApprove) -> bool {
    args.list || args.deny
}

/// Refuse a decision with nobody behind it.
pub(super) fn approval_gate(args: &AiArgsApprove, human: Human) -> Result<()> {
    if human == Human::AtTerminal || scripted_ok(args) {
        return Ok(());
    }
    bail!(
        "approvals need a human at a terminal (or the desktop app): stdin, stdout and stderr \
         must be a TTY. Without one only `bssh ai approve --list` and `--deny` work."
    )
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::base;
    use super::*;
    use crate::services::ai::types::{ApprovalSubject, Scope};

    fn approve_args() -> AiArgsApprove {
        AiArgsApprove {
            request_id: None,
            deny: false,
            reason: None,
            list: false,
            grant: None,
            follow: false,
        }
    }

    #[test]
    fn deciding_needs_a_terminal_but_listing_and_denying_do_not() {
        let mut a = approve_args();
        assert!(
            approval_gate(&a, Human::Piped).is_err(),
            "a pipe can't approve"
        );
        assert!(approval_gate(&a, Human::AtTerminal).is_ok());
        a.request_id = Some("440947ca".into());
        a.grant = Some(60);
        assert!(
            approval_gate(&a, Human::Piped).is_err(),
            "granting is still approving"
        );
        a.deny = true;
        assert!(approval_gate(&a, Human::Piped).is_ok());
        let mut l = approve_args();
        l.list = true;
        assert!(approval_gate(&l, Human::Piped).is_ok());
    }

    #[test]
    fn keys_map_to_choices() {
        assert_eq!(choice_for("a", GrantKey::Unavailable), Choice::Approve);
        assert_eq!(choice_for("D", GrantKey::Unavailable), Choice::Deny);
        assert_eq!(choice_for("v", GrantKey::Unavailable), Choice::Full);
        assert_eq!(choice_for("s", GrantKey::Unavailable), Choice::Skip);
        assert_eq!(choice_for("g", GrantKey::Offered), Choice::Grant);
        assert_eq!(choice_for("g", GrantKey::Unavailable), Choice::Other);
        assert_eq!(choice_for("", GrantKey::Offered), Choice::Other);
        assert_eq!(choice_for("x", GrantKey::Offered), Choice::Other);
    }

    #[test]
    fn find_request_by_id_and_prefix() {
        let mut a = base(ApprovalSubject::Access {
            scope: Scope::default(),
            reason: "x".into(),
            duration: "1h".into(),
        });
        a.request_id = "abc111".into();
        let mut b = a.clone();
        b.request_id = "abd222".into();
        let all = vec![(0, a), (0, b)];
        assert_eq!(find_request(&all, "abc111").unwrap(), 0);
        assert_eq!(find_request(&all, "abd").unwrap(), 1);
        assert!(find_request(&all, "ab").is_err());
        assert!(find_request(&all, "zzz").is_err());
    }
}
