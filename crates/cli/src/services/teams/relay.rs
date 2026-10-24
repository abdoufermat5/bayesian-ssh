//! Approval relay, client side (bssh AI, Teams; PROTOCOL.md "Approval relay").
//!
//! An agent posts an approval request encrypted to the team's admin devices;
//! an admin decides it from their device; the agent only believes a decision
//! signed by an approver key it has pinned. The server queues the opaque
//! messages and never sees what is asked or decided.

use super::client::{ClientResult, TeamsClient, MAX_POLL_WAIT_SECS};
use super::crypto::{self, DeviceKeys, RelayDraft, RelayTarget};
use crate::services::ai::types::{ApprovalRequest, Grant};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use bssh_teams_proto::api::{RelayAck, RelayKind, RelayMessage, RelayPoll};
use bssh_teams_proto::wire::PublicKey32;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// The server never keeps a message longer than this.
pub const MAX_RELAY_TTL: Duration = Duration::from_secs(3600);

/// A device whose keys are pinned: an admin approver, or an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub device_id: Uuid,
    pub account_id: Uuid,
    /// Email of a human, or the agent's name.
    pub label: String,
    pub ed25519: PublicKey32,
    pub x25519: PublicKey32,
}

impl Peer {
    fn target(&self) -> RelayTarget {
        RelayTarget {
            device_id: self.device_id,
            x25519_public: self.x25519,
        }
    }
}

/// What the agent asks, as the admins see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelayRequestPayload {
    pub v: u8,
    /// The agent's name.
    pub agent: String,
    /// 1 for T2, 2 distinct admins for T3.
    pub required_approvals: u8,
    pub request: ApprovalRequest,
}

/// An admin's answer. `request_hash` binds it to the exact request bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelayDecisionPayload {
    pub v: u8,
    pub request_id: Uuid,
    pub request_hash: String,
    pub approved: bool,
    /// The approver's email.
    pub by: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub grant: Option<Grant>,
    pub decided_at: DateTime<Utc>,
}

/// Wire access to the relay. [`TeamsClient`] is the production impl.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait RelayTransport: Send + Sync {
    async fn post_relay(&self, team_id: Uuid, msg: &RelayMessage) -> ClientResult<RelayAck>;
    async fn poll_relay(
        &self,
        team_id: Uuid,
        kind: Option<RelayKind>,
        request_id: Option<Uuid>,
        after: u64,
        wait_secs: u64,
    ) -> ClientResult<RelayPoll>;
}

#[async_trait]
impl RelayTransport for TeamsClient {
    async fn post_relay(&self, team_id: Uuid, msg: &RelayMessage) -> ClientResult<RelayAck> {
        TeamsClient::post_relay(self, team_id, msg).await
    }

    async fn poll_relay(
        &self,
        team_id: Uuid,
        kind: Option<RelayKind>,
        request_id: Option<Uuid>,
        after: u64,
        wait_secs: u64,
    ) -> ClientResult<RelayPoll> {
        TeamsClient::poll_relay(self, team_id, kind, request_id, after, wait_secs).await
    }
}

/// SHA-256 hex of the exact plaintext request bytes.
pub fn request_hash(plaintext: &[u8]) -> String {
    Sha256::digest(plaintext)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What waiting for decisions ended with.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Approved {
        /// Approver emails, in the order their decisions arrived.
        by: Vec<String>,
        grant: Option<Grant>,
    },
    Denied {
        by: String,
        reason: Option<String>,
    },
    Expired,
}

/// A request as an admin device receives it, verified and decrypted.
#[derive(Debug, Clone)]
pub struct IncomingRequest {
    pub seq: u64,
    pub message_id: Uuid,
    pub relay_request_id: Uuid,
    pub sender: Peer,
    pub expires_at: DateTime<Utc>,
    pub payload: RelayRequestPayload,
    pub request_hash: String,
}

/// One device's connection to the relay of one team.
pub struct RelayEndpoint {
    pub team_id: Uuid,
    pub device_id: Uuid,
    pub keys: Arc<DeviceKeys>,
    pub transport: Arc<dyn RelayTransport>,
}

impl RelayEndpoint {
    /// Post an approval request to `admins`. Returns the relay request id and the hash decisions must carry.
    pub async fn send_request(
        &self,
        relay_request_id: Uuid,
        payload: &RelayRequestPayload,
        admins: &[Peer],
        ttl: Duration,
    ) -> Result<String> {
        if admins.is_empty() {
            bail!("no admin device is pinned to receive approval requests");
        }
        let plain = serde_json::to_vec(payload)?;
        let now = Utc::now();
        let ttl = ttl.min(MAX_RELAY_TTL);
        let targets: Vec<RelayTarget> = admins.iter().map(Peer::target).collect();
        let msg = crypto::seal_relay(
            &RelayDraft {
                team_id: self.team_id,
                request_id: relay_request_id,
                kind: RelayKind::Request,
                sender_device_id: self.device_id,
                created_at: now,
                expires_at: now + chrono::Duration::from_std(ttl)?,
            },
            &plain,
            &targets,
            &self.keys,
        )?;
        self.transport
            .post_relay(self.team_id, &msg)
            .await
            .context("posting the approval request")?;
        Ok(request_hash(&plain))
    }

    /// Wait until `needed` distinct approver accounts approved, one denied, or `deadline`.
    ///
    /// Only decisions signed by a key in `approvers` count; anything else is
    /// ignored (logged), as is a decision for another request or hash.
    pub async fn wait_verdict(
        &self,
        relay_request_id: Uuid,
        hash: &str,
        approvers: &[Peer],
        needed: usize,
        deadline: Instant,
    ) -> Result<Verdict> {
        let mut after = 0u64;
        let mut approved: Vec<(Uuid, String)> = Vec::new();
        let mut grant = None;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(Verdict::Expired);
            }
            let wait = remaining.as_secs().clamp(0, MAX_POLL_WAIT_SECS);
            let poll = match self
                .transport
                .poll_relay(
                    self.team_id,
                    Some(RelayKind::Decision),
                    Some(relay_request_id),
                    after,
                    wait,
                )
                .await
            {
                Ok(p) => p,
                Err(e) if e.is_transient() => {
                    tracing::warn!("relay poll failed, retrying: {e}");
                    tokio::time::sleep(Duration::from_secs(2).min(remaining)).await;
                    continue;
                }
                Err(e) => return Err(e).context("waiting for the approval decision"),
            };
            after = poll.next.max(after);
            for env in poll.messages {
                let m = env.message;
                let Some(peer) = approvers.iter().find(|p| p.device_id == m.sender_device_id)
                else {
                    tracing::warn!("ignoring a decision from an unpinned device");
                    continue;
                };
                if m.kind != RelayKind::Decision || m.request_id != relay_request_id {
                    continue;
                }
                let plain = match crypto::open_relay(&m, self.device_id, &self.keys, &peer.ed25519)
                {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("ignoring a decision that does not verify: {e}");
                        continue;
                    }
                };
                let Ok(d) = serde_json::from_slice::<RelayDecisionPayload>(&plain) else {
                    tracing::warn!("ignoring an unreadable decision");
                    continue;
                };
                if d.request_id != relay_request_id || d.request_hash != hash {
                    tracing::warn!("ignoring a decision for a different request");
                    continue;
                }
                if !d.approved {
                    return Ok(Verdict::Denied {
                        by: peer.label.clone(),
                        reason: d.reason,
                    });
                }
                if !approved.iter().any(|(acct, _)| *acct == peer.account_id) {
                    approved.push((peer.account_id, peer.label.clone()));
                    grant = grant.or(d.grant);
                }
                if approved.len() >= needed {
                    return Ok(Verdict::Approved {
                        by: approved.into_iter().map(|(_, l)| l).collect(),
                        grant,
                    });
                }
            }
        }
    }

    /// Admin side: fetch requests addressed to this device with `seq > after`.
    /// `senders` are the agent devices whose requests are accepted.
    pub async fn pending_requests(
        &self,
        senders: &[Peer],
        after: u64,
        wait_secs: u64,
    ) -> Result<(Vec<IncomingRequest>, u64)> {
        let poll = self
            .transport
            .poll_relay(
                self.team_id,
                Some(RelayKind::Request),
                None,
                after,
                wait_secs,
            )
            .await
            .context("fetching approval requests")?;
        let mut out = Vec::new();
        for env in poll.messages {
            let m = env.message;
            let Some(sender) = senders.iter().find(|p| p.device_id == m.sender_device_id) else {
                tracing::warn!("ignoring a request from an unknown device");
                continue;
            };
            let Ok(plain) = crypto::open_relay(&m, self.device_id, &self.keys, &sender.ed25519)
            else {
                tracing::warn!("ignoring a request that does not verify");
                continue;
            };
            let Ok(payload) = serde_json::from_slice::<RelayRequestPayload>(&plain) else {
                continue;
            };
            out.push(IncomingRequest {
                seq: env.seq,
                message_id: m.message_id,
                relay_request_id: m.request_id,
                sender: sender.clone(),
                expires_at: m.expires_at,
                request_hash: request_hash(&plain),
                payload,
            });
        }
        Ok((out, poll.next))
    }

    /// Admin side: decisions already taken on requests (by any admin), so a
    /// pending list can hide what is done.
    pub async fn seen_decisions(&self, approvers: &[Peer]) -> Result<BTreeSet<(Uuid, Uuid)>> {
        let poll = self
            .transport
            .poll_relay(self.team_id, Some(RelayKind::Decision), None, 0, 0)
            .await?;
        let mut out = BTreeSet::new();
        for env in poll.messages {
            let m = env.message;
            let Some(p) = approvers.iter().find(|p| p.device_id == m.sender_device_id) else {
                continue;
            };
            if crypto::open_relay(&m, self.device_id, &self.keys, &p.ed25519).is_ok() {
                out.insert((m.request_id, p.account_id));
            }
        }
        Ok(out)
    }

    /// Admin side: answer `req`, encrypted to the agent and the other admins.
    pub async fn send_decision(
        &self,
        req: &IncomingRequest,
        approved: bool,
        by_email: &str,
        reason: Option<String>,
        grant: Option<Grant>,
        other_admins: &[Peer],
    ) -> Result<()> {
        let now = Utc::now();
        let payload = RelayDecisionPayload {
            v: 1,
            request_id: req.relay_request_id,
            request_hash: req.request_hash.clone(),
            approved,
            by: by_email.to_string(),
            reason,
            grant,
            decided_at: now,
        };
        let mut targets = vec![req.sender.target()];
        targets.extend(
            other_admins
                .iter()
                .filter(|p| p.device_id != self.device_id && p.device_id != req.sender.device_id)
                .map(Peer::target),
        );
        // Live until the request itself expires, so every recipient sees it.
        let expires = req
            .expires_at
            .min(now + chrono::Duration::from_std(MAX_RELAY_TTL)?);
        if expires <= now {
            return Err(anyhow!("that request has expired"));
        }
        let msg = crypto::seal_relay(
            &RelayDraft {
                team_id: self.team_id,
                request_id: req.relay_request_id,
                kind: RelayKind::Decision,
                sender_device_id: self.device_id,
                created_at: now,
                expires_at: expires,
            },
            &serde_json::to_vec(&payload)?,
            &targets,
            &self.keys,
        )?;
        self.transport
            .post_relay(self.team_id, &msg)
            .await
            .context("posting the decision")?;
        Ok(())
    }
}

/// An in-memory relay for tests: one shared queue, one handle per device.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use crate::services::ai::types::{ApprovalSubject, Scope, Tier};
    use crate::services::teams::client::TeamsError;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeRelay {
        pub queue: Mutex<Vec<(u64, RelayMessage)>>,
        /// Fail the next N polls with a network error.
        pub fail_polls: Mutex<u32>,
    }

    pub struct Handle {
        pub relay: Arc<FakeRelay>,
        pub device_id: Uuid,
    }

    #[async_trait]
    impl RelayTransport for Handle {
        async fn post_relay(&self, _team: Uuid, msg: &RelayMessage) -> ClientResult<RelayAck> {
            let mut q = self.relay.queue.lock().unwrap();
            if let Some((seq, _)) = q.iter().find(|(_, m)| m.message_id == msg.message_id) {
                return Ok(RelayAck {
                    message_id: msg.message_id,
                    seq: *seq,
                });
            }
            let seq = q.len() as u64 + 1;
            q.push((seq, msg.clone()));
            Ok(RelayAck {
                message_id: msg.message_id,
                seq,
            })
        }

        async fn poll_relay(
            &self,
            _team: Uuid,
            kind: Option<RelayKind>,
            request_id: Option<Uuid>,
            after: u64,
            wait_secs: u64,
        ) -> ClientResult<RelayPoll> {
            {
                let mut f = self.relay.fail_polls.lock().unwrap();
                if *f > 0 {
                    *f -= 1;
                    return Err(TeamsError::Network("boom".into()));
                }
            }
            let started = Instant::now();
            loop {
                let found: Vec<_> = self
                    .relay
                    .queue
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(seq, m)| {
                        *seq > after
                            && m.expires_at > Utc::now()
                            && m.recipients.iter().any(|r| r.device_id == self.device_id)
                            && kind.is_none_or(|k| k == m.kind)
                            && request_id.is_none_or(|r| r == m.request_id)
                    })
                    .map(|(seq, m)| bssh_teams_proto::api::RelayEnvelope {
                        seq: *seq,
                        message: m.clone(),
                    })
                    .collect();
                if !found.is_empty() || started.elapsed() >= Duration::from_secs(wait_secs) {
                    let next = found.last().map_or(after, |e| e.seq);
                    return Ok(RelayPoll {
                        messages: found,
                        next,
                    });
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }

    pub(crate) struct Cast {
        pub relay: Arc<FakeRelay>,
        pub team: Uuid,
        pub agent: (Peer, RelayEndpoint),
        pub admins: Vec<(Peer, RelayEndpoint)>,
    }

    pub(crate) fn peer(label: &str, account: Uuid, keys: &DeviceKeys) -> Peer {
        Peer {
            device_id: Uuid::new_v4(),
            account_id: account,
            label: label.into(),
            ed25519: keys.ed25519_public(),
            x25519: keys.x25519_public(),
        }
    }

    pub(crate) fn endpoint(
        relay: &Arc<FakeRelay>,
        team: Uuid,
        p: &Peer,
        keys: DeviceKeys,
    ) -> RelayEndpoint {
        RelayEndpoint {
            team_id: team,
            device_id: p.device_id,
            keys: Arc::new(keys),
            transport: Arc::new(Handle {
                relay: relay.clone(),
                device_id: p.device_id,
            }),
        }
    }

    pub(crate) fn cast(admin_accounts: &[(&str, Uuid)]) -> Cast {
        let relay = Arc::new(FakeRelay::default());
        let team = Uuid::new_v4();
        let ak = DeviceKeys::generate();
        let ap = peer("deployer", Uuid::new_v4(), &ak);
        let agent = (ap.clone(), endpoint(&relay, team, &ap, ak));
        let admins = admin_accounts
            .iter()
            .map(|(email, acct)| {
                let k = DeviceKeys::generate();
                let p = peer(email, *acct, &k);
                (p.clone(), endpoint(&relay, team, &p, k))
            })
            .collect();
        Cast {
            relay,
            team,
            agent,
            admins,
        }
    }

    pub(crate) fn approval(id: &str, tier: Tier) -> ApprovalRequest {
        ApprovalRequest {
            request_id: id.into(),
            run_id: "run-1".into(),
            client: "deployer".into(),
            tier,
            subject: ApprovalSubject::Access {
                scope: Scope::default(),
                reason: "restart nginx on web-1".into(),
                duration: "30m".into(),
            },
            created_at: Utc::now().to_rfc3339(),
            expires_at: (Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::cloned_ref_to_slice_refs)]
mod tests {
    use super::fake::*;
    use super::*;
    use crate::services::ai::types::Tier;

    fn payload(needed: u8, tier: Tier) -> RelayRequestPayload {
        RelayRequestPayload {
            v: 1,
            agent: "deployer".into(),
            required_approvals: needed,
            request: approval("r1", tier),
        }
    }

    fn deadline(secs: u64) -> Instant {
        Instant::now() + Duration::from_secs(secs)
    }

    #[tokio::test]
    async fn request_round_trips_and_one_approval_is_enough() {
        let c = cast(&[("alice@x.io", Uuid::new_v4()), ("bob@x.io", Uuid::new_v4())]);
        let admin_peers: Vec<Peer> = c.admins.iter().map(|(p, _)| p.clone()).collect();
        let rid = Uuid::new_v4();
        let hash = c
            .agent
            .1
            .send_request(
                rid,
                &payload(1, Tier::OutOfBand),
                &admin_peers,
                Duration::from_secs(300),
            )
            .await
            .unwrap();

        // Both admins see it; the agent does not (not a recipient).
        let (alice, bob) = (&c.admins[0], &c.admins[1]);
        let (reqs, _) = alice
            .1
            .pending_requests(&[c.agent.0.clone()], 0, 0)
            .await
            .unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].payload.request.request_id, "r1");
        assert_eq!(reqs[0].request_hash, hash);
        assert_eq!(
            bob.1
                .pending_requests(&[c.agent.0.clone()], 0, 0)
                .await
                .unwrap()
                .0
                .len(),
            1
        );
        assert!(c
            .agent
            .1
            .pending_requests(&[c.agent.0.clone()], 0, 0)
            .await
            .unwrap()
            .0
            .is_empty());
        // A request from a device the admin doesn't know is dropped.
        assert!(alice
            .1
            .pending_requests(&[], 0, 0)
            .await
            .unwrap()
            .0
            .is_empty());

        alice
            .1
            .send_decision(&reqs[0], true, "alice@x.io", None, None, &admin_peers)
            .await
            .unwrap();
        let v = c
            .agent
            .1
            .wait_verdict(rid, &hash, &admin_peers, 1, deadline(5))
            .await
            .unwrap();
        assert_eq!(
            v,
            Verdict::Approved {
                by: vec!["alice@x.io".into()],
                grant: None
            }
        );
        // Bob saw alice's decision too (it was addressed to him).
        let seen = bob.1.seen_decisions(&admin_peers).await.unwrap();
        assert!(seen.iter().any(|(r, _)| *r == rid));
    }

    #[tokio::test]
    async fn denial_wins_and_carries_the_reason() {
        let c = cast(&[("alice@x.io", Uuid::new_v4())]);
        let admins = vec![c.admins[0].0.clone()];
        let rid = Uuid::new_v4();
        let hash = c
            .agent
            .1
            .send_request(
                rid,
                &payload(1, Tier::OutOfBand),
                &admins,
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        let (reqs, _) = c.admins[0]
            .1
            .pending_requests(&[c.agent.0.clone()], 0, 0)
            .await
            .unwrap();
        c.admins[0]
            .1
            .send_decision(
                &reqs[0],
                false,
                "alice@x.io",
                Some("not now".into()),
                None,
                &admins,
            )
            .await
            .unwrap();
        let v = c
            .agent
            .1
            .wait_verdict(rid, &hash, &admins, 1, deadline(5))
            .await
            .unwrap();
        assert_eq!(
            v,
            Verdict::Denied {
                by: "alice@x.io".into(),
                reason: Some("not now".into())
            }
        );
    }

    #[tokio::test]
    async fn two_person_needs_two_distinct_accounts() {
        let alice_account = Uuid::new_v4();
        // alice has two devices: two approvals from one person are one approval.
        let mut c = cast(&[("alice@x.io", alice_account), ("bob@x.io", Uuid::new_v4())]);
        let k = DeviceKeys::generate();
        let alice2 = peer("alice@x.io", alice_account, &k);
        let alice2_ep = endpoint(&c.relay, c.team, &alice2, k);
        c.admins.push((alice2, alice2_ep));
        let admin_peers: Vec<Peer> = c.admins.iter().map(|(p, _)| p.clone()).collect();
        let rid = Uuid::new_v4();
        let hash = c
            .agent
            .1
            .send_request(
                rid,
                &payload(2, Tier::TwoPerson),
                &admin_peers,
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        let senders = [c.agent.0.clone()];

        let (reqs, _) = c.admins[0]
            .1
            .pending_requests(&senders, 0, 0)
            .await
            .unwrap();
        c.admins[0]
            .1
            .send_decision(&reqs[0], true, "alice@x.io", None, None, &admin_peers)
            .await
            .unwrap();
        let (reqs2, _) = c.admins[2]
            .1
            .pending_requests(&senders, 0, 0)
            .await
            .unwrap();
        c.admins[2]
            .1
            .send_decision(&reqs2[0], true, "alice@x.io", None, None, &admin_peers)
            .await
            .unwrap();
        // Same person twice: still waiting.
        let v = c
            .agent
            .1
            .wait_verdict(
                rid,
                &hash,
                &admin_peers,
                2,
                Instant::now() + Duration::from_millis(300),
            )
            .await
            .unwrap();
        assert_eq!(v, Verdict::Expired);

        let (reqs3, _) = c.admins[1]
            .1
            .pending_requests(&senders, 0, 0)
            .await
            .unwrap();
        c.admins[1]
            .1
            .send_decision(&reqs3[0], true, "bob@x.io", None, None, &admin_peers)
            .await
            .unwrap();
        let v = c
            .agent
            .1
            .wait_verdict(rid, &hash, &admin_peers, 2, deadline(5))
            .await
            .unwrap();
        match v {
            Verdict::Approved { by, .. } => assert_eq!(by, vec!["alice@x.io", "bob@x.io"]),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn decisions_from_unpinned_or_mismatched_sources_are_ignored() {
        let c = cast(&[("alice@x.io", Uuid::new_v4())]);
        let admins = vec![c.admins[0].0.clone()];
        let rid = Uuid::new_v4();
        let hash = c
            .agent
            .1
            .send_request(
                rid,
                &payload(1, Tier::OutOfBand),
                &admins,
                Duration::from_secs(60),
            )
            .await
            .unwrap();

        // An attacker (e.g. a server-injected device) addresses a decision to the agent.
        let evil_keys = DeviceKeys::generate();
        let evil = peer("evil@x.io", Uuid::new_v4(), &evil_keys);
        let evil_ep = endpoint(&c.relay, c.team, &evil, evil_keys);
        let fake = IncomingRequest {
            seq: 1,
            message_id: Uuid::new_v4(),
            relay_request_id: rid,
            sender: c.agent.0.clone(),
            expires_at: Utc::now() + chrono::Duration::minutes(1),
            payload: payload(1, Tier::OutOfBand),
            request_hash: hash.clone(),
        };
        evil_ep
            .send_decision(&fake, true, "evil@x.io", None, None, &[])
            .await
            .unwrap();

        // A real admin approves a different request (hash mismatch).
        let (reqs, _) = c.admins[0]
            .1
            .pending_requests(&[c.agent.0.clone()], 0, 0)
            .await
            .unwrap();
        let mut other = reqs[0].clone();
        other.request_hash = "deadbeef".into();
        c.admins[0]
            .1
            .send_decision(&other, true, "alice@x.io", None, None, &[])
            .await
            .unwrap();

        let v = c
            .agent
            .1
            .wait_verdict(
                rid,
                &hash,
                &admins,
                1,
                Instant::now() + Duration::from_millis(300),
            )
            .await
            .unwrap();
        assert_eq!(v, Verdict::Expired);
    }

    #[tokio::test]
    async fn transient_poll_errors_are_retried_and_nothing_expires() {
        let c = cast(&[("alice@x.io", Uuid::new_v4())]);
        let admins = vec![c.admins[0].0.clone()];
        let rid = Uuid::new_v4();
        let hash = c
            .agent
            .1
            .send_request(
                rid,
                &payload(1, Tier::OutOfBand),
                &admins,
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        let (reqs, _) = c.admins[0]
            .1
            .pending_requests(&[c.agent.0.clone()], 0, 0)
            .await
            .unwrap();
        c.admins[0]
            .1
            .send_decision(&reqs[0], true, "alice@x.io", None, None, &admins)
            .await
            .unwrap();
        *c.relay.fail_polls.lock().unwrap() = 1;
        let v = c
            .agent
            .1
            .wait_verdict(rid, &hash, &admins, 1, deadline(10))
            .await
            .unwrap();
        assert!(matches!(v, Verdict::Approved { .. }));
    }

    #[tokio::test]
    async fn sending_needs_an_admin_and_an_expiry_is_capped() {
        let c = cast(&[]);
        assert!(c
            .agent
            .1
            .send_request(
                Uuid::new_v4(),
                &payload(1, Tier::OutOfBand),
                &[],
                Duration::from_secs(60)
            )
            .await
            .is_err());
        let c = cast(&[("alice@x.io", Uuid::new_v4())]);
        let admins = vec![c.admins[0].0.clone()];
        c.agent
            .1
            .send_request(
                Uuid::new_v4(),
                &payload(1, Tier::OutOfBand),
                &admins,
                Duration::from_secs(86_400),
            )
            .await
            .unwrap();
        let q = c.relay.queue.lock().unwrap();
        let m = &q[0].1;
        assert!(m.expires_at - m.created_at <= chrono::Duration::seconds(3600));
    }
}
