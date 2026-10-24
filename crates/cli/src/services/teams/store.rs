//! Local persistence for bssh Teams, in the existing SQLite database (schema
//! v8). Only ciphertext and wrapped keys are stored: team keys are never
//! written unwrapped, and vault entries are decrypted on read.

use super::codec::{check_incoming_keys, VaultCodec};
use crate::config::AppConfig;
use crate::database::Database;
use anyhow::{anyhow, Context, Result};
use bssh_teams_proto::api::{AuditActor, AuditEvent, AuditKind, EncryptedEntry, WrappedKey};
use bssh_teams_proto::layout::{audit_event_hash, AUDIT_GENESIS};
use bssh_teams_proto::wire::{Fixed, PublicKey32};
use chrono::{DateTime, SubsecRound, Utc};
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

/// Tables added by schema v8. Run once by `Database::migrate_v8_teams_tables`.
pub(crate) const SCHEMA_V8: &str = "
CREATE TABLE IF NOT EXISTS teams_local (
    team_id TEXT PRIMARY KEY,
    server_url TEXT NOT NULL,
    account_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    trust_root_ed25519 BLOB NOT NULL,
    last_pulled_revision INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS team_wrapped_keys (
    team_id TEXT NOT NULL,
    key_version INTEGER NOT NULL,
    wrapped_key_json TEXT NOT NULL,
    PRIMARY KEY (team_id, key_version)
);
CREATE TABLE IF NOT EXISTS vault_entries (
    team_id TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    entry_json TEXT NOT NULL,
    PRIMARY KEY (team_id, entry_id)
);
CREATE TABLE IF NOT EXISTS vault_outbox (
    team_id TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    base_revision INTEGER NOT NULL,
    sealed_entry_json TEXT NOT NULL,
    queued_at TEXT NOT NULL,
    PRIMARY KEY (team_id, entry_id)
);
CREATE TABLE IF NOT EXISTS audit_outbox (
    team_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    event_json TEXT NOT NULL,
    PRIMARY KEY (team_id, device_id, seq)
);
CREATE TABLE IF NOT EXISTS audit_chain (
    team_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    next_seq INTEGER NOT NULL,
    prev_hash BLOB NOT NULL,
    PRIMARY KEY (team_id, device_id)
);
CREATE TABLE IF NOT EXISTS device_keys (
    device_id TEXT PRIMARY KEY,
    sealed BLOB NOT NULL,
    created_at TEXT NOT NULL
);
";

/// A team this device has joined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamLocal {
    pub team_id: Uuid,
    pub server_url: String,
    pub account_id: Uuid,
    pub device_id: Uuid,
    /// Pinned Ed25519 key of the creating admin device.
    pub trust_root_ed25519: PublicKey32,
    pub last_pulled_revision: u64,
    pub created_at: DateTime<Utc>,
}

/// A queued local edit, already sealed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    pub entry: EncryptedEntry,
    /// The exact JSON stored, used to remove precisely this edit once accepted.
    pub sealed_json: String,
    pub queued_at: DateTime<Utc>,
}

pub struct TeamsStore {
    db: Database,
}

fn uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).with_context(|| format!("corrupt uuid in local teams store: {s}"))
}

fn to_i64(v: u64) -> Result<i64> {
    i64::try_from(v).map_err(|_| anyhow!("value {v} out of range"))
}

fn to_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn parse_time(s: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc))
}

fn key32(b: Vec<u8>) -> Result<PublicKey32> {
    let a: [u8; 32] = b
        .try_into()
        .map_err(|_| anyhow!("corrupt 32-byte value in local teams store"))?;
    Ok(Fixed(a))
}

impl TeamsStore {
    /// Open (and migrate, if needed) the database described by `config`.
    pub fn open(config: &AppConfig) -> Result<Self> {
        Ok(Self {
            db: Database::new(config)?,
        })
    }

    pub fn from_database(db: Database) -> Self {
        Self { db }
    }

    /// The underlying database, for the connection tables team vaults are
    /// materialized into.
    pub fn database(&self) -> &Database {
        &self.db
    }

    // -- teams ---------------------------------------------------------------

    pub fn upsert_team(&self, t: &TeamLocal) -> Result<()> {
        self.db.conn.execute(
            "INSERT INTO teams_local (team_id, server_url, account_id, device_id,
                 trust_root_ed25519, last_pulled_revision, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(team_id) DO UPDATE SET
                 server_url = excluded.server_url,
                 account_id = excluded.account_id,
                 device_id = excluded.device_id,
                 trust_root_ed25519 = excluded.trust_root_ed25519",
            params![
                t.team_id.to_string(),
                t.server_url,
                t.account_id.to_string(),
                t.device_id.to_string(),
                t.trust_root_ed25519.0.to_vec(),
                to_i64(t.last_pulled_revision)?,
                t.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_team(&self, team_id: Uuid) -> Result<Option<TeamLocal>> {
        Ok(self
            .list_teams()?
            .into_iter()
            .find(|t| t.team_id == team_id))
    }

    pub fn list_teams(&self) -> Result<Vec<TeamLocal>> {
        let mut stmt = self.db.conn.prepare(
            "SELECT team_id, server_url, account_id, device_id, trust_root_ed25519,
                    last_pulled_revision, created_at
             FROM teams_local ORDER BY created_at, team_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (team, url, acct, dev, root, rev, created) = row?;
            out.push(TeamLocal {
                team_id: uuid(&team)?,
                server_url: url,
                account_id: uuid(&acct)?,
                device_id: uuid(&dev)?,
                trust_root_ed25519: key32(root)?,
                last_pulled_revision: to_u64(rev),
                created_at: parse_time(&created)?,
            });
        }
        Ok(out)
    }

    pub fn last_pulled_revision(&self, team_id: Uuid) -> Result<u64> {
        let v: Option<i64> = self
            .db
            .conn
            .query_row(
                "SELECT last_pulled_revision FROM teams_local WHERE team_id = ?1",
                [team_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        v.map(to_u64)
            .ok_or_else(|| anyhow!("unknown team {team_id}"))
    }

    // -- keys ----------------------------------------------------------------

    /// Validate wrapped keys from the server ([`check_incoming_keys`]) and
    /// store the new ones exactly as received. Returns how many were new.
    /// `current_version` is the team's `current_key_version` if known.
    pub fn accept_wrapped_keys(
        &self,
        team_id: Uuid,
        my_device_id: Uuid,
        incoming: &[WrappedKey],
        current_version: Option<u32>,
    ) -> Result<usize> {
        let stored = self.wrapped_keys(team_id)?;
        let fresh = check_incoming_keys(team_id, my_device_id, &stored, incoming, current_version)?;
        self.save_wrapped_keys(team_id, &fresh)?;
        Ok(fresh.len())
    }

    /// Store wrapped keys exactly as received. Never pass an unwrapped key.
    /// Unchecked: network input goes through [`Self::accept_wrapped_keys`].
    pub(crate) fn save_wrapped_keys(&self, team_id: Uuid, keys: &[WrappedKey]) -> Result<()> {
        let tx = self.db.conn.unchecked_transaction()?;
        for k in keys {
            if k.team_id != team_id {
                anyhow::bail!("wrapped key belongs to another team");
            }
            tx.execute(
                "INSERT OR REPLACE INTO team_wrapped_keys (team_id, key_version, wrapped_key_json)
                 VALUES (?1, ?2, ?3)",
                params![
                    team_id.to_string(),
                    k.key_version,
                    serde_json::to_string(k)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Wrapped keys, oldest version first.
    pub fn wrapped_keys(&self, team_id: Uuid) -> Result<Vec<WrappedKey>> {
        let mut stmt = self.db.conn.prepare(
            "SELECT wrapped_key_json FROM team_wrapped_keys
             WHERE team_id = ?1 ORDER BY key_version",
        )?;
        let rows = stmt.query_map([team_id.to_string()], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    /// Store the passphrase-sealed device key blob (from `DeviceKeys::seal`).
    pub fn save_sealed_device_keys(&self, device_id: Uuid, sealed: &[u8]) -> Result<()> {
        self.db.conn.execute(
            "INSERT OR REPLACE INTO device_keys (device_id, sealed, created_at)
             VALUES (?1, ?2, ?3)",
            params![device_id.to_string(), sealed, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn sealed_device_keys(&self, device_id: Uuid) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .conn
            .query_row(
                "SELECT sealed FROM device_keys WHERE device_id = ?1",
                [device_id.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    // -- vault ---------------------------------------------------------------

    /// Apply pulled entries and advance the pull cursor, atomically. Per
    /// `entry_id` the highest revision wins. `cursor` is the highest revision
    /// the page covered, including entries the caller rejected. Returns how many entries changed
    /// the local copy.
    pub fn apply_pulled(
        &self,
        team_id: Uuid,
        entries: &[EncryptedEntry],
        cursor: u64,
    ) -> Result<usize> {
        let mut sorted: Vec<&EncryptedEntry> = entries.iter().collect();
        sorted.sort_by_key(|e| e.revision);
        let tx = self.db.conn.unchecked_transaction()?;
        let mut applied = 0;
        let mut max_rev = cursor;
        for e in sorted {
            applied += upsert_entry(&tx, team_id, e)?;
            max_rev = max_rev.max(e.revision);
        }
        tx.execute(
            "UPDATE teams_local SET last_pulled_revision = MAX(last_pulled_revision, ?2)
             WHERE team_id = ?1",
            params![team_id.to_string(), to_i64(max_rev)?],
        )?;
        tx.commit()?;
        Ok(applied)
    }

    /// Record an entry the server accepted, so local reads see its revision
    /// before the next pull.
    pub fn record_accepted(
        &self,
        team_id: Uuid,
        entry: &EncryptedEntry,
        revision: u64,
    ) -> Result<()> {
        let mut e = entry.clone();
        e.revision = revision;
        upsert_entry(&self.db.conn, team_id, &e)?;
        Ok(())
    }

    pub fn entry(&self, team_id: Uuid, entry_id: Uuid) -> Result<Option<EncryptedEntry>> {
        let json: Option<String> = self
            .db
            .conn
            .query_row(
                "SELECT entry_json FROM vault_entries WHERE team_id = ?1 AND entry_id = ?2",
                params![team_id.to_string(), entry_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        json.map(|j| serde_json::from_str(&j).map_err(Into::into))
            .transpose()
    }

    /// Latest encrypted entry per `entry_id`, in revision order.
    pub fn entries(&self, team_id: Uuid) -> Result<Vec<EncryptedEntry>> {
        let mut stmt = self
            .db
            .conn
            .prepare("SELECT entry_json FROM vault_entries WHERE team_id = ?1 ORDER BY revision")?;
        let rows = stmt.query_map([team_id.to_string()], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    // -- outbox --------------------------------------------------------------

    /// Queue a sealed edit. A newer edit to the same entry replaces the older one.
    pub fn queue_outbox(&self, team_id: Uuid, entry: &EncryptedEntry) -> Result<()> {
        self.db.conn.execute(
            "INSERT OR REPLACE INTO vault_outbox
                 (team_id, entry_id, base_revision, sealed_entry_json, queued_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                team_id.to_string(),
                entry.entry_id.to_string(),
                to_i64(entry.base_revision)?,
                serde_json::to_string(entry)?,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Pending edits, oldest first.
    pub fn outbox(&self, team_id: Uuid) -> Result<Vec<OutboxRow>> {
        let mut stmt = self.db.conn.prepare(
            "SELECT sealed_entry_json, queued_at FROM vault_outbox
             WHERE team_id = ?1 ORDER BY queued_at, rowid",
        )?;
        let rows = stmt.query_map([team_id.to_string()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (json, queued) = row?;
            out.push(OutboxRow {
                entry: serde_json::from_str(&json)?,
                sealed_json: json,
                queued_at: parse_time(&queued)?,
            });
        }
        Ok(out)
    }

    /// Remove an outbox row, but only if it still holds exactly `sealed_json`
    /// (a newer edit queued meanwhile stays). Returns whether a row was removed.
    pub fn remove_outbox_exact(
        &self,
        team_id: Uuid,
        entry_id: Uuid,
        sealed_json: &str,
    ) -> Result<bool> {
        let n = self.db.conn.execute(
            "DELETE FROM vault_outbox
             WHERE team_id = ?1 AND entry_id = ?2 AND sealed_entry_json = ?3",
            params![team_id.to_string(), entry_id.to_string(), sealed_json],
        )?;
        Ok(n > 0)
    }

    /// Replace a pending edit with a re-sealed one, only if `old_json` is
    /// still what is queued.
    pub fn replace_outbox_exact(
        &self,
        team_id: Uuid,
        old_json: &str,
        new_entry: &EncryptedEntry,
    ) -> Result<bool> {
        let n = self.db.conn.execute(
            "UPDATE vault_outbox SET base_revision = ?4, sealed_entry_json = ?5
             WHERE team_id = ?1 AND entry_id = ?2 AND sealed_entry_json = ?3",
            params![
                team_id.to_string(),
                new_entry.entry_id.to_string(),
                old_json,
                to_i64(new_entry.base_revision)?,
                serde_json::to_string(new_entry)?,
            ],
        )?;
        Ok(n > 0)
    }

    // -- audit ---------------------------------------------------------------

    /// Write the next event of this device's hash chain locally, signed via
    /// `codec`, and queue it for upload. Chain head and queue move together.
    pub fn append_audit_event(
        &self,
        team_id: Uuid,
        codec: &dyn VaultCodec,
        kind: AuditKind,
        entry_id: Option<Uuid>,
    ) -> Result<AuditEvent> {
        self.append_audit_event_at(team_id, codec, kind, entry_id, Utc::now())
    }

    /// [`Self::append_audit_event`] for something that happened at
    /// `occurred_at`, e.g. a session recorded while the device keys were
    /// locked and signed at the next sync.
    pub fn append_audit_event_at(
        &self,
        team_id: Uuid,
        codec: &dyn VaultCodec,
        kind: AuditKind,
        entry_id: Option<Uuid>,
        occurred_at: DateTime<Utc>,
    ) -> Result<AuditEvent> {
        self.append_audit_event_full(team_id, codec, kind, entry_id, occurred_at, None, None)
    }

    /// [`Self::append_audit_event_at`] for a bssh AI event: `actor` is signed
    /// with the event, `detail` (plaintext) is encrypted to the team audit key
    /// before signing, so the server never sees it.
    #[allow(clippy::too_many_arguments)]
    pub fn append_audit_event_full(
        &self,
        team_id: Uuid,
        codec: &dyn VaultCodec,
        kind: AuditKind,
        entry_id: Option<Uuid>,
        occurred_at: DateTime<Utc>,
        actor: Option<AuditActor>,
        detail: Option<&[u8]>,
    ) -> Result<AuditEvent> {
        let device_id = codec.device_id();
        let tx = self.db.conn.unchecked_transaction()?;
        let head: Option<(i64, Vec<u8>)> = tx
            .query_row(
                "SELECT next_seq, prev_hash FROM audit_chain WHERE team_id = ?1 AND device_id = ?2",
                params![team_id.to_string(), device_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (seq, prev_hash) = match head {
            Some((s, h)) => (to_u64(s), key32(h)?),
            None => (0, AUDIT_GENESIS),
        };
        let mut event = AuditEvent {
            event_id: Uuid::new_v4(),
            team_id,
            device_id,
            seq,
            kind,
            entry_id,
            // Only millisecond precision is signed; truncate so the stored
            // event matches what the server verifies.
            occurred_at: occurred_at.trunc_subsecs(3),
            prev_hash,
            signature: Fixed::ZERO,
            actor,
            detail: None,
        };
        if let Some(plain) = detail {
            event.detail = Some(codec.seal_audit_detail(&event, plain)?);
        }
        codec.sign_audit(&mut event);
        let hash = audit_event_hash(&event);
        tx.execute(
            "INSERT INTO audit_outbox (team_id, device_id, seq, event_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                team_id.to_string(),
                device_id.to_string(),
                to_i64(seq)?,
                serde_json::to_string(&event)?
            ],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO audit_chain (team_id, device_id, next_seq, prev_hash)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                team_id.to_string(),
                device_id.to_string(),
                to_i64(seq + 1)?,
                hash.0.to_vec()
            ],
        )?;
        tx.commit()?;
        Ok(event)
    }

    /// Events not yet acknowledged by the server, in `seq` order.
    pub fn pending_audit(
        &self,
        team_id: Uuid,
        device_id: Uuid,
        limit: usize,
    ) -> Result<Vec<AuditEvent>> {
        let mut stmt = self.db.conn.prepare(
            "SELECT event_json FROM audit_outbox
             WHERE team_id = ?1 AND device_id = ?2 ORDER BY seq LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![team_id.to_string(), device_id.to_string(), limit as i64],
            |r| r.get::<_, String>(0),
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    pub fn remove_audit(&self, team_id: Uuid, device_id: Uuid, seqs: &[u64]) -> Result<()> {
        let tx = self.db.conn.unchecked_transaction()?;
        for s in seqs {
            tx.execute(
                "DELETE FROM audit_outbox WHERE team_id = ?1 AND device_id = ?2 AND seq = ?3",
                params![team_id.to_string(), device_id.to_string(), to_i64(*s)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// Insert or raise an entry; returns 1 if the stored row changed.
fn upsert_entry(conn: &rusqlite::Connection, team_id: Uuid, e: &EncryptedEntry) -> Result<usize> {
    Ok(conn.execute(
        "INSERT INTO vault_entries (team_id, entry_id, revision, entry_json)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(team_id, entry_id) DO UPDATE SET
             revision = excluded.revision, entry_json = excluded.entry_json
         WHERE excluded.revision > vault_entries.revision",
        params![
            team_id.to_string(),
            e.entry_id.to_string(),
            to_i64(e.revision)?,
            serde_json::to_string(e)?
        ],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bssh_teams_proto::api::AuditKind;
    use bssh_teams_proto::wire::Bytes;

    fn store() -> (tempfile::TempDir, TeamsStore, Uuid) {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("t.db"),
            ..Default::default()
        };
        let store = TeamsStore::open(&config).unwrap();
        let team = Uuid::new_v4();
        store
            .upsert_team(&TeamLocal {
                team_id: team,
                server_url: "https://teams.example.com".into(),
                account_id: Uuid::new_v4(),
                device_id: Uuid::new_v4(),
                trust_root_ed25519: Fixed([3u8; 32]),
                last_pulled_revision: 0,
                created_at: Utc::now(),
            })
            .unwrap();
        (dir, store, team)
    }

    fn entry(id: Uuid, revision: u64, base: u64, ct: u8) -> EncryptedEntry {
        EncryptedEntry {
            entry_id: id,
            revision,
            base_revision: base,
            key_version: 1,
            author_device_id: Uuid::nil(),
            nonce: Fixed([ct; 12]),
            ciphertext: Bytes(vec![ct; 8]),
            signature: Fixed([ct; 64]),
        }
    }

    #[test]
    fn team_round_trips_and_wrapped_keys_are_stored_verbatim() {
        let (_d, store, team) = store();
        let t = store.get_team(team).unwrap().unwrap();
        assert_eq!(t.trust_root_ed25519, Fixed([3u8; 32]));
        assert_eq!(t.server_url, "https://teams.example.com");

        let wk = WrappedKey {
            team_id: team,
            key_version: 1,
            recipient_device_id: Uuid::new_v4(),
            wrapping_device_id: Uuid::new_v4(),
            ephemeral_public: Fixed([1u8; 32]),
            nonce: Fixed([2u8; 12]),
            ciphertext: Bytes(vec![7; 48]),
            signature: Fixed([4u8; 64]),
        };
        store
            .save_wrapped_keys(team, std::slice::from_ref(&wk))
            .unwrap();
        assert_eq!(store.wrapped_keys(team).unwrap(), vec![wk.clone()]);

        let mut foreign = wk;
        foreign.team_id = Uuid::new_v4();
        assert!(store.save_wrapped_keys(team, &[foreign]).is_err());
    }

    #[test]
    fn highest_revision_wins_and_cursor_advances() {
        let (_d, store, team) = store();
        let id = Uuid::new_v4();
        let n = store
            .apply_pulled(team, &[entry(id, 5, 4, 5), entry(id, 3, 0, 3)], 5)
            .unwrap();
        assert_eq!(n, 2, "sorted ascending: both applied in turn");
        assert_eq!(store.entry(team, id).unwrap().unwrap().revision, 5);
        assert_eq!(store.last_pulled_revision(team).unwrap(), 5);
        // An older copy never overwrites a newer one.
        assert_eq!(
            store.apply_pulled(team, &[entry(id, 4, 3, 4)], 4).unwrap(),
            0
        );
        assert_eq!(store.entry(team, id).unwrap().unwrap().revision, 5);
        assert_eq!(store.last_pulled_revision(team).unwrap(), 5);
    }

    #[test]
    fn outbox_keeps_one_row_per_entry_and_removes_exactly() {
        let (_d, store, team) = store();
        let id = Uuid::new_v4();
        store.queue_outbox(team, &entry(id, 0, 0, 1)).unwrap();
        store.queue_outbox(team, &entry(id, 0, 2, 2)).unwrap();
        let rows = store.outbox(team).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].entry.base_revision, 2);
        // A stale snapshot does not remove a newer edit.
        let stale = serde_json::to_string(&entry(id, 0, 0, 1)).unwrap();
        assert!(!store.remove_outbox_exact(team, id, &stale).unwrap());
        assert!(store
            .remove_outbox_exact(team, id, &rows[0].sealed_json)
            .unwrap());
        assert!(store.outbox(team).unwrap().is_empty());
    }

    #[test]
    fn audit_chain_survives_reopen() {
        use crate::services::teams::sync::testutil::FakeCodec;
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("t.db"),
            ..Default::default()
        };
        let team = Uuid::new_v4();
        let codec = FakeCodec::new(Uuid::new_v4());
        {
            let s = TeamsStore::open(&config).unwrap();
            s.append_audit_event(team, &codec, AuditKind::SessionStart, None)
                .unwrap();
        }
        let s = TeamsStore::open(&config).unwrap();
        let e = s
            .append_audit_event(team, &codec, AuditKind::SessionEnd, None)
            .unwrap();
        assert_eq!(e.seq, 1);
        assert_ne!(e.prev_hash, AUDIT_GENESIS);
    }

    #[test]
    fn accept_wrapped_keys_enforces_checks_and_never_overwrites() {
        use crate::services::teams::codec::KeyCheckError;
        let (_d, store, team) = store();
        let me = Uuid::new_v4();
        let mk = |v: u32, tag: u8, dev: Uuid| WrappedKey {
            team_id: team,
            key_version: v,
            recipient_device_id: dev,
            wrapping_device_id: Uuid::nil(),
            ephemeral_public: Fixed([tag; 32]),
            nonce: Fixed([tag; 12]),
            ciphertext: Bytes(vec![tag; 48]),
            signature: Fixed([tag; 64]),
        };
        assert_eq!(
            store
                .accept_wrapped_keys(team, me, &[mk(1, 1, me), mk(2, 2, me)], Some(2))
                .unwrap(),
            2
        );
        let kind = |r: Result<usize>| r.unwrap_err().downcast::<KeyCheckError>().unwrap();
        assert_eq!(
            kind(store.accept_wrapped_keys(team, me, &[mk(2, 9, me)], None)),
            KeyCheckError::ConflictsWithStored(2)
        );
        assert_eq!(
            kind(store.accept_wrapped_keys(team, me, &[mk(3, 3, Uuid::new_v4())], None)),
            KeyCheckError::WrongRecipient(3)
        );
        // Re-delivery is idempotent; the stored wraps are unchanged.
        assert_eq!(
            store
                .accept_wrapped_keys(team, me, &[mk(1, 1, me)], None)
                .unwrap(),
            0
        );
        assert_eq!(store.wrapped_keys(team).unwrap().len(), 2);
    }
}
