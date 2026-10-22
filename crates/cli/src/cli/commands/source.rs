use crate::cli::utils::{confirm, format_duration};
use crate::cli::SourceCommands;
use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Source, SourceKind};
use crate::services::inventory::{provider_for, sync};
use anyhow::{bail, Result};
use chrono::Utc;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

struct AddOptions {
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
    include_offline: bool,
}

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
        } => add(
            AddOptions {
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
            },
            &config,
        ),
        SourceCommands::List => list(&config),
        SourceCommands::Sync {
            name,
            all,
            dry_run,
            prune,
            prune_after,
        } => sync_sources(&config, name, all, dry_run, prune, prune_after),
        SourceCommands::Remove {
            name,
            delete_hosts,
            force,
        } => remove(&config, &name, delete_hosts, force),
    }
}

fn add(options: AddOptions, config: &AppConfig) -> Result<()> {
    let name = options.name.trim();
    if name.is_empty() {
        bail!("Source name cannot be empty.");
    }
    let kind: SourceKind = options.kind.parse().map_err(anyhow::Error::msg)?;

    ensure_flags_allowed(kind, &options)?;

    let mut source_config = Map::new();
    match kind {
        SourceKind::SshConfig => {
            if let Some(path) = &options.path {
                source_config.insert("path".into(), json!(path));
            }
        }
        SourceKind::Ansible => {
            let inventory = options
                .inventory
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--inventory is required for an ansible source"))?;
            source_config.insert("inventory".into(), json!(inventory));
        }
        SourceKind::Tailscale => {
            if options.include_offline {
                source_config.insert("include_offline".into(), json!(true));
            }
        }
        SourceKind::Aws => {
            if options.region.is_empty() {
                bail!("--region is required for an aws source");
            }
            let address = options.address.as_deref().unwrap_or("private");
            if !matches!(address, "private" | "public" | "dns") {
                bail!("--address must be private, public or dns for an aws source");
            }
            if let Some(profile) = &options.profile {
                source_config.insert("profile".into(), json!(profile));
            }
            source_config.insert("regions".into(), json!(options.region));
            source_config.insert("address".into(), json!(address));
        }
        SourceKind::Gcp => {
            let project = options
                .project
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--project is required for a gcp source"))?;
            let address = options.address.as_deref().unwrap_or("internal");
            if !matches!(address, "internal" | "external") {
                bail!("--address must be internal or external for a gcp source");
            }
            source_config.insert("project".into(), json!(project));
            source_config.insert("address".into(), json!(address));
        }
    }
    if let Some(user) = &options.user {
        source_config.insert("user".into(), json!(user));
    }
    if let Some(bastion) = &options.bastion {
        source_config.insert("bastion".into(), json!(bastion));
    }
    if let Some(bastion_user) = &options.bastion_user {
        source_config.insert("bastion_user".into(), json!(bastion_user));
    }

    let db = Database::new(config)?;
    if db.get_source(name)?.is_some() {
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

fn ensure_flags_allowed(kind: SourceKind, options: &AddOptions) -> Result<()> {
    let provided = [
        ("path", options.path.is_some()),
        ("inventory", options.inventory.is_some()),
        ("profile", options.profile.is_some()),
        ("region", !options.region.is_empty()),
        ("address", options.address.is_some()),
        ("project", options.project.is_some()),
        ("user", options.user.is_some()),
        ("bastion", options.bastion.is_some()),
        ("bastion-user", options.bastion_user.is_some()),
        ("include-offline", options.include_offline),
    ];
    let allowed: &[&str] = match kind {
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

fn sync_sources(
    config: &AppConfig,
    name: Option<String>,
    all: bool,
    dry_run: bool,
    prune: bool,
    prune_after: Option<u32>,
) -> Result<()> {
    let db = Database::new(config)?;

    let sources: Vec<Source> = if let Some(name) = name {
        match db.get_source(&name)? {
            Some(source) => vec![source],
            None => bail!("Source '{}' not found.", name),
        }
    } else if all {
        db.list_sources()?
            .into_iter()
            .map(|summary| summary.source)
            .collect()
    } else {
        bail!("Specify a source name or --all.");
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
                if all {
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

        let plan = sync::plan(&discovered, &existing, &taken_names, &source.name);
        print_plan(&plan);

        if dry_run {
            let report = plan.report(true);
            print_summary(&report);
            println!("Dry run: no changes written.");
            continue;
        }

        let prune_days = if prune {
            Some(prune_after.unwrap_or(30))
        } else {
            None
        };
        let report = sync::apply(&db, config, &source, plan, prune_days)?;
        print_summary(&report);
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

fn remove(config: &AppConfig, name: &str, delete_hosts: bool, force: bool) -> Result<()> {
    let db = Database::new(config)?;
    let source = db
        .get_source(name)?
        .ok_or_else(|| anyhow::anyhow!("Source '{}' not found.", name))?;

    let hosts = db.connections_for_source(source.id)?.len();
    println!(
        "\n⚠️  WARNING: Remove source '{}' ({}), {} host connection(s) will be {}.",
        source.name,
        source.kind,
        hosts,
        if delete_hosts { "deleted" } else { "detached" }
    );

    if !force && !confirm(&format!("Remove source '{}'?", source.name), false)? {
        println!("❌ Removal cancelled.");
        return Ok(());
    }

    db.remove_source(source.id, delete_hosts)?;
    println!("✅ Source '{}' removed.", source.name);
    Ok(())
}
