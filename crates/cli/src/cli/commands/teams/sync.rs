//! `bssh teams sync`: pulling and pushing a team, and the report every other
//! command prints after a sync.

use super::passphrase::{Passphrase, passphrase};
use crate::config::AppConfig;
use crate::services::teams::TeamsStore;
use crate::services::teams::session::{self, Session, TeamSync};
use crate::services::teams::sync::AUDIT_QUOTA_MESSAGE;
use anyhow::{Result, bail};

/// Syncs one team, or every team on this machine, and prints each report.
///
/// # Errors
/// Fails when a team name cannot be resolved, or after reporting when any team
/// failed to sync.
pub(super) async fn execute(
    store: &TeamsStore,
    config: &AppConfig,
    team: Option<&str>,
) -> Result<()> {
    let db = store.database();
    let targets = match team {
        Some(t) => vec![session::resolve_team(db, t)?],
        None => session::local_teams(db)?,
    };
    if targets.is_empty() {
        println!("No teams on this machine.");
        return Ok(());
    }
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let mut failed = Vec::new();
    for (team_id, source) in targets {
        match s.sync_team(team_id, Vec::new()).await {
            Ok(sync) => print_sync(&source.name, &sync),
            Err(e) => {
                eprintln!("❌ {}: {e:#}", source.name);
                failed.push(source.name);
            }
        }
    }
    if !failed.is_empty() {
        bail!(
            "{} team(s) failed to sync: {}",
            failed.len(),
            failed.join(", ")
        );
    }
    Ok(())
}

pub(super) fn print_sync(name: &str, sync: &TeamSync) {
    if sync.waiting_for_key {
        println!("⏳ {name}: waiting for an admin to grant this device the team key.");
        return;
    }
    let r = &sync.report;
    let mut parts = vec![
        format!("pulled {}", r.pulled),
        format!("pushed {}", r.pushed),
    ];
    if r.conflicts_resolved > 0 {
        parts.push(format!("{} conflict(s) resolved", r.conflicts_resolved));
    }
    if r.audit_uploaded > 0 {
        parts.push(format!("{} audit event(s) uploaded", r.audit_uploaded));
    }
    if let Some(h) = &sync.hosts {
        parts.push(format!(
            "hosts +{} ~{} stale {}",
            h.added, h.updated, h.stale
        ));
    }
    println!(
        "🔄 {}: {}",
        sync.name.as_deref().unwrap_or(name),
        parts.join(", ")
    );
    if let Some(v) = sync.rotated_to {
        println!("   🔑 Rotated the team key to version {v}.");
    }
    for d in &r.dropped_edits {
        println!(
            "   ⚠️  your edit to {} was dropped: someone else deleted it",
            d.entry_id
        );
    }
    if !r.rejected.is_empty() {
        println!(
            "   🚨 {} entr(y/ies) from the server failed integrity checks and were ignored:",
            r.rejected.len()
        );
        for e in &r.rejected {
            println!("      {} rev {}: {}", e.entry_id, e.revision, e.reason);
        }
    }
    if sync.unreadable > 0 {
        println!(
            "   ⚠️  {} entr(y/ies) couldn't be verified or decrypted",
            sync.unreadable
        );
    }
    if r.rotation_required {
        println!(
            "   ⚠️  a member was removed: an admin must sync to rotate the team key; your edits stay queued."
        );
    }
    if r.audit_quota_exceeded {
        println!("   ⚠️  {}", AUDIT_QUOTA_MESSAGE);
    }
    for e in &r.errors {
        println!("   ❌ {e}");
        if e.contains("AuditChainBroken") {
            println!(
                "      The server's copy of your audit log is missing events this device uploaded \
                 before. That can mean they were deleted on the server: tell your team admin."
            );
        }
    }
}
