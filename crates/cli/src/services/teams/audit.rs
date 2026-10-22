//! Client-side verification of a team's audit log, as downloaded from the
//! server. Nothing the server says is taken on trust: every event's signature
//! is checked against the author device's key, and each device's events must
//! form an unbroken hash chain (`seq`, `prev_hash`; see PROTOCOL.md "Audit").

use super::crypto;
use bssh_teams_proto::api::AuditRecord;
use bssh_teams_proto::layout::{audit_event_hash, AUDIT_GENESIS};
use bssh_teams_proto::wire::PublicKey32;
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

/// Verdict on one event on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCheck {
    Valid,
    /// The signature doesn't verify: the event was altered after signing.
    BadSignature,
    /// No known key for the device that supposedly wrote it.
    UnknownDevice,
    /// The event belongs to a different team than the one asked for.
    WrongTeam,
}

/// Something wrong with a device's chain of events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainProblem {
    /// Events between `after` and `next` are missing (deleted or withheld).
    Gap {
        device_id: Uuid,
        after: u64,
        next: u64,
    },
    /// Event `seq` doesn't point at the hash of event `seq - 1`.
    BrokenLink { device_id: Uuid, seq: u64 },
    /// The first event (`seq` 0) doesn't start from the genesis hash.
    BadGenesis { device_id: Uuid },
    /// Two different events claim the same `seq`.
    Duplicate { device_id: Uuid, seq: u64 },
}

impl std::fmt::Display for ChainProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainProblem::Gap {
                device_id,
                after,
                next,
            } => write!(
                f,
                "device {device_id}: events {} to {} are missing",
                after + 1,
                next - 1
            ),
            ChainProblem::BrokenLink { device_id, seq } => {
                write!(
                    f,
                    "device {device_id}: event {seq} doesn't link to event {}",
                    seq - 1
                )
            }
            ChainProblem::BadGenesis { device_id } => {
                write!(
                    f,
                    "device {device_id}: the first event doesn't start the chain"
                )
            }
            ChainProblem::Duplicate { device_id, seq } => {
                write!(
                    f,
                    "device {device_id}: two different events claim number {seq}"
                )
            }
        }
    }
}

/// One device's chain as seen in the downloaded log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainSummary {
    pub device_id: Uuid,
    pub events: usize,
    pub first_seq: u64,
    pub last_seq: u64,
}

impl ChainSummary {
    /// The window starts at the device's first event ever, so the whole chain
    /// up to `last_seq` was checked.
    pub fn from_genesis(&self) -> bool {
        self.first_seq == 0
    }
}

#[derive(Debug, Clone)]
pub struct CheckedRecord {
    pub record: AuditRecord,
    pub check: EventCheck,
}

#[derive(Debug, Clone)]
pub struct AuditVerification {
    /// In the order received (newest first).
    pub records: Vec<CheckedRecord>,
    pub chains: Vec<ChainSummary>,
    pub problems: Vec<ChainProblem>,
}

impl AuditVerification {
    pub fn invalid_events(&self) -> usize {
        self.records
            .iter()
            .filter(|r| r.check != EventCheck::Valid)
            .count()
    }

    /// Every event verified and every chain intact.
    pub fn is_clean(&self) -> bool {
        self.invalid_events() == 0 && self.problems.is_empty()
    }
}

/// Verify `records` for `team_id` against known device keys.
///
/// `filtered` says the log was narrowed (by host, device or time), so missing
/// sequence numbers are expected and not reported as gaps; links between
/// consecutive events are still checked.
pub fn verify(
    team_id: Uuid,
    records: Vec<AuditRecord>,
    authors: &HashMap<Uuid, PublicKey32>,
    filtered: bool,
) -> AuditVerification {
    let records: Vec<CheckedRecord> = records
        .into_iter()
        .map(|record| {
            let event = &record.event;
            let check = if event.team_id != team_id {
                EventCheck::WrongTeam
            } else {
                match authors.get(&event.device_id) {
                    None => EventCheck::UnknownDevice,
                    Some(key) => match crypto::verify_audit_event(event, key) {
                        Ok(()) => EventCheck::Valid,
                        Err(_) => EventCheck::BadSignature,
                    },
                }
            };
            CheckedRecord { record, check }
        })
        .collect();

    // Chains are only meaningful over events that verified on their own.
    let mut by_device: BTreeMap<Uuid, Vec<&CheckedRecord>> = BTreeMap::new();
    for r in records.iter().filter(|r| r.check == EventCheck::Valid) {
        by_device
            .entry(r.record.event.device_id)
            .or_default()
            .push(r);
    }

    let mut chains = Vec::new();
    let mut problems = Vec::new();
    for (device_id, mut events) in by_device {
        events.sort_by_key(|r| r.record.event.seq);
        let first = &events[0].record.event;
        if first.seq == 0 && first.prev_hash != AUDIT_GENESIS {
            problems.push(ChainProblem::BadGenesis { device_id });
        }
        for pair in events.windows(2) {
            let (prev, next) = (&pair[0].record.event, &pair[1].record.event);
            if next.seq == prev.seq {
                if next != prev {
                    problems.push(ChainProblem::Duplicate {
                        device_id,
                        seq: next.seq,
                    });
                }
            } else if next.seq == prev.seq + 1 {
                if next.prev_hash != audit_event_hash(prev) {
                    problems.push(ChainProblem::BrokenLink {
                        device_id,
                        seq: next.seq,
                    });
                }
            } else if !filtered {
                problems.push(ChainProblem::Gap {
                    device_id,
                    after: prev.seq,
                    next: next.seq,
                });
            }
        }
        chains.push(ChainSummary {
            device_id,
            events: events.len(),
            first_seq: first.seq,
            last_seq: events[events.len() - 1].record.event.seq,
        });
    }

    AuditVerification {
        records,
        chains,
        problems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::teams::crypto::{sign_audit_event, DeviceKeys};
    use bssh_teams_proto::api::{AuditEvent, AuditKind};
    use bssh_teams_proto::wire::Fixed;
    use chrono::{TimeZone, Utc};

    struct Device {
        id: Uuid,
        keys: DeviceKeys,
    }

    fn device() -> Device {
        Device {
            id: Uuid::new_v4(),
            keys: DeviceKeys::generate(),
        }
    }

    /// A correctly signed, correctly chained run of `n` events.
    fn chain(team: Uuid, d: &Device, n: u64) -> Vec<AuditRecord> {
        let mut prev = AUDIT_GENESIS;
        (0..n)
            .map(|seq| {
                let mut event = AuditEvent {
                    event_id: Uuid::new_v4(),
                    team_id: team,
                    device_id: d.id,
                    seq,
                    kind: AuditKind::SessionStart,
                    entry_id: Some(Uuid::new_v4()),
                    occurred_at: Utc
                        .timestamp_millis_opt(1_700_000_000_000 + seq as i64)
                        .unwrap(),
                    prev_hash: prev,
                    signature: Fixed::ZERO,
                };
                sign_audit_event(&mut event, &d.keys);
                prev = audit_event_hash(&event);
                AuditRecord {
                    event,
                    received_at: Utc::now(),
                    client_ip: Some("127.0.0.1".into()),
                }
            })
            .collect()
    }

    fn authors(ds: &[&Device]) -> HashMap<Uuid, PublicKey32> {
        ds.iter().map(|d| (d.id, d.keys.ed25519_public())).collect()
    }

    #[test]
    fn clean_log_verifies() {
        let team = Uuid::new_v4();
        let (a, b) = (device(), device());
        let mut log = chain(team, &a, 3);
        log.extend(chain(team, &b, 2));
        log.reverse(); // newest first, as served
        let v = verify(team, log, &authors(&[&a, &b]), false);
        assert!(v.is_clean(), "{:?}", v.problems);
        assert_eq!(v.chains.len(), 2);
        assert!(v.chains.iter().all(|c| c.from_genesis()));
    }

    #[test]
    fn altered_event_fails_its_signature() {
        let team = Uuid::new_v4();
        let a = device();
        let mut log = chain(team, &a, 2);
        log[1].event.kind = AuditKind::VaultWrite;
        let v = verify(team, log, &authors(&[&a]), false);
        assert_eq!(v.records[1].check, EventCheck::BadSignature);
        assert!(!v.is_clean());
    }

    #[test]
    fn deleted_event_is_a_gap_unless_filtered() {
        let team = Uuid::new_v4();
        let a = device();
        let mut log = chain(team, &a, 4);
        log.remove(2);
        let v = verify(team, log.clone(), &authors(&[&a]), false);
        assert_eq!(
            v.problems,
            vec![ChainProblem::Gap {
                device_id: a.id,
                after: 1,
                next: 3
            }]
        );
        assert!(verify(team, log, &authors(&[&a]), true).is_clean());
    }

    #[test]
    fn fork_breaks_the_link() {
        // A device that signs an event not chained to its previous one (e.g. a
        // rewritten history) is caught even though each signature is valid.
        let team = Uuid::new_v4();
        let a = device();
        let mut log = chain(team, &a, 3);
        log[2].event.prev_hash = Fixed([7; 32]);
        sign_audit_event(&mut log[2].event, &a.keys);
        let v = verify(team, log, &authors(&[&a]), false);
        assert_eq!(
            v.problems,
            vec![ChainProblem::BrokenLink {
                device_id: a.id,
                seq: 2
            }]
        );
    }

    #[test]
    fn unknown_device_and_wrong_team_are_flagged() {
        let team = Uuid::new_v4();
        let (a, stranger) = (device(), device());
        let mut log = chain(team, &a, 1);
        log.extend(chain(team, &stranger, 1));
        log.extend(chain(Uuid::new_v4(), &a, 1));
        let v = verify(team, log, &authors(&[&a]), false);
        let checks: Vec<_> = v.records.iter().map(|r| r.check).collect();
        assert_eq!(
            checks,
            vec![
                EventCheck::Valid,
                EventCheck::UnknownDevice,
                EventCheck::WrongTeam
            ]
        );
        assert_eq!(v.invalid_events(), 2);
    }

    #[test]
    fn bad_genesis_and_duplicates() {
        let team = Uuid::new_v4();
        let a = device();
        let mut log = chain(team, &a, 2);
        log[0].event.prev_hash = Fixed([1; 32]);
        sign_audit_event(&mut log[0].event, &a.keys);
        let mut twin = chain(team, &a, 2)[1].clone();
        twin.event.prev_hash = audit_event_hash(&log[0].event);
        sign_audit_event(&mut twin.event, &a.keys);
        log.push(twin);
        let v = verify(team, log, &authors(&[&a]), false);
        assert!(v
            .problems
            .contains(&ChainProblem::BadGenesis { device_id: a.id }));
        assert!(v.problems.contains(&ChainProblem::Duplicate {
            device_id: a.id,
            seq: 1
        }));
    }

    #[test]
    fn window_not_from_genesis_is_reported_as_such() {
        let team = Uuid::new_v4();
        let a = device();
        let log: Vec<_> = chain(team, &a, 5).into_iter().skip(2).collect();
        let v = verify(team, log, &authors(&[&a]), false);
        assert!(v.is_clean());
        assert!(!v.chains[0].from_genesis());
        assert_eq!((v.chains[0].first_seq, v.chains[0].last_seq), (2, 4));
    }
}
