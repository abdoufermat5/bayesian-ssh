use super::audit::audit_query_pairs;
use super::error::org_route;
use super::http::TeamsClient;
use super::mock::mock_server;
use super::*;
use bssh_teams_proto::api::{
    ApiError, AuditKind, AuditListQuery, AuditOrder, AuditRecord, ChallengeResponse,
    CreateAccountRequest, EmailCodeRequest, EmailCodeResponse, ErrorCode, NewDevice, OrgInfo,
    OrgOwner, OrgRole, TokenResponse,
};
use bssh_teams_proto::wire::{PublicKey32, Signature64};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::sync::Arc;
use uuid::Uuid;

use bssh_teams_proto::wire::Fixed;

struct FakeSigner;
impl AuthSigner for FakeSigner {
    fn sign_challenge(&self, _: Uuid, _: Uuid, _: &PublicKey32) -> Signature64 {
        Fixed([9u8; 64])
    }
}

#[test]
fn url_policy() {
    for ok in [
        "https://teams.example.com",
        "https://teams.example.com/base/",
        "http://localhost:8080",
        "http://127.0.0.1:9000",
        "http://[::1]:9000",
    ] {
        assert!(validate_server_url(ok).is_ok(), "{ok}");
    }
    for bad in [
        "http://teams.example.com",
        "http://localhost.evil.com",
        "http://127.0.0.1.evil.com",
        "http://127.0.0.1@evil.com",
        "http://10.0.0.5",
        "ftp://localhost",
        "https://user:pw@teams.example.com",
        "not a url",
    ] {
        assert!(validate_server_url(bad).is_err(), "{bad}");
    }
    assert!(matches!(
        validate_server_url("http://teams.example.com"),
        Err(TeamsError::InsecureUrl(_))
    ));
}

#[test]
fn client_refuses_plain_http_to_remote_host() {
    let r = TeamsClient::new(
        "http://teams.example.com",
        Uuid::new_v4(),
        Arc::new(FakeSigner),
    );
    assert!(matches!(r, Err(TeamsError::InsecureUrl(_))));
}

fn challenge_json() -> String {
    serde_json::to_string(&ChallengeResponse {
        challenge_id: Uuid::new_v4(),
        nonce: Fixed([1u8; 32]),
        expires_at: Utc::now() + ChronoDuration::seconds(60),
    })
    .unwrap()
}

fn token_json(tok: &str, ttl_secs: i64) -> String {
    serde_json::to_string(&TokenResponse {
        access_token: tok.into(),
        expires_at: Utc::now() + ChronoDuration::seconds(ttl_secs),
    })
    .unwrap()
}

fn record(device: u8, seq: u64, received_secs: i64) -> AuditRecord {
    use bssh_teams_proto::api::{AuditEvent, AuditKind};
    AuditRecord {
        event: AuditEvent {
            event_id: Uuid::new_v4(),
            team_id: Uuid::from_bytes([1; 16]),
            device_id: Uuid::from_bytes([device; 16]),
            seq,
            kind: AuditKind::SessionStart,
            entry_id: None,
            occurred_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            prev_hash: Fixed::ZERO,
            signature: Fixed::ZERO,
            actor: None,
            detail: None,
        },
        received_at: DateTime::from_timestamp(received_secs, 0).unwrap(),
        client_ip: None,
    }
}

/// Drive [`walk_audit_pages`] over `pages` (one per request), returning
/// the queries it sent, the records handed to `each` and how it ended.
async fn walk(
    first: AuditListQuery,
    pages: Vec<Vec<AuditRecord>>,
    stop_after: Option<usize>,
) -> (Vec<AuditListQuery>, usize, AuditWalk) {
    let sent = std::cell::RefCell::new(Vec::new());
    let calls = std::cell::Cell::new(0usize);
    let mut got = 0;
    let end = walk_audit_pages(
        first,
        |q| {
            sent.borrow_mut().push(q);
            let i = calls.get();
            calls.set(i + 1);
            // A server that has no more pages repeats nothing: empty.
            let page = pages.get(i).cloned().unwrap_or_default();
            async move { Ok(page) }
        },
        |page| {
            got += page.len();
            stop_after.is_none_or(|n| got < n)
        },
    )
    .await
    .unwrap();
    (sent.into_inner(), got, end)
}

#[tokio::test]
async fn chain_walk_follows_the_cursor_and_stops_on_a_short_page() {
    let first = AuditListQuery {
        order: Some(AuditOrder::Chain),
        limit: Some(2),
        ..Default::default()
    };
    let pages = vec![
        vec![record(1, 0, 10), record(1, 1, 11)],
        vec![record(1, 2, 12), record(2, 0, 13)],
        vec![record(2, 1, 14)],
        vec![record(9, 9, 99)], // never asked for
    ];
    let (sent, got, end) = walk(first, pages, None).await;
    assert_eq!((got, end), (5, AuditWalk::Finished));
    assert_eq!(sent.len(), 3);
    assert_eq!((sent[0].after_device_id, sent[0].after_seq), (None, None));
    assert_eq!(sent[1].after_device_id, Some(Uuid::from_bytes([1; 16])));
    assert_eq!(sent[1].after_seq, Some(1));
    assert_eq!(sent[2].after_device_id, Some(Uuid::from_bytes([2; 16])));
    assert_eq!(sent[2].after_seq, Some(0));
    assert!(sent.iter().all(|q| q.limit == Some(2)));
}

#[tokio::test]
async fn full_last_page_costs_one_empty_request_then_stops() {
    let first = AuditListQuery {
        limit: Some(2),
        ..Default::default()
    };
    let pages = vec![vec![record(1, 5, 20), record(1, 4, 19)]];
    let (sent, got, end) = walk(first, pages, None).await;
    assert_eq!((got, end, sent.len()), (2, AuditWalk::Finished, 2));
    assert_eq!(sent[1].before_seq, Some(4));
    assert_eq!(sent[1].before_received_at, DateTime::from_timestamp(19, 0));
}

#[tokio::test]
async fn newest_walk_stops_when_a_page_does_not_advance() {
    // An old server ignores the cursor and sends the first page again.
    let first = AuditListQuery {
        limit: Some(2),
        ..Default::default()
    };
    let page = vec![record(1, 5, 20), record(1, 4, 19)];
    let (sent, got, end) = walk(first, vec![page.clone(), page.clone(), page], None).await;
    assert_eq!(end, AuditWalk::Stalled);
    assert_eq!(sent.len(), 2, "asks once more, then gives up");
    assert_eq!(got, 2, "the repeated page is not handed over twice");
}

#[tokio::test]
async fn chain_walk_stops_when_a_page_does_not_advance() {
    let first = AuditListQuery {
        order: Some(AuditOrder::Chain),
        limit: Some(2),
        ..Default::default()
    };
    let page = vec![record(1, 0, 10), record(1, 1, 11)];
    let (sent, got, end) = walk(first, vec![page.clone(), page], None).await;
    assert_eq!((got, end, sent.len()), (2, AuditWalk::Stalled, 2));
}

#[tokio::test]
async fn walk_stops_when_the_caller_has_enough() {
    let first = AuditListQuery {
        limit: Some(2),
        ..Default::default()
    };
    let pages = vec![
        vec![record(1, 5, 20), record(1, 4, 19)],
        vec![record(1, 3, 18), record(1, 2, 17)],
    ];
    let (sent, got, end) = walk(first, pages, Some(2)).await;
    assert_eq!((got, end, sent.len()), (2, AuditWalk::Stopped, 1));
}

#[test]
fn audit_query_is_sent_as_the_protocol_names_it() {
    let device = Uuid::from_bytes([4; 16]);
    let q = AuditListQuery {
        kind: Some(AuditKind::VaultRead),
        actor: Some("ci bot".into()),
        limit: Some(50),
        order: Some(AuditOrder::Newest),
        before_received_at: DateTime::from_timestamp(1_700_000_000, 0),
        before_device_id: Some(device),
        before_seq: Some(7),
        ..Default::default()
    };
    let mut pairs = audit_query_pairs(&q).unwrap();
    pairs.sort();
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("kind"), Some("vault_read"));
    assert_eq!(get("actor"), Some("ci bot"));
    assert_eq!(get("limit"), Some("50"));
    assert_eq!(get("order"), Some("newest"));
    assert_eq!(get("before_seq"), Some("7"));
    assert_eq!(get("before_device_id"), Some(device.to_string().as_str()));
    assert_eq!(get("before_received_at"), Some("2023-11-14T22:13:20Z"));
    assert_eq!(pairs.len(), 7, "unset fields are not sent: {pairs:?}");
}

#[tokio::test]
async fn revoke_device_sends_delete_and_quota_is_not_transient() {
    let device = Uuid::from_u128(5);
    let quota = serde_json::to_string(&ApiError {
        code: ErrorCode::AuditQuotaExceeded,
        message: "audit storage is full".into(),
    })
    .unwrap();
    let (url, seen) = mock_server(move |i| match i {
        0 => (200, challenge_json()),
        1 => (200, token_json("t", 3600)),
        2 => (204, String::new()),
        _ => (403, quota.clone()),
    })
    .await;
    let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();
    client.revoke_device(device).await.unwrap();
    let err = client
        .list_audit(Uuid::nil(), &AuditListQuery::default())
        .await
        .unwrap_err();
    assert!(
        err.is_audit_quota_exceeded() && !err.is_transient(),
        "{err}"
    );
    let seen = seen.lock().unwrap();
    assert!(
        seen[2].starts_with("DELETE /v1/devices/00000000-0000-0000-0000-000000000005 HTTP/1.1"),
        "{}",
        seen[2]
    );
    assert!(
        seen[3].starts_with("GET /v1/teams/00000000-0000-0000-0000-000000000000/audit HTTP/1.1"),
        "{}",
        seen[3]
    );
}

#[test]
fn org_routes_on_legacy_servers() {
    assert!(matches!(
        org_route(TeamsError::Http {
            status: 404,
            body: "no route".into()
        }),
        TeamsError::OrgUnsupported
    ));
    assert!(matches!(
        org_route(TeamsError::Http {
            status: 405,
            body: String::new()
        }),
        TeamsError::OrgUnsupported
    ));
    // A real API error (with its code) is kept: it says something useful.
    let api = TeamsError::Api {
        status: 403,
        error: ApiError {
            code: ErrorCode::Forbidden,
            message: "not an owner".into(),
        },
    };
    assert!(matches!(org_route(api), TeamsError::Api { .. }));
    // A JSON 404 (route missing behind a fallback) is the same as a bare one.
    let api_404 = TeamsError::Api {
        status: 404,
        error: ApiError {
            code: ErrorCode::NotFound,
            message: "no route".into(),
        },
    };
    assert!(matches!(org_route(api_404), TeamsError::OrgUnsupported));
}

#[tokio::test]
async fn org_calls_use_org_routes() {
    let org = OrgInfo {
        org_id: Uuid::from_u128(7),
        my_role: OrgRole::Owner,
        owners: vec![OrgOwner {
            account_id: Uuid::from_u128(8),
            email: "ana@example.com".into(),
        }],
        team_ids: vec![],
        created_at: Utc::now(),
    };
    let org_json = serde_json::to_string(&org).unwrap();
    let owner_json = org_json.clone();
    let (url, seen) = mock_server(move |i| match i {
        0 => (200, challenge_json()),
        1 => (200, token_json("t", 3600)),
        2 => (200, format!("[{org_json}]")),
        3 => (200, owner_json.clone()),
        4 => (
            404,
            "{\"code\":\"not_found\",\"message\":\"no route\"}".into(),
        ),
        _ => (200, "{}".to_string()),
    })
    .await;
    let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();

    assert_eq!(client.list_orgs().await.unwrap()[0].org_id, org.org_id);
    assert_eq!(
        client
            .add_org_owner(org.org_id, "bo@example.com")
            .await
            .unwrap(),
        org
    );
    let err = client.org(org.org_id).await.unwrap_err();
    assert!(matches!(err, TeamsError::OrgUnsupported), "{err}");

    let seen = seen.lock().unwrap();
    assert!(seen[2].starts_with("GET /v1/orgs HTTP/1.1"), "{}", seen[2]);
    assert!(
        seen[3].starts_with("POST /v1/orgs/00000000-0000-0000-0000-000000000007/owners HTTP/1.1"),
        "{}",
        seen[3]
    );
    // The mock records headers only: the body's length proves the request.
    assert!(seen[3].contains("content-length: 26"), "{}", seen[3]);
    assert!(
        seen[4].starts_with("GET /v1/orgs/00000000-0000-0000-0000-000000000007 HTTP/1.1"),
        "{}",
        seen[4]
    );
}

#[test]
fn billing_errors_show_the_server_message_as_is() {
    for code in [
        ErrorCode::PaymentRequired,
        ErrorCode::SeatLimit,
        ErrorCode::Internal,
    ] {
        let err = TeamsError::Api {
            status: 402,
            error: ApiError {
                code,
                message: "the team has no free seat; ask a team admin".into(),
            },
        };
        assert_eq!(
            err.to_string(),
            "the team has no free seat; ask a team admin"
        );
        assert_eq!(
            crate::services::teams::session::explain(&anyhow::Error::new(err)),
            None,
            "{code:?}: no hint replaces the server's text"
        );
    }
    let other = TeamsError::Api {
        status: 409,
        error: ApiError {
            code: ErrorCode::Conflict,
            message: "taken".into(),
        },
    };
    assert!(other.to_string().ends_with(": taken"), "{other}");
}

#[tokio::test]
async fn headers_auth_refresh_on_401_and_api_errors() {
    // Sequence: challenge, token(t1), GET -> 401, challenge, token(t2),
    // GET -> 200, GET -> 409 rotation_required.
    let rot = serde_json::to_string(&ApiError {
        code: ErrorCode::RotationRequired,
        message: "rotate".into(),
    })
    .unwrap();
    let (url, seen) = mock_server(move |i| match i {
        0 | 3 => (200, challenge_json()),
        1 => (200, token_json("t1", 3600)),
        2 => (
            401,
            "{\"code\":\"unauthorized\",\"message\":\"expired\"}".into(),
        ),
        4 => (200, token_json("t2", 3600)),
        5 => (200, "[]".to_string()),
        _ => (409, rot.clone()),
    })
    .await;
    let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();

    assert!(client.list_teams().await.unwrap().is_empty());
    // Token cached: no new challenge for the next call, which errors typed.
    let err = client.team(Uuid::new_v4()).await.unwrap_err();
    assert!(err.is_rotation_required(), "{err}");

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 7, "{seen:#?}");
    for head in seen.iter() {
        let lower = head.to_ascii_lowercase();
        assert!(lower.contains("bssh-protocol: 1"), "{head}");
        assert!(
            lower.contains(&format!(
                "bssh-client-version: {}",
                env!("CARGO_PKG_VERSION")
            )),
            "{head}"
        );
    }
    assert!(seen[2].to_ascii_lowercase().contains("bearer t1"));
    assert!(seen[5].to_ascii_lowercase().contains("bearer t2"));
    assert!(seen[6].to_ascii_lowercase().contains("bearer t2"));
    assert!(!seen[0].to_ascii_lowercase().contains("authorization"));
}

#[tokio::test]
async fn token_near_expiry_is_refreshed() {
    let (url, seen) = mock_server(|i| match i {
        0 | 3 => (200, challenge_json()),
        1 => (200, token_json("short", 30)), // inside the skew window
        4 => (200, token_json("fresh", 3600)),
        _ => (200, "[]".into()),
    })
    .await;
    let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();
    client.list_teams().await.unwrap();
    client.list_teams().await.unwrap();
    let seen = seen.lock().unwrap();
    assert!(
        seen[5].to_ascii_lowercase().contains("bearer fresh"),
        "{seen:#?}"
    );
}

#[tokio::test]
async fn email_code_request_is_unauthenticated_json() {
    let answer = serde_json::to_string(&EmailCodeResponse {
        expires_in_secs: 900,
    })
    .unwrap();
    let (url, seen) = mock_server(move |_| (200, answer.clone())).await;
    let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();

    let req = EmailCodeRequest {
        email: "ana@example.com".into(),
    };
    let resp = client.request_email_code(&req).await.unwrap();
    assert_eq!(resp.expires_in_secs, 900);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "one request, no auth handshake: {seen:#?}");
    assert!(
        seen[0].starts_with("POST /v1/accounts/email-code HTTP/1.1"),
        "{}",
        seen[0]
    );
    assert!(!seen[0].to_ascii_lowercase().contains("authorization"));
    let body = serde_json::to_string(&req).unwrap();
    assert!(seen[0].contains(&body), "{}", seen[0]);
}

#[tokio::test]
async fn email_verification_errors_keep_their_code() {
    let required = serde_json::to_string(&ApiError {
        code: ErrorCode::EmailVerificationRequired,
        message: "verify your email first".into(),
    })
    .unwrap();
    let invalid = serde_json::to_string(&ApiError {
        code: ErrorCode::InvalidEmailCode,
        message: "that code is wrong or expired".into(),
    })
    .unwrap();
    let (url, _seen) = mock_server(move |i| match i {
        0 => (403, required.clone()),
        _ => (403, invalid.clone()),
    })
    .await;
    let client = TeamsClient::new(&url, Uuid::new_v4(), Arc::new(FakeSigner)).unwrap();
    let req = CreateAccountRequest {
        email: "ana@example.com".into(),
        device: NewDevice {
            name: "laptop".into(),
            x25519_public: Fixed([1u8; 32]),
            ed25519_public: Fixed([2u8; 32]),
        },
        invite_token: None,
        email_code: None,
    };

    let err = client.create_account(&req).await.unwrap_err();
    assert!(err.is_email_verification_required(), "{err}");
    assert!(!err.is_invalid_email_code(), "{err}");
    assert_eq!(err.code(), Some(ErrorCode::EmailVerificationRequired));
    // Still distinguishable in what the user reads.
    assert!(
        err.to_string().contains("EmailVerificationRequired"),
        "{err}"
    );

    let err = client.create_account(&req).await.unwrap_err();
    assert!(err.is_invalid_email_code(), "{err}");
    assert!(!err.is_email_verification_required(), "{err}");
    assert!(err.to_string().contains("InvalidEmailCode"), "{err}");
}
