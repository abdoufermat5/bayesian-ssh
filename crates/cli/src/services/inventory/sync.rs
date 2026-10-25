use super::config_str;
use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Connection, DiscoveredHost, Source, SourceKind};
use anyhow::Result;
use chrono::{Duration, Utc};
use rusqlite::{params, Transaction};
use serde::Serialize;
use std::collections::HashSet;

/// How a sync would change the stored connections for one source.
#[derive(Debug, Default, Clone)]
pub struct SyncPlan {
    pub add: Vec<DiscoveredHost>,
    pub update: Vec<(Connection, DiscoveredHost)>,
    pub mark_stale: Vec<Connection>,
    pub unstale: Vec<Connection>,
    pub renamed: Vec<(String, String)>,
    pub duplicates: Vec<String>,
    /// Hosts rejected by `Connection::validate`, with the reason.
    pub invalid: Vec<(String, String)>,
}

/// What a sync (or its preview) did.
#[derive(Debug, Default, Clone, Serialize)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub stale: usize,
    pub unstaled: usize,
    pub pruned: usize,
    pub skipped_invalid: usize,
    pub renamed: Vec<(String, String)>,
    pub duplicates: Vec<String>,
    pub dry_run: bool,
}

impl SyncPlan {
    pub fn report(&self, dry_run: bool) -> SyncReport {
        SyncReport {
            added: self.add.len(),
            updated: self.update.len(),
            stale: self.mark_stale.len(),
            unstaled: self.unstale.len(),
            pruned: 0,
            skipped_invalid: self.invalid.len(),
            renamed: self.renamed.clone(),
            duplicates: self.duplicates.clone(),
            dry_run,
        }
    }
}

/// Diff the hosts currently reported by an inventory against the connections
/// already owned by the source.
///
/// Hosts are matched only by `external_id`, so a changed IP updates the same
/// connection instead of creating a duplicate. Names are assigned once, when a
/// host is first seen, and never renamed afterwards.
pub fn plan(
    discovered: &[DiscoveredHost],
    existing_for_source: &[Connection],
    taken_names: &HashSet<String>,
    source: &Source,
) -> SyncPlan {
    let mut plan = SyncPlan::default();
    let now = Utc::now();
    let authoritative = source.kind == SourceKind::Team;
    let source_name = source.name.as_str();

    let mut used_names: HashSet<String> = taken_names.clone();
    used_names.extend(existing_for_source.iter().map(|c| c.name.clone()));

    let mut matched: HashSet<String> = HashSet::new();
    let mut seen: HashSet<String> = HashSet::new();

    for host in discovered {
        if !seen.insert(host.external_id.clone()) {
            plan.duplicates.push(host.external_id.clone());
            continue;
        }
        matched.insert(host.external_id.clone());

        if let Some(existing) = existing_for_source
            .iter()
            .find(|c| c.external_id.as_deref() == Some(host.external_id.as_str()))
        {
            let mut updated = existing.clone();
            updated.host = host.host.clone();
            if let Some(port) = host.port {
                updated.port = port;
            }
            if let Some(user) = &host.user {
                updated.user = user.clone();
            }
            if let Some(key) = &host.key_path {
                updated.key_path = Some(key.clone());
            }
            for tag in &host.tags {
                if let Some(tag) = Connection::normalize_tag(tag) {
                    if !updated.tags.contains(&tag) {
                        updated.tags.push(tag);
                    }
                }
            }

            let changed = updated.host != existing.host
                || updated.port != existing.port
                || updated.user != existing.user
                || updated.key_path != existing.key_path
                || updated.tags != existing.tags
                // A team vault is authoritative for routing, so a bastion or
                // Kerberos change must reach `apply` even on its own.
                || (authoritative
                    && (host.bastion != existing.bastion
                        || host.bastion_user != existing.bastion_user
                        || host.use_kerberos != existing.use_kerberos));
            // Provider values reach the ssh argv, so an update must pass the
            // same validation as a new host.
            if changed {
                match updated.validate() {
                    Ok(()) => plan.update.push((existing.clone(), host.clone())),
                    Err(reason) => plan.invalid.push((existing.name.clone(), reason)),
                }
            }
            if existing.stale_since.is_some() {
                let mut refreshed = existing.clone();
                refreshed.stale_since = None;
                plan.unstale.push(refreshed);
            }
            continue;
        }

        let base = if host.name.trim().is_empty() {
            host.external_id.clone()
        } else {
            host.name.clone()
        };
        let name = unique_name(&base, source_name, &used_names);
        let mut planned = host.clone();
        if name != base {
            plan.renamed.push((base.clone(), name.clone()));
            planned.name = name.clone();
        }

        let probe = Connection::new(
            name.clone(),
            host.host.clone(),
            host.user.clone().unwrap_or_else(|| "inventory".to_string()),
            host.port.unwrap_or(22),
            None,
            None,
            false,
            host.key_path.clone(),
        );
        if let Err(reason) = probe.validate() {
            plan.invalid.push((name, reason));
            continue;
        }
        used_names.insert(name);

        plan.add.push(planned);
    }

    for existing in existing_for_source {
        if existing
            .external_id
            .as_deref()
            .map(|id| !matched.contains(id))
            .unwrap_or(false)
            && existing.stale_since.is_none()
        {
            let mut stale = existing.clone();
            stale.stale_since = Some(now);
            plan.mark_stale.push(stale);
        }
    }

    plan
}

fn unique_name(base: &str, source_name: &str, used: &HashSet<String>) -> String {
    if !used.contains(base) {
        return base.to_string();
    }
    let scoped = format!("{}@{}", base, source_name);
    if !used.contains(&scoped) {
        return scoped;
    }
    let mut suffix = 2;
    loop {
        let candidate = format!("{}-{}", scoped, suffix);
        if !used.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Write a plan to the database in one transaction. A provider error is
/// recorded separately with [`Database::record_sync_result`].
pub fn apply(
    db: &Database,
    config: &AppConfig,
    source: &Source,
    plan: SyncPlan,
    prune_after_days: Option<u32>,
) -> Result<SyncReport> {
    let mut report = plan.report(false);
    let now = Utc::now();
    let bastion = config_str(&source.config, "bastion");
    let bastion_user = config_str(&source.config, "bastion_user");
    let source_user =
        config_str(&source.config, "user").or_else(|| Some(config.default_user.clone()));

    let tx = db.conn.unchecked_transaction()?;

    for host in &plan.add {
        let mut connection = Connection::new(
            host.name.clone(),
            host.host.clone(),
            host.user
                .clone()
                .or_else(|| source_user.clone())
                .unwrap_or_else(|| config.default_user.clone()),
            host.port.unwrap_or(config.default_port),
            host.bastion.clone().or_else(|| bastion.clone()),
            host.bastion_user.clone().or_else(|| bastion_user.clone()),
            host.use_kerberos,
            host.key_path.clone(),
        );
        connection.source_id = Some(source.id);
        connection.external_id = Some(host.external_id.clone());
        connection.tags = normalized_tags(&host.tags);
        insert_connection(&tx, &connection)?;
    }

    for (existing, host) in &plan.update {
        let mut updated = existing.clone();
        updated.host = host.host.clone();
        if let Some(port) = host.port {
            updated.port = port;
        }
        if let Some(user) = &host.user {
            updated.user = user.clone();
        }
        if let Some(key) = &host.key_path {
            updated.key_path = Some(key.clone());
        }
        for tag in &host.tags {
            if let Some(tag) = Connection::normalize_tag(tag) {
                if !updated.tags.contains(&tag) {
                    updated.tags.push(tag);
                }
            }
        }
        if source.kind == SourceKind::Team {
            // A team vault is authoritative for routing: a removed bastion or
            // a Kerberos change must propagate, not just additions. Any route
            // change waits for the user's confirmation before the next
            // connect, and drops agent forwarding (a remote edit must never
            // re-enable it).
            updated.bastion = host.bastion.clone();
            updated.bastion_user = host.bastion_user.clone();
            updated.use_kerberos = host.use_kerberos;
            let route_changed = updated.host != existing.host
                || updated.port != existing.port
                || updated.user != existing.user
                || updated.bastion != existing.bastion
                || updated.bastion_user != existing.bastion_user
                || updated.use_kerberos != existing.use_kerberos;
            if route_changed {
                // Successive edits before a confirmation keep the last confirmed route
                // as "before", so a second edit can't launder the first one.
                updated.pending_change = Some(match &existing.pending_change {
                    Some(earlier) => earlier.clone(),
                    None => crate::models::PendingChange::detect(existing, now),
                });
                updated.forward_agent = false;
            }
            // Back on the confirmed route: nothing left to confirm.
            if updated
                .pending_change
                .as_ref()
                .is_some_and(|c| c.previous == crate::models::RouteValues::of(&updated))
            {
                updated.pending_change = None;
            }
        } else if host.bastion.is_some() {
            updated.bastion = host.bastion.clone();
            updated.bastion_user = host.bastion_user.clone();
        } else if bastion.is_some() {
            updated.bastion = bastion.clone();
            updated.bastion_user = bastion_user.clone();
        }
        updated.tags = normalized_tags(&updated.tags);
        update_connection(&tx, &updated)?;
    }

    for connection in &plan.mark_stale {
        tx.execute(
            "UPDATE connections SET stale_since = ?1 WHERE id = ?2",
            params![
                connection.stale_since.map(|t| t.to_rfc3339()),
                connection.id.to_string()
            ],
        )?;
    }

    for connection in &plan.unstale {
        tx.execute(
            "UPDATE connections SET stale_since = NULL WHERE id = ?1",
            params![connection.id.to_string()],
        )?;
    }

    if let Some(days) = prune_after_days {
        let cutoff = now - Duration::days(days as i64);
        let stale_ids: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM connections
                 WHERE source_id = ?1 AND stale_since IS NOT NULL AND stale_since < ?2",
            )?;
            let rows = stmt
                .query_map(params![source.id.to_string(), cutoff.to_rfc3339()], |row| {
                    row.get(0)
                })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for id in stale_ids {
            delete_connection(&tx, &id)?;
            report.pruned += 1;
        }
    }

    tx.commit()?;
    db.record_sync_result(source.id, now, None)?;
    Ok(report)
}

fn normalized_tags(tags: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for tag in tags {
        if let Some(tag) = Connection::normalize_tag(tag) {
            if !normalized.contains(&tag) {
                normalized.push(tag);
            }
        }
    }
    normalized
}

fn insert_connection(tx: &Transaction, connection: &Connection) -> Result<()> {
    tx.execute(
        "INSERT INTO connections
         (id, name, host, user, port, bastion, bastion_user, use_kerberos, key_path, created_at,
          last_used, tags, source_id, external_id, stale_since, forward_agent, pending_change)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            connection.id.to_string(),
            connection.name,
            connection.host,
            connection.user,
            connection.port,
            connection.bastion,
            connection.bastion_user,
            connection.use_kerberos,
            connection.key_path,
            connection.created_at.to_rfc3339(),
            connection.last_used.map(|t| t.to_rfc3339()),
            serde_json::to_string(&connection.tags)?,
            connection.source_id.map(|id| id.to_string()),
            connection.external_id,
            connection.stale_since.map(|t| t.to_rfc3339()),
            connection.forward_agent,
            connection
                .pending_change
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        ],
    )?;
    replace_tags(tx, connection)?;
    Ok(())
}

fn update_connection(tx: &Transaction, connection: &Connection) -> Result<()> {
    tx.execute(
        "UPDATE connections SET host = ?1, user = ?2, port = ?3, bastion = ?4,
            bastion_user = ?5, key_path = ?6, tags = ?7, stale_since = ?8,
            use_kerberos = ?9, forward_agent = ?10, pending_change = ?11
         WHERE id = ?12",
        params![
            connection.host,
            connection.user,
            connection.port,
            connection.bastion,
            connection.bastion_user,
            connection.key_path,
            serde_json::to_string(&connection.tags)?,
            connection.stale_since.map(|t| t.to_rfc3339()),
            connection.use_kerberos,
            connection.forward_agent,
            connection
                .pending_change
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            connection.id.to_string(),
        ],
    )?;
    replace_tags(tx, connection)?;
    Ok(())
}

fn replace_tags(tx: &Transaction, connection: &Connection) -> Result<()> {
    tx.execute(
        "DELETE FROM connection_tags WHERE connection_id = ?1",
        params![connection.id.to_string()],
    )?;
    for tag in &connection.tags {
        tx.execute(
            "INSERT OR IGNORE INTO connection_tags (connection_id, tag) VALUES (?1, ?2)",
            params![connection.id.to_string(), tag],
        )?;
    }
    Ok(())
}

/// Same cleanup as `Database::remove_connection`, for pruned inventory hosts.
fn delete_connection(tx: &Transaction, id: &str) -> Result<()> {
    tx.execute(
        "UPDATE sessions SET connection_id = NULL WHERE connection_id = ?1",
        params![id],
    )?;
    tx.execute("DELETE FROM aliases WHERE connection_id = ?1", params![id])?;
    tx.execute(
        "DELETE FROM connection_tags WHERE connection_id = ?1",
        params![id],
    )?;
    tx.execute("DELETE FROM connections WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn discovered(id: &str, name: &str, host: &str, host_tag: Option<&str>) -> DiscoveredHost {
        DiscoveredHost {
            external_id: id.into(),
            name: name.into(),
            host: host.into(),
            user: Some("deploy".into()),
            port: None,
            key_path: None,
            tags: host_tag.map(|tag| vec![tag.into()]).unwrap_or_default(),
            ..Default::default()
        }
    }

    fn existing(_id: &str, name: &str, host: &str, external: &str) -> Connection {
        let mut connection = Connection::new(
            name.into(),
            host.into(),
            "deploy".into(),
            22,
            None,
            None,
            false,
            None,
        );
        connection.source_id = Some(Uuid::new_v4());
        connection.external_id = Some(external.into());
        connection
    }

    fn source(kind: SourceKind) -> Source {
        Source::new("aws-prod".into(), kind, serde_json::Value::Null)
    }

    #[test]
    fn plans_add_update_unchanged_stale_unstale_and_name_collisions() {
        let existing_rows = vec![
            existing("1", "changed", "10.0.0.1", "changed"),
            existing("2", "same", "10.0.0.2", "same"),
            existing("3", "gone", "10.0.0.3", "gone"),
            {
                let mut back = existing("4", "back", "10.0.0.4", "back");
                back.stale_since = Some(Utc::now());
                back
            },
        ];
        let discovered_hosts = vec![
            discovered("changed", "changed", "10.0.0.10", Some("new")),
            discovered("same", "same", "10.0.0.2", None),
            discovered("back", "back", "10.0.0.4", None),
            discovered("new-1", "web", "10.0.0.11", None),
            discovered("new-2", "web", "10.0.0.12", None),
            discovered("dup", "dup", "10.0.0.13", None),
            discovered("dup", "dup", "10.0.0.14", None),
        ];
        let mut taken = HashSet::new();
        taken.insert("web".to_string());

        let plan = plan(
            &discovered_hosts,
            &existing_rows,
            &taken,
            &source(SourceKind::Aws),
        );

        assert_eq!(plan.update.len(), 1);
        assert_eq!(plan.update[0].1.host, "10.0.0.10");
        assert!(plan.update[0].1.tags.contains(&"new".to_string()));

        assert_eq!(plan.mark_stale.len(), 1);
        assert_eq!(plan.mark_stale[0].name, "gone");
        assert!(plan.mark_stale[0].stale_since.is_some());

        assert_eq!(plan.unstale.len(), 1);
        assert_eq!(plan.unstale[0].name, "back");

        assert_eq!(plan.add.len(), 3);
        let added: Vec<_> = plan.add.iter().map(|host| host.name.as_str()).collect();
        assert_eq!(added, ["web@aws-prod", "web@aws-prod-2", "dup"]);
        assert_eq!(
            plan.renamed,
            vec![
                ("web".to_string(), "web@aws-prod".to_string()),
                ("web".to_string(), "web@aws-prod-2".to_string()),
            ]
        );
        assert_eq!(plan.duplicates, ["dup"]);
    }

    fn test_db() -> (tempfile::TempDir, Database, AppConfig) {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        let db = Database::new(&config).unwrap();
        (dir, db, config)
    }

    fn synced_source(kind: SourceKind, db: &Database) -> Source {
        let source = Source::new("team-alpha".into(), kind, serde_json::Value::Null);
        db.add_source(&source).unwrap();
        source
    }

    fn stored_host(db: &Database, source: &Source, forward_agent: bool) -> Connection {
        let mut conn = Connection::new(
            "db-prod".into(),
            "db.example".into(),
            "alice".into(),
            22,
            Some("bastion.example".into()),
            None,
            true,
            None,
        );
        conn.source_id = Some(source.id);
        conn.external_id = Some("entry-1".into());
        conn.forward_agent = forward_agent;
        db.add_connection(&conn).unwrap();
        conn
    }

    fn moved_host() -> Vec<DiscoveredHost> {
        vec![DiscoveredHost {
            external_id: "entry-1".into(),
            name: "db-prod".into(),
            host: "attacker.example".into(),
            user: Some("alice".into()),
            port: Some(22),
            bastion: Some("bastion.example".into()),
            use_kerberos: true,
            ..Default::default()
        }]
    }

    fn apply_move(db: &Database, config: &AppConfig, source: &Source) {
        let existing = db.connections_for_source(source.id).unwrap();
        let plan = plan(&moved_host(), &existing, &HashSet::new(), source);
        assert_eq!(plan.update.len(), 1, "the host change is an update");
        apply(db, config, source, plan, None).unwrap();
    }

    #[test]
    fn a_team_host_change_waits_for_confirmation_and_drops_agent_forwarding() {
        let (_dir, db, config) = test_db();
        let source = synced_source(SourceKind::Team, &db);
        stored_host(&db, &source, true);

        apply_move(&db, &config, &source);

        let stored = db.get_connection("db-prod").unwrap().unwrap();
        assert_eq!(stored.host, "attacker.example");
        let change = stored.pending_change.expect("pending change recorded");
        assert_eq!(change.previous.host, "db.example");
        assert!(
            !stored.forward_agent,
            "a remote edit must not keep forwarding the agent"
        );
    }

    #[test]
    fn successive_team_edits_keep_the_confirmed_route_as_before() {
        let (_dir, db, config) = test_db();
        let source = synced_source(SourceKind::Team, &db);
        stored_host(&db, &source, false);
        apply_move(&db, &config, &source);

        // A second edit before the user confirmed: "before" stays the confirmed host.
        let mut host = moved_host().remove(0);
        host.host = "attacker2.example".into();
        let existing = db.connections_for_source(source.id).unwrap();
        apply(
            &db,
            &config,
            &source,
            plan(&[host], &existing, &HashSet::new(), &source),
            None,
        )
        .unwrap();
        let stored = db.get_connection("db-prod").unwrap().unwrap();
        assert_eq!(stored.host, "attacker2.example");
        let change = stored.pending_change.expect("still pending");
        assert_eq!(change.previous.host, "db.example");

        // Reverted to the confirmed route: the pending change goes away.
        let mut host = moved_host().remove(0);
        host.host = "db.example".into();
        let existing = db.connections_for_source(source.id).unwrap();
        apply(
            &db,
            &config,
            &source,
            plan(&[host], &existing, &HashSet::new(), &source),
            None,
        )
        .unwrap();
        let stored = db.get_connection("db-prod").unwrap().unwrap();
        assert_eq!(stored.host, "db.example");
        assert!(
            stored.pending_change.is_none(),
            "{:?}",
            stored.pending_change
        );
    }

    #[test]
    fn a_team_bastion_only_change_is_also_recorded() {
        let (_dir, db, config) = test_db();
        let source = synced_source(SourceKind::Team, &db);
        stored_host(&db, &source, false);

        let mut host = moved_host().remove(0);
        host.host = "db.example".into();
        host.bastion = Some("evil-bastion.example".into());
        let plan = plan(
            &[host],
            &db.connections_for_source(source.id).unwrap(),
            &HashSet::new(),
            &source,
        );
        assert_eq!(
            plan.update.len(),
            1,
            "a bastion-only change must reach apply"
        );
        apply(&db, &config, &source, plan, None).unwrap();

        let stored = db.get_connection("db-prod").unwrap().unwrap();
        let change = stored.pending_change.expect("pending change recorded");
        assert_eq!(change.previous.bastion.as_deref(), Some("bastion.example"));
        assert_eq!(stored.bastion.as_deref(), Some("evil-bastion.example"));
    }

    #[test]
    fn a_non_team_sync_marks_nothing_and_keeps_forward_agent() {
        let (_dir, db, config) = test_db();
        let source = synced_source(SourceKind::Aws, &db);
        stored_host(&db, &source, true);

        apply_move(&db, &config, &source);

        let stored = db.get_connection("db-prod").unwrap().unwrap();
        assert_eq!(stored.host, "attacker.example");
        assert!(
            stored.pending_change.is_none(),
            "AWS IPs churn; no confirmation"
        );
        assert!(stored.forward_agent, "the local choice is preserved");
    }

    #[test]
    fn a_first_team_materialization_starts_without_forwarding_or_a_pending_change() {
        let (_dir, db, config) = test_db();
        let source = synced_source(SourceKind::Team, &db);

        let plan = plan(&moved_host(), &[], &HashSet::new(), &source);
        assert_eq!(plan.add.len(), 1);
        apply(&db, &config, &source, plan, None).unwrap();

        let stored = db.get_connection("db-prod").unwrap().unwrap();
        assert!(
            stored.pending_change.is_none(),
            "new entries need no confirmation"
        );
        assert!(!stored.forward_agent);
    }
}
