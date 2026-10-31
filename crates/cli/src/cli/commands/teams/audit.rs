//! `bssh teams audit`: the team's verified audit log.

use super::passphrase::{Passphrase, passphrase};
use crate::config::AppConfig;
use crate::services::teams::TeamsStore;
use crate::services::teams::audit::EventCheck;
use crate::services::teams::session::{self, AuditQuery, Session, TeamAudit};
use anyhow::{Result, anyhow, bail};

/// How much of the audit log to show.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AuditWindow {
    /// The newest events, at most this many.
    Newest(Option<u32>),
    /// Every matching event.
    All,
}

/// The arguments of `bssh teams audit`.
pub(super) struct AuditRequest<'a> {
    pub team: &'a str,
    /// Only events for this team connection.
    pub host: Option<&'a str>,
    /// Only events by this member.
    pub member: Option<String>,
    /// Only events received since this date or RFC 3339 time.
    pub since: Option<&'a str>,
    pub window: AuditWindow,
}

/// Prints the audit log and fails when any event fails verification.
///
/// # Errors
/// Fails when the team or host cannot be resolved, `--since` cannot be parsed,
/// or the log is not clean.
pub(super) async fn execute(
    store: &TeamsStore,
    config: &AppConfig,
    request: AuditRequest<'_>,
) -> Result<()> {
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, request.team)?;
    let since = request.since.map(parse_since).transpose()?;
    let (limit, all) = match request.window {
        AuditWindow::Newest(limit) => (limit, false),
        AuditWindow::All => (None, true),
    };
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let entry_id = request
        .host
        .map(|h| s.entry_for_connection(team_id, h))
        .transpose()?;
    let log = s
        .audit(
            team_id,
            AuditQuery {
                entry_id,
                email: request.member,
                since,
                limit,
                all,
            },
        )
        .await?;
    print_audit(&source.name, &log);
    if !log.is_clean() {
        bail!("the audit log failed verification");
    }
    Ok(())
}

fn parse_since(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    use chrono::{NaiveDate, TimeZone, Utc};
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(t.with_timezone(&Utc));
    }
    let day = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| anyhow!("--since must be YYYY-MM-DD or an RFC 3339 time, not '{s}'"))?;
    Ok(Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight exists")))
}

fn print_audit(team: &str, log: &TeamAudit) {
    use bssh_teams_proto::api::AuditKind;
    println!("🧾 Audit log for '{team}' ({} event(s))", log.lines.len());
    if log.lines.is_empty() {
        println!("   No events yet. Team members' connections appear here after they sync.");
    } else {
        println!(
            "{:<17} {:<28} {:<14} {:<22} {:<16} OK",
            "WHEN", "WHO", "EVENT", "HOST", "FROM"
        );
        for l in &log.lines {
            let when = l
                .occurred_at
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M");
            let event = match l.kind {
                AuditKind::SessionStart => "session start",
                AuditKind::SessionEnd => "session end",
                AuditKind::VaultRead => "vault read",
                AuditKind::VaultWrite => "vault write",
                AuditKind::AiCall => "ai call",
                AuditKind::AiApproval => "ai approval",
                AuditKind::AiChange => "ai change",
                AuditKind::AiRun => "ai run",
            };
            let ok = match l.check {
                EventCheck::Valid => "✓",
                EventCheck::BadSignature => "✗ bad signature",
                EventCheck::UnknownDevice => "? unknown device",
                EventCheck::WrongTeam => "✗ other team",
            };
            let who = if l.device_name.is_empty() {
                l.who.clone()
            } else {
                format!("{} ({})", l.who, l.device_name)
            };
            println!(
                "{:<17} {:<28} {:<14} {:<22} {:<16} {}",
                when,
                crate::cli::utils::truncate_display(&who, 28),
                event,
                crate::cli::utils::truncate_display(l.host.as_deref().unwrap_or("-"), 22),
                l.client_ip.as_deref().unwrap_or("-"),
                ok
            );
        }
    }
    println!();
    if log.is_clean() {
        println!(
            "✅ Verified on this machine, the whole log: {} signature(s) valid, {} device chain(s) intact from each device's first event.",
            log.verified_events,
            log.chains.len()
        );
    } else {
        println!(
            "🚨 Verification FAILED: {} event(s) invalid, {} chain problem(s).",
            log.invalid_events,
            log.problems.len()
        );
        for p in &log.problems {
            println!("   - {p}");
        }
        println!(
            "   Events can't be edited, reordered or removed without this showing; treat it as tampering."
        );
    }
    if !log.complete {
        println!(
            "   Showing the newest {} event(s) only: --limit <N> shows more, --all shows every one.",
            log.lines.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn parse_since_accepts_a_date_at_midnight_utc() {
        let at = parse_since("2026-03-04").expect("a plain date");
        assert_eq!(
            (at.year(), at.month(), at.day(), at.hour(), at.minute()),
            (2026, 3, 4, 0, 0)
        );
    }

    #[test]
    fn parse_since_accepts_rfc3339_in_any_offset() {
        let at = parse_since("2026-03-04T05:06:07+02:00").expect("an RFC 3339 time");
        assert_eq!(
            (at.year(), at.month(), at.day(), at.hour(), at.minute()),
            (2026, 3, 4, 3, 6)
        );
    }

    #[test]
    fn parse_since_rejects_anything_else_with_the_flag_name() {
        let err = parse_since("yesterday").expect_err("not a date");
        assert_eq!(
            err.to_string(),
            "--since must be YYYY-MM-DD or an RFC 3339 time, not 'yesterday'"
        );
    }
}
