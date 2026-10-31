//! Wire-compatibility tests: payloads written by older clients must still
//! decode, and newer tags must never reuse older bytes.

use super::*;
use crate::wire::Fixed;
use uuid::Uuid;

// An audit event as written by a pre-AI client (kinds 1-4, no actor/detail).
const OLD_EVENT: &str = r#"{
    "event_id":"06060606-0606-0606-0606-060606060606",
    "team_id":"01010101-0101-0101-0101-010101010101",
    "device_id":"04040404-0404-0404-0404-040404040404",
    "seq":7,"kind":"session_start",
    "entry_id":"02020202-0202-0202-0202-020202020202",
    "occurred_at":"2023-11-14T22:13:20.123Z",
    "prev_hash":"CAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg=",
    "signature":"rxfVq0KLe2evDDwJi3+L3wfJ3elaQEOv9jRqOaTt2WecUc7YuwjcupTSvqKYLo77/Cn4zekMuwpOuOe+Zdm5Cg=="
}"#;

#[test]
fn old_audit_events_decode_and_reserialize_unchanged() {
    let ev: AuditEvent = serde_json::from_str(OLD_EVENT).unwrap();
    assert!(ev.actor.is_none() && ev.detail.is_none());
    let json = serde_json::to_string(&ev).unwrap();
    assert!(
        !json.contains("actor") && !json.contains("detail"),
        "{json}"
    );
    let again: AuditEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(ev, again);
}

#[test]
fn old_tags_are_never_reused() {
    assert_eq!(
        [
            AuditKind::SessionStart,
            AuditKind::SessionEnd,
            AuditKind::VaultRead,
            AuditKind::VaultWrite
        ]
        .map(AuditKind::tag),
        [1, 2, 3, 4]
    );
    assert_eq!(
        [
            AuditKind::AiCall,
            AuditKind::AiApproval,
            AuditKind::AiChange,
            AuditKind::AiRun
        ]
        .map(AuditKind::tag),
        [5, 6, 7, 8]
    );
    for k in [
        AuditKind::SessionStart,
        AuditKind::AiCall,
        AuditKind::AiApproval,
        AuditKind::AiChange,
        AuditKind::AiRun,
    ] {
        assert_eq!(AuditKind::parse(k.as_str()), Some(k));
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, format!("\"{}\"", k.as_str()));
    }
}

#[test]
fn old_vault_payloads_and_members_still_decode() {
    let p: VaultPayload = serde_json::from_str(r#"{"type":"team_profile","name":"ops"}"#).unwrap();
    assert!(matches!(p, VaultPayload::TeamProfile { .. }));
    let m: Member = serde_json::from_str(
        r#"{"member_id":"01010101-0101-0101-0101-010101010101",
            "account_id":"01010101-0101-0101-0101-010101010101",
            "email":"a@b.c","role":"admin","devices":[],
            "joined_at":"2023-11-14T22:13:20Z"}"#,
    )
    .unwrap();
    assert_eq!(m.kind, MemberKind::Human);
}

#[test]
fn agent_policy_payload_is_tagged() {
    let p = VaultPayload::AgentPolicy(SignedAgentPolicy {
        agent_device_id: Uuid::nil(),
        name: "default".into(),
        policy_json: "{}".into(),
        version: 1,
        signed_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        signer_device_id: Uuid::nil(),
        signature: Fixed::ZERO,
    });
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.starts_with(r#"{"type":"agent_policy""#), "{json}");
    assert_eq!(serde_json::from_str::<VaultPayload>(&json).unwrap(), p);
}
