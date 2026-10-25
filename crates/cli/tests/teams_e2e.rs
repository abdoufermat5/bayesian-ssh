//! End-to-end test of bssh Teams against a real sync server.
//!
//! Skipped unless `BSSH_TEAMS_E2E_URL` points at a running bssh-cloud API
//! (e.g. `http://127.0.0.1:8080`) with a fresh database. Exercises the real
//! client, crypto, local store and sync engine; nothing is faked.

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::services::teams::client::TeamsClient;
use bayesian_ssh::services::teams::codec::{unwrap_all, CryptoCodec};
use bayesian_ssh::services::teams::crypto::{self, DeviceKeys, TeamKey};
use bayesian_ssh::services::teams::store::TeamLocal;
use bayesian_ssh::services::teams::{self as teams, SyncEngine, TeamsStore};
use bssh_teams_proto::api::{
    AcceptInviteRequest, AuditKind, ConnectionRecord, CreateAccountRequest, CreateInviteRequest,
    CreateTeamRequest, DeviceInfo, PutKeysRequest, Role, VaultPayload,
};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

struct Device {
    id: Uuid,
    sealed: Vec<u8>,
    info: DeviceInfo,
}

const PASS: &str = "e2e-test-passphrase";

impl Device {
    fn keys(&self) -> DeviceKeys {
        DeviceKeys::unseal(&self.sealed, PASS).unwrap()
    }
}

async fn register(url: &str, email: &str) -> (Device, TeamsClient) {
    let keys = DeviceKeys::generate();
    let anon = TeamsClient::new(url, Uuid::nil(), Arc::new(DeviceKeys::generate())).unwrap();
    let created = anon
        .create_account(&CreateAccountRequest {
            email: email.into(),
            device: keys.to_new_device("laptop"),
            // The dev server (bssh-cloud docker-compose) accepts open signups.
            invite_token: None,
            email_code: None,
        })
        .await
        .unwrap();
    let info = DeviceInfo {
        device_id: created.device_id,
        account_id: created.account_id,
        name: "laptop".into(),
        x25519_public: keys.x25519_public(),
        ed25519_public: keys.ed25519_public(),
        created_at: chrono::Utc::now(),
        revoked_at: None,
    };
    let sealed = keys.seal(PASS).unwrap();
    let client = TeamsClient::new(url, created.device_id, Arc::new(keys)).unwrap();
    (
        Device {
            id: created.device_id,
            sealed,
            info,
        },
        client,
    )
}

fn local_store(dir: &tempfile::TempDir) -> TeamsStore {
    TeamsStore::open(&AppConfig {
        database_path: dir.path().join("bssh.db"),
        ..Default::default()
    })
    .unwrap()
}

fn record(host: &str) -> ConnectionRecord {
    ConnectionRecord {
        name: "db-prod".into(),
        host: host.into(),
        user: "ops".into(),
        port: 22,
        bastion: None,
        bastion_user: None,
        use_kerberos: false,
        tags: vec!["prod".into()],
        notes: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn teams_end_to_end() {
    let Ok(url) = std::env::var("BSSH_TEAMS_E2E_URL") else {
        eprintln!("BSSH_TEAMS_E2E_URL not set; skipping");
        return;
    };
    let suffix = Uuid::new_v4().simple().to_string();
    let admin_email = format!("admin-{suffix}@example.com");
    let member_email = format!("member-{suffix}@example.com");

    // 1. Admin creates a team with TK_1 wrapped to their own device.
    let (admin, admin_api) = register(&url, &admin_email).await;
    let team_id = Uuid::new_v4();
    let tk1 = TeamKey::generate(1);
    let initial_key =
        crypto::wrap_team_key(team_id, &tk1, &admin.info, admin.id, &admin.keys()).unwrap();
    let team = admin_api
        .create_team(&CreateTeamRequest {
            initial_key,
            org_id: None,
        })
        .await
        .unwrap();
    assert_eq!(team.my_role, Role::Admin);
    let trust_root = admin.info.ed25519_public;

    let admin_dir = tempfile::tempdir().unwrap();
    let admin_store = local_store(&admin_dir);
    admin_store
        .upsert_team(&TeamLocal {
            team_id,
            server_url: url.clone(),
            account_id: admin.info.account_id,
            device_id: admin.id,
            trust_root_ed25519: trust_root,
            last_pulled_revision: 0,
            created_at: chrono::Utc::now(),
        })
        .unwrap();
    let admin_tk1 = unwrap_all(
        team_id,
        admin.id,
        &admin_api.get_keys(team_id).await.unwrap(),
        &admin.keys(),
        &trust_root,
    )
    .unwrap();
    let mut admin_codec =
        CryptoCodec::new(team_id, admin.id, admin.keys(), admin_tk1, HashMap::new()).unwrap();

    // 2. Admin shares a connection and names the team; sync pushes both.
    let entry_id = teams::queue_connection(
        &admin_store,
        &admin_codec,
        team_id,
        None,
        record("10.0.0.5"),
    )
    .unwrap();
    teams::queue_edit(
        &admin_store,
        &admin_codec,
        team_id,
        Uuid::new_v4(),
        &VaultPayload::TeamProfile {
            name: "Platform".into(),
        },
    )
    .unwrap();
    let r = SyncEngine::new(&admin_store, &admin_api, &admin_codec, team_id)
        .sync()
        .await;
    assert!(r.is_clean(), "{r:?}");
    assert_eq!(r.pushed, 2);

    // 3. Member joins by invite; admin wraps TK_1 to the member's device.
    let (member, member_api) = register(&url, &member_email).await;
    let invite = admin_api
        .create_invite(
            team_id,
            &CreateInviteRequest {
                email: member_email.clone(),
                role: Role::Member,
            },
        )
        .await
        .unwrap();
    member_api
        .accept_invite(&AcceptInviteRequest {
            token: invite.token.expect("token is returned once"),
        })
        .await
        .unwrap();
    let pending = admin_api.pending_devices(team_id).await.unwrap();
    assert_eq!(pending.devices.len(), 1);
    assert_eq!(pending.devices[0].device_id, member.id);
    let grant =
        crypto::wrap_team_key(team_id, &tk1, &pending.devices[0], admin.id, &admin.keys()).unwrap();
    admin_api
        .put_keys(team_id, &PutKeysRequest { keys: vec![grant] })
        .await
        .unwrap();

    // 4. Member unwraps against the pinned admin key and pulls the vault.
    let member_dir = tempfile::tempdir().unwrap();
    let member_store = local_store(&member_dir);
    member_store
        .upsert_team(&TeamLocal {
            team_id,
            server_url: url.clone(),
            account_id: member.info.account_id,
            device_id: member.id,
            trust_root_ed25519: trust_root,
            last_pulled_revision: 0,
            created_at: chrono::Utc::now(),
        })
        .unwrap();
    let member_keys = unwrap_all(
        team_id,
        member.id,
        &member_api.get_keys(team_id).await.unwrap(),
        &member.keys(),
        &trust_root,
    )
    .unwrap();
    let member_codec = CryptoCodec::new(
        team_id,
        member.id,
        member.keys(),
        member_keys,
        HashMap::from([(admin.id, trust_root)]),
    )
    .unwrap();
    let r = SyncEngine::new(&member_store, &member_api, &member_codec, team_id)
        .sync()
        .await;
    assert!(r.is_clean(), "{r:?}");
    assert_eq!(r.pulled, 2);
    let view = teams::read_vault(&member_store, &member_codec, team_id).unwrap();
    assert_eq!(view.team_name.as_deref(), Some("Platform"));
    assert_eq!(view.connections.len(), 1);
    assert_eq!(view.connections[0].record.host, "10.0.0.5");

    // 5. Member edits the connection and logs an audit event; admin sees both.
    teams::queue_connection(
        &member_store,
        &member_codec,
        team_id,
        Some(entry_id),
        record("10.0.0.6"),
    )
    .unwrap();
    member_store
        .append_audit_event(
            team_id,
            &member_codec,
            AuditKind::SessionStart,
            Some(entry_id),
        )
        .unwrap();
    let r = SyncEngine::new(&member_store, &member_api, &member_codec, team_id)
        .sync()
        .await;
    assert!(r.is_clean(), "{r:?}");
    assert_eq!((r.pushed, r.audit_uploaded), (1, 1));

    admin_codec.add_author(member.id, member.info.ed25519_public);
    let r = SyncEngine::new(&admin_store, &admin_api, &admin_codec, team_id)
        .sync()
        .await;
    assert!(r.is_clean(), "{r:?}");
    let view = teams::read_vault(&admin_store, &admin_codec, team_id).unwrap();
    assert_eq!(view.connections[0].record.host, "10.0.0.6");

    // 6. Admin removes the member: pushes need a rotation, the member is locked out.
    let members = admin_api.list_members(team_id).await.unwrap();
    let member_row = members
        .iter()
        .find(|m| m.email == member_email)
        .expect("member listed");
    admin_api
        .remove_member(team_id, member_row.member_id)
        .await
        .unwrap();

    teams::queue_connection(
        &admin_store,
        &admin_codec,
        team_id,
        Some(entry_id),
        record("10.0.0.7"),
    )
    .unwrap();
    let r = SyncEngine::new(&admin_store, &admin_api, &admin_codec, team_id)
        .sync()
        .await;
    assert!(r.rotation_required, "old key refused after removal: {r:?}");
    assert_eq!(
        admin_store.outbox(team_id).unwrap().len(),
        1,
        "edit stays queued"
    );

    let r = SyncEngine::new(&member_store, &member_api, &member_codec, team_id)
        .sync()
        .await;
    assert!(!r.errors.is_empty(), "removed member can't sync: {r:?}");

    // 7. Rotation to TK_2 (only the admin's device remains), then the edit goes through.
    let tk2 = TeamKey::generate(2);
    let rotated =
        crypto::wrap_team_key(team_id, &tk2, &admin.info, admin.id, &admin.keys()).unwrap();
    admin_api
        .put_keys(
            team_id,
            &PutKeysRequest {
                keys: vec![rotated],
            },
        )
        .await
        .unwrap();
    let keys = unwrap_all(
        team_id,
        admin.id,
        &admin_api.get_keys(team_id).await.unwrap(),
        &admin.keys(),
        &trust_root,
    )
    .unwrap();
    assert_eq!(keys.len(), 2);
    let mut admin_codec =
        CryptoCodec::new(team_id, admin.id, admin.keys(), keys, HashMap::new()).unwrap();
    admin_codec.add_author(member.id, member.info.ed25519_public);
    // The queued edit was sealed under TK_1; re-queue it under the new key.
    teams::queue_connection(
        &admin_store,
        &admin_codec,
        team_id,
        Some(entry_id),
        record("10.0.0.7"),
    )
    .unwrap();
    let r = SyncEngine::new(&admin_store, &admin_api, &admin_codec, team_id)
        .sync()
        .await;
    assert!(r.is_clean(), "{r:?}");
    assert_eq!(r.pushed, 1);
    let view = teams::read_vault(&admin_store, &admin_codec, team_id).unwrap();
    assert_eq!(view.connections[0].record.host, "10.0.0.7");

    println!("E2E_TEAM_ID={team_id}");
}
