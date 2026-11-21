//! `bssh teams members` and `bssh teams remove`: who is in a team, their
//! devices, and removing a member with a key rotation.

use super::passphrase::{Passphrase, passphrase};
use super::sync::print_sync;
use crate::cli::utils::confirm;
use crate::config::AppConfig;
use crate::services::teams::TeamsStore;
use crate::services::teams::session::{self, Session};
use crate::ui::{Glyph, Style, Table, Tone, heading};
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
    let style = Style::terminal();
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, team)?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let members = s.members(team_id).await?;

    let meta = format!("{} member(s)", members.len());
    println!("{}", heading(&style, &source.name, Some(&meta)));

    let mut table = Table::new(&[
        "MEMBER",
        "ROLE",
        "DEVICE",
        "FINGERPRINT",
        "DEVICE ID",
        "STATUS",
    ]);
    for m in members {
        let role_badge = match m.role {
            Role::Admin => style.paint(Tone::Info, "admin"),
            Role::Member => style.paint(Tone::Dim, "member"),
        };
        if m.devices.is_empty() {
            table.row(vec![
                style.paint(Tone::Strong, &m.email),
                role_badge,
                style.paint(Tone::Dim, "-"),
                style.paint(Tone::Dim, "-"),
                style.paint(Tone::Dim, "-"),
                style.paint(Tone::Dim, "no devices"),
            ]);
        } else {
            for (idx, d) in m.devices.iter().enumerate() {
                let email_cell = if idx == 0 {
                    style.paint(Tone::Strong, &m.email)
                } else {
                    String::new()
                };
                let role_cell = if idx == 0 {
                    role_badge.clone()
                } else {
                    String::new()
                };
                let fp = crate::services::teams::crypto::fingerprint(&d.ed25519_public);
                let status_cell = match d.revoked_at {
                    Some(at) => {
                        style.paint(Tone::Warn, &format!("revoked {}", at.format("%Y-%m-%d")))
                    }
                    None => style.paint(Tone::Ok, "active"),
                };
                table.row(vec![
                    email_cell,
                    role_cell,
                    d.name.clone(),
                    style.paint(Tone::Dim, &fp),
                    style.paint(Tone::Dim, &d.device_id.to_string()),
                    status_cell,
                ]);
            }
        }
    }
    if !table.is_empty() {
        println!("{}", table.render(&style).trim_end_matches('\n'));
    }

    if let Some(invites) = s.invites(team_id).await?.filter(|i| !i.is_empty()) {
        let dot = style.glyph(Glyph::Dot);
        let inv_meta = format!("{} {dot} each holding a seat", invites.len());
        println!("\n{}", heading(&style, "Pending Invites", Some(&inv_meta)));

        let mut inv_table = Table::new(&["EMAIL", "EXPIRES"]);
        for i in &invites {
            inv_table.row(vec![
                style.paint(Tone::Strong, &i.email),
                style.paint(Tone::Dim, &i.expires_at.format("%Y-%m-%d").to_string()),
            ]);
        }
        println!("{}", inv_table.render(&style).trim_end_matches('\n'));
        println!(
            "\n{}",
            style.paint(
                Tone::Dim,
                &format!("Revoke one: `bssh teams uninvite {} <email>`", source.name)
            )
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
    let style = Style::terminal();
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, member.team)?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let preview = s.remove_preview(team_id, member.email).await?;
    if !preview.blocking.is_empty() {
        println!(
            "{} These devices were never approved on this machine, so the team key can't be \
             rotated yet:",
            style.mark(Glyph::Ask)
        );
        let mut table = Table::new(&["EMAIL", "DEVICE", "FINGERPRINT"]);
        for d in &preview.blocking {
            table.row(vec![
                d.email.clone(),
                d.name.clone(),
                style.paint(Tone::Dim, &d.fingerprint),
            ]);
        }
        println!("{}", table.render(&style));
        println!(
            "  {}",
            style.paint(
                Tone::Dim,
                &format!(
                    "{} is still removed; nobody can push until you approve them \
                     (`bssh teams grant {}`) and sync.",
                    member.email, source.name
                )
            )
        );
    }
    if !confirm(
        &format!(
            "Remove {} from '{}' and rotate the team key?",
            member.email, source.name
        ),
        false,
    )? {
        println!("{}", style.paint(Tone::Dim, "Cancelled."));
        return Ok(());
    }
    let sync = s.remove(team_id, member.email).await?;
    println!(
        "{} Removed {} from '{}'.",
        style.mark(Glyph::Ok),
        style.paint(Tone::Strong, member.email),
        source.name
    );
    print_sync(&source.name, &sync);
    println!(
        "  {}",
        style.paint(
            Tone::Dim,
            "They can no longer sync, and nothing written from now on is readable by them.\n  \
             They may still have copies of what they already synced: rotate any credentials that matter."
        )
    );
    Ok(())
}
