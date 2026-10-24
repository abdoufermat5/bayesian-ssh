//! Teams side of bssh AI: approvals relayed to admins, audit forwarding.
//!
//! - [`RelayBroker`]: an [`ApprovalBroker`] that posts the request, end-to-end
//!   encrypted, to the team's admin devices and waits for their signed
//!   decisions (T2: one admin, T3: two distinct admins).
//! - [`forward_event`]: queues an event from the local audit chain; the next
//!   Teams sync encrypts it to the team audit key, signs it as the new
//!   `ai_*` audit kinds and uploads it. A no-op outside Teams.
//!
//! The server sees the team, the acting device, kind, size and time, never
//! commands, hosts or output (PERIMETER.md).

use super::types::{
    AiEvent, ApprovalBroker, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Tier,
};
use crate::config::AppConfig;
use crate::database::Database;
use crate::services::teams::agent::AgentRuntime;
use crate::services::teams::codec::VaultCodec;
use crate::services::teams::relay::{Peer, RelayEndpoint, RelayRequestPayload, Verdict};
use crate::services::teams::session::{self, kind_from_str, kind_str, team_of_connection};
use crate::services::teams::TeamsStore;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use bssh_teams_proto::api::{AuditActor, AuditKind, MemberKind};
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use std::sync::RwLock;
use std::time::{Duration, Instant};
use uuid::Uuid;

// ---------------------------------------------------------------- broker

/// Approvals through the team's admins' devices.
pub struct RelayBroker {
    endpoint: RelayEndpoint,
    agent_name: String,
    approvers: Vec<Peer>,
    default_timeout: Duration,
}

impl RelayBroker {
    pub fn new(
        endpoint: RelayEndpoint,
        agent_name: String,
        approvers: Vec<Peer>,
        default_timeout: Duration,
    ) -> Self {
        Self {
            endpoint,
            agent_name,
            approvers,
            default_timeout,
        }
    }

    /// The broker of a gateway agent: approvers are the admins pinned in its credential.
    pub fn from_runtime(rt: &AgentRuntime, config: &AppConfig) -> Self {
        Self::new(
            RelayEndpoint {
                team_id: rt.cred.team_id,
                device_id: rt.cred.device_id,
                keys: rt.keys.clone(),
                transport: rt.client.clone(),
            },
            rt.cred.agent_name.clone(),
            rt.cred.approvers.clone(),
            Duration::from_secs(config.ai.approval_timeout_secs),
        )
    }

    fn deadline(&self, req: &ApprovalRequest) -> Duration {
        let until_expiry = DateTime::parse_from_rfc3339(&req.expires_at)
            .ok()
            .and_then(|t| (t.with_timezone(&Utc) - Utc::now()).to_std().ok());
        until_expiry
            .unwrap_or(self.default_timeout)
            .min(self.default_timeout)
    }
}

#[async_trait]
impl ApprovalBroker for RelayBroker {
    async fn request(&self, req: ApprovalRequest) -> Result<ApprovalOutcome> {
        let needed: usize = if req.tier == Tier::TwoPerson { 2 } else { 1 };
        let accounts: HashSet<Uuid> = self.approvers.iter().map(|p| p.account_id).collect();
        if accounts.len() < needed {
            return Ok(ApprovalOutcome::Denied {
                by: "bssh".into(),
                reason: Some(format!(
                    "{needed} distinct admins are needed but only {} are pinned for this agent",
                    accounts.len()
                )),
            });
        }
        let wait = self.deadline(&req);
        if wait.is_zero() {
            return Ok(ApprovalOutcome::Expired);
        }
        let relay_id = Uuid::parse_str(&req.request_id).unwrap_or_else(|_| Uuid::new_v4());
        let payload = RelayRequestPayload {
            v: 1,
            agent: self.agent_name.clone(),
            required_approvals: needed as u8,
            request: req,
        };
        let hash = self
            .endpoint
            .send_request(relay_id, &payload, &self.approvers, wait)
            .await?;
        let verdict = self
            .endpoint
            .wait_verdict(
                relay_id,
                &hash,
                &self.approvers,
                needed,
                Instant::now() + wait,
            )
            .await?;
        Ok(match verdict {
            Verdict::Approved { by, grant } => ApprovalOutcome::Approved {
                by: by.join(" + "),
                channel: "teams-relay".into(),
                grant,
            },
            Verdict::Denied { by, reason } => ApprovalOutcome::Denied { by, reason },
            Verdict::Expired => ApprovalOutcome::Expired,
        })
    }
}

// ---------------------------------------------------------------- forwarding

/// Set by the gateway: events then belong to this agent in its own Teams store.
#[derive(Clone)]
pub struct AgentContext {
    /// Config whose database is the agent's Teams store.
    pub agent_config: AppConfig,
    pub team_id: Uuid,
    pub agent_name: String,
}

static AGENT_CONTEXT: RwLock<Option<AgentContext>> = RwLock::new(None);

pub fn set_agent_context(ctx: Option<AgentContext>) {
    if let Ok(mut g) = AGENT_CONTEXT.write() {
        *g = ctx;
    }
}

fn agent_context() -> Option<AgentContext> {
    AGENT_CONTEXT.read().ok().and_then(|g| g.clone())
}

const PENDING_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS ai_audit_pending (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id TEXT NOT NULL,
    entry_id TEXT,
    kind TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    actor_json TEXT NOT NULL,
    detail_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS ai_audit_runs (
    run_id TEXT PRIMARY KEY,
    team_id TEXT,
    client TEXT
);
";

/// Which audit kind an event is filed under.
pub fn kind_of(event: &AiEvent) -> AuditKind {
    match event {
        AiEvent::RunStarted { .. } | AiEvent::CircuitBreaker { .. } | AiEvent::RunEnded { .. } => {
            AuditKind::AiRun
        }
        AiEvent::Call { .. } | AiEvent::Result { .. } => AuditKind::AiCall,
        AiEvent::Approval { .. } => AuditKind::AiApproval,
        AiEvent::Change { .. } => AuditKind::AiChange,
    }
}

fn run_id_of(event: &AiEvent) -> &str {
    match event {
        AiEvent::RunStarted { run_id, .. }
        | AiEvent::Approval { run_id, .. }
        | AiEvent::Result { run_id, .. }
        | AiEvent::Change { run_id, .. }
        | AiEvent::CircuitBreaker { run_id, .. }
        | AiEvent::RunEnded { run_id, .. } => run_id,
        AiEvent::Call { call, .. } => &call.run_id,
    }
}

/// Forward an event already appended to the local audit chain to the team's
/// audit log (encrypted to the team audit key). No-op outside Teams. Called by
/// `audit::LocalAudit`.
///
/// It only queues: signing and encrypting need the team key, which is
/// available at the next `bssh teams sync` (or the gateway's own sync).
pub fn forward_event(config: &AppConfig, event: &AiEvent) -> Result<()> {
    if let Some(ctx) = agent_context() {
        let db = Database::new(&ctx.agent_config)?;
        let actor = AuditActor {
            kind: MemberKind::Agent,
            name: ctx.agent_name.clone(),
            on_behalf_of: None,
        };
        return queue_event(&db, ctx.team_id, actor, None, event);
    }
    let db = Database::new(config)?;
    let Some(identity) = session::load_identity(&db)? else {
        return Ok(());
    };
    db.conn.execute_batch(PENDING_SCHEMA)?;
    let run = run_id_of(event).to_string();

    // The team: the host's, else the run's, else the only team there is.
    let mut entry_id = None;
    let mut team = None;
    if let AiEvent::Call { call, .. } = event {
        let conn = match call.host.as_deref() {
            Some(h) => db.get_connection(h)?,
            None => None,
        };
        if let Some(conn) = conn {
            if let Some((t, e)) = team_of_connection(&db, &conn)? {
                team = Some(t);
                entry_id = Some(e);
            }
        }
    }
    if team.is_none() {
        let known: Option<Option<String>> = db
            .conn
            .query_row(
                "SELECT team_id FROM ai_audit_runs WHERE run_id = ?1",
                [&run],
                |r| r.get(0),
            )
            .optional()?;
        team = known.flatten().and_then(|t| Uuid::parse_str(&t).ok());
    }
    if team.is_none() {
        let teams = session::local_teams(&db)?;
        if let [(only, _)] = teams.as_slice() {
            team = Some(*only);
        }
    }
    // Remember run -> team and the client name for later events of the run.
    let client = if let AiEvent::RunStarted { client, .. } = event {
        Some(client.clone())
    } else {
        None
    };
    db.conn.execute(
        "INSERT INTO ai_audit_runs (run_id, team_id, client) VALUES (?1, ?2, ?3)
         ON CONFLICT(run_id) DO UPDATE SET
           team_id = COALESCE(excluded.team_id, ai_audit_runs.team_id),
           client = COALESCE(excluded.client, ai_audit_runs.client)",
        params![run, team.map(|t| t.to_string()), client],
    )?;
    let Some(team) = team else {
        tracing::debug!("AI event not forwarded: no team to file it under");
        return Ok(());
    };
    let name: Option<String> = db
        .conn
        .query_row(
            "SELECT client FROM ai_audit_runs WHERE run_id = ?1",
            [&run],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let actor = AuditActor {
        kind: MemberKind::Agent,
        name: name.unwrap_or_else(|| "agent".into()),
        on_behalf_of: Some(identity.email),
    };
    queue_event(&db, team, actor, entry_id, event)
}

/// Queue `event` for the next sync of `team_id`.
pub fn queue_event(
    db: &Database,
    team_id: Uuid,
    actor: AuditActor,
    entry_id: Option<Uuid>,
    event: &AiEvent,
) -> Result<()> {
    db.conn.execute_batch(PENDING_SCHEMA)?;
    db.conn.execute(
        "INSERT INTO ai_audit_pending (team_id, entry_id, kind, occurred_at, actor_json, detail_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            team_id.to_string(),
            entry_id.map(|e| e.to_string()),
            kind_str(kind_of(event)),
            Utc::now().to_rfc3339(),
            serde_json::to_string(&actor)?,
            serde_json::to_string(event)?,
        ],
    )?;
    Ok(())
}

/// Turn queued events of `team_id` into signed, encrypted audit events in the
/// device's chain (uploaded by the sync engine).
pub(crate) fn flush_pending(
    store: &TeamsStore,
    codec: &dyn VaultCodec,
    team_id: Uuid,
) -> Result<()> {
    let db = store.database();
    db.conn.execute_batch(PENDING_SCHEMA)?;
    type Row = (i64, Option<String>, String, String, String, String);
    let rows: Vec<Row> = {
        let mut stmt = db.conn.prepare(
            "SELECT id, entry_id, kind, occurred_at, actor_json, detail_json
             FROM ai_audit_pending WHERE team_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([team_id.to_string()], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (id, entry, kind, at, actor, detail) in rows {
        let kind = kind_from_str(&kind).ok_or_else(|| anyhow!("bad queued kind {kind}"))?;
        let at = DateTime::parse_from_rfc3339(&at)
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        store.append_audit_event_full(
            team_id,
            codec,
            kind,
            entry.as_deref().and_then(|e| Uuid::parse_str(e).ok()),
            at,
            Some(serde_json::from_str(&actor)?),
            Some(detail.as_bytes()),
        )?;
        db.conn
            .execute("DELETE FROM ai_audit_pending WHERE id = ?1", [id])?;
    }
    Ok(())
}

/// Short text for a subject, for `bssh teams agent pending`.
pub fn describe(req: &ApprovalRequest) -> String {
    match &req.subject {
        ApprovalSubject::Call { call, .. } => format!(
            "{:?} on {}: {}",
            call.tool,
            call.host.as_deref().unwrap_or("-"),
            call.command
                .as_deref()
                .or(call.path.as_deref())
                .unwrap_or("")
        ),
        ApprovalSubject::Plan {
            plan, plan_hash, ..
        } => format!(
            "change plan '{}' ({} steps, hash {})",
            plan.title,
            plan.steps.len(),
            &plan_hash[..plan_hash.len().min(12)]
        ),
        ApprovalSubject::Access {
            scope,
            reason,
            duration,
        } => {
            format!("access for {duration} to {:?}: {reason}", scope.tags)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::{Decision, ToolCall, ToolName};
    use crate::services::teams::codec::CryptoCodec;
    use crate::services::teams::crypto::{self, DeviceKeys, TeamKey};
    use crate::services::teams::relay::fake::{approval, cast};
    use crate::services::teams::store::TeamLocal;
    use std::collections::HashMap;
    use std::sync::Arc;

    #[tokio::test]
    async fn broker_gets_a_t2_approval_from_one_admin() {
        let c = cast(&[("alice@x.io", Uuid::new_v4())]);
        let admins = vec![c.admins[0].0.clone()];
        let broker = RelayBroker::new(
            c.agent.1,
            "deployer".into(),
            admins.clone(),
            Duration::from_secs(10),
        );
        let admin_ep = c.admins.into_iter().next().unwrap().1;
        let agent_peer = c.agent.0.clone();
        let admin = tokio::spawn(async move {
            let (reqs, _) = admin_ep
                .pending_requests(&[agent_peer], 0, 5)
                .await
                .unwrap();
            assert_eq!(reqs[0].payload.agent, "deployer");
            assert_eq!(reqs[0].payload.required_approvals, 1);
            admin_ep
                .send_decision(&reqs[0], true, "alice@x.io", None, None, &admins)
                .await
                .unwrap();
        });
        let out = broker
            .request(approval("req-1", Tier::OutOfBand))
            .await
            .unwrap();
        admin.await.unwrap();
        assert_eq!(
            out,
            ApprovalOutcome::Approved {
                by: "alice@x.io".into(),
                channel: "teams-relay".into(),
                grant: None
            }
        );
    }

    #[tokio::test]
    async fn broker_t3_needs_two_admins() {
        let c = cast(&[("alice@x.io", Uuid::new_v4()), ("bob@x.io", Uuid::new_v4())]);
        let admins: Vec<Peer> = c.admins.iter().map(|(p, _)| p.clone()).collect();
        let broker = RelayBroker::new(
            c.agent.1,
            "deployer".into(),
            admins.clone(),
            Duration::from_secs(10),
        );
        let agent_peer = c.agent.0.clone();
        let mut tasks = Vec::new();
        for (i, (_, ep)) in c.admins.into_iter().enumerate() {
            let (agent_peer, admins) = (agent_peer.clone(), admins.clone());
            tasks.push(tokio::spawn(async move {
                let (reqs, _) = ep.pending_requests(&[agent_peer], 0, 5).await.unwrap();
                assert_eq!(reqs[0].payload.required_approvals, 2);
                let who = if i == 0 { "alice@x.io" } else { "bob@x.io" };
                ep.send_decision(&reqs[0], true, who, None, None, &admins)
                    .await
                    .unwrap();
            }));
        }
        let out = broker
            .request(approval("req-2", Tier::TwoPerson))
            .await
            .unwrap();
        for t in tasks {
            t.await.unwrap();
        }
        match out {
            ApprovalOutcome::Approved { by, .. } => {
                assert!(by.contains("alice@x.io") && by.contains("bob@x.io"), "{by}")
            }
            other => panic!("{other:?}"),
        }

        // One pinned admin cannot satisfy T3.
        let c = cast(&[("alice@x.io", Uuid::new_v4())]);
        let broker = RelayBroker::new(
            c.agent.1,
            "d".into(),
            vec![c.admins[0].0.clone()],
            Duration::from_secs(1),
        );
        let out = broker
            .request(approval("req-3", Tier::TwoPerson))
            .await
            .unwrap();
        assert!(matches!(out, ApprovalOutcome::Denied { .. }));
    }

    #[tokio::test]
    async fn broker_denial_and_expiry() {
        let c = cast(&[("alice@x.io", Uuid::new_v4())]);
        let admins = vec![c.admins[0].0.clone()];
        let broker = RelayBroker::new(
            c.agent.1,
            "d".into(),
            admins.clone(),
            Duration::from_millis(400),
        );
        assert_eq!(
            broker
                .request(approval("req-4", Tier::OutOfBand))
                .await
                .unwrap(),
            ApprovalOutcome::Expired
        );

        let c = cast(&[("alice@x.io", Uuid::new_v4())]);
        let admins = vec![c.admins[0].0.clone()];
        let broker = RelayBroker::new(
            c.agent.1,
            "d".into(),
            admins.clone(),
            Duration::from_secs(10),
        );
        let ep = c.admins.into_iter().next().unwrap().1;
        let ap = c.agent.0.clone();
        let t = tokio::spawn(async move {
            let (reqs, _) = ep.pending_requests(&[ap], 0, 5).await.unwrap();
            ep.send_decision(
                &reqs[0],
                false,
                "alice@x.io",
                Some("no".into()),
                None,
                &admins,
            )
            .await
            .unwrap();
        });
        let out = broker
            .request(approval("req-5", Tier::OutOfBand))
            .await
            .unwrap();
        t.await.unwrap();
        assert_eq!(
            out,
            ApprovalOutcome::Denied {
                by: "alice@x.io".into(),
                reason: Some("no".into())
            }
        );
    }

    #[test]
    fn forward_is_a_noop_outside_teams() {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("t.db"),
            ..Default::default()
        };
        let ev = AiEvent::RunEnded {
            run_id: "r".into(),
            reason: "done".into(),
        };
        forward_event(&config, &ev).unwrap();
        let db = Database::new(&config).unwrap();
        let has: bool = db
            .conn
            .query_row(
                "SELECT count(*) > 0 FROM sqlite_master WHERE name = 'ai_audit_pending'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!has);
    }

    #[test]
    fn queued_events_become_signed_encrypted_ai_audit_events() {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("t.db"),
            ..Default::default()
        };
        let store = TeamsStore::open(&config).unwrap();
        let (team, device) = (Uuid::new_v4(), Uuid::new_v4());
        let keys = Arc::new(DeviceKeys::generate());
        store
            .upsert_team(&TeamLocal {
                team_id: team,
                server_url: "https://t.example.com".into(),
                account_id: Uuid::new_v4(),
                device_id: device,
                trust_root_ed25519: keys.ed25519_public(),
                last_pulled_revision: 0,
                created_at: Utc::now(),
            })
            .unwrap();
        let tk = TeamKey::generate(1);
        let codec = CryptoCodec::new(team, device, keys.clone(), vec![tk], HashMap::new()).unwrap();

        let actor = AuditActor {
            kind: MemberKind::Agent,
            name: "deployer".into(),
            on_behalf_of: None,
        };
        let call = AiEvent::Call {
            call: ToolCall {
                run_id: "r1".into(),
                call_id: "c1".into(),
                tool: ToolName::Run,
                host: Some("web-1".into()),
                command: Some("systemctl restart nginx".into()),
                path: None,
                args: serde_json::json!({}),
            },
            decision: Decision::Allow {
                reason: "grant".into(),
            },
        };
        queue_event(store.database(), team, actor.clone(), None, &call).unwrap();
        queue_event(
            store.database(),
            team,
            actor,
            None,
            &AiEvent::RunEnded {
                run_id: "r1".into(),
                reason: "done".into(),
            },
        )
        .unwrap();
        flush_pending(&store, &codec, team).unwrap();
        flush_pending(&store, &codec, team).unwrap();

        let events = store.pending_audit(team, device, 10).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, AuditKind::AiCall);
        assert_eq!(events[1].kind, AuditKind::AiRun);
        for e in &events {
            assert!(crypto::verify_audit_event(e, &keys.ed25519_public()).is_ok());
            assert_eq!(e.actor.as_ref().unwrap().name, "deployer");
        }
        let wire = serde_json::to_string(&events[0]).unwrap();
        assert!(!wire.contains("systemctl") && !wire.contains("web-1"));
        let plain = codec.open_audit_detail(&events[0]).unwrap();
        let back: AiEvent = serde_json::from_slice(&plain).unwrap();
        assert_eq!(back, call);
        assert_eq!(
            events[1].prev_hash,
            bssh_teams_proto::layout::audit_event_hash(&events[0])
        );
    }
}
