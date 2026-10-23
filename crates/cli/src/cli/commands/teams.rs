use crate::cli::utils::confirm;
use crate::cli::TeamsCommands;
use crate::config::AppConfig;
use crate::services::teams::audit::EventCheck;
use crate::services::teams::billing::{self, ReadOnlyReason};
use crate::services::teams::session::{
    self, AuditQuery, JoinWith, PendingDevice, Session, TeamAudit, TeamSync,
};
use crate::services::teams::TeamsStore;
use anyhow::{anyhow, bail, Result};
use bssh_teams_proto::api::{BillingInterval, Role};
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
            invite,
        } => {
            let pass = passphrase(true)?;
            let name = device_name.unwrap_or_else(default_device_name);
            let identity =
                session::init(store, &server, &email, &name, &pass, invite.as_deref()).await?;
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
                "  bssh teams init --server {} --email {} --invite {token}",
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
            if reached_server(&sync) {
                println!("🗑️  Removed '{}' from the team vault.", conn.name);
            } else {
                println!(
                    "📥 Removing '{}' is queued: it happens once a sync goes through.",
                    conn.name
                );
            }
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
        }

        TeamsCommands::Uninvite { team, email } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(false)?)?;
            let invite = s.uninvite(team_id, &email).await?;
            println!(
                "🚫 Revoked the invite for {} to '{}': its token no longer works and its seat is free.",
                invite.email, source.name
            );
        }

        TeamsCommands::Billing {
            team,
            checkout,
            seats,
            yearly,
            manage,
        } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(false)?)?;
            if checkout || manage {
                let url = if manage {
                    s.portal_url(team_id).await?
                } else {
                    let info = s.billing(team_id).await?;
                    let seats = seats.unwrap_or_else(|| info.seats_used.max(info.free_seats + 1));
                    let interval = if yearly {
                        BillingInterval::Year
                    } else {
                        BillingInterval::Month
                    };
                    s.checkout_url(team_id, seats, interval).await?
                };
                println!(
                    "Opening the billing page for '{}' in your browser:",
                    source.name
                );
                println!("  {url}");
                open_in_browser(&url);
                return Ok(());
            }
            let info = s.billing(team_id).await?;
            print_billing(&source.name, &info);
        }

        TeamsCommands::Audit {
            team,
            host,
            member,
            since,
            limit,
        } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let since = since.as_deref().map(parse_since).transpose()?;
            let s = Session::open(store, config, &passphrase(false)?)?;
            let entry_id = host
                .as_deref()
                .map(|h| s.entry_for_connection(team_id, h))
                .transpose()?;
            let log = s
                .audit(
                    team_id,
                    AuditQuery {
                        entry_id,
                        email: member,
                        since,
                        limit,
                    },
                )
                .await?;
            print_audit(&source.name, &log);
            if !log.is_clean() {
                bail!("the audit log failed verification");
            }
        }

        TeamsCommands::Remove { team, email } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(false)?)?;
            let preview = s.remove_preview(team_id, &email).await?;
            if !preview.blocking.is_empty() {
                println!(
                    "⚠️  These devices were never approved on this machine, so the team key can't be \
                     rotated yet:"
                );
                for d in &preview.blocking {
                    println!("   {} {} ({})", d.email, d.name, d.fingerprint);
                }
                println!(
                    "   {email} is still removed; nobody can push until you approve them \
                     (`bssh teams grant {}`) and sync.",
                    source.name
                );
            }
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
        let from_start = log.chains.iter().all(|c| c.from_genesis());
        println!(
            "✅ Verified on this machine: {} signature(s) valid, {} device chain(s) intact{}.",
            log.lines.len(),
            log.chains.len(),
            if from_start && log.complete {
                ", complete from each device's first event"
            } else {
                ""
            }
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
        println!("   Events can't be edited, reordered or removed without this showing; treat it as tampering.");
    }
    if !log.complete {
        println!(
            "   Showing the newest events only; narrow with --since or --host to see older ones."
        );
    }
}

fn print_pending(p: &PendingDevice) {
    println!("  {:<28} {:<16} {}", p.email, p.device.name, p.fingerprint);
}

/// The server took this run's queued edits (or there was nothing to refuse).
fn reached_server(sync: &TeamSync) -> bool {
    let r = &sync.report;
    r.pushed > 0 || (r.errors.is_empty() && !r.rotation_required && !sync.waiting_for_key)
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
        if e.contains("AuditChainBroken") {
            println!(
                "      The server's copy of your audit log is missing events this device uploaded \
                 before. That can mean they were deleted on the server: tell your team admin."
            );
        }
    }
}

fn print_billing(team: &str, info: &bssh_teams_proto::api::BillingInfo) {
    use bssh_teams_proto::api::Plan;
    if !info.billing_enabled {
        println!(
            "💳 {team}: {} of {} seats used (this server has no billing)",
            info.seats_used, info.seat_limit
        );
        return;
    }
    let plan = match info.plan {
        Plan::Free => format!("Free plan, {} seats", info.free_seats),
        Plan::Team => format!(
            "Team plan, {} seats{}",
            info.paid_seats,
            match info.interval {
                Some(BillingInterval::Year) => ", billed yearly",
                Some(BillingInterval::Month) => ", billed monthly",
                None => "",
            }
        ),
    };
    println!("💳 {team}: {plan}");
    println!(
        "   {} of {} seats used (members and pending invites)",
        info.seats_used, info.seat_limit
    );
    if let Some(end) = info.current_period_end.filter(|_| info.plan == Plan::Team) {
        let date = end.format("%Y-%m-%d");
        if info.cancel_at_period_end {
            println!("   Ends on {date}, then the team falls back to the free plan.");
        } else {
            println!("   Renews on {date}");
        }
    }
    match info.status.as_deref() {
        Some("past_due") => println!(
            "   ⚠️  Payment failed, Stripe is retrying: update the card with `bssh teams billing {team} --manage`."
        ),
        Some(status) if !billing::is_live(Some(status)) => {
            println!("   Subscription status: {status}")
        }
        _ => {}
    }
    match billing::read_only_reason(info) {
        Some(ReadOnlyReason::NotEnoughSeats) => {
            println!();
            println!(
                "⚠️  Read-only: more members than paid seats ({} paid).",
                info.paid_seats
            );
            println!(
                "   Members can still sync and connect; changes are refused until an admin adds seats"
            );
            println!("   (`bssh teams billing {team} --manage`) or removes members.");
        }
        Some(ReadOnlyReason::NoSubscription) => {
            println!();
            println!(
                "⚠️  Read-only: no active subscription, and more members than the {} free seats.",
                info.free_seats
            );
            println!(
                "   Members can still sync and connect; changes are refused until an admin subscribes"
            );
            println!(
                "   (`bssh teams billing {team} --checkout`) or removes members down to the free seats."
            );
        }
        None if info.plan == Plan::Free && info.seats_used >= info.seat_limit => {
            println!();
            println!("   All free seats are taken. More: `bssh teams billing {team} --checkout`.");
        }
        None => {}
    }
}

/// Opens a URL in the default browser; the URL is printed first, so this may fail quietly.
fn open_in_browser(url: &str) {
    let mut cmd = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    let _ = cmd
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
