//! Local tables and the bridge between a team's source and the connections
//! table: schema, team lookups and linking shared connections.

use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Connection, DiscoveredHost, Source, SourceKind};
use crate::services::inventory::sync as inventory;
use crate::services::teams::VaultView;
use anyhow::{anyhow, bail, Result};
use bssh_teams_proto::api::AuditKind;
use bssh_teams_proto::wire::{Fixed, PublicKey32};
use chrono::Utc;
use rusqlite::params;
use std::collections::HashSet;
use uuid::Uuid;

pub(crate) const SCHEMA_V9: &str = "
CREATE TABLE IF NOT EXISTS teams_identity (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    server_url TEXT NOT NULL,
    email TEXT NOT NULL,
    account_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS team_authors (
    team_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    ed25519 BLOB NOT NULL,
    PRIMARY KEY (team_id, device_id)
);
CREATE TABLE IF NOT EXISTS team_approved_devices (
    team_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    email TEXT NOT NULL,
    x25519 BLOB NOT NULL,
    ed25519 BLOB NOT NULL,
    approved_at TEXT NOT NULL,
    PRIMARY KEY (team_id, device_id)
);
CREATE TABLE IF NOT EXISTS team_audit_pending (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id TEXT NOT NULL,
    entry_id TEXT,
    kind TEXT NOT NULL,
    occurred_at TEXT NOT NULL
);
";

pub(super) fn uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| anyhow!("corrupt id {s:?} in local teams tables: {e}"))
}

pub(super) fn key32(b: Vec<u8>) -> Result<PublicKey32> {
    let a: [u8; 32] = b
        .try_into()
        .map_err(|_| anyhow!("corrupt key in local teams tables"))?;
    Ok(Fixed(a))
}

pub(crate) fn kind_str(kind: AuditKind) -> &'static str {
    kind.as_str()
}

pub(crate) fn kind_from_str(s: &str) -> Option<AuditKind> {
    AuditKind::parse(s)
}

// ── Identity ────────────────────────────────────────────────────────────────

pub(super) fn team_id_of(source: &Source) -> Option<Uuid> {
    if source.kind != SourceKind::Team {
        return None;
    }
    source
        .config
        .get("team_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
}

/// Every team this machine knows, as `(team_id, source)`.
pub fn local_teams(db: &Database) -> Result<Vec<(Uuid, Source)>> {
    Ok(db
        .list_sources()?
        .into_iter()
        .filter_map(|s| team_id_of(&s.source).map(|id| (id, s.source)))
        .collect())
}

/// Every team this machine knows, as `(team_id, local name)`. Organizations
/// are named by their teams in `<org>` arguments.
pub fn local_team_names(db: &Database) -> Result<Vec<(Uuid, String)>> {
    Ok(local_teams(db)?
        .into_iter()
        .map(|(team_id, source)| (team_id, source.name))
        .collect())
}

/// Find a team by its local name, or by a prefix of its id.
pub fn resolve_team(db: &Database, name_or_id: &str) -> Result<(Uuid, Source)> {
    let teams = local_teams(db)?;
    if let Some(t) = teams.iter().find(|(_, s)| s.name == name_or_id) {
        return Ok(t.clone());
    }
    let by_id: Vec<_> = teams
        .iter()
        .filter(|(id, _)| id.to_string().starts_with(&name_or_id.to_lowercase()))
        .collect();
    match by_id.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!("no team named '{name_or_id}' (see `bssh teams list`)"),
        _ => bail!("'{name_or_id}' matches several teams; use more of the id"),
    }
}

pub(crate) fn team_of_connection(
    db: &Database,
    connection: &Connection,
) -> Result<Option<(Uuid, Uuid)>> {
    let Some(source_id) = connection.source_id else {
        return Ok(None);
    };
    let Some(source) = db.get_source(&source_id.to_string())? else {
        return Ok(None);
    };
    let (Some(team_id), Some(entry)) = (team_id_of(&source), connection.external_id.as_deref())
    else {
        return Ok(None);
    };
    Ok(Some((team_id, uuid(entry)?)))
}

/// Queue a "session started" audit event if `connection` comes from a team
/// vault. Needs no passphrase: the event is signed and uploaded by the next
/// `bssh teams sync`, with the time it actually happened.
pub fn record_session_start(db: &Database, connection: &Connection) -> Result<()> {
    if let Some((team_id, entry_id)) = team_of_connection(db, connection)? {
        db.conn.execute(
            "INSERT INTO team_audit_pending (team_id, entry_id, kind, occurred_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                team_id.to_string(),
                entry_id.to_string(),
                kind_str(AuditKind::SessionStart),
                Utc::now().to_rfc3339()
            ],
        )?;
    }
    Ok(())
}

// ── Session ─────────────────────────────────────────────────────────────────

/// Link a local connection to a team `source` under a vault entry id, before
/// the share's sync materializes the vault: the entry of a copy an earlier
/// share left behind (`name@team`, `name@team-2`..., which is removed), or a
/// new one. Returns the entry id.
pub(crate) fn link_to_team_source(
    db: &Database,
    source: &Source,
    connection: &Connection,
) -> Result<Uuid> {
    let copy_name = format!("{}@{}", connection.name, source.name);
    let copy = db.connections_for_source(source.id)?.into_iter().find(|c| {
        c.name == copy_name
            || c.name
                .strip_prefix(&copy_name)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|n| n.parse::<u32>().is_ok())
    });
    let entry_id = match &copy {
        Some(c) => uuid(c.external_id.as_deref().unwrap_or_default())?,
        None => Uuid::new_v4(),
    };
    if let Some(c) = copy {
        db.remove_connection(&c.id.to_string())?;
    }
    let mut linked = connection.clone();
    linked.source_id = Some(source.id);
    linked.external_id = Some(entry_id.to_string());
    db.update_connection(&linked)?;
    Ok(entry_id)
}

/// Mirror a decrypted vault into `db` through the inventory machinery: hosts are
/// keyed by entry id, removed ones go stale. The team needs a local source.
pub(crate) fn materialize_into(
    db: &Database,
    config: &AppConfig,
    team_id: Uuid,
    view: &VaultView,
) -> Result<inventory::SyncReport> {
    let (_, mut source) = local_teams(db)?
        .into_iter()
        .find(|(id, _)| *id == team_id)
        .ok_or_else(|| anyhow!("team {team_id} has no local source"))?;

    if let Some(name) = &view.team_name {
        if source.name != *name
            && source.name.starts_with("team-")
            && db.get_source(name)?.is_none()
        {
            db.conn.execute(
                "UPDATE sources SET name = ?1 WHERE id = ?2",
                params![name, source.id.to_string()],
            )?;
            source.name = name.clone();
        }
    }

    let discovered: Vec<DiscoveredHost> = view
        .connections
        .iter()
        .map(|c| DiscoveredHost {
            external_id: c.entry_id.to_string(),
            name: c.record.name.clone(),
            host: c.record.host.clone(),
            user: Some(c.record.user.clone()),
            port: Some(c.record.port),
            key_path: None,
            tags: c.record.tags.clone(),
            bastion: c.record.bastion.clone(),
            bastion_user: c.record.bastion_user.clone(),
            use_kerberos: c.record.use_kerberos,
        })
        .collect();
    let existing = db.connections_for_source(source.id)?;
    let taken: HashSet<String> = db
        .list_connections(None, false)?
        .into_iter()
        .filter(|c| c.source_id != Some(source.id))
        .map(|c| c.name)
        .collect();
    let plan = inventory::plan(&discovered, &existing, &taken, &source);
    inventory::apply(db, config, &source, plan, None)
}
