//! bssh AI agents: creating, listing and removing them, and deciding
//! approval requests relayed through the team.

use super::handle::Session;
use super::types::{AgentCreated, Edit, TeamSync};
use crate::config::AppConfig;
use crate::services::teams::agent::{credential_path, AgentCredential, AgentCredentialParams};
use crate::services::teams::client::TeamsClient;
use crate::services::teams::crypto::{self, fingerprint, DeviceKeys};
use crate::services::teams::relay::{IncomingRequest, Peer, RelayEndpoint};
use anyhow::{anyhow, bail, Result};
use bssh_teams_proto::api::{
    DeviceInfo, Member, MemberKind, PutKeysRequest, Role, SignedAgentPolicy,
};
use bssh_teams_proto::wire::Fixed;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// The JSON text of local policy `name`, as it will be signed.
fn local_policy_json(config: &AppConfig, name: &str) -> Result<String> {
    let policy = config
        .ai
        .policies
        .get(name)
        .cloned()
        .or_else(|| (name == "default").then(Default::default))
        .ok_or_else(|| anyhow!("no policy '{name}' in config.json (ai.policies)"))?;
    Ok(serde_json::to_string(&policy)?)
}

fn device_peer(label: &str, account_id: Uuid, d: &DeviceInfo) -> Peer {
    Peer {
        device_id: d.device_id,
        account_id,
        label: label.to_string(),
        ed25519: d.ed25519_public,
        x25519: d.x25519_public,
    }
}

/// Live devices of the team's human admins.
fn admin_peers(members: &[Member]) -> Vec<Peer> {
    members
        .iter()
        .filter(|m| m.role == Role::Admin && m.kind == MemberKind::Human)
        .flat_map(|m| {
            m.devices
                .iter()
                .filter(|d| d.revoked_at.is_none())
                .map(|d| device_peer(&m.email, m.account_id, d))
        })
        .collect()
}

impl<'a> Session<'a> {
    fn relay_endpoint(&self, team_id: Uuid) -> Result<RelayEndpoint> {
        let client = TeamsClient::new(
            &self.identity.server_url,
            self.identity.device_id,
            self.keys.clone(),
        )?;
        Ok(RelayEndpoint {
            team_id,
            device_id: self.identity.device_id,
            keys: self.keys.clone(),
            transport: Arc::new(client),
        })
    }

    /// Create an agent identity (admin): its own device keys, a seat, the team
    /// key, a 0600 credential file for the gateway and, signed by this admin,
    /// its policy.
    pub async fn agent_add(
        &self,
        team_id: Uuid,
        team_name: &str,
        name: &str,
        policy: Option<&str>,
    ) -> Result<AgentCreated> {
        let info = self.require_admin(team_id).await?;
        if info.rotation_required {
            bail!("a key rotation is pending: run `bssh teams sync {team_name}` first");
        }
        let policy_name = policy.unwrap_or("default").to_string();
        let policy_json = local_policy_json(self.config, &policy_name)?;
        let path = credential_path(self.config, team_name, name);
        if path.exists() {
            bail!("{} already exists", path.display());
        }
        let local = self.local_team(team_id)?;
        let key = self
            .team_keys(team_id, info.current_key_version)
            .await?
            .into_iter()
            .find(|k| k.key_version() == info.current_key_version)
            .ok_or_else(|| anyhow!("this device has no team key yet"))?;

        let keys = DeviceKeys::generate();
        let created = self
            .client
            .create_agent(
                team_id,
                &bssh_teams_proto::api::CreateAgentRequest {
                    name: name.to_string(),
                    device: keys.to_new_device(name),
                },
            )
            .await?;
        let wrapped = crypto::wrap_team_key(
            team_id,
            &key,
            &created.device,
            self.identity.device_id,
            &self.keys,
        )?;
        self.client
            .put_keys(
                team_id,
                &PutKeysRequest {
                    keys: vec![wrapped],
                },
            )
            .await?;
        self.approve_device(team_id, &created.device, &format!("agent:{name}"))?;
        let members = self.client.list_members(team_id).await?;
        self.remember_authors(team_id, &members)?;

        let cred = AgentCredential::from_params(
            AgentCredentialParams {
                server_url: self.identity.server_url.clone(),
                team_id,
                team_name: team_name.to_string(),
                agent_name: name.to_string(),
                account_id: created.account_id,
                device_id: created.device.device_id,
                trust_root: local.trust_root_ed25519,
                approvers: admin_peers(&members),
                policy: policy_name.clone(),
            },
            &keys,
        );
        cred.save(&path)?;
        self.set_agent_policy(team_id, created.device.device_id, &policy_name, policy_json)
            .await?;
        Ok(AgentCreated {
            info: created,
            credential_path: path,
            fingerprint: fingerprint(&keys.ed25519_public()),
            policy: policy_name,
        })
    }

    async fn set_agent_policy(
        &self,
        team_id: Uuid,
        agent_device_id: Uuid,
        policy_name: &str,
        policy_json: String,
    ) -> Result<u64> {
        let existing = self.vault_view(team_id).await?.and_then(|v| {
            v.agent_policies.into_iter().find(|p| {
                p.policy.agent_device_id == agent_device_id && p.policy.name == policy_name
            })
        });
        let version = existing.as_ref().map_or(1, |p| p.policy.version + 1);
        let mut signed = SignedAgentPolicy {
            agent_device_id,
            name: policy_name.to_string(),
            policy_json,
            version,
            signed_at: Utc::now(),
            signer_device_id: self.identity.device_id,
            signature: Fixed::ZERO,
        };
        crypto::sign_policy(team_id, &mut signed, &self.keys);
        self.sync_team(
            team_id,
            vec![Edit::AgentPolicy {
                entry_id: existing.map(|p| p.entry_id),
                policy: signed,
            }],
        )
        .await?;
        Ok(version)
    }

    /// Re-sign `policy_name` (from config.json) for an existing agent.
    pub async fn agent_set_policy(
        &self,
        team_id: Uuid,
        name: &str,
        policy_name: &str,
    ) -> Result<u64> {
        self.require_admin(team_id).await?;
        let json = local_policy_json(self.config, policy_name)?;
        let agent = self.find_agent(team_id, name).await?;
        self.set_agent_policy(team_id, agent.device.device_id, policy_name, json)
            .await
    }

    async fn find_agent(
        &self,
        team_id: Uuid,
        name: &str,
    ) -> Result<bssh_teams_proto::api::AgentInfo> {
        self.client
            .list_agents(team_id)
            .await?
            .into_iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow!("no agent named '{name}' in this team"))
    }

    pub async fn agent_list(&self, team_id: Uuid) -> Result<Vec<bssh_teams_proto::api::AgentInfo>> {
        Ok(self.client.list_agents(team_id).await?)
    }

    /// Remove an agent: frees its seat, rotates the team key (it held it).
    pub async fn agent_remove(&self, team_id: Uuid, name: &str) -> Result<TeamSync> {
        let agent = self.find_agent(team_id, name).await?;
        let members = self.client.list_members(team_id).await?;
        let email = members
            .iter()
            .find(|m| m.member_id == agent.member_id)
            .map(|m| m.email.clone())
            .ok_or_else(|| anyhow!("agent '{name}' is not in the member list"))?;
        self.remove(team_id, &email).await
    }

    /// Open approval requests addressed to this admin device that it has not decided yet.
    pub async fn pending_approvals(&self, team_id: Uuid) -> Result<Vec<IncomingRequest>> {
        self.require_admin(team_id).await?;
        let ep = self.relay_endpoint(team_id)?;
        let members = self.client.list_members(team_id).await?;
        let admins = admin_peers(&members);
        let agents: Vec<Peer> = self
            .client
            .list_agents(team_id)
            .await?
            .iter()
            .map(|a| device_peer(&a.name, a.account_id, &a.device))
            .collect();
        let (reqs, _) = ep.pending_requests(&agents, 0, 0).await?;
        let done = ep.seen_decisions(&admins).await?;
        Ok(reqs
            .into_iter()
            .filter(|r| {
                !done.iter().any(|(req, acct)| {
                    *req == r.relay_request_id && *acct == self.identity.account_id
                })
            })
            .collect())
    }

    /// Approve or deny the pending request whose id starts with `prefix`.
    pub async fn decide_approval(
        &self,
        team_id: Uuid,
        prefix: &str,
        approve: bool,
        reason: Option<String>,
    ) -> Result<IncomingRequest> {
        let pending = self.pending_approvals(team_id).await?;
        let matches: Vec<_> = pending
            .iter()
            .filter(|r| {
                r.payload.request.request_id.starts_with(prefix)
                    || r.relay_request_id.to_string().starts_with(prefix)
            })
            .collect();
        let req = match matches.as_slice() {
            [one] => (*one).clone(),
            [] => bail!("no pending request starting with '{prefix}'"),
            _ => bail!("'{prefix}' matches several requests; use more of the id"),
        };
        let ep = self.relay_endpoint(team_id)?;
        let members = self.client.list_members(team_id).await?;
        ep.send_decision(
            &req,
            approve,
            &self.identity.email,
            reason,
            None,
            &admin_peers(&members),
        )
        .await?;
        Ok(req)
    }
}
