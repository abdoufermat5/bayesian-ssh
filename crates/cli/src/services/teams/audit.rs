//! Client-side verification of a team's audit log, as downloaded from the
//! server. Nothing the server says is taken on trust: every event's signature
//! is checked against the author device's key, and each device's events must
//! form an unbroken hash chain from its first event (`seq`, `prev_hash`; see
//! PROTOCOL.md "Audit").
//!
//! The whole log is checked, not a window of it. It arrives in chain order
//! (`device_id`, then `seq`, see `AuditOrder::Chain`), page by page, so
//! [`ChainVerifier`] takes one event at a time and keeps only the last event
//! of the device it is on: memory doesn't grow with the log.

use super::crypto;
use bssh_teams_proto::api::{AuditEvent, AuditRecord};
use bssh_teams_proto::layout::{audit_event_hash, AUDIT_GENESIS};
use bssh_teams_proto::wire::PublicKey32;
use std::collections::HashMap;
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
    /// The device's log starts at `first_seq`, not 0: its first events are
    /// missing (deleted or withheld).
    MissingStart { device_id: Uuid, first_seq: u64 },
    /// Event `seq` doesn't point at the hash of event `seq - 1`.
    BrokenLink { device_id: Uuid, seq: u64 },
    /// The first event (`seq` 0) doesn't start from the genesis hash.
    BadGenesis { device_id: Uuid },
    /// Two different events claim the same `seq`.
    Duplicate { device_id: Uuid, seq: u64 },
    /// The server broke the chain order it was asked for, so this event could
    /// not be placed in its device's chain.
    OutOfOrder { device_id: Uuid, seq: u64 },
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
            ChainProblem::MissingStart {
                device_id,
                first_seq,
            } => write!(
                f,
                "device {device_id}: events 0 to {} are missing",
                first_seq - 1
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
            ChainProblem::OutOfOrder { device_id, seq } => {
                write!(
                    f,
                    "device {device_id}: the server sent event {seq} out of order"
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
    /// The log starts at the device's first event ever, so the whole chain
    /// up to `last_seq` was checked.
    pub fn from_genesis(&self) -> bool {
        self.first_seq == 0
    }
}

/// Verdict on a whole log: what [`ChainVerifier::finish`] returns.
#[derive(Debug, Clone, Default)]
pub struct ChainReport {
    /// Events that failed on their own (signature, unknown device, other team).
    pub invalid_events: usize,
    pub chains: Vec<ChainSummary>,
    pub problems: Vec<ChainProblem>,
}

impl ChainReport {
    /// Every event verified and every chain intact.
    pub fn is_clean(&self) -> bool {
        self.invalid_events == 0 && self.problems.is_empty()
    }
}

/// Check one event on its own against the keys of the devices we know.
pub fn check_event(
    team_id: Uuid,
    event: &AuditEvent,
    authors: &HashMap<Uuid, PublicKey32>,
) -> EventCheck {
    if event.team_id != team_id {
        return EventCheck::WrongTeam;
    }
    match authors.get(&event.device_id) {
        None => EventCheck::UnknownDevice,
        Some(key) => match crypto::verify_audit_event(event, key) {
            Ok(()) => EventCheck::Valid,
            Err(_) => EventCheck::BadSignature,
        },
    }
}

/// The device whose chain is being read.
struct Run {
    device_id: Uuid,
    first_seq: u64,
    events: usize,
    last: AuditEvent,
}

impl Run {
    fn summary(self) -> ChainSummary {
        ChainSummary {
            device_id: self.device_id,
            events: self.events,
            first_seq: self.first_seq,
            last_seq: self.last.seq,
        }
    }
}

/// Verifies a team's whole audit log, one event at a time.
///
/// Feed events in chain order: devices ascending, each device's `seq`
/// ascending, as `GET …/audit?order=chain` serves them. Chains are only
/// checked over events that verified on their own.
pub struct ChainVerifier<'a> {
    team_id: Uuid,
    authors: &'a HashMap<Uuid, PublicKey32>,
    run: Option<Run>,
    report: ChainReport,
}

impl<'a> ChainVerifier<'a> {
    pub fn new(team_id: Uuid, authors: &'a HashMap<Uuid, PublicKey32>) -> Self {
        Self {
            team_id,
            authors,
            run: None,
            report: ChainReport::default(),
        }
    }

    /// Check `record` and place it in its device's chain.
    pub fn push(&mut self, record: &AuditRecord) -> EventCheck {
        let event = &record.event;
        let check = check_event(self.team_id, event, self.authors);
        if check != EventCheck::Valid {
            self.report.invalid_events += 1;
            return check;
        }
        let device_id = event.device_id;

        let Some(run) = self.run.as_mut().filter(|r| r.device_id == device_id) else {
            if self.run.as_ref().is_some_and(|r| device_id < r.device_id) {
                self.report.problems.push(ChainProblem::OutOfOrder {
                    device_id,
                    seq: event.seq,
                });
                return check;
            }
            // Next device: close the previous one and start this chain.
            if let Some(done) = self.run.take() {
                self.report.chains.push(done.summary());
            }
            if event.seq != 0 {
                self.report.problems.push(ChainProblem::MissingStart {
                    device_id,
                    first_seq: event.seq,
                });
            } else if event.prev_hash != AUDIT_GENESIS {
                self.report
                    .problems
                    .push(ChainProblem::BadGenesis { device_id });
            }
            self.run = Some(Run {
                device_id,
                first_seq: event.seq,
                events: 1,
                last: event.clone(),
            });
            return check;
        };

        let prev = &run.last;
        if event.seq < prev.seq {
            self.report.problems.push(ChainProblem::OutOfOrder {
                device_id,
                seq: event.seq,
            });
            return check;
        }
        run.events += 1;
        if event.seq == prev.seq {
            if event != prev {
                self.report.problems.push(ChainProblem::Duplicate {
                    device_id,
                    seq: event.seq,
                });
            }
            return check;
        }
        if event.seq == prev.seq + 1 {
            if event.prev_hash != audit_event_hash(prev) {
                self.report.problems.push(ChainProblem::BrokenLink {
                    device_id,
                    seq: event.seq,
                });
            }
        } else {
            self.report.problems.push(ChainProblem::Gap {
                device_id,
                after: prev.seq,
                next: event.seq,
            });
        }
        run.last = event.clone();
        check
    }

    /// Close the last device's chain and return the verdict.
    pub fn finish(mut self) -> ChainReport {
        if let Some(done) = self.run.take() {
            self.report.chains.push(done.summary());
        }
        self.report
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
                    actor: None,
                    detail: None,
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

    /// The server's chain order: devices ascending, `seq` ascending.
    fn in_chain_order(mut log: Vec<AuditRecord>) -> Vec<AuditRecord> {
        log.sort_by_key(|r| (r.event.device_id, r.event.seq));
        log
    }

    fn run(
        team: Uuid,
        log: &[AuditRecord],
        authors: &HashMap<Uuid, PublicKey32>,
    ) -> (Vec<EventCheck>, ChainReport) {
        let mut v = ChainVerifier::new(team, authors);
        let checks = log.iter().map(|r| v.push(r)).collect();
        (checks, v.finish())
    }

    #[test]
    fn clean_log_verifies() {
        let team = Uuid::new_v4();
        let (a, b) = (device(), device());
        let mut log = chain(team, &a, 3);
        log.extend(chain(team, &b, 2));
        let (_, report) = run(team, &in_chain_order(log), &authors(&[&a, &b]));
        assert!(report.is_clean(), "{:?}", report.problems);
        assert_eq!(report.chains.len(), 2);
        assert!(report.chains.iter().all(|c| c.from_genesis()));
        let mut events: Vec<_> = report.chains.iter().map(|c| c.events).collect();
        events.sort();
        assert_eq!(events, vec![2, 3]);
    }

    #[test]
    fn altered_event_fails_its_signature() {
        let team = Uuid::new_v4();
        let a = device();
        let mut log = chain(team, &a, 2);
        log[1].event.kind = AuditKind::VaultWrite;
        let (checks, report) = run(team, &log, &authors(&[&a]));
        assert_eq!(checks[1], EventCheck::BadSignature);
        assert_eq!(report.invalid_events, 1);
        assert!(!report.is_clean());
    }

    #[test]
    fn deleted_event_is_a_gap() {
        let team = Uuid::new_v4();
        let a = device();
        let mut log = chain(team, &a, 4);
        log.remove(2);
        let (_, report) = run(team, &log, &authors(&[&a]));
        assert_eq!(
            report.problems,
            vec![ChainProblem::Gap {
                device_id: a.id,
                after: 1,
                next: 3
            }]
        );
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
        let (_, report) = run(team, &log, &authors(&[&a]));
        assert_eq!(
            report.problems,
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
        let (mut checks, report) = run(team, &log, &authors(&[&a]));
        checks.sort_by_key(|c| format!("{c:?}"));
        assert_eq!(
            checks,
            vec![
                EventCheck::UnknownDevice,
                EventCheck::Valid,
                EventCheck::WrongTeam
            ]
        );
        assert_eq!(report.invalid_events, 2);
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
        log.insert(2, twin);
        let (_, report) = run(team, &log, &authors(&[&a]));
        assert!(report
            .problems
            .contains(&ChainProblem::BadGenesis { device_id: a.id }));
        assert!(report.problems.contains(&ChainProblem::Duplicate {
            device_id: a.id,
            seq: 1
        }));
    }

    #[test]
    fn missing_first_events_are_reported() {
        // The whole log is checked, so a chain that doesn't start at 0 is a
        // deletion, not an unseen window.
        let team = Uuid::new_v4();
        let a = device();
        let log: Vec<_> = chain(team, &a, 5).into_iter().skip(2).collect();
        let (_, report) = run(team, &log, &authors(&[&a]));
        assert_eq!(
            report.problems,
            vec![ChainProblem::MissingStart {
                device_id: a.id,
                first_seq: 2
            }]
        );
        assert!(!report.chains[0].from_genesis());
        assert_eq!(
            (report.chains[0].first_seq, report.chains[0].last_seq),
            (2, 4)
        );
    }

    #[test]
    fn events_out_of_chain_order_are_flagged_not_trusted() {
        let team = Uuid::new_v4();
        let (a, b) = (device(), device());
        let mut log = in_chain_order(
            chain(team, &a, 3)
                .into_iter()
                .chain(chain(team, &b, 2))
                .collect(),
        );
        // Newest first within the first device: seq goes backwards.
        let first_device = log[0].event.device_id;
        let n = log
            .iter()
            .filter(|r| r.event.device_id == first_device)
            .count();
        log[..n].reverse();
        let (_, report) = run(team, &log, &authors(&[&a, &b]));
        assert!(report
            .problems
            .iter()
            .any(|p| matches!(p, ChainProblem::OutOfOrder { .. })));
        assert!(!report.is_clean());
    }

    #[test]
    fn a_device_coming_back_after_its_turn_is_out_of_order() {
        let team = Uuid::new_v4();
        let (a, b) = (device(), device());
        let mut log = in_chain_order(
            chain(team, &a, 1)
                .into_iter()
                .chain(chain(team, &b, 1))
                .collect(),
        );
        let again = log[0].clone();
        log.push(again);
        let (_, report) = run(team, &log, &authors(&[&a, &b]));
        assert!(report
            .problems
            .iter()
            .any(|p| matches!(p, ChainProblem::OutOfOrder { .. })));
    }
}
