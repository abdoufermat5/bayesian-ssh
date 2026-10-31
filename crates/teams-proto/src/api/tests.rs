//! Serialization and wire-compatibility tests for the API types.

use super::*;
use crate::wire::Fixed;
use uuid::Uuid;

#[test]
fn error_codes_are_snake_case() {
    let e = ApiError {
        code: ErrorCode::RotationRequired,
        message: "x".into(),
    };
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("\"rotation_required\""), "{json}");
}

#[test]
fn vault_payload_is_tagged() {
    let p = VaultPayload::Tombstone;
    assert_eq!(
        serde_json::to_string(&p).unwrap(),
        r#"{"type":"tombstone"}"#
    );
    let c: VaultPayload = serde_json::from_str(
        r#"{"type":"connection","name":"db","host":"10.0.0.5","user":"ops","port":22}"#,
    )
    .unwrap();
    assert!(matches!(c, VaultPayload::Connection(ref r) if r.tags.is_empty()));
}

#[test]
fn invite_token_is_optional_on_the_wire() {
    let req = CreateAccountRequest {
        email: "ana@example.com".into(),
        device: NewDevice {
            name: "laptop".into(),
            x25519_public: Fixed::ZERO,
            ed25519_public: Fixed::ZERO,
        },
        invite_token: None,
        email_code: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("invite_token"), "{json}");
    assert!(!json.contains("email_code"), "{json}");
    // A request from an older client still parses.
    let old: CreateAccountRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(old.invite_token, None);
    assert_eq!(old.email_code, None);
}

#[test]
fn invites_from_older_servers_are_not_emailed() {
    let json = r#"{"invite_id":"00000000-0000-0000-0000-000000000001",
        "team_id":"00000000-0000-0000-0000-000000000002","email":"ana@example.com",
        "role":"member","expires_at":"2026-10-25T00:00:00Z","token":"t"}"#;
    let invite: Invite = serde_json::from_str(json).unwrap();
    assert!(!invite.emailed);
}

#[test]
fn email_verification_codes_round_trip() {
    for code in [
        ErrorCode::EmailVerificationRequired,
        ErrorCode::InvalidEmailCode,
    ] {
        let json = serde_json::to_string(&code).unwrap();
        assert_eq!(serde_json::from_str::<ErrorCode>(&json).unwrap(), code);
    }
}

#[test]
fn unknown_error_codes_still_parse() {
    let e: ApiError = serde_json::from_str(r#"{"code":"something_new","message":"x"}"#).unwrap();
    assert_eq!(e.code, ErrorCode::Unknown);
    let e: ApiError = serde_json::from_str(r#"{"code":"payment_required","message":"x"}"#).unwrap();
    assert_eq!(e.code, ErrorCode::PaymentRequired);
}

#[test]
fn audit_quota_code_round_trips() {
    let json = serde_json::to_string(&ErrorCode::AuditQuotaExceeded).unwrap();
    assert_eq!(json, "\"audit_quota_exceeded\"");
    assert_eq!(
        serde_json::from_str::<ErrorCode>(&json).unwrap(),
        ErrorCode::AuditQuotaExceeded
    );
}

fn record(device: u8, seq: u64, received_secs: i64) -> AuditRecord {
    AuditRecord {
        event: AuditEvent {
            event_id: Uuid::from_bytes([9; 16]),
            team_id: Uuid::from_bytes([1; 16]),
            device_id: Uuid::from_bytes([device; 16]),
            seq,
            kind: AuditKind::SessionStart,
            entry_id: None,
            occurred_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            prev_hash: Fixed::ZERO,
            signature: Fixed::ZERO,
            actor: None,
            detail: None,
        },
        received_at: chrono::DateTime::from_timestamp(received_secs, 0).unwrap(),
        client_ip: None,
    }
}

#[test]
fn audit_order_is_lowercase_and_defaults_to_newest() {
    assert_eq!(
        serde_json::to_string(&AuditOrder::Chain).unwrap(),
        "\"chain\""
    );
    assert_eq!(
        serde_json::from_str::<AuditOrder>("\"newest\"").unwrap(),
        AuditOrder::Newest
    );
    assert_eq!(AuditOrder::default(), AuditOrder::Newest);
}

#[test]
fn empty_audit_query_serializes_to_nothing() {
    let json = serde_json::to_string(&AuditListQuery::default()).unwrap();
    assert_eq!(json, "{}");
    let q: AuditListQuery = serde_json::from_str(r#"{"kind":"vault_read","limit":5}"#).unwrap();
    assert_eq!(q.kind, Some(AuditKind::VaultRead));
    assert_eq!(q.limit, Some(5));
}

#[test]
fn after_sets_the_newest_cursor_and_keeps_filters() {
    let q = AuditListQuery {
        actor: Some("ci".into()),
        limit: Some(50),
        after_device_id: Some(Uuid::nil()),
        after_seq: Some(3),
        ..Default::default()
    };
    let next = q.after(&record(4, 7, 1_700_000_500));
    assert_eq!(next.actor.as_deref(), Some("ci"));
    assert_eq!(next.limit, Some(50));
    assert_eq!(next.before_device_id, Some(Uuid::from_bytes([4; 16])));
    assert_eq!(next.before_seq, Some(7));
    assert_eq!(
        next.before_received_at,
        chrono::DateTime::from_timestamp(1_700_000_500, 0)
    );
    assert_eq!((next.after_device_id, next.after_seq), (None, None));
}

#[test]
fn after_sets_the_chain_cursor_in_chain_order() {
    let q = AuditListQuery {
        order: Some(AuditOrder::Chain),
        device_id: Some(Uuid::from_bytes([4; 16])),
        limit: Some(1000),
        before_seq: Some(1),
        ..Default::default()
    };
    let next = q.after(&record(4, 7, 1_700_000_500));
    assert_eq!(next.order, Some(AuditOrder::Chain));
    assert_eq!(next.device_id, Some(Uuid::from_bytes([4; 16])));
    assert_eq!(next.after_device_id, Some(Uuid::from_bytes([4; 16])));
    assert_eq!(next.after_seq, Some(7));
    assert_eq!(
        (
            next.before_received_at,
            next.before_device_id,
            next.before_seq
        ),
        (None, None, None)
    );
}
