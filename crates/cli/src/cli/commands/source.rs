//! `bssh source`: inventory sources and reconciling their hosts into the
//! connection database.

use crate::cli::SourceCommands;
use crate::cli::utils::{confirm, format_duration};
use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Source, SourceKind};
use crate::services::inventory::{provider_for, sync};
use anyhow::{Result, anyhow, bail};
use chrono::Utc;
use serde_json::{Map, Value, json};
use std::collections::HashSet;

/// Age in days after which `source sync --prune` deletes a stale host; the
/// documented default of `--prune-after`.
const DEFAULT_PRUNE_DAYS: u32 = 30;

/// Which Tailscale peers `source add` imports.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PeerScope {
    /// Only peers that are online right now.
    Online,
    /// Online peers and offline ones too (`--include-offline`).
    IncludeOffline,
}

/// What `source add` was told, one field per flag.
struct AddRequest {
    name: String,
    kind: String,
    path: Option<String>,
    inventory: Option<String>,
    profile: Option<String>,
    region: Vec<String>,
    address: Option<String>,
    project: Option<String>,
    user: Option<String>,
    bastion: Option<String>,
    bastion_user: Option<String>,
    peers: PeerScope,
}

/// The sources one `source sync` run covers.
enum Selection {
    /// A single named source; its failure aborts the run.
    Named(String),
    /// Every configured source; failures are collected and reported.
    All,
}

/// What `source sync` does with the plan it computes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SyncMode {
    /// Print the plan and write nothing.
    Preview,
    /// Write the plan; `prune_after` is `Some` when stale hosts are deleted.
    Apply { prune_after: Option<u32> },
}

/// What happens to a removed source's connections.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hosts {
    /// Keep the connections, without a source.
    Detach,
    /// Delete the connections with the source.
    Delete,
}

/// Whether `source remove` asks before acting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirmation {
    /// Ask first.
    Ask,
    /// `--force`: act without asking.
    Skip,
}

/// The arguments of `bssh source remove`.
struct RemoveRequest {
    name: String,
    hosts: Hosts,
    confirmation: Confirmation,
}

/// The arguments of `bssh source sync`.
struct SyncRequest {
    selection: Selection,
    mode: SyncMode,
}

/// Runs `bssh source <command>`.
///
/// # Errors
/// Fails when a source cannot be added, synced or removed; the message names
/// the offending source, kind or flag.
pub async fn execute(command: SourceCommands, config: AppConfig) -> Result<()> {
    match command {
        SourceCommands::Add {
            name,
            kind,
            path,
            inventory,
            profile,
            region,
            address,
            project,
            user,
            bastion,
            bastion_user,
            include_offline,
        } => {
            let peers = if include_offline {
                PeerScope::IncludeOffline
            } else {
                PeerScope::Online
            };
            add(
                AddRequest {
                    name,
                    kind,
                    path,
                    inventory,
                    profile,
                    region,
                    address,
                    project,
                    user,
                    bastion,
                    bastion_user,
                    peers,
                },
                &config,
            )
        }
        SourceCommands::List => list(&config),
        SourceCommands::Sync {
            name,
            all,
            dry_run,
            prune,
            prune_after,
        } => {
            // Opened here, before the flags are resolved, as it always was.
            let db = Database::new(&config)?;
            let selection = match (name, all) {
                (Some(name), _) => Selection::Named(name),
                (None, true) => Selection::All,
                (None, false) => bail!("Specify a source name or --all."),
            };
            let mode = if dry_run {
                SyncMode::Preview
            } else if prune {
                SyncMode::Apply {
                    prune_after: Some(prune_after.unwrap_or(DEFAULT_PRUNE_DAYS)),
                }
            } else {
                SyncMode::Apply { prune_after: None }
            };
            sync_sources(&db, &config, SyncRequest { selection, mode })
        }
        SourceCommands::Remove {
            name,
            delete_hosts,
            force,
        } => {
            let hosts = if delete_hosts {
                Hosts::Delete
            } else {
                Hosts::Detach
            };
            let confirmation = if force {
                Confirmation::Skip
            } else {
                Confirmation::Ask
            };
            remove(
                &config,
                RemoveRequest {
                    name,
                    hosts,
                    confirmation,
                },
            )
        }
    }
}

fn add(request: AddRequest, config: &AppConfig) -> Result<()> {
    let name = request.name.trim();
    if name.is_empty() {
        bail!("Source name cannot be empty.");
    }
    let kind: SourceKind = request.kind.parse().map_err(anyhow::Error::msg)?;
    if kind == SourceKind::Team {
        bail!("team sources are created by `bssh teams create` or `bssh teams join`");
    }

    ensure_flags_allowed(kind, &request)?;

    let mut source_config = Map::new();
    match kind {
        SourceKind::SshConfig => {
            if let Some(path) = &request.path {
                source_config.insert("path".into(), json!(path));
            }
        }
        SourceKind::Ansible => {
            let inventory = request
                .inventory
                .as_ref()
                .ok_or_else(|| anyhow!("--inventory is required for an ansible source"))?;
            source_config.insert("inventory".into(), json!(inventory));
        }
        SourceKind::Tailscale => {
            if request.peers == PeerScope::IncludeOffline {
                source_config.insert("include_offline".into(), json!(true));
            }
        }
        SourceKind::Aws => {
            if request.region.is_empty() {
                bail!("--region is required for an aws source");
            }
            let address = request.address.as_deref().unwrap_or("private");
            if !matches!(address, "private" | "public" | "dns") {
                bail!("--address must be private, public or dns for an aws source");
            }
            if let Some(profile) = &request.profile {
                source_config.insert("profile".into(), json!(profile));
            }
            source_config.insert("regions".into(), json!(request.region));
            source_config.insert("address".into(), json!(address));
        }
        SourceKind::Gcp => {
            let project = request
                .project
                .as_ref()
                .ok_or_else(|| anyhow!("--project is required for a gcp source"))?;
            let address = request.address.as_deref().unwrap_or("internal");
            if !matches!(address, "internal" | "external") {
                bail!("--address must be internal or external for a gcp source");
            }
            source_config.insert("project".into(), json!(project));
            source_config.insert("address".into(), json!(address));
        }
        SourceKind::Team => unreachable!("rejected above"),
    }
    if let Some(user) = &request.user {
        source_config.insert("user".into(), json!(user));
    }
    if let Some(bastion) = &request.bastion {
        source_config.insert("bastion".into(), json!(bastion));
    }
    if let Some(bastion_user) = &request.bastion_user {
        source_config.insert("bastion_user".into(), json!(bastion_user));
    }

    let db = Database::new(config)?;
    if db.source(name)?.is_some() {
        bail!("A source named '{}' already exists.", name);
    }

    let source = Source::new(name.to_string(), kind, Value::Object(source_config));
    db.add_source(&source)?;

    println!("✅ Added source '{}' ({}).", source.name, source.kind);
    println!(
        "Run `bssh source sync {}` to import its hosts.",
        source.name
    );
    Ok(())
}

fn ensure_flags_allowed(kind: SourceKind, request: &AddRequest) -> Result<()> {
    let provided = [
        ("path", request.path.is_some()),
        ("inventory", request.inventory.is_some()),
        ("profile", request.profile.is_some()),
        ("region", !request.region.is_empty()),
        ("address", request.address.is_some()),
        ("project", request.project.is_some()),
        ("user", request.user.is_some()),
        ("bastion", request.bastion.is_some()),
        ("bastion-user", request.bastion_user.is_some()),
        (
            "include-offline",
            request.peers == PeerScope::IncludeOffline,
        ),
    ];
    let allowed: &[&str] = match kind {
        SourceKind::Team => &[],
        SourceKind::SshConfig => &["path"],
        SourceKind::Ansible => &["inventory"],
        SourceKind::Tailscale => &["include-offline"],
        SourceKind::Aws => &[
            "profile",
            "region",
            "address",
            "user",
            "bastion",
            "bastion-user",
        ],
        SourceKind::Gcp => &["project", "address", "user", "bastion", "bastion-user"],
    };

    for (flag, present) in provided {
        if present && !allowed.contains(&flag) {
            bail!("--{} is not valid for a {} source", flag, kind);
        }
    }
    Ok(())
}

fn list(config: &AppConfig) -> Result<()> {
    let db = Database::new(config)?;
    let sources = db.list_sources()?;
    if sources.is_empty() {
        println!("No inventory sources configured. Add one with `bssh source add`.");
        return Ok(());
    }

    println!("📦 Inventory sources");
    println!(
        "{:<20} {:<12} {:>5} {:>5}  {:<16} ERROR",
        "NAME", "KIND", "HOSTS", "STALE", "LAST SYNC"
    );
    for summary in sources {
        let source = &summary.source;
        let last_sync = source
            .last_sync_at
            .map(format_duration)
            .unwrap_or_else(|| "never".to_string());
        let error = source
            .last_sync_error
            .as_deref()
            .map(|error| {
                if error.chars().count() > 40 {
                    format!("{}…", error.chars().take(40).collect::<String>())
                } else {
                    error.to_string()
                }
            })
            .unwrap_or_default();
        println!(
            "{:<20} {:<12} {:>5} {:>5}  {:<16} {}",
            source.name, source.kind, summary.hosts, summary.stale, last_sync, error
        );
    }
    Ok(())
}

fn sync_sources(db: &Database, config: &AppConfig, request: SyncRequest) -> Result<()> {
    let keep_going = matches!(&request.selection, Selection::All);
    let sources: Vec<Source> = match request.selection {
        Selection::Named(name) => match db.source(&name)? {
            Some(source) => vec![source],
            None => bail!("Source '{}' not found.", name),
        },
        Selection::All => db
            .list_sources()?
            .into_iter()
            .map(|summary| summary.source)
            .collect(),
    };

    if sources.is_empty() {
        println!("No inventory sources configured. Add one with `bssh source add`.");
        return Ok(());
    }

    let mut failures = Vec::new();
    for source in sources {
        println!("Syncing '{}' ({}):", source.name, source.kind);
        let discovered = match provider_for(source.kind).fetch(&source) {
            Ok(hosts) => hosts,
            Err(error) => {
                let message = format!("{:#}", error);
                db.record_sync_result(source.id, Utc::now(), Some(&message))?;
                eprintln!("❌ Failed to sync '{}': {}", source.name, message);
                if keep_going {
                    failures.push(source.name.clone());
                    continue;
                }
                return Err(error);
            }
        };

        let existing = db.connections_for_source(source.id)?;
        let taken_names: HashSet<String> = db
            .list_connections(None, false)?
            .into_iter()
            .filter(|connection| connection.source_id != Some(source.id))
            .map(|connection| connection.name)
            .collect();

        let plan = sync::plan(&discovered, &existing, &taken_names, &source);
        print_plan(&plan);

        match request.mode {
            SyncMode::Preview => {
                let report = plan.report(true);
                print_summary(&report);
                println!("Dry run: no changes written.");
            }
            SyncMode::Apply { prune_after } => {
                let report = sync::apply(db, config, &source, plan, prune_after)?;
                print_summary(&report);
            }
        }
    }

    if !failures.is_empty() {
        bail!(
            "{} source(s) failed to sync: {}",
            failures.len(),
            failures.join(", ")
        );
    }
    Ok(())
}

fn print_plan(plan: &sync::SyncPlan) {
    for host in &plan.add {
        println!("+ added {} ({})", host.name, host.host);
    }
    for (connection, host) in &plan.update {
        if connection.host != host.host {
            println!(
                "~ updated {} (host {} → {})",
                connection.name, connection.host, host.host
            );
        } else {
            println!("~ updated {}", connection.name);
        }
    }
    for connection in &plan.mark_stale {
        println!("- stale {}", connection.name);
    }
    for connection in &plan.unstale {
        println!("↺ back {}", connection.name);
    }
    for (from, to) in &plan.renamed {
        println!("renamed: {} → {}", from, to);
    }
    for (name, reason) in &plan.invalid {
        eprintln!("skipped {} (invalid): {}", name, reason);
    }
    for duplicate in &plan.duplicates {
        eprintln!("duplicate external id ignored: {}", duplicate);
    }
    println!("skipped (invalid): {}", plan.invalid.len());
}

fn print_summary(report: &sync::SyncReport) {
    let pruned = if report.pruned > 0 {
        format!(", pruned {}", report.pruned)
    } else {
        String::new()
    };
    println!(
        "Added {}, updated {}, marked {} stale, restored {}{}.",
        report.added, report.updated, report.stale, report.unstaled, pruned
    );
}

fn remove(config: &AppConfig, request: RemoveRequest) -> Result<()> {
    let db = Database::new(config)?;
    let source = db
        .source(&request.name)?
        .ok_or_else(|| anyhow!("Source '{}' not found.", request.name))?;

    let hosts = db.connections_for_source(source.id)?.len();
    println!(
        "\n⚠️  WARNING: Remove source '{}' ({}), {} host connection(s) will be {}.",
        source.name,
        source.kind,
        hosts,
        if request.hosts == Hosts::Delete {
            "deleted"
        } else {
            "detached"
        }
    );

    if request.confirmation == Confirmation::Ask
        && !confirm(&format!("Remove source '{}'?", source.name), false)?
    {
        println!("❌ Removal cancelled.");
        return Ok(());
    }

    db.remove_source(source.id, request.hosts == Hosts::Delete)?;
    println!("✅ Source '{}' removed.", source.name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_request(peers: PeerScope) -> AddRequest {
        AddRequest {
            name: "src".into(),
            kind: "kind".into(),
            path: None,
            inventory: None,
            profile: None,
            region: Vec::new(),
            address: None,
            project: None,
            user: None,
            bastion: None,
            bastion_user: None,
            peers,
        }
    }

    #[test]
    fn a_flag_of_another_kind_is_rejected_by_its_flag_name() {
        let mut request = add_request(PeerScope::Online);
        request.inventory = Some("hosts.ini".into());
        let err = ensure_flags_allowed(SourceKind::SshConfig, &request)
            .expect_err("inventory belongs to ansible");
        assert_eq!(
            err.to_string(),
            "--inventory is not valid for a ssh-config source"
        );
    }

    #[test]
    fn tailscale_accepts_only_include_offline() {
        let mut request = add_request(PeerScope::IncludeOffline);
        request.region.push("eu-west-1".into());
        let err = ensure_flags_allowed(SourceKind::Tailscale, &request)
            .expect_err("region belongs to aws");
        assert_eq!(
            err.to_string(),
            "--region is not valid for a tailscale source"
        );
        assert!(
            ensure_flags_allowed(
                SourceKind::Tailscale,
                &add_request(PeerScope::IncludeOffline)
            )
            .is_ok()
        );
    }

    #[test]
    fn aws_accepts_its_own_flags() {
        let mut request = add_request(PeerScope::Online);
        request.region.push("eu-west-1".into());
        request.profile = Some("work".into());
        request.bastion = Some("jump.example.com".into());
        ensure_flags_allowed(SourceKind::Aws, &request).expect("aws flags are valid");
    }

    #[test]
    fn flags_are_checked_in_the_order_they_are_listed() {
        let mut request = add_request(PeerScope::Online);
        request.path = Some("~/.ssh/config".into());
        request.region.push("eu-west-1".into());
        let err =
            ensure_flags_allowed(SourceKind::Gcp, &request).expect_err("neither flag fits gcp");
        assert_eq!(err.to_string(), "--path is not valid for a gcp source");
    }

    #[test]
    fn team_sources_accept_no_flags() {
        let mut request = add_request(PeerScope::Online);
        request.user = Some("root".into());
        let err = ensure_flags_allowed(SourceKind::Team, &request).expect_err("team takes nothing");
        assert_eq!(err.to_string(), "--user is not valid for a team source");
    }
}
