//! `bssh teams share` and `bssh teams unshare`: connections in the team vault.

use super::passphrase::{Passphrase, passphrase};
use super::sync::print_sync;
use crate::cli::utils::confirm;
use crate::config::AppConfig;
use crate::services::teams::TeamsStore;
use crate::services::teams::session::{self, Session, TeamSync};
use anyhow::{Result, anyhow};

/// The arguments of `bssh teams share`.
pub(super) struct ShareRequest<'a> {
    pub connection: &'a str,
    pub team: &'a str,
}

/// Shares a local connection with a team, or pushes edits to a team connection.
///
/// # Errors
/// Fails when the team or the connection cannot be found, or the server refuses
/// the share.
pub(super) async fn share(
    store: &TeamsStore,
    config: &AppConfig,
    request: ShareRequest<'_>,
) -> Result<()> {
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, request.team)?;
    let conn = db
        .connection_or_alias(request.connection)?
        .ok_or_else(|| anyhow!("no connection named '{}'", request.connection))?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let (_, sync) = s.share(team_id, &conn).await?;
    if !reached_server(&sync) {
        println!(
            "📥 '{}' is queued for '{}': it is shared once a sync goes through.",
            conn.name, source.name
        );
        print_sync(&source.name, &sync);
        return Ok(());
    }
    println!("📤 Shared '{}' with '{}'.", conn.name, source.name);
    print_sync(&source.name, &sync);
    if conn.source_id.is_none() {
        println!(
            "   Your local '{}' stays as is; the team's copy is listed under '{}'.",
            conn.name, source.name
        );
    }
    Ok(())
}

/// Deletes a team connection for every member, after confirmation.
///
/// # Errors
/// Fails when the connection cannot be found or the removal is refused.
pub(super) async fn unshare(
    store: &TeamsStore,
    config: &AppConfig,
    connection: &str,
) -> Result<()> {
    let db = store.database();
    let conn = db
        .connection_or_alias(connection)?
        .ok_or_else(|| anyhow!("no connection named '{connection}'"))?;
    if !confirm(
        &format!(
            "Delete '{}' from the team vault for every member?",
            conn.name
        ),
        false,
    )? {
        println!("Cancelled.");
        return Ok(());
    }
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let sync = s.unshare(&conn).await?;
    if reached_server(&sync) {
        println!("🗑️  Removed '{}' from the team vault.", conn.name);
    } else {
        println!(
            "📥 Removing '{}' is queued: it happens once a sync goes through.",
            conn.name
        );
    }
    print_sync(sync.name.as_deref().unwrap_or("team"), &sync);
    println!(
        "   Kept '{}' here as a local connection (`bssh remove {}` deletes it).",
        conn.name, conn.name
    );
    Ok(())
}

/// The server took this run's queued edits (or there was nothing to refuse).
fn reached_server(sync: &TeamSync) -> bool {
    let r = &sync.report;
    r.pushed > 0 || (r.errors.is_empty() && !r.rotation_required && !sync.waiting_for_key)
}
