use super::CommandError;
use super::open_db_and_config;
use bayesian_ssh::database::SourceSummary;
use bayesian_ssh::models::{Source, SourceKind};
use bayesian_ssh::services::inventory::{provider_for, sync};
use chrono::Utc;
use std::collections::HashSet;

#[tauri::command]
pub async fn list_sources() -> Result<Vec<SourceSummary>, CommandError> {
    tokio::task::spawn_blocking(|| {
        let (db, _) = open_db_and_config()?;
        db.list_sources().map_err(CommandError::from)
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}

#[tauri::command]
pub async fn add_source(
    name: String,
    kind: String,
    config: serde_json::Value,
) -> Result<Source, CommandError> {
    tokio::task::spawn_blocking(move || {
        let (db, _) = open_db_and_config()?;
        let name = name.trim();
        if name.is_empty() {
            return Err("Source name cannot be empty.".into());
        }
        let kind: SourceKind = kind.parse().map_err(|e: String| e)?;
        if db.source(name)?.is_some() {
            return Err(format!("A source named '{}' already exists.", name).into());
        }

        let source = Source::new(name.to_string(), kind, config);
        db.add_source(&source)?;
        Ok(source)
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}

#[tauri::command]
pub async fn remove_source(name: String, delete_hosts: bool) -> Result<(), CommandError> {
    tokio::task::spawn_blocking(move || {
        let (db, _) = open_db_and_config()?;
        let source = db
            .source(&name)?
            .ok_or_else(|| format!("Source '{}' not found.", name))?;
        db.remove_source(source.id, delete_hosts)
            .map_err(CommandError::from)
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}

#[tauri::command]
pub async fn sync_source(name: String, dry_run: bool) -> Result<sync::SyncReport, CommandError> {
    tokio::task::spawn_blocking(move || {
        let (db, config) = open_db_and_config()?;
        let source = db
            .source(&name)?
            .ok_or_else(|| format!("Source '{}' not found.", name))?;

        let discovered = match provider_for(source.kind).fetch(&source) {
            Ok(hosts) => hosts,
            Err(error) => {
                let message = format!("{:#}", error);
                let _ = db.record_sync_result(source.id, Utc::now(), Some(&message));
                return Err(message.into());
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
        if dry_run {
            return Ok(plan.report(true));
        }
        sync::apply(&db, &config, &source, plan, None).map_err(CommandError::from)
    })
    .await
    .map_err(|e| CommandError::from(format!("Background task failed: {e}")))?
}
