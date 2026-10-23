//! bssh Teams client: end-to-end encrypted shared vaults.
//!
//! The wire contract and design live in `crates/teams-proto` (see its
//! `PROTOCOL.md`). This module is the client side:
//!
//! - [`crypto`]: device keys, team-key wrapping, entry sealing, audit signing.
//!   All cryptography in bssh Teams is here, in the open (PERIMETER.md, P2).
//! - [`codec`]: [`VaultCodec`], the seal/open seam the sync engine is written
//!   against (production impl: [`codec::CryptoCodec`]).
//! - [`store`]: local SQLite tables (schema v8): ciphertext, wrapped keys,
//!   outbox, audit chain.
//! - [`billing`]: why a team is read-only, from the server's billing info.
//! - [`client`]: the HTTPS client for the `/v1` API.
//! - [`sync`]: the local-first sync engine.
//! - [`session`]: the workflows behind `bssh teams` (identity, invites,
//!   device approval, rotation) and the trust rules they enforce.
//!
//! Local-first (PERIMETER.md R1): [`read_vault`] and [`queue_edit`] only touch
//! the local store. Nothing here waits on the network; [`sync::SyncEngine`]
//! reconciles with the server in the background.

#![allow(dead_code)]

pub mod audit;
pub mod billing;
pub mod client;
pub mod codec;
pub mod crypto;
pub mod session;
pub mod store;
pub mod sync;

pub use codec::VaultCodec;
pub use store::TeamsStore;
pub use sync::{SyncEngine, SyncReport};

use anyhow::Result;
use bssh_teams_proto::api::{ConnectionRecord, VaultPayload};
use uuid::Uuid;

/// A shared connection as seen locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamConnection {
    pub entry_id: Uuid,
    /// Last revision known from the server (0 if never pushed).
    pub revision: u64,
    /// True while a local edit to this entry is waiting to be pushed.
    pub pending: bool,
    pub record: ConnectionRecord,
}

/// The decrypted local view of a team vault.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VaultView {
    pub team_name: Option<String>,
    /// Live (non-deleted) connections, sorted by name.
    pub connections: Vec<TeamConnection>,
    /// Entries that could not be verified or decrypted (unknown author,
    /// missing key version, bad signature). They are skipped, never shown.
    pub unreadable: Vec<Uuid>,
}

/// Decrypt the local copy of a team's vault. Pending local edits override the
/// server copy, so an edit is visible before it is synced. Plaintext is never
/// stored; this decrypts on every call.
pub fn read_vault(store: &TeamsStore, codec: &dyn VaultCodec, team_id: Uuid) -> Result<VaultView> {
    use std::collections::BTreeMap;

    let mut latest: BTreeMap<Uuid, (u64, bool, bssh_teams_proto::api::EncryptedEntry)> =
        BTreeMap::new();
    for e in store.entries(team_id)? {
        latest.insert(e.entry_id, (e.revision, false, e));
    }
    for row in store.outbox(team_id)? {
        let revision = latest.get(&row.entry.entry_id).map_or(0, |v| v.0);
        latest.insert(row.entry.entry_id, (revision, true, row.entry));
    }

    let mut view = VaultView::default();
    for (entry_id, (revision, pending, entry)) in latest {
        match codec.open(&entry) {
            Ok(VaultPayload::Connection(record)) => view.connections.push(TeamConnection {
                entry_id,
                revision,
                pending,
                record,
            }),
            Ok(VaultPayload::TeamProfile { name }) => view.team_name = Some(name),
            Ok(VaultPayload::Tombstone) => {}
            Err(e) => {
                tracing::warn!("team entry {entry_id} unreadable: {e}");
                view.unreadable.push(entry_id);
            }
        }
    }
    view.connections
        .sort_by(|a, b| a.record.name.cmp(&b.record.name));
    Ok(view)
}

/// Seal `payload` as an edit of `entry_id` on top of the locally known
/// revision and queue it for the next sync. A newer queued edit to the same
/// entry replaces the older one.
pub fn queue_edit(
    store: &TeamsStore,
    codec: &dyn VaultCodec,
    team_id: Uuid,
    entry_id: Uuid,
    payload: &VaultPayload,
) -> Result<()> {
    let base = store.entry(team_id, entry_id)?.map_or(0, |e| e.revision);
    let sealed = codec.seal(entry_id, base, payload)?;
    store.queue_outbox(team_id, &sealed)
}

/// Queue a new or updated shared connection; returns its entry id.
pub fn queue_connection(
    store: &TeamsStore,
    codec: &dyn VaultCodec,
    team_id: Uuid,
    entry_id: Option<Uuid>,
    record: ConnectionRecord,
) -> Result<Uuid> {
    let id = entry_id.unwrap_or_else(Uuid::new_v4);
    queue_edit(store, codec, team_id, id, &VaultPayload::Connection(record))?;
    Ok(id)
}

/// Queue a deletion (a tombstone; the entry stays in server history).
pub fn queue_delete(
    store: &TeamsStore,
    codec: &dyn VaultCodec,
    team_id: Uuid,
    entry_id: Uuid,
) -> Result<()> {
    queue_edit(store, codec, team_id, entry_id, &VaultPayload::Tombstone)
}
