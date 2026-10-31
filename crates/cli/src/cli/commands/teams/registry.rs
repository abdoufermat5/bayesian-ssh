//! `bssh teams create` and `bssh teams list`: the teams stored on this machine.

use super::passphrase::{passphrase, Passphrase};
use super::sync::print_sync;
use crate::config::AppConfig;
use crate::services::teams::orgs;
use crate::services::teams::session::{self, Session};
use crate::services::teams::TeamsStore;
use anyhow::Result;

/// The arguments of `bssh teams create`.
pub(super) struct CreateRequest<'a> {
    pub name: &'a str,
    /// Organization id prefix or one of its team names.
    pub org: Option<&'a str>,
}

/// Creates a team, in an organization when one is named, and prints the sync
/// report.
///
/// # Errors
/// Fails when the organization cannot be selected, or when the server refuses
/// the new team.
pub(super) async fn create(
    store: &TeamsStore,
    config: &AppConfig,
    request: CreateRequest<'_>,
) -> Result<()> {
    let db = store.database();
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let org_id = match request.org {
        Some(org) => {
            let visible = s.list_orgs().await?;
            Some(orgs::select(
                &visible,
                Some(org),
                &session::local_team_names(db)?,
            )?)
        }
        None => None,
    };
    let sync = s.create_team_in(request.name, org_id).await?;
    println!(
        "✅ Created team '{}' ({}); you're its admin.",
        request.name, sync.team_id
    );
    print_sync(request.name, &sync);
    println!();
    println!(
        "Share a connection:  bssh teams share <connection> --team {}",
        request.name
    );
    println!(
        "Invite someone:      bssh teams invite {} <email>",
        request.name
    );
    Ok(())
}

/// Prints the teams this machine knows, with their host counts and last sync.
///
/// # Errors
/// Fails when the database cannot be read.
pub(super) fn list(store: &TeamsStore) -> Result<()> {
    let db = store.database();
    let teams = session::local_teams(db)?;
    if teams.is_empty() {
        println!("No teams on this machine. Create one with `bssh teams create <name>`.");
        return Ok(());
    }
    let summaries = db.list_sources()?;
    println!("👥 Teams");
    println!("{:<24} {:<38} {:>5}  LAST SYNC", "NAME", "ID", "HOSTS");
    for (team_id, source) in teams {
        let hosts = summaries
            .iter()
            .find(|s| s.source.id == source.id)
            .map_or(0, |s| s.hosts);
        let last = source
            .last_sync_at
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "never".into());
        println!("{:<24} {:<38} {:>5}  {}", source.name, team_id, hosts, last);
    }
    Ok(())
}
