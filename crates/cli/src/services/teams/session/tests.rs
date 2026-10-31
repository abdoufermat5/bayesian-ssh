use super::audit::AuditQuery;
use super::billing::{explain, org_message};
use super::handle::Session;
use super::identity::{init_interactive, load_identity, EmailCodePrompt};
use super::local::{link_to_team_source, materialize_into};
use super::removal::{plan_removal, ApprovedDevices};
use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Connection, Source, SourceKind};
use crate::services::teams::client::TeamsError;
use crate::services::teams::crypto::{self, DeviceKeys};
use crate::services::teams::store::{TeamLocal, TeamsStore};
use crate::services::teams::VaultView;
use anyhow::Result;
use bssh_teams_proto::api::{
    AuditKind, ConnectionRecord, DeviceInfo, EmailCodeResponse, ErrorCode, Member, MemberKind,
    Role, TeamInfo,
};
use bssh_teams_proto::wire::{Fixed, PublicKey32};
use chrono::{DateTime, Utc};
use rusqlite::params;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

fn shared_record(c: &Connection) -> super::super::TeamConnection {
    super::super::TeamConnection {
        entry_id: Uuid::new_v4(),
        revision: 1,
        pending: false,
        record: ConnectionRecord {
            name: c.name.clone(),
            host: c.host.clone(),
            user: c.user.clone(),
            port: c.port,
            bastion: c.bastion.clone(),
            bastion_user: c.bastion_user.clone(),
            use_kerberos: c.use_kerberos,
            tags: c.tags.clone(),
            notes: None,
        },
    }
}

fn team_db() -> (tempfile::TempDir, Database, AppConfig, Uuid, Source) {
    let dir = tempfile::tempdir().unwrap();
    let config = AppConfig {
        database_path: dir.path().join("bssh.db"),
        ..AppConfig::default()
    };
    let db = Database::new(&config).unwrap();
    let team_id = Uuid::new_v4();
    let source = Source::new(
        "homelab".into(),
        SourceKind::Team,
        serde_json::json!({ "team_id": team_id.to_string() }),
    );
    db.add_source(&source).unwrap();
    (dir, db, config, team_id, source)
}

fn local_backup(db: &Database) -> Connection {
    let mut c = Connection::new(
        "backup-pc".into(),
        "10.8.0.10".into(),
        "abdoufermat".into(),
        22,
        None,
        None,
        false,
        Some("/home/me/.ssh/id_rsa".into()),
    );
    c.forward_agent = true;
    db.add_connection(&c).unwrap();
    c
}

#[test]
fn sharing_links_the_local_connection_instead_of_adding_a_copy() {
    let (_dir, db, config, team_id, source) = team_db();
    let local = local_backup(&db);

    let entry_id = link_to_team_source(&db, &source, &local).unwrap();
    let mut shared = shared_record(&local);
    shared.entry_id = entry_id;
    let view = VaultView {
        connections: vec![shared],
        ..VaultView::default()
    };
    materialize_into(&db, &config, team_id, &view).unwrap();

    let all = db.list_connections(None, false).unwrap();
    assert_eq!(
        all.len(),
        1,
        "{:?}",
        all.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    let linked = db.connection("backup-pc").unwrap().unwrap();
    assert_eq!(
        linked.id, local.id,
        "the user's own connection, not a new one"
    );
    assert_eq!(linked.source_id, Some(source.id));
    assert_eq!(linked.external_id, Some(entry_id.to_string()));
    // Local-only settings survive, and an unchanged route asks nothing.
    assert_eq!(linked.key_path.as_deref(), Some("/home/me/.ssh/id_rsa"));
    assert!(linked.forward_agent);
    assert!(linked.pending_change.is_none());

    // A teammate's edit now reaches it, behind the confirmation.
    let mut moved = view.connections[0].clone();
    moved.record.host = "10.10.10.30".into();
    moved.revision = 2;
    let view = VaultView {
        connections: vec![moved],
        ..VaultView::default()
    };
    materialize_into(&db, &config, team_id, &view).unwrap();
    let moved = db.connection("backup-pc").unwrap().unwrap();
    assert_eq!(moved.host, "10.10.10.30");
    assert_eq!(moved.pending_change.unwrap().previous.host, "10.8.0.10");
    assert!(!moved.forward_agent);
}

#[test]
fn sharing_again_adopts_the_copy_an_earlier_share_left() {
    let (_dir, db, config, team_id, source) = team_db();
    let local = local_backup(&db);
    // Before the fix: the share's sync added the entry as `backup-pc@homelab`.
    let earlier = shared_record(&local);
    let view = VaultView {
        connections: vec![earlier.clone()],
        ..VaultView::default()
    };
    materialize_into(&db, &config, team_id, &view).unwrap();
    assert!(db.connection("backup-pc@homelab").unwrap().is_some());

    let entry_id = link_to_team_source(&db, &source, &local).unwrap();
    assert_eq!(
        entry_id, earlier.entry_id,
        "the existing vault entry is reused"
    );
    materialize_into(&db, &config, team_id, &view).unwrap();

    let names: Vec<String> = db
        .list_connections(None, false)
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["backup-pc"]);
    let linked = db.connection("backup-pc").unwrap().unwrap();
    assert_eq!(linked.id, local.id);
    assert_eq!(linked.external_id, Some(earlier.entry_id.to_string()));
}

fn api_error(status: u16, code: ErrorCode, message: &str) -> TeamsError {
    TeamsError::Api {
        status,
        error: bssh_teams_proto::api::ApiError {
            code,
            message: message.into(),
        },
    }
}

#[test]
fn org_errors_keep_the_server_rule() {
    let forbidden = org_message(api_error(
        403,
        ErrorCode::Forbidden,
        "only an owner of the organization can manage billing",
    ));
    assert_eq!(
        forbidden.to_string(),
        "only an owner of the organization can manage billing"
    );
    assert_eq!(explain(&forbidden), None, "no generic hint over it");

    let last_owner = org_message(api_error(
        409,
        ErrorCode::Conflict,
        "the organization must keep at least one owner",
    ));
    assert_eq!(
        last_owner.to_string(),
        "the organization must keep at least one owner"
    );

    // Credentials keep their actionable hint.
    let unauthorized = org_message(api_error(401, ErrorCode::Unauthorized, "unauthorized"));
    assert!(explain(&unauthorized).is_some());
}

fn pk(b: u8) -> PublicKey32 {
    PublicKey32::from([b; 32])
}

fn device(n: u8, revoked: bool) -> DeviceInfo {
    DeviceInfo {
        device_id: Uuid::from_u128(n as u128),
        account_id: Uuid::nil(),
        name: format!("dev{n}"),
        x25519_public: pk(n),
        ed25519_public: pk(n + 100),
        created_at: Utc::now(),
        revoked_at: revoked.then(Utc::now),
    }
}

fn member(n: u128, email: &str, devices: Vec<DeviceInfo>) -> Member {
    Member {
        kind: Default::default(),
        member_id: Uuid::from_u128(1000 + n),
        account_id: Uuid::from_u128(2000 + n),
        email: email.into(),
        role: Role::Member,
        devices,
        joined_at: Utc::now(),
    }
}

fn approve(map: &mut ApprovedDevices, d: &DeviceInfo) {
    map.insert(d.device_id, ("x".into(), d.x25519_public, d.ed25519_public));
}

#[test]
fn plan_splits_recipients_and_blocking() {
    let (a, b, c, gone) = (
        device(1, false),
        device(2, false),
        device(3, false),
        device(4, true),
    );
    let members = vec![
        member(1, "admin@x.io", vec![a.clone()]),
        member(2, "bob@x.io", vec![b.clone(), gone]),
        member(3, "carol@x.io", vec![c.clone()]),
    ];
    let mut approved = HashMap::new();
    approve(&mut approved, &a);
    approve(&mut approved, &b);
    let p = plan_removal(&members, " Bob@X.io ", &approved, 3, 5).unwrap();
    assert_eq!(p.email, "bob@x.io");
    assert_eq!(p.next_key_version, 4);
    assert_eq!(p.shared_hosts, 5);
    assert_eq!(p.removed_devices.len(), 1, "revoked devices are skipped");
    assert!(p.removed_devices[0].approved);
    assert_eq!(p.recipients.len(), 1);
    assert_eq!(p.recipients[0].email, "admin@x.io");
    assert_eq!(p.blocking.len(), 1);
    assert_eq!(p.blocking[0].email, "carol@x.io");
    assert!(!p.blocking[0].approved);
}

#[test]
fn changed_keys_block_rotation() {
    let a = device(1, false);
    let b = device(2, false);
    let members = vec![
        member(1, "a@x.io", vec![a.clone()]),
        member(2, "b@x.io", vec![b]),
    ];
    let mut approved = HashMap::new();
    let mut stale = a.clone();
    stale.x25519_public = pk(99);
    approve(&mut approved, &stale);
    let p = plan_removal(&members, "b@x.io", &approved, 1, 0).unwrap();
    assert!(p.recipients.is_empty());
    assert_eq!(p.blocking.len(), 1);
}

#[test]
fn unknown_member_is_an_error() {
    let members = vec![member(1, "a@x.io", vec![])];
    assert!(plan_removal(&members, "nobody@x.io", &HashMap::new(), 1, 0).is_err());
}

// ── Email verification on account creation ──────────────────────────────

use crate::services::teams::client::mock::mock_server;
use bssh_teams_proto::api::CreateAccountResponse;

/// Answers `read_code` from a script and records the notices it was shown.
struct ScriptedPrompt {
    terminal: bool,
    answers: std::collections::VecDeque<Option<&'static str>>,
    notices: Vec<String>,
}

impl ScriptedPrompt {
    fn new(terminal: bool, answers: &[Option<&'static str>]) -> Self {
        Self {
            terminal,
            answers: answers.iter().copied().collect(),
            notices: Vec::new(),
        }
    }
}

impl EmailCodePrompt for ScriptedPrompt {
    fn terminal(&self) -> bool {
        self.terminal
    }

    fn read_code(&mut self, notice: Option<&str>) -> Result<Option<String>> {
        if let Some(notice) = notice {
            self.notices.push(notice.to_string());
        }
        Ok(self
            .answers
            .pop_front()
            .expect("the code was asked for more often than scripted")
            .map(str::to_string))
    }
}

fn local_store(dir: &tempfile::TempDir) -> TeamsStore {
    TeamsStore::open(&AppConfig {
        database_path: dir.path().join("bssh.db"),
        ..Default::default()
    })
    .unwrap()
}

fn verification_required() -> String {
    r#"{"code":"email_verification_required","message":"verify your email first"}"#.to_string()
}

fn invalid_code() -> String {
    r#"{"code":"invalid_email_code","message":"that code is wrong or expired"}"#.to_string()
}

fn code_sent(expires_in_secs: u64) -> String {
    serde_json::to_string(&EmailCodeResponse { expires_in_secs }).unwrap()
}

fn account_created() -> String {
    serde_json::to_string(&CreateAccountResponse {
        account_id: Uuid::from_u128(5),
        device_id: Uuid::from_u128(6),
    })
    .unwrap()
}

#[tokio::test]
async fn init_mails_a_code_and_retries_with_it() {
    let (url, seen) = mock_server(|i| match i {
        0 => (403, verification_required()),
        1 => (200, code_sent(900)),
        _ => (200, account_created()),
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let store = local_store(&dir);
    let mut prompt = ScriptedPrompt::new(true, &[Some("123456")]);

    let identity = init_interactive(
        &store,
        &url,
        "Ana@Example.com",
        "laptop",
        "a long passphrase",
        None,
        None,
        &mut prompt,
    )
    .await
    .unwrap();

    assert_eq!(identity.email, "ana@example.com");
    assert_eq!(identity.account_id, Uuid::from_u128(5));
    assert!(load_identity(store.database()).unwrap().is_some());
    assert_eq!(
        prompt.notices,
        ["A verification code was sent to ana@example.com (valid 15 minutes)."]
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3, "required, code mailed, retry: {seen:#?}");
    assert!(
        seen[0].starts_with("POST /v1/accounts HTTP/1.1"),
        "{}",
        seen[0]
    );
    assert!(!seen[0].contains("email_code"), "{}", seen[0]);
    assert!(
        seen[1].starts_with("POST /v1/accounts/email-code HTTP/1.1"),
        "{}",
        seen[1]
    );
    assert!(
        seen[1].contains(r#""email":"ana@example.com""#),
        "{}",
        seen[1]
    );
    assert!(
        seen[2].starts_with("POST /v1/accounts HTTP/1.1"),
        "{}",
        seen[2]
    );
    assert!(seen[2].contains(r#""email_code":"123456""#), "{}", seen[2]);
}

#[tokio::test]
async fn init_sends_a_code_it_was_given() {
    let (url, seen) = mock_server(|_| (200, account_created())).await;
    let dir = tempfile::tempdir().unwrap();
    let store = local_store(&dir);

    init_interactive(
        &store,
        &url,
        "ana@example.com",
        "laptop",
        "a long passphrase",
        None,
        Some("777777"),
        &mut ScriptedPrompt::new(true, &[]),
    )
    .await
    .unwrap();

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "{seen:#?}");
    assert!(seen[0].contains(r#""email_code":"777777""#), "{}", seen[0]);
}

#[tokio::test]
async fn init_without_a_terminal_mails_a_code_and_says_how_to_rerun() {
    let (url, seen) = mock_server(|i| match i {
        0 => (403, verification_required()),
        _ => (200, code_sent(600)),
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let store = local_store(&dir);

    let err = init_interactive(
        &store,
        &url,
        "ana@example.com",
        "laptop",
        "a long passphrase",
        None,
        None,
        &mut ScriptedPrompt::new(false, &[]),
    )
    .await
    .unwrap_err();

    let text = err.to_string();
    assert!(text.contains("no terminal"), "{text}");
    assert!(
        text.contains("a new code was sent to ana@example.com (valid 10 minutes)"),
        "{text}"
    );
    assert!(text.contains("rerun with `--email-code <CODE>`"), "{text}");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "the code was requested: {seen:#?}");
    assert!(
        seen[1].starts_with("POST /v1/accounts/email-code HTTP/1.1"),
        "{}",
        seen[1]
    );
    assert!(load_identity(store.database()).unwrap().is_none());
}

#[tokio::test]
async fn init_reprompts_after_a_refused_code_and_gives_up_after_three() {
    let (url, seen) = mock_server(|i| match i {
        0 => (403, verification_required()),
        1 => (200, code_sent(900)),
        2..=4 => (403, invalid_code()),
        _ => (200, account_created()),
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let store = local_store(&dir);
    let mut prompt = ScriptedPrompt::new(true, &[Some("111111"), Some("222222"), Some("333333")]);

    let err = init_interactive(
        &store,
        &url,
        "ana@example.com",
        "laptop",
        "a long passphrase",
        None,
        None,
        &mut prompt,
    )
    .await
    .unwrap_err();

    let text = err.to_string();
    assert!(
        text.contains("refused the verification code 3 times"),
        "{text}"
    );
    assert!(text.contains("`--email-code <CODE>`"), "{text}");
    assert_eq!(prompt.notices.len(), 3, "{:?}", prompt.notices);
    assert_eq!(
        prompt.notices[0],
        "A verification code was sent to ana@example.com (valid 15 minutes)."
    );
    assert_eq!(
        prompt.notices[1],
        "That code is wrong, expired or already used."
    );
    assert_eq!(prompt.notices[2], prompt.notices[1]);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 5, "one code mailed, three refused: {seen:#?}");
    // Nothing half-set-up blocks the rerun the error asks for.
    assert!(load_identity(store.database()).unwrap().is_none());
}

#[tokio::test]
async fn an_empty_line_mails_a_new_code() {
    let (url, seen) = mock_server(|i| match i {
        0 => (403, verification_required()),
        1 | 2 => (200, code_sent(60)),
        _ => (200, account_created()),
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let store = local_store(&dir);
    let mut prompt = ScriptedPrompt::new(true, &[None, Some("424242")]);

    init_interactive(
        &store,
        &url,
        "ana@example.com",
        "laptop",
        "a long passphrase",
        None,
        None,
        &mut prompt,
    )
    .await
    .unwrap();

    assert_eq!(prompt.notices.len(), 2);
    assert!(
        prompt
            .notices
            .iter()
            .all(|n| n.ends_with("(valid 1 minute).")),
        "{:?}",
        prompt.notices
    );
    let seen = seen.lock().unwrap();
    let mailed = seen
        .iter()
        .filter(|h| h.starts_with("POST /v1/accounts/email-code HTTP/1.1"))
        .count();
    assert_eq!(mailed, 2, "the empty line mailed one more: {seen:#?}");
}

// ── Audit verification and device revocation ────────────────────────────

use bssh_teams_proto::api::{AuditEvent, AuditRecord};
use bssh_teams_proto::layout::{audit_event_hash, AUDIT_GENESIS};

const MY_DEVICE: u128 = 6;

/// An unlocked admin session on `url`, with its identity and team stored.
fn admin_session<'a>(
    store: &'a TeamsStore,
    config: &'a AppConfig,
    keys: &Arc<DeviceKeys>,
    url: &str,
    team_id: Uuid,
) -> Session<'a> {
    store
        .database()
        .conn
        .execute(
            "INSERT INTO teams_identity (id, server_url, email, account_id, device_id, created_at)
             VALUES (1, ?1, 'ana@example.com', ?2, ?3, '2026-01-01T00:00:00Z')",
            params![
                url,
                Uuid::from_u128(5).to_string(),
                Uuid::from_u128(MY_DEVICE).to_string()
            ],
        )
        .unwrap();
    store
        .save_sealed_device_keys(
            Uuid::from_u128(MY_DEVICE),
            &keys.seal("pw pw pw pw").unwrap(),
        )
        .unwrap();
    store
        .upsert_team(&TeamLocal {
            team_id,
            server_url: url.to_string(),
            account_id: Uuid::from_u128(5),
            device_id: Uuid::from_u128(MY_DEVICE),
            trust_root_ed25519: keys.ed25519_public(),
            last_pulled_revision: 0,
            created_at: Utc::now(),
        })
        .unwrap();
    Session::with_keys(store, config, keys.clone()).unwrap()
}

fn json_challenge() -> String {
    serde_json::to_string(&bssh_teams_proto::api::ChallengeResponse {
        challenge_id: Uuid::new_v4(),
        nonce: Fixed([1u8; 32]),
        expires_at: Utc::now() + chrono::Duration::seconds(60),
    })
    .unwrap()
}

fn json_token() -> String {
    serde_json::to_string(&bssh_teams_proto::api::TokenResponse {
        access_token: "t".into(),
        expires_at: Utc::now() + chrono::Duration::seconds(3600),
    })
    .unwrap()
}

/// `n` correctly signed and chained events of `keys`' device.
fn signed_chain(team: Uuid, device: Uuid, keys: &DeviceKeys, n: u64) -> Vec<AuditRecord> {
    let mut prev = AUDIT_GENESIS;
    (0..n)
        .map(|seq| {
            let mut event = AuditEvent {
                event_id: Uuid::new_v4(),
                team_id: team,
                device_id: device,
                seq,
                kind: AuditKind::SessionStart,
                entry_id: None,
                occurred_at: DateTime::from_timestamp(1_700_000_000 + seq as i64, 0).unwrap(),
                prev_hash: prev,
                signature: Fixed::ZERO,
                actor: None,
                detail: None,
            };
            crypto::sign_audit_event(&mut event, keys);
            prev = audit_event_hash(&event);
            AuditRecord {
                event,
                received_at: DateTime::from_timestamp(1_700_000_100 + seq as i64, 0).unwrap(),
                client_ip: None,
            }
        })
        .collect()
}

fn admin_team_json(team_id: Uuid) -> String {
    serde_json::to_string(&TeamInfo {
        team_id,
        seats: 5,
        member_count: 1,
        current_key_version: 1,
        rotation_required: false,
        my_role: Role::Admin,
        created_at: Utc::now(),
        org_id: None,
    })
    .unwrap()
}

fn admin_members_json(keys: &DeviceKeys) -> String {
    serde_json::to_string(&vec![Member {
        member_id: Uuid::from_u128(1),
        account_id: Uuid::from_u128(5),
        email: "ana@example.com".into(),
        role: Role::Admin,
        kind: MemberKind::Human,
        devices: vec![DeviceInfo {
            device_id: Uuid::from_u128(MY_DEVICE),
            account_id: Uuid::from_u128(5),
            name: "laptop".into(),
            x25519_public: keys.x25519_public(),
            ed25519_public: keys.ed25519_public(),
            created_at: Utc::now(),
            revoked_at: None,
        }],
        joined_at: Utc::now(),
    }])
    .unwrap()
}

#[tokio::test]
async fn audit_verifies_the_whole_log_whatever_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let config = AppConfig {
        database_path: dir.path().join("bssh.db"),
        ..AppConfig::default()
    };
    let store = TeamsStore::open(&config).unwrap();
    let team_id = Uuid::from_u128(9);
    let device = Uuid::from_u128(MY_DEVICE);
    // The admin's own keys sign the log, so build the fixture around them.
    let keys = Arc::new(DeviceKeys::generate());
    let log = signed_chain(team_id, device, &keys, 5);
    let newest: Vec<_> = log.iter().rev().take(2).cloned().collect();
    // The server lost event 2 of the chain, far behind the listed window.
    let mut chain_page = log.clone();
    chain_page.remove(2);
    let (listing, chain) = (
        serde_json::to_string(&newest).unwrap(),
        serde_json::to_string(&chain_page).unwrap(),
    );
    let (team, members) = (admin_team_json(team_id), admin_members_json(&keys));
    let (url, seen) = mock_server(move |i| match i {
        0 => (200, json_challenge()),
        1 => (200, json_token()),
        2 => (200, team.clone()),
        3 => (200, members.clone()),
        4 => (200, "[]".to_string()), // keys: no team key here, hosts stay unnamed
        5 => (200, listing.clone()),
        _ => (200, chain.clone()),
    })
    .await;
    let s = admin_session(&store, &config, &keys, &url, team_id);

    let audit = s
        .audit(
            team_id,
            AuditQuery {
                limit: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(audit.lines.len(), 2, "the listing honors the limit");
    assert!(!audit.complete, "and says it stopped there");
    assert!(
        !audit.is_clean(),
        "the deletion is found: {:?}",
        audit.problems
    );
    assert_eq!(
        audit.problems,
        vec![format!("device {device}: events 2 to 2 are missing")]
    );
    assert_eq!(audit.verified_events, 4);
    assert_eq!(audit.chains.len(), 1);

    let seen = seen.lock().unwrap();
    assert!(seen[5].contains("limit=2"), "{}", seen[5]);
    assert!(!seen[5].contains("order="), "{}", seen[5]);
    assert!(seen[6].contains("order=chain"), "{}", seen[6]);
    assert!(seen[6].contains("limit=1000"), "{}", seen[6]);
    assert!(!seen[6].contains("since="), "no filter on the chain read");
}

#[tokio::test]
async fn revoking_this_device_removes_the_local_identity() {
    let dir = tempfile::tempdir().unwrap();
    let config = AppConfig {
        database_path: dir.path().join("bssh.db"),
        ..AppConfig::default()
    };
    let store = TeamsStore::open(&config).unwrap();
    let not_found = r#"{"code":"not_found","message":"no such device"}"#.to_string();
    let (url, seen) = mock_server(move |i| match i {
        0 => (200, json_challenge()),
        1 => (200, json_token()),
        2 => (404, not_found.clone()),
        _ => (204, String::new()),
    })
    .await;
    let keys = Arc::new(DeviceKeys::generate());
    let s = admin_session(&store, &config, &keys, &url, Uuid::from_u128(9));

    // A device the caller can't see: a plain message, identity untouched.
    let other = Uuid::from_u128(77);
    let err = s.revoke_device(other).await.unwrap_err().to_string();
    assert!(
        err.contains("no device") && err.contains("you can revoke"),
        "{err}"
    );
    assert!(load_identity(store.database()).unwrap().is_some());

    // This machine's own device: revoked, then forgotten here.
    assert!(s.revoke_device(Uuid::from_u128(MY_DEVICE)).await.unwrap());
    assert!(load_identity(store.database()).unwrap().is_none());
    assert!(store
        .sealed_device_keys(Uuid::from_u128(MY_DEVICE))
        .unwrap()
        .is_none());
    let seen = seen.lock().unwrap();
    assert!(
        seen[2].starts_with(&format!("DELETE /v1/devices/{other} ")),
        "{}",
        seen[2]
    );
    assert!(
        seen[3].starts_with(&format!(
            "DELETE /v1/devices/{} ",
            Uuid::from_u128(MY_DEVICE)
        )),
        "{}",
        seen[3]
    );
}
