//! Reading and verifying a team's audit log.

use super::handle::Session;
use crate::services::teams::audit::{
    check_event, ChainProblem, ChainSummary, ChainVerifier, EventCheck,
};
use crate::services::teams::client::{walk_audit_pages, AuditWalk};
use crate::services::teams::read_vault;
use anyhow::Result;
use bssh_teams_proto::api::{AuditKind, AuditListQuery, AuditOrder};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

/// Events shown by [`Session::audit`] when the caller sets no limit.
pub const DEFAULT_AUDIT_LIMIT: u32 = 100;

/// Largest page the server serves; also the page size when reading more.
const AUDIT_PAGE: u32 = 1000;

/// Shown when the log can't be read in full because the server ignores the
/// order and the cursor of an audit request.
const OLD_SERVER_PAGING: &str = "this Teams server is too old to read its whole audit log in \
     pages, so the log was not verified in full: ask its operator to update it";

/// Which part of the audit log to show, newest first. `email` narrows
/// client-side. Whatever is shown, the whole log is verified.
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    pub entry_id: Option<Uuid>,
    pub email: Option<String>,
    pub since: Option<DateTime<Utc>>,
    /// Newest events to show (default [`DEFAULT_AUDIT_LIMIT`]); may exceed one
    /// server page, in which case the next pages are fetched.
    pub limit: Option<u32>,
    /// Show every matching event; `limit` is ignored.
    pub all: bool,
}

/// One audit event, verified and with names resolved.
#[derive(Debug, Clone)]
pub struct AuditLine {
    pub occurred_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub kind: AuditKind,
    /// Member email, or a placeholder for a device this machine can't name.
    pub who: String,
    pub device_name: String,
    /// The connection's name, decrypted locally; `None` if the event names no host.
    pub host: Option<String>,
    pub client_ip: Option<String>,
    pub check: EventCheck,
}

/// A team's audit log as checked by this client.
#[derive(Debug, Clone)]
pub struct TeamAudit {
    /// The events asked for, newest first.
    pub lines: Vec<AuditLine>,
    /// One per device, from verifying the whole log (not just `lines`).
    pub chains: Vec<ChainSummary>,
    /// Events of the whole log whose signature checked out.
    pub verified_events: usize,
    pub problems: Vec<String>,
    pub invalid_events: usize,
    /// `lines` holds every event that matched: the limit cut nothing off.
    pub complete: bool,
}

impl TeamAudit {
    pub fn is_clean(&self) -> bool {
        self.invalid_events == 0 && self.problems.is_empty()
    }
}

impl<'a> Session<'a> {
    /// Show the audit log (admins only), newest first, and verify the whole of
    /// it here, whatever is shown: every signature against device keys this
    /// machine knows, and each device's hash chain from its first event.
    ///
    /// The log is read twice. The listing is `order=newest` with the filters
    /// and the limit asked for. The verification is `order=chain`, which the
    /// server allows no filter on (a filter would drop events from the middle
    /// of a chain), page by page, so it takes memory for one page however
    /// large the log.
    pub async fn audit(&self, team_id: Uuid, query: AuditQuery) -> Result<TeamAudit> {
        let info = self.require_admin(team_id).await?;
        let members = self.client.list_members(team_id).await?;
        self.remember_authors(team_id, &members)?;
        let authors = self.authors(team_id)?;

        // Names: current members first, then devices this admin approved
        // (which covers members removed since).
        let mut names: HashMap<Uuid, (String, String)> = HashMap::new();
        for (device, (email, _, _)) in self.approved_devices(team_id)? {
            names.insert(device, (email, String::new()));
        }
        for m in &members {
            for d in &m.devices {
                names.insert(d.device_id, (m.email.clone(), d.name.clone()));
            }
        }
        let hosts: HashMap<Uuid, String> =
            match self.codec(team_id, info.current_key_version).await? {
                Some(codec) => read_vault(self.store, &codec, team_id)?
                    .connections
                    .into_iter()
                    .map(|c| (c.entry_id, c.record.name))
                    .collect(),
                None => HashMap::new(),
            };

        // The listing. `--member` narrows here, by device owner, so the limit
        // counts events shown, not events fetched.
        let wanted =
            (!query.all).then(|| query.limit.unwrap_or(DEFAULT_AUDIT_LIMIT).max(1) as usize);
        let email_filter = query.email.as_deref().map(|e| e.trim().to_lowercase());
        let mut lines: Vec<AuditLine> = Vec::new();
        let listing_end = walk_audit_pages(
            AuditListQuery {
                entry_id: query.entry_id,
                since: query.since,
                limit: Some(wanted.map_or(AUDIT_PAGE, |n| n.min(AUDIT_PAGE as usize) as u32)),
                ..Default::default()
            },
            |q| {
                let client = &self.client;
                async move { client.list_audit(team_id, &q).await }
            },
            |page| {
                for r in page {
                    let e = &r.event;
                    let (who, device_name) =
                        names.get(&e.device_id).cloned().unwrap_or_else(|| {
                            (format!("unknown device {}", e.device_id), String::new())
                        });
                    if email_filter.as_deref().is_some_and(|f| f != who) {
                        continue;
                    }
                    lines.push(AuditLine {
                        occurred_at: e.occurred_at,
                        received_at: r.received_at,
                        kind: e.kind,
                        who,
                        device_name,
                        host: e.entry_id.map(|id| {
                            hosts
                                .get(&id)
                                .cloned()
                                .unwrap_or_else(|| "(deleted host)".to_string())
                        }),
                        client_ip: r.client_ip.clone(),
                        check: check_event(team_id, e, &authors),
                    });
                    if wanted.is_some_and(|n| lines.len() >= n) {
                        return false;
                    }
                }
                true
            },
        )
        .await?;
        let mut problems = Vec::new();
        let shown_invalid = lines
            .iter()
            .filter(|l| l.check != EventCheck::Valid)
            .count();

        // The verification of the whole log.
        let mut verifier = ChainVerifier::new(team_id, &authors);
        let mut previous: Option<(Uuid, u64)> = None;
        let mut unordered = false;
        let end = walk_audit_pages(
            AuditListQuery {
                order: Some(AuditOrder::Chain),
                limit: Some(AUDIT_PAGE),
                ..Default::default()
            },
            |q| {
                let client = &self.client;
                async move { client.list_audit(team_id, &q).await }
            },
            |mut page| {
                let key = |r: &bssh_teams_proto::api::AuditRecord| (r.event.device_id, r.event.seq);
                let in_order = |page: &[bssh_teams_proto::api::AuditRecord]| {
                    page.windows(2).all(|w| key(&w[0]) <= key(&w[1]))
                        && previous.is_none_or(|p| key(&page[0]) > p)
                };
                if !in_order(&page) {
                    // A server older than paging ignores the order and sends
                    // newest first. If its one page is the whole log, sorting
                    // it here is enough; otherwise the log can't be read in
                    // chain order.
                    if previous.is_none() && page.len() < AUDIT_PAGE as usize {
                        page.sort_by_key(key);
                    } else {
                        unordered = true;
                        return false;
                    }
                }
                for r in &page {
                    verifier.push(r);
                }
                previous = page.last().map(key);
                true
            },
        )
        .await?;
        if unordered || end == AuditWalk::Stalled {
            problems.push(OLD_SERVER_PAGING.to_string());
        }
        let report = verifier.finish();
        problems.extend(report.problems.iter().map(ChainProblem::to_string));
        problems.dedup();

        Ok(TeamAudit {
            lines,
            invalid_events: report.invalid_events.max(shown_invalid),
            verified_events: report.chains.iter().map(|c| c.events).sum(),
            problems,
            chains: report.chains,
            // Stopped by the limit, or a server that doesn't page: more may exist.
            complete: listing_end == AuditWalk::Finished,
        })
    }
}
