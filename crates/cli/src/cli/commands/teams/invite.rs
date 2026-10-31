//! Invites: sending one, joining with its token, granting the team key to a
//! waiting device, and revoking an invite.

use super::members::MemberRef;
use super::passphrase::{passphrase, Passphrase};
use crate::config::AppConfig;
use crate::services::teams::session::{self, JoinWith, PendingDevice, Session};
use crate::services::teams::TeamsStore;
use anyhow::{anyhow, bail, Result};
use bssh_teams_proto::api::{Invite, Role};

/// The arguments of `bssh teams invite`.
pub(super) struct InviteRequest<'a> {
    pub team: &'a str,
    pub email: &'a str,
    pub role: Role,
}

/// The arguments of `bssh teams join`.
pub(super) struct JoinRequest {
    pub token: Option<String>,
    pub team: Option<uuid::Uuid>,
    pub fingerprint: String,
}

/// The arguments of `bssh teams grant`.
pub(super) struct GrantRequest<'a> {
    pub team: &'a str,
    pub fingerprint: Option<&'a str>,
}

/// Invites someone by email and prints their setup commands.
///
/// # Errors
/// Fails when the team cannot be resolved or the server returns no token.
pub(super) async fn invite(
    store: &TeamsStore,
    config: &AppConfig,
    request: InviteRequest<'_>,
) -> Result<()> {
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, request.team)?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let (invite, fp) = s.invite(team_id, request.email, request.role).await?;
    let token = invite
        .token
        .as_deref()
        .ok_or_else(|| anyhow!("the server didn't return an invite token"))?;
    print!(
        "{}",
        invite_text(&invite, &source.name, token, &fp, &s.identity.server_url)
    );
    Ok(())
}

/// Joins a team with a token, or by team id after a fingerprint mismatch, and
/// prints this device's fingerprint to send to the admin.
///
/// # Errors
/// Fails when neither a token nor a team is given, or when the join is refused.
pub(super) async fn join(
    store: &TeamsStore,
    config: &AppConfig,
    request: JoinRequest,
) -> Result<()> {
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let with = match (&request.token, request.team) {
        (Some(token), _) => JoinWith::Token(token),
        (None, Some(team)) => JoinWith::Team(team),
        (None, None) => bail!("give an invite token or --team"),
    };
    let (team_id, name) = s.join(with, &request.fingerprint).await?;
    println!("✅ Joined team {team_id} (shown locally as '{name}' until its first sync).");
    println!("   Pinned admin fingerprint {}", request.fingerprint.trim());
    println!();
    println!("Send your device fingerprint to the admin so they can grant you the team key:");
    println!();
    println!("  {}", s.my_fingerprint());
    println!();
    println!("Then run `bssh teams sync`.");
    Ok(())
}

/// Lists the devices waiting for the team key, or grants it to one.
///
/// # Errors
/// Fails when the team cannot be resolved or the grant is refused.
pub(super) async fn grant(
    store: &TeamsStore,
    config: &AppConfig,
    request: GrantRequest<'_>,
) -> Result<()> {
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, request.team)?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    match request.fingerprint {
        None => {
            let pending = s.pending(team_id).await?;
            if pending.is_empty() {
                println!("No device is waiting for the '{}' key.", source.name);
            } else {
                println!("Devices waiting for the '{}' key:", source.name);
                for p in &pending {
                    print_pending(p);
                }
                println!();
                println!("Only grant a device whose fingerprint its owner sent you:");
                println!(
                    "  bssh teams grant {} --fingerprint <FINGERPRINT>",
                    source.name
                );
            }
        }
        Some(fp) => {
            let granted = s.grant(team_id, fp).await?;
            println!(
                "🔑 Granted the '{}' key to {} ('{}').",
                source.name, granted.email, granted.device.name
            );
            println!("   They can now run `bssh teams sync`.");
        }
    }
    Ok(())
}

/// Revokes a pending invite, freeing its seat.
///
/// # Errors
/// Fails when the team cannot be resolved or the invite cannot be revoked.
pub(super) async fn uninvite(
    store: &TeamsStore,
    config: &AppConfig,
    member: MemberRef<'_>,
) -> Result<()> {
    let db = store.database();
    let (team_id, source) = session::resolve_team(db, member.team)?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let invite = s.uninvite(team_id, member.email).await?;
    println!(
        "🚫 Revoked the invite for {} to '{}': its token no longer works and its seat is free.",
        invite.email, source.name
    );
    Ok(())
}

fn print_pending(p: &PendingDevice) {
    println!("  {:<28} {:<16} {}", p.email, p.device.name, p.fingerprint);
}

/// What `bssh teams invite` prints: the invitee's setup commands, whether the
/// server mailed the token, and this admin device's fingerprint, which must
/// reach the invitee out of band: the server never chooses the trust root.
fn invite_text(
    invite: &Invite,
    team: &str,
    token: &str,
    admin_fingerprint: &str,
    server_url: &str,
) -> String {
    let mut text = format!(
        "✉️  Invited {} to '{}' (expires {}).\n",
        invite.email,
        team,
        invite.expires_at.format("%Y-%m-%d")
    );
    if invite.emailed {
        text.push_str(&format!(
            "\nThe server emailed the invite to {}. The commands below are the fallback if it\n\
             doesn't arrive (the token is shown only once):\n\n",
            invite.email
        ));
    } else {
        text.push_str(
            "\nSend them these, over a channel you trust (the token is shown only once):\n\n",
        );
    }
    text.push_str(&format!(
        "  bssh teams init --server {server_url} --email {} --invite {token}\n",
        invite.email
    ));
    text.push_str(&format!(
        "  bssh teams join {token} --fingerprint {admin_fingerprint}\n"
    ));
    text.push_str("\nSend them your device fingerprint yourself, ");
    if invite.emailed {
        text.push_str(
            "not by replying to the server's email:\nthey must know the invite really is from you.\n",
        );
    } else {
        text.push_str(
            "over that same channel: it is how\nthey know the invite really is from you.\n",
        );
    }
    text.push_str(&format!("\n  {admin_fingerprint}\n"));
    text.push_str("\nWhen they send back their device fingerprint, run:\n");
    text.push_str(&format!(
        "  bssh teams grant {team} --fingerprint <THEIR FINGERPRINT>\n"
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn base_invite() -> Invite {
        Invite {
            invite_id: uuid::Uuid::nil(),
            team_id: uuid::Uuid::nil(),
            email: "ana@example.com".into(),
            role: Role::Member,
            expires_at: chrono::Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap(),
            token: Some("tok-123".into()),
            emailed: false,
        }
    }

    fn emailed_invite() -> Invite {
        Invite {
            emailed: true,
            ..base_invite()
        }
    }

    fn manual_invite() -> Invite {
        base_invite()
    }

    fn render(invite: &Invite) -> String {
        invite_text(
            invite,
            "platform",
            "tok-123",
            "SHA256:abc",
            "https://teams.example.com",
        )
    }

    #[test]
    fn invite_text_says_when_the_server_emailed_the_invite() {
        let text = render(&emailed_invite());
        assert!(
            text.contains("The server emailed the invite to ana@example.com"),
            "{text}"
        );
        // The token is still shown, as a fallback.
        assert!(text.contains("--invite tok-123"), "{text}");
        assert!(
            text.contains("bssh teams join tok-123 --fingerprint SHA256:abc"),
            "{text}"
        );
        // The fingerprint goes out of band, never through the server's mail.
        assert!(
            text.contains("not by replying to the server's email"),
            "{text}"
        );
        assert!(text.contains("  SHA256:abc\n"), "{text}");
        assert!(
            text.contains("bssh teams grant platform --fingerprint <THEIR FINGERPRINT>"),
            "{text}"
        );
        assert!(text.contains("expires 2026-11-01"), "{text}");
    }

    #[test]
    fn invite_text_without_the_email_keeps_the_manual_instructions() {
        let text = render(&manual_invite());
        assert!(!text.contains("emailed"), "{text}");
        assert!(text.contains("over a channel you trust"), "{text}");
        assert!(
            text.contains(
                "bssh teams init --server https://teams.example.com --email ana@example.com \
                 --invite tok-123"
            ),
            "{text}"
        );
        assert!(
            text.contains("bssh teams join tok-123 --fingerprint SHA256:abc"),
            "{text}"
        );
        assert!(text.contains("  SHA256:abc\n"), "{text}");
        assert!(
            text.contains("bssh teams grant platform --fingerprint <THEIR FINGERPRINT>"),
            "{text}"
        );
    }
}
