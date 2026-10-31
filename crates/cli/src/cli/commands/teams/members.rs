//! `bssh teams members` and `bssh teams remove`: who is in a team, their
//! devices, and removing a member with a key rotation.

use super::passphrase::{passphrase, Passphrase};
use super::sync::print_sync;
use crate::cli::utils::confirm;
use crate::config::AppConfig;
use crate::services::teams::session::{self, Session};
use crate::services::teams::TeamsStore;
use anyhow::Result;
use bssh_teams_proto::api::Role;

/// A member addressed the way the CLI names them: by team and email.
pub(super) struct MemberRef<'a> {
    pub team: &'a str,
    pub email: &'a str,
}

/// Prints a team's members, their devices, and its pending invites.
///
/// # Errors
/// Fails when the team cannot be resolved or the member list cannot be fetched.
pub(super) async fn members(store: &TeamsStore, config: &AppConfig, team: &str) -> Result<()> {
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, team)?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let members = s.members(team_id).await?;
    println!("👥 {} ({} member(s))", source.name, members.len());
    for m in members {
        let role = match m.role {
            Role::Admin => "admin",
            Role::Member => "member",
        };
        println!("  {} [{role}]", m.email);
        for d in &m.devices {
            println!("{}", device_line(d));
        }
    }
    if let Some(invites) = s.invites(team_id).await?.filter(|i| !i.is_empty()) {
        println!();
        println!(
            "✉️  Pending invites ({}), each holding a seat:",
            invites.len()
        );
        for i in &invites {
            println!(
                "  {:<32} expires {}",
                i.email,
                i.expires_at.format("%Y-%m-%d")
            );
        }
        println!(
            "   Revoke one: `bssh teams uninvite {} <email>`",
            source.name
        );
    }
    Ok(())
}

/// Removes a member after showing devices that still block the key rotation,
/// then rotates the team key.
///
/// # Errors
/// Fails when the team cannot be resolved, the preview cannot be fetched, or the
/// removal is refused.
pub(super) async fn remove(
    store: &TeamsStore,
    config: &AppConfig,
    member: MemberRef<'_>,
) -> Result<()> {
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, member.team)?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let preview = s.remove_preview(team_id, member.email).await?;
    if !preview.blocking.is_empty() {
        println!(
            "⚠️  These devices were never approved on this machine, so the team key can't be \
             rotated yet:"
        );
        for d in &preview.blocking {
            println!("   {} {} ({})", d.email, d.name, d.fingerprint);
        }
        println!(
            "   {} is still removed; nobody can push until you approve them \
             (`bssh teams grant {}`) and sync.",
            member.email, source.name
        );
    }
    if !confirm(
        &format!(
            "Remove {} from '{}' and rotate the team key?",
            member.email, source.name
        ),
        false,
    )? {
        println!("Cancelled.");
        return Ok(());
    }
    let sync = s.remove(team_id, member.email).await?;
    println!("✅ Removed {} from '{}'.", member.email, source.name);
    print_sync(&source.name, &sync);
    println!(
        "   They can no longer sync, and nothing written from now on is readable by them.\n   \
         They may still have copies of what they already synced: rotate any credentials that matter."
    );
    Ok(())
}

/// One device in `bssh teams members`: name, fingerprint and id (the argument
/// of `bssh teams device revoke`). Revoked devices stay listed, marked, since
/// they still count as authors of what they wrote.
fn device_line(d: &bssh_teams_proto::api::DeviceInfo) -> String {
    let mut line = format!(
        "      {:<20} {}  {}",
        d.name,
        crate::services::teams::crypto::fingerprint(&d.ed25519_public),
        d.device_id
    );
    if let Some(at) = d.revoked_at {
        line.push_str(&format!("  revoked {}", at.format("%Y-%m-%d")));
    }
    line
}
