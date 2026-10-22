//! The sync engine (PROTOCOL.md, "Sync"; PERIMETER.md R1-R3).
//!
//! Local-first: callers read and write the local store and never wait on this.
//! `sync()` is best-effort and never loses a queued edit: outbox rows are
//! removed only after the server accepted them, and every failure is reported
//! in the [`SyncReport`] instead of aborting the run.
//!
//! Order of a run: pull (cursor-based, until `has_more` is false), push the
//! outbox (resolving conflicts by re-sealing on top of the new current
//! revision), then upload queued audit events in `seq` order.

use super::client::ClientResult;
use super::codec::VaultCodec;
use super::store::{OutboxRow, TeamsStore};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use bssh_teams_proto::api::{
    EncryptedEntry, IngestAuditRequest, IngestAuditResponse, PullResponse, PushRequest,
    PushResponse, VaultPayload,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Entries requested per pull page (server default is 500, max 1000).
pub const DEFAULT_PULL_PAGE: u32 = 500;
/// Audit events uploaded per request.
const AUDIT_BATCH: usize = 100;
/// Push attempts per sync run, so a pathological server can't loop us forever.
const MAX_PUSH_ROUNDS: usize = 5;

/// What the engine needs from the server. Implemented by
/// [`TeamsClient`](super::client::TeamsClient); tests use an in-memory fake.
#[allow(clippy::double_must_use)]
#[async_trait(?Send)]
pub trait TeamsTransport {
    async fn pull(&self, team_id: Uuid, since: u64, limit: u32) -> ClientResult<PullResponse>;
    async fn push(&self, team_id: Uuid, req: PushRequest) -> ClientResult<PushResponse>;
    async fn ingest_audit(
        &self,
        team_id: Uuid,
        req: IngestAuditRequest,
    ) -> ClientResult<IngestAuditResponse>;
}

/// A local edit that was discarded because the entry was deleted remotely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedEdit {
    pub entry_id: Uuid,
    pub reason: String,
}

/// A pulled entry that failed the integrity checks and was not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedEntry {
    pub entry_id: Uuid,
    pub revision: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Entries newly applied to the local copy from the server.
    pub pulled: usize,
    /// Local edits the server accepted.
    pub pushed: usize,
    /// Conflicts resolved by re-sealing the local edit on the new revision.
    pub conflicts_resolved: usize,
    /// Local edits dropped because the entry was tombstoned remotely. Tell the user.
    pub dropped_edits: Vec<DroppedEdit>,
    /// Audit events the server acknowledged (stored or already stored).
    pub audit_uploaded: usize,
    /// The server refused pushes: a member was removed and an admin must post
    /// a new key version first. Edits stay queued.
    pub rotation_required: bool,
    /// Pulled entries that broke the revision chain or came from an unknown
    /// author: a sign of a misbehaving or malicious server. Not applied.
    /// Reported once; the pull cursor moves past them.
    pub rejected: Vec<RejectedEntry>,
    pub errors: Vec<String>,
}

impl SyncReport {
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty() && !self.rotation_required && self.rejected.is_empty()
    }
}

pub struct SyncEngine<'a, T: TeamsTransport + ?Sized, C: VaultCodec + ?Sized> {
    store: &'a TeamsStore,
    transport: &'a T,
    codec: &'a C,
    team_id: Uuid,
    page_size: u32,
}

impl<'a, T: TeamsTransport + ?Sized, C: VaultCodec + ?Sized> SyncEngine<'a, T, C> {
    pub fn new(store: &'a TeamsStore, transport: &'a T, codec: &'a C, team_id: Uuid) -> Self {
        Self {
            store,
            transport,
            codec,
            team_id,
            page_size: DEFAULT_PULL_PAGE,
        }
    }

    pub fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = page_size.max(1);
        self
    }

    /// Run one sync. Never fails: problems are listed in the report.
    pub async fn sync(&self) -> SyncReport {
        let mut report = SyncReport::default();
        match self.pull(&mut report).await {
            Ok(()) => {
                if let Err(e) = self.push_outbox(&mut report).await {
                    report.errors.push(format!("push: {e:#}"));
                }
            }
            // Without a fresh pull the push could only conflict; try later.
            Err(e) => report.errors.push(format!("pull: {e:#}")),
        }
        if let Err(e) = self.upload_audit(&mut report).await {
            report.errors.push(format!("audit: {e:#}"));
        }
        report
    }

    /// Pull until `has_more` is false and apply entries in revision order.
    async fn pull(&self, report: &mut SyncReport) -> Result<()> {
        loop {
            let since = self.store.last_pulled_revision(self.team_id)?;
            let page = self
                .transport
                .pull(self.team_id, since, self.page_size)
                .await?;
            if page.entries.iter().any(|e| e.revision <= since) {
                return Err(anyhow!("server returned an entry at or below the cursor"));
            }
            if page.entries.is_empty() {
                if page.has_more {
                    return Err(anyhow!("server reported more entries but sent none"));
                }
                return Ok(());
            }
            let cursor = page
                .entries
                .iter()
                .map(|e| e.revision)
                .max()
                .unwrap_or(since);
            let vetted = self.vet_page(&page.entries, report)?;
            report.pulled += self.store.apply_pulled(self.team_id, &vetted, cursor)?;
            if !page.has_more {
                return Ok(());
            }
        }
    }

    /// Integrity checks on pulled entries. The server-assigned `revision` is
    /// not signed but `base_revision` is, and the pull is append-only, so per
    /// entry each pulled edit must be based on exactly the revision we hold
    /// (0 if new) and carry a higher revision. Anything else is a relabeled or
    /// skipped revision. Authors must be known devices of the team.
    fn vet_page(
        &self,
        entries: &[EncryptedEntry],
        report: &mut SyncReport,
    ) -> Result<Vec<EncryptedEntry>> {
        let mut sorted: Vec<&EncryptedEntry> = entries.iter().collect();
        sorted.sort_by_key(|e| e.revision);
        let mut held: HashMap<Uuid, u64> = HashMap::new();
        let mut ok = Vec::new();
        for e in sorted {
            let stored = self.store.entry(self.team_id, e.entry_id)?;
            let have = match held.get(&e.entry_id) {
                Some(h) => *h,
                None => stored.as_ref().map_or(0, |s| s.revision),
            };
            let reason = if !self.codec.knows_author(e.author_device_id) {
                Some(format!("unknown author device {}", e.author_device_id))
            } else if have > 0 && e.revision == have {
                // Re-delivery of what we hold (e.g. our own accepted push).
                match &stored {
                    Some(s) if same_content(s, e) => continue,
                    _ => Some(format!("revision {have} redefined with different content")),
                }
            } else if e.revision <= have {
                Some(format!(
                    "revision {} does not exceed held revision {have}",
                    e.revision
                ))
            } else if e.base_revision != have {
                Some(format!(
                    "base revision {} does not match held revision {have}",
                    e.base_revision
                ))
            } else {
                None
            };
            match reason {
                None => {
                    held.insert(e.entry_id, e.revision);
                    ok.push(e.clone());
                }
                Some(reason) => report.rejected.push(RejectedEntry {
                    entry_id: e.entry_id,
                    revision: e.revision,
                    reason,
                }),
            }
        }
        Ok(ok)
    }

    async fn push_outbox(&self, report: &mut SyncReport) -> Result<()> {
        for _round in 0..MAX_PUSH_ROUNDS {
            let rows = self.store.outbox(self.team_id)?;
            if rows.is_empty() {
                return Ok(());
            }
            let req = PushRequest {
                entries: rows.iter().map(|r| r.entry.clone()).collect(),
            };
            let resp = match self.transport.push(self.team_id, req).await {
                Ok(r) => r,
                Err(e) if e.is_rotation_required() => {
                    report.rotation_required = true;
                    report.errors.push(
                        "push refused: the team key must be rotated by an admin; \
                         edits stay queued"
                            .into(),
                    );
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            };

            let mut progress = false;
            for acc in &resp.accepted {
                let Some(row) = rows.iter().find(|r| r.entry.entry_id == acc.entry_id) else {
                    continue;
                };
                self.store
                    .record_accepted(self.team_id, &row.entry, acc.revision)?;
                if self.store.remove_outbox_exact(
                    self.team_id,
                    row.entry.entry_id,
                    &row.sealed_json,
                )? {
                    report.pushed += 1;
                    progress = true;
                }
            }

            if resp.conflicts.is_empty() {
                if !progress {
                    return Err(anyhow!("server accepted none of the queued edits"));
                }
                continue;
            }

            self.pull(report).await?;
            for c in &resp.conflicts {
                let Some(row) = rows.iter().find(|r| r.entry.entry_id == c.entry_id) else {
                    continue;
                };
                if let Err(e) = self.resolve_conflict(row, c.current_revision, report) {
                    report
                        .errors
                        .push(format!("entry {}: {e:#}; edit stays queued", c.entry_id));
                }
            }
        }
        report.errors.push(format!(
            "gave up after {MAX_PUSH_ROUNDS} push rounds; remaining edits stay queued"
        ));
        Ok(())
    }

    /// The server's current revision of `row`'s entry differs from the base
    /// the edit was made on. The local copy has been pulled; decide what to do.
    fn resolve_conflict(
        &self,
        row: &OutboxRow,
        server_revision: u64,
        report: &mut SyncReport,
    ) -> Result<()> {
        let id = row.entry.entry_id;
        let current = self
            .store
            .entry(self.team_id, id)?
            .ok_or_else(|| anyhow!("conflict on an entry the server did not return"))?;
        if current.revision < server_revision {
            return Err(anyhow!(
                "pull did not reach revision {server_revision} (have {})",
                current.revision
            ));
        }

        // A lost response: the server already stored exactly this edit.
        if same_content(&current, &row.entry) {
            self.store
                .remove_outbox_exact(self.team_id, id, &row.sealed_json)?;
            report.pushed += 1;
            return Ok(());
        }

        let remote = self.codec.open(&current)?;
        let ours = self.codec.open(&row.entry)?;
        if matches!(remote, VaultPayload::Tombstone) {
            self.store
                .remove_outbox_exact(self.team_id, id, &row.sealed_json)?;
            // Deleting something already deleted loses nothing worth mentioning.
            if !matches!(ours, VaultPayload::Tombstone) {
                report.dropped_edits.push(DroppedEdit {
                    entry_id: id,
                    reason: "the entry was deleted by another member".into(),
                });
            }
            return Ok(());
        }

        // Last push wins: re-seal on the new current revision. The overwritten
        // version stays in the server's history.
        let resealed = self.codec.seal(id, current.revision, &ours)?;
        if self
            .store
            .replace_outbox_exact(self.team_id, &row.sealed_json, &resealed)?
        {
            report.conflicts_resolved += 1;
        }
        Ok(())
    }

    /// Upload queued audit events in `seq` order. Duplicates count as success.
    async fn upload_audit(&self, report: &mut SyncReport) -> Result<()> {
        let device_id = self.codec.device_id();
        loop {
            let batch = self
                .store
                .pending_audit(self.team_id, device_id, AUDIT_BATCH)?;
            if batch.is_empty() {
                return Ok(());
            }
            let resp = self
                .transport
                .ingest_audit(
                    self.team_id,
                    IngestAuditRequest {
                        events: batch.clone(),
                    },
                )
                .await?;
            let acked: HashSet<Uuid> = resp
                .accepted
                .iter()
                .chain(resp.duplicates.iter())
                .copied()
                .collect();
            let seqs: Vec<u64> = batch
                .iter()
                .take_while(|e| acked.contains(&e.event_id))
                .map(|e| e.seq)
                .collect();
            self.store.remove_audit(self.team_id, device_id, &seqs)?;
            report.audit_uploaded += seqs.len();
            if seqs.len() < batch.len() {
                return Err(anyhow!(
                    "server acknowledged {} of {} audit events",
                    seqs.len(),
                    batch.len()
                ));
            }
        }
    }
}

/// Same sealed edit, ignoring the server-assigned revision.
fn same_content(a: &EncryptedEntry, b: &EncryptedEntry) -> bool {
    a.entry_id == b.entry_id
        && a.author_device_id == b.author_device_id
        && a.key_version == b.key_version
        && a.nonce == b.nonce
        && a.ciphertext == b.ciphertext
        && a.signature == b.signature
}

#[cfg(test)]
pub(crate) mod testutil {
    //! A fake codec (JSON + fake signature) and an in-memory server.
    use super::*;
    use crate::config::AppConfig;
    use crate::services::teams::client::TeamsError;
    use crate::services::teams::store::TeamLocal;
    use bssh_teams_proto::api::*;
    use bssh_teams_proto::wire::{Bytes, Fixed};
    use chrono::Utc;
    use std::cell::{Cell, RefCell};

    pub struct FakeCodec {
        pub device: Uuid,
        pub key_version: Cell<u32>,
        /// `None`: every author is known.
        pub known: RefCell<Option<HashSet<Uuid>>>,
        counter: Cell<u8>,
    }

    impl FakeCodec {
        pub fn new(device: Uuid) -> Self {
            Self {
                device,
                key_version: Cell::new(1),
                known: RefCell::new(None),
                counter: Cell::new(0),
            }
        }
    }

    impl VaultCodec for FakeCodec {
        fn seal(
            &self,
            entry_id: Uuid,
            base_revision: u64,
            payload: &VaultPayload,
        ) -> Result<EncryptedEntry> {
            self.counter.set(self.counter.get().wrapping_add(1));
            let mut nonce = [0u8; 12];
            nonce[0] = self.counter.get();
            nonce[1..9].copy_from_slice(&base_revision.to_be_bytes());
            let mut sig = [0u8; 64];
            sig[..16].copy_from_slice(self.device.as_bytes());
            Ok(EncryptedEntry {
                entry_id,
                revision: 0,
                base_revision,
                key_version: self.key_version.get(),
                author_device_id: self.device,
                nonce: Fixed(nonce),
                ciphertext: Bytes(serde_json::to_vec(payload)?),
                signature: Fixed(sig),
            })
        }

        fn open(&self, entry: &EncryptedEntry) -> Result<VaultPayload> {
            if entry.signature.0[..16] != *entry.author_device_id.as_bytes() {
                return Err(anyhow!("fake signature mismatch"));
            }
            Ok(serde_json::from_slice(&entry.ciphertext.0)?)
        }

        fn key_version(&self) -> u32 {
            self.key_version.get()
        }

        fn device_id(&self) -> Uuid {
            self.device
        }

        fn sign_audit(&self, event: &mut AuditEvent) {
            event.signature = Fixed([event.seq as u8 + 1; 64]);
        }

        fn knows_author(&self, device_id: Uuid) -> bool {
            self.known
                .borrow()
                .as_ref()
                .is_none_or(|k| k.contains(&device_id))
        }
    }

    #[derive(Default)]
    pub struct ServerState {
        pub history: Vec<EncryptedEntry>,
        pub rotation_required: bool,
        pub audit_seen: HashSet<Uuid>,
        pub audit_stored: Vec<AuditEvent>,
        pub next_audit_seq: u64,
    }

    impl ServerState {
        pub fn current(&self, id: Uuid) -> u64 {
            self.history
                .iter()
                .filter(|e| e.entry_id == id)
                .map(|e| e.revision)
                .max()
                .unwrap_or(0)
        }

        /// Store with an arbitrary revision label (a lying server).
        pub fn put_at(&mut self, mut e: EncryptedEntry, revision: u64) {
            e.revision = revision;
            self.history.push(e);
        }

        pub fn put(&mut self, mut e: EncryptedEntry) -> u64 {
            let rev = self.history.len() as u64 + 1;
            e.revision = rev;
            self.history.push(e);
            rev
        }
    }

    pub type Hook = Box<dyn FnOnce(&mut ServerState)>;

    #[derive(Default)]
    pub struct FakeServer {
        pub state: RefCell<ServerState>,
        pub pull_calls: Cell<usize>,
        pub push_calls: Cell<usize>,
        /// Fail this many push calls with a network error before storing anything.
        pub fail_pushes: Cell<u32>,
        /// Store the push but answer with a network error (lost response).
        pub lose_next_push_response: Cell<bool>,
        pub before_push: RefCell<Option<Hook>>,
    }

    #[async_trait(?Send)]
    impl TeamsTransport for FakeServer {
        async fn pull(&self, _team: Uuid, since: u64, limit: u32) -> ClientResult<PullResponse> {
            self.pull_calls.set(self.pull_calls.get() + 1);
            let st = self.state.borrow();
            let newer: Vec<_> = st
                .history
                .iter()
                .filter(|e| e.revision > since)
                .cloned()
                .collect();
            let has_more = newer.len() > limit as usize;
            Ok(PullResponse {
                entries: newer.into_iter().take(limit as usize).collect(),
                latest_revision: st.history.len() as u64,
                has_more,
            })
        }

        async fn push(&self, _team: Uuid, req: PushRequest) -> ClientResult<PushResponse> {
            self.push_calls.set(self.push_calls.get() + 1);
            if let Some(h) = self.before_push.borrow_mut().take() {
                h(&mut self.state.borrow_mut());
            }
            if self.fail_pushes.get() > 0 {
                self.fail_pushes.set(self.fail_pushes.get() - 1);
                return Err(TeamsError::Network("connection reset".into()));
            }
            let mut st = self.state.borrow_mut();
            if st.rotation_required {
                return Err(TeamsError::Api {
                    status: 409,
                    error: ApiError {
                        code: ErrorCode::RotationRequired,
                        message: "rotate".into(),
                    },
                });
            }
            let mut resp = PushResponse {
                accepted: vec![],
                conflicts: vec![],
            };
            for e in req.entries {
                let cur = st.current(e.entry_id);
                if e.base_revision == cur {
                    let id = e.entry_id;
                    let revision = st.put(e);
                    resp.accepted.push(Accepted {
                        entry_id: id,
                        revision,
                    });
                } else {
                    resp.conflicts.push(PushConflict {
                        entry_id: e.entry_id,
                        current_revision: cur,
                    });
                }
            }
            if self.lose_next_push_response.replace(false) {
                return Err(TeamsError::Network("timeout".into()));
            }
            Ok(resp)
        }

        async fn ingest_audit(
            &self,
            _team: Uuid,
            req: IngestAuditRequest,
        ) -> ClientResult<IngestAuditResponse> {
            let mut st = self.state.borrow_mut();
            let mut resp = IngestAuditResponse {
                accepted: vec![],
                duplicates: vec![],
            };
            for ev in req.events {
                if st.audit_seen.contains(&ev.event_id) {
                    resp.duplicates.push(ev.event_id);
                    continue;
                }
                if ev.seq != st.next_audit_seq {
                    return Err(TeamsError::Api {
                        status: 409,
                        error: ApiError {
                            code: ErrorCode::AuditChainBroken,
                            message: "gap".into(),
                        },
                    });
                }
                st.next_audit_seq += 1;
                st.audit_seen.insert(ev.event_id);
                resp.accepted.push(ev.event_id);
                st.audit_stored.push(ev);
            }
            Ok(resp)
        }
    }

    pub struct Fixture {
        pub _dir: tempfile::TempDir,
        pub store: TeamsStore,
        pub team: Uuid,
        pub me: FakeCodec,
        pub other: FakeCodec,
        pub server: FakeServer,
    }

    pub fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("test.db"),
            ..Default::default()
        };
        let store = TeamsStore::open(&config).unwrap();
        let team = Uuid::new_v4();
        let me = FakeCodec::new(Uuid::new_v4());
        store
            .upsert_team(&TeamLocal {
                team_id: team,
                server_url: "https://teams.example.com".into(),
                account_id: Uuid::new_v4(),
                device_id: me.device,
                trust_root_ed25519: Fixed([5u8; 32]),
                last_pulled_revision: 0,
                created_at: Utc::now(),
            })
            .unwrap();
        Fixture {
            _dir: dir,
            store,
            team,
            me,
            other: FakeCodec::new(Uuid::new_v4()),
            server: FakeServer::default(),
        }
    }

    pub fn conn(name: &str) -> VaultPayload {
        VaultPayload::Connection(ConnectionRecord {
            name: name.into(),
            host: format!("{name}.example.com"),
            user: "ops".into(),
            port: 22,
            bastion: None,
            bastion_user: None,
            use_kerberos: false,
            tags: vec![],
            notes: None,
        })
    }

    pub fn conn_name(p: &VaultPayload) -> String {
        match p {
            VaultPayload::Connection(c) => c.name.clone(),
            other => panic!("not a connection: {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use crate::services::teams::{queue_edit, read_vault};
    use bssh_teams_proto::api::AuditKind;
    use bssh_teams_proto::layout::{audit_event_hash, AUDIT_GENESIS};

    fn engine(f: &Fixture) -> SyncEngine<'_, FakeServer, FakeCodec> {
        SyncEngine::new(&f.store, &f.server, &f.me, f.team)
    }

    fn names(f: &Fixture) -> Vec<String> {
        let mut n: Vec<_> = read_vault(&f.store, &f.me, f.team)
            .unwrap()
            .connections
            .iter()
            .map(|c| c.record.name.clone())
            .collect();
        n.sort();
        n
    }

    #[tokio::test]
    async fn initial_pull_with_pagination() {
        let f = fixture();
        for i in 0..5 {
            let e = f
                .other
                .seal(Uuid::new_v4(), 0, &conn(&format!("host{i}")))
                .unwrap();
            f.server.state.borrow_mut().put(e);
        }
        let report = engine(&f).with_page_size(2).sync().await;
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(report.pulled, 5);
        assert_eq!(f.server.pull_calls.get(), 3, "2 + 2 + 1 entries");
        assert_eq!(f.store.last_pulled_revision(f.team).unwrap(), 5);
        assert_eq!(names(&f).len(), 5);

        // A second run pulls nothing.
        let again = engine(&f).sync().await;
        assert_eq!(again.pulled, 0);
    }

    #[tokio::test]
    async fn highest_revision_per_entry_wins() {
        let f = fixture();
        let id = Uuid::new_v4();
        let v1 = f.other.seal(id, 0, &conn("old")).unwrap();
        let v2 = f.other.seal(id, 1, &conn("new")).unwrap();
        {
            let mut st = f.server.state.borrow_mut();
            st.put(v1);
            st.put(v2);
        }
        engine(&f).sync().await;
        assert_eq!(names(&f), vec!["new"]);
        assert_eq!(f.store.entries(f.team).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn local_edit_is_visible_immediately_and_pushed() {
        let f = fixture();
        let id = Uuid::new_v4();
        queue_edit(&f.store, &f.me, f.team, id, &conn("db1")).unwrap();
        // Local-first: readable before any sync, flagged as pending.
        let view = read_vault(&f.store, &f.me, f.team).unwrap();
        assert_eq!(view.connections.len(), 1);
        assert!(view.connections[0].pending);

        let report = engine(&f).sync().await;
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(report.pushed, 1);
        assert!(f.store.outbox(f.team).unwrap().is_empty());
        let st = f.server.state.borrow();
        assert_eq!(st.history.len(), 1);
        assert_eq!(st.history[0].revision, 1);
        drop(st);
        let view = read_vault(&f.store, &f.me, f.team).unwrap();
        assert!(!view.connections[0].pending);
        assert_eq!(view.connections[0].revision, 1);
    }

    #[tokio::test]
    async fn concurrent_edit_is_resealed_and_repushed() {
        let f = fixture();
        let id = Uuid::new_v4();
        queue_edit(&f.store, &f.me, f.team, id, &conn("shared-v1")).unwrap();
        engine(&f).sync().await;

        // Local edit based on revision 1 ...
        queue_edit(&f.store, &f.me, f.team, id, &conn("mine")).unwrap();
        // ... but someone else edits first, after our pull and before our push.
        let theirs = f.other.seal(id, 1, &conn("theirs")).unwrap();
        *f.server.before_push.borrow_mut() = Some(Box::new(move |st| {
            st.put(theirs);
        }));

        let report = engine(&f).sync().await;
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(report.conflicts_resolved, 1);
        assert_eq!(report.pushed, 1);
        assert!(f.store.outbox(f.team).unwrap().is_empty());

        let st = f.server.state.borrow();
        assert_eq!(st.history.len(), 3, "their version stays in history");
        let last = st.history.last().unwrap();
        assert_eq!(last.base_revision, 2, "re-sealed on the new revision");
        assert_eq!(conn_name(&f.me.open(last).unwrap()), "mine");
        drop(st);
        assert_eq!(names(&f), vec!["mine"]);
    }

    #[tokio::test]
    async fn remote_tombstone_drops_local_edit_and_reports_it() {
        let f = fixture();
        let id = Uuid::new_v4();
        queue_edit(&f.store, &f.me, f.team, id, &conn("doomed")).unwrap();
        engine(&f).sync().await;

        queue_edit(&f.store, &f.me, f.team, id, &conn("doomed-edited")).unwrap();
        let tomb = f.other.seal(id, 1, &VaultPayload::Tombstone).unwrap();
        f.server.state.borrow_mut().put(tomb);

        let report = engine(&f).sync().await;
        assert_eq!(report.dropped_edits.len(), 1, "{report:?}");
        assert_eq!(report.dropped_edits[0].entry_id, id);
        assert_eq!(report.pushed, 0);
        assert!(f.store.outbox(f.team).unwrap().is_empty());
        assert!(names(&f).is_empty());
        assert_eq!(f.server.state.borrow().history.len(), 2, "nothing pushed");
    }

    #[tokio::test]
    async fn server_failure_mid_push_keeps_outbox_then_recovers() {
        let f = fixture();
        queue_edit(&f.store, &f.me, f.team, Uuid::new_v4(), &conn("a")).unwrap();
        queue_edit(&f.store, &f.me, f.team, Uuid::new_v4(), &conn("b")).unwrap();
        f.server.fail_pushes.set(1);

        let report = engine(&f).sync().await;
        assert!(!report.is_clean());
        assert_eq!(report.pushed, 0);
        assert_eq!(f.store.outbox(f.team).unwrap().len(), 2, "outbox intact");
        assert_eq!(names(&f), vec!["a", "b"], "still readable locally");

        let report = engine(&f).sync().await;
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(report.pushed, 2);
        assert!(f.store.outbox(f.team).unwrap().is_empty());
        assert_eq!(f.server.state.borrow().history.len(), 2);
    }

    #[tokio::test]
    async fn lost_push_response_does_not_duplicate_the_edit() {
        let f = fixture();
        let id = Uuid::new_v4();
        queue_edit(&f.store, &f.me, f.team, id, &conn("once")).unwrap();
        f.server.lose_next_push_response.set(true);

        let first = engine(&f).sync().await;
        assert!(!first.is_clean());
        assert_eq!(f.store.outbox(f.team).unwrap().len(), 1);
        assert_eq!(f.server.state.borrow().history.len(), 1, "server has it");

        let second = engine(&f).sync().await;
        assert!(second.is_clean(), "{second:?}");
        assert_eq!(second.pushed, 1);
        assert!(f.store.outbox(f.team).unwrap().is_empty());
        assert_eq!(f.server.state.borrow().history.len(), 1, "no duplicate");
    }

    #[tokio::test]
    async fn audit_events_upload_in_order_and_duplicates_are_success() {
        let f = fixture();
        let id = Some(Uuid::new_v4());
        let mut events = Vec::new();
        for kind in [
            AuditKind::SessionStart,
            AuditKind::VaultRead,
            AuditKind::SessionEnd,
        ] {
            events.push(f.store.append_audit_event(f.team, &f.me, kind, id).unwrap());
        }
        // Hash chain: genesis, then each event hashes its predecessor.
        assert_eq!(events[0].prev_hash, AUDIT_GENESIS);
        assert_eq!(events[1].prev_hash, audit_event_hash(&events[0]));
        assert_eq!(events[2].prev_hash, audit_event_hash(&events[1]));
        assert_eq!(
            events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        // The server already stored event 0 (an earlier response was lost).
        {
            let mut st = f.server.state.borrow_mut();
            st.audit_seen.insert(events[0].event_id);
            st.next_audit_seq = 1;
            st.audit_stored.push(events[0].clone());
        }
        let report = engine(&f).sync().await;
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(report.audit_uploaded, 3);
        assert!(f
            .store
            .pending_audit(f.team, f.me.device, 10)
            .unwrap()
            .is_empty());
        let st = f.server.state.borrow();
        assert_eq!(
            st.audit_stored.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        drop(st);

        // The chain continues across uploads.
        let next = f
            .store
            .append_audit_event(f.team, &f.me, AuditKind::VaultWrite, None)
            .unwrap();
        assert_eq!(next.seq, 3);
        assert_eq!(next.prev_hash, audit_event_hash(&events[2]));
    }

    #[tokio::test]
    async fn audit_chain_error_keeps_events_queued() {
        let f = fixture();
        f.store
            .append_audit_event(f.team, &f.me, AuditKind::SessionStart, None)
            .unwrap();
        f.server.state.borrow_mut().next_audit_seq = 7; // server expects another seq
        let report = engine(&f).sync().await;
        assert!(!report.is_clean());
        assert_eq!(
            f.store
                .pending_audit(f.team, f.me.device, 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn rotation_required_is_surfaced_and_edits_stay_queued() {
        let f = fixture();
        queue_edit(&f.store, &f.me, f.team, Uuid::new_v4(), &conn("a")).unwrap();
        f.server.state.borrow_mut().rotation_required = true;

        let report = engine(&f).sync().await;
        assert!(report.rotation_required);
        assert_eq!(report.pushed, 0);
        assert_eq!(f.store.outbox(f.team).unwrap().len(), 1);
        assert_eq!(f.server.push_calls.get(), 1, "stops after the refusal");

        // Once an admin rotates, the same edit goes through.
        f.server.state.borrow_mut().rotation_required = false;
        let report = engine(&f).sync().await;
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(report.pushed, 1);
    }

    #[tokio::test]
    async fn relabeled_old_entry_is_rejected() {
        let f = fixture();
        let id = Uuid::new_v4();
        let v1 = f.other.seal(id, 0, &conn("v1")).unwrap();
        let v2 = f.other.seal(id, 1, &conn("v2")).unwrap();
        {
            let mut st = f.server.state.borrow_mut();
            st.put(v1.clone());
            st.put(v2);
        }
        engine(&f).sync().await;
        assert_eq!(names(&f), vec!["v2"]);

        // The server replays the old ciphertext under a newer revision label.
        f.server.state.borrow_mut().put_at(v1, 3);
        let report = engine(&f).sync().await;
        assert_eq!(report.rejected.len(), 1, "{report:?}");
        assert_eq!(report.rejected[0].entry_id, id);
        assert!(!report.is_clean());
        assert_eq!(report.pulled, 0);
        assert_eq!(names(&f), vec!["v2"], "local copy untouched");
        assert_eq!(f.store.entry(f.team, id).unwrap().unwrap().revision, 2);
        assert_eq!(f.store.last_pulled_revision(f.team).unwrap(), 3);
    }

    #[tokio::test]
    async fn skipped_revision_is_rejected_but_valid_entries_still_apply() {
        let f = fixture();
        let id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        f.server
            .state
            .borrow_mut()
            .put(f.other.seal(id, 0, &conn("a1")).unwrap());
        engine(&f).sync().await;

        // Revision 2 of `id` is hidden: the next edit is based on 2, not 1.
        let skipped = f.other.seal(id, 2, &conn("a3")).unwrap();
        let fine = f.other.seal(other_id, 0, &conn("b")).unwrap();
        {
            let mut st = f.server.state.borrow_mut();
            st.put_at(skipped, 3);
            st.put_at(fine, 4);
        }
        let report = engine(&f).sync().await;
        assert_eq!(report.rejected.len(), 1, "{report:?}");
        assert_eq!(report.rejected[0].revision, 3);
        assert_eq!(report.pulled, 1);
        assert_eq!(names(&f), vec!["a1", "b"]);
    }

    #[tokio::test]
    async fn unknown_author_is_rejected() {
        let f = fixture();
        *f.me.known.borrow_mut() = Some([f.me.device].into_iter().collect());
        let id = Uuid::new_v4();
        f.server
            .state
            .borrow_mut()
            .put(f.other.seal(id, 0, &conn("evil")).unwrap());
        let report = engine(&f).sync().await;
        assert_eq!(report.rejected.len(), 1, "{report:?}");
        assert!(report.rejected[0].reason.contains("unknown author"));
        assert!(f.store.entry(f.team, id).unwrap().is_none());
        assert!(names(&f).is_empty());
    }

    #[tokio::test]
    async fn own_accepted_push_is_not_flagged_when_pulled_back() {
        let f = fixture();
        queue_edit(&f.store, &f.me, f.team, Uuid::new_v4(), &conn("mine")).unwrap();
        let first = engine(&f).sync().await;
        assert!(first.is_clean(), "{first:?}");
        // Cursor was still 0 after the push, so the next pull re-delivers it.
        let second = engine(&f).sync().await;
        assert!(second.is_clean(), "{second:?}");
        assert_eq!(second.pulled, 0);
    }
}
