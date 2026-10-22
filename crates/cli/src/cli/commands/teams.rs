use crate::cli::utils::confirm;
use crate::cli::TeamsCommands;
use crate::config::AppConfig;
use crate::services::teams::session::{self, JoinWith, PendingDevice, Session, TeamSync};
use crate::services::teams::TeamsStore;
use anyhow::{anyhow, bail, Result};
use bssh_teams_proto::api::Role;
use zeroize::Zeroizing;

/// Env var read instead of prompting, for scripts.
const PASSPHRASE_ENV: &str = "BSSH_TEAMS_PASSPHRASE";

fn passphrase(new: bool) -> Result<Zeroizing<String>> {
    if let Ok(p) = std::env::var(PASSPHRASE_ENV) {
        if !p.is_empty() {
            return Ok(Zeroizing::new(p));
        }
    }
    let prompt = if new {
        "Choose a passphrase for this device's Teams keys: "
    } else {
        "Teams passphrase: "
    };
    let p = Zeroizing::new(rpassword::prompt_password(prompt)?);
    if new {
        if p.chars().count() < 8 {
            bail!("use at least 8 characters");
        }
        let again = Zeroizing::new(rpassword::prompt_password("Repeat it: ")?);
        if *again != *p {
            bail!("the passphrases don't match");
        }
    }
    Ok(p)
}

fn default_device_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "device".to_string())
}

pub async fn execute(command: TeamsCommands, config: AppConfig) -> Result<()> {
    let store = TeamsStore::open(&config)?;
    run(command, &store, &config)
        .await
        .map_err(|e| match session::explain(&e) {
            Some(hint) => e.context(hint),
            None => e,
        })
}

async fn run(command: TeamsCommands, store: &TeamsStore, config: &AppConfig) -> Result<()> {
    let db = store.database();
    match command {
        TeamsCommands::Init {
            server,
            email,
            device_name,
        } => {
            let pass = passphrase(true)?;
            let name = device_name.unwrap_or_else(default_device_name);
            let identity = session::init(store, &server, &email, &name, &pass).await?;
            let s = Session::open(store, config, &pass)?;
            println!(
                "✅ Teams is set up for {} on {}",
                identity.email, identity.server_url
            );
            println!("   Device '{name}', fingerprint {}", s.my_fingerprint());
            println!();
            println!("Next: `bssh teams create <name>`, or join a team you were invited to with");
            println!("      `bssh teams join <TOKEN> --fingerprint <ADMIN FINGERPRINT>`.");
        }

        TeamsCommands::Whoami => {
            let identity = session::load_identity(db)?
                .ok_or_else(|| anyhow!("Teams isn't set up on this machine (`bssh teams init`)"))?;
            let s = Session::open(store, config, &passphrase(false)?)?;
            println!("Account:     {}", identity.email);
            println!("Server:      {}", identity.server_url);
            println!("Device:      {}", identity.device_id);
            println!("Fingerprint: {}", s.my_fingerprint());
        }

        TeamsCommands::Create { name } => {
            let s = Session::open(store, config, &passphrase(false)?)?;
            let sync = s.create_team(&name).await?;
            println!(
                "✅ Created team '{name}' ({}); you're its admin.",
                sync.team_id
            );
            print_sync(&name, &sync);
            println!();
            println!("Share a connection:  bssh teams share <connection> --team {name}");
            println!("Invite someone:      bssh teams invite {name} <email>");
        }

        TeamsCommands::List => {
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
        }

        TeamsCommands::Invite { team, email, admin } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(false)?)?;
            let role = if admin { Role::Admin } else { Role::Member };
            let (invite, fp) = s.invite(team_id, &email, role).await?;
            let token = invite
                .token
                .ok_or_else(|| anyhow!("the server didn't return an invite token"))?;
            println!(
                "✉️  Invited {} to '{}' (expires {}).",
                invite.email,
                source.name,
                invite.expires_at.format("%Y-%m-%d")
            );
            println!();
            println!("Send them these, over a channel you trust (the token is shown only once):");
            println!();
            println!(
                "  bssh teams init --server {} --email {}",
                s.identity.server_url, invite.email
            );
            println!("  bssh teams join {token} --fingerprint {fp}");
            println!();
            println!("When they send back their device fingerprint, run:");
            println!(
                "  bssh teams grant {} --fingerprint <THEIR FINGERPRINT>",
                source.name
            );
        }

        TeamsCommands::Join {
            token,
            team,
            fingerprint,
        } => {
            let s = Session::open(store, config, &passphrase(false)?)?;
            let with = match (&token, team) {
                (Some(token), _) => JoinWith::Token(token),
                (None, Some(team)) => JoinWith::Team(team),
                (None, None) => bail!("give an invite token or --team"),
            };
            let (team_id, name) = s.join(with, &fingerprint).await?;
            println!("✅ Joined team {team_id} (shown locally as '{name}' until its first sync).");
            println!("   Pinned admin fingerprint {}", fingerprint.trim());
            println!();
            println!(
                "Send your device fingerprint to the admin so they can grant you the team key:"
            );
            println!();
            println!("  {}", s.my_fingerprint());
            println!();
            println!("Then run `bssh teams sync`.");
        }

        TeamsCommands::Grant { team, fingerprint } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(false)?)?;
            match fingerprint {
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
                    let granted = s.grant(team_id, &fp).await?;
                    println!(
                        "🔑 Granted the '{}' key to {} ('{}').",
                        source.name, granted.email, granted.device.name
                    );
                    println!("   They can now run `bssh teams sync`.");
                }
            }
        }

        TeamsCommands::Sync { team } => {
            let targets = match team {
                Some(t) => vec![session::resolve_team(db, &t)?],
                None => session::local_teams(db)?,
            };
            if targets.is_empty() {
                println!("No teams on this machine.");
                return Ok(());
            }
            let s = Session::open(store, config, &passphrase(false)?)?;
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
        }

        TeamsCommands::Share { connection, team } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let conn = db
                .get_connection_or_alias(&connection)?
                .ok_or_else(|| anyhow!("no connection named '{connection}'"))?;
            let s = Session::open(store, config, &passphrase(false)?)?;
            let (_, sync) = s.share(team_id, &conn).await?;
            println!("📤 Shared '{}' with '{}'.", conn.name, source.name);
            print_sync(&source.name, &sync);
            if conn.source_id.is_none() {
                println!(
                    "   Your local '{}' stays as is; the team's copy is listed under '{}'.",
                    conn.name, source.name
                );
            }
        }

        TeamsCommands::Unshare { connection } => {
            let conn = db
                .get_connection_or_alias(&connection)?
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
            let s = Session::open(store, config, &passphrase(false)?)?;
            let sync = s.unshare(&conn).await?;
            println!("🗑️  Removed '{}' from the team vault.", conn.name);
            print_sync(sync.name.as_deref().unwrap_or("team"), &sync);
        }

        TeamsCommands::Members { team } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(false)?)?;
            let members = s.members(team_id).await?;
            println!("👥 {} ({} member(s))", source.name, members.len());
            for m in members {
                let role = match m.role {
                    Role::Admin => "admin",
                    Role::Member => "member",
                };
                println!("  {} [{role}]", m.email);
                for d in m.devices.iter().filter(|d| d.revoked_at.is_none()) {
                    println!(
                        "      {:<20} {}",
                        d.name,
                        crate::services::teams::crypto::fingerprint(&d.ed25519_public)
                    );
                }
            }
        }

        TeamsCommands::Remove { team, email } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            if !confirm(
                &format!(
                    "Remove {email} from '{}' and rotate the team key?",
                    source.name
                ),
                false,
            )? {
                println!("Cancelled.");
                return Ok(());
            }
            let s = Session::open(store, config, &passphrase(false)?)?;
            let sync = s.remove(team_id, &email).await?;
            println!("✅ Removed {email} from '{}'.", source.name);
            print_sync(&source.name, &sync);
            println!(
                "   They can no longer sync, and nothing written from now on is readable by them.\n   \
                 They may still have copies of what they already synced: rotate any credentials that matter."
            );
        }
    }
    Ok(())
}

fn print_pending(p: &PendingDevice) {
    println!("  {:<28} {:<16} {}", p.email, p.device.name, p.fingerprint);
}

fn print_sync(name: &str, sync: &TeamSync) {
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
        println!("   ⚠️  a member was removed: an admin must sync to rotate the team key; your edits stay queued.");
    }
    for e in &r.errors {
        println!("   ❌ {e}");
    }
}
