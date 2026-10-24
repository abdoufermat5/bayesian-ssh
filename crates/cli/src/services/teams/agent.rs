//! Agent identities (bssh AI, Teams): the credential file an admin creates for
//! an unattended agent, and the runtime the gateway builds from it.
//!
//! The credential holds the agent's private device keys (0600, never sent),
//! the pinned trust root and the pinned admin approvers. The agent trusts
//! only those keys: for policies (admin signature) and for approval
//! decisions (relay).

use super::client::TeamsClient;
use super::crypto::{self, DeviceKeys};
use super::relay::Peer;
use super::session::{self, Session};
use super::store::{TeamLocal, TeamsStore};
use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Source, SourceKind};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use bssh_teams_proto::api::SignedAgentPolicy;
use bssh_teams_proto::wire::PublicKey32;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;
use zeroize::Zeroizing;

pub const CREDENTIAL_VERSION: u8 = 1;

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentCredential {
    pub version: u8,
    pub server_url: String,
    pub team_id: Uuid,
    pub team_name: String,
    pub agent_name: String,
    pub account_id: Uuid,
    pub device_id: Uuid,
    /// Base64 of `x25519_secret ‖ ed25519_seed` (64 bytes).
    device_secrets: String,
    /// The team's trust root (creating admin device), pinned.
    pub trust_root: PublicKey32,
    /// Admin devices whose signatures the agent accepts (policies, decisions).
    pub approvers: Vec<Peer>,
    /// Policy name the gateway serves for this agent.
    pub policy: String,
}

impl std::fmt::Debug for AgentCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AgentCredential({}/{}, <redacted>)",
            self.team_name, self.agent_name
        )
    }
}

impl AgentCredential {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        server_url: String,
        team_id: Uuid,
        team_name: String,
        agent_name: String,
        account_id: Uuid,
        device_id: Uuid,
        keys: &DeviceKeys,
        trust_root: PublicKey32,
        approvers: Vec<Peer>,
        policy: String,
    ) -> Self {
        Self {
            version: CREDENTIAL_VERSION,
            server_url,
            team_id,
            team_name,
            agent_name,
            account_id,
            device_id,
            device_secrets: STANDARD.encode(&keys.to_secret_bytes()[..]),
            trust_root,
            approvers,
            policy,
        }
    }

    pub fn device_keys(&self) -> Result<DeviceKeys> {
        let raw = Zeroizing::new(
            STANDARD
                .decode(self.device_secrets.as_bytes())
                .context("credential: bad key encoding")?,
        );
        Ok(DeviceKeys::from_secret_bytes(&raw)?)
    }

    /// `<team>/<agent>`, as `bssh ai token` takes it.
    pub fn id(&self) -> String {
        format!("{}/{}", self.team_name, self.agent_name)
    }

    /// Write the file with mode 0600; refuses to overwrite.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        write_private(path, &json)
    }

    /// Read a credential; refuses a file other users can read.
    pub fn load(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)
                .with_context(|| format!("reading {}", path.display()))?
                .permissions()
                .mode();
            if mode & 0o077 != 0 {
                bail!(
                    "{} is readable by other users (mode {:o}); run `chmod 600` on it",
                    path.display(),
                    mode & 0o777
                );
            }
        }
        let cred: Self = serde_json::from_slice(&std::fs::read(path)?)
            .with_context(|| format!("parsing {}", path.display()))?;
        if cred.version != CREDENTIAL_VERSION {
            bail!("unsupported agent credential version {}", cred.version);
        }
        Ok(cred)
    }
}

fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .with_context(|| format!("creating {} (does it already exist?)", path.display()))?;
    f.write_all(data)?;
    Ok(())
}

/// Where `bssh teams agent add` writes a credential.
pub fn credential_path(config: &crate::config::AppConfig, team_name: &str, agent: &str) -> PathBuf {
    crate::services::ai::ai_dir(config)
        .join("agents")
        .join(format!("{team_name}.{agent}.json"))
}

fn agents_dir(config: &AppConfig) -> PathBuf {
    crate::services::ai::ai_dir(config).join("agents")
}

/// Find a credential by `team/agent` in the default location.
pub fn find_credential(config: &crate::config::AppConfig, id: &str) -> Result<AgentCredential> {
    let (team, agent) = id
        .split_once('/')
        .ok_or_else(|| anyhow!("expected <team>/<agent>, got '{id}'"))?;
    if team.contains(['/', '\\']) || agent.contains(['/', '\\']) || team.contains("..") {
        bail!("invalid agent id '{id}'");
    }
    AgentCredential::load(&credential_path(config, team, agent))
}

/// Highest policy version seen per (agent, name), so a server that replays an
/// older signed policy is refused.
fn state_path(cred_path_dir: &Path, cred: &AgentCredential) -> PathBuf {
    cred_path_dir.join(format!("{}.{}.state.json", cred.team_name, cred.agent_name))
}

/// A policy that passed signature, agent and rollback checks.
#[derive(Debug, Clone)]
pub struct VerifiedPolicy {
    pub policy: crate::services::ai::types::Policy,
    pub signed: SignedAgentPolicy,
    /// SHA-256 hex of the signed policy text.
    pub hash: String,
}

/// Pick, verify and parse the agent's signed policy `name` from vault entries.
///
/// Accepts only a signature by a pinned approver (or the trust root), for this
/// agent's device, and never a version lower than `min_version`.
pub fn verify_policy(
    cred: &AgentCredential,
    candidates: &[SignedAgentPolicy],
    name: &str,
    min_version: u64,
) -> Result<VerifiedPolicy> {
    let mut best: Option<&SignedAgentPolicy> = None;
    for p in candidates {
        if p.name != name || p.agent_device_id != cred.device_id {
            continue;
        }
        let Some(signer) = cred
            .approvers
            .iter()
            .find(|a| a.device_id == p.signer_device_id)
        else {
            tracing::warn!("policy '{name}' signed by an unpinned device: ignored");
            continue;
        };
        if crypto::verify_policy(cred.team_id, p, &signer.ed25519).is_err() {
            tracing::warn!("policy '{name}' has a bad signature: ignored");
            continue;
        }
        if best.is_none_or(|b| p.version > b.version) {
            best = Some(p);
        }
    }
    let signed = best.ok_or_else(|| {
        anyhow!(
            "no admin-signed policy '{name}' for agent {} in the team vault: \
             ask an admin to run `bssh teams agent policy {} {} --policy {name}`",
            cred.agent_name,
            cred.team_name,
            cred.agent_name
        )
    })?;
    if signed.version < min_version {
        bail!(
            "policy '{name}' is at version {} but version {min_version} was seen before: \
             refusing to go back (possible replay)",
            signed.version
        );
    }
    let policy = serde_json::from_str(&signed.policy_json).context("parsing the signed policy")?;
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(signed.policy_json.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(VerifiedPolicy {
        policy,
        signed: signed.clone(),
        hash,
    })
}

/// Read the highest version recorded for `name`.
pub fn load_min_version(dir: &Path, cred: &AgentCredential, name: &str) -> u64 {
    std::fs::read(state_path(dir, cred))
        .ok()
        .and_then(|b| serde_json::from_slice::<std::collections::BTreeMap<String, u64>>(&b).ok())
        .and_then(|m| m.get(name).copied())
        .unwrap_or(0)
}

pub fn store_min_version(dir: &Path, cred: &AgentCredential, name: &str, v: u64) -> Result<()> {
    let path = state_path(dir, cred);
    let mut m: std::collections::BTreeMap<String, u64> = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if m.get(name).is_none_or(|old| *old < v) {
        m.insert(name.to_string(), v);
        std::fs::write(&path, serde_json::to_vec(&m)?)?;
    }
    Ok(())
}

/// Outcome of [`AgentRuntime::refresh`].
#[derive(Debug)]
pub struct Refreshed {
    pub policy: VerifiedPolicy,
    pub hosts: usize,
}

/// Make sure `db` has a local source for the team (hosts are filed under it).
fn ensure_team_source(db: &Database, team_id: Uuid, team_name: &str) -> Result<()> {
    if session::local_teams(db)?
        .iter()
        .any(|(id, _)| *id == team_id)
    {
        return Ok(());
    }
    let name = if db.get_source(team_name)?.is_some() {
        format!("team-{}", &team_id.to_string()[..8])
    } else {
        team_name.to_string()
    };
    db.add_source(&Source::new(
        name,
        SourceKind::Team,
        serde_json::json!({ "team_id": team_id.to_string() }),
    ))
}

/// What the gateway holds for one agent.
pub struct AgentRuntime {
    pub cred: AgentCredential,
    pub keys: Arc<DeviceKeys>,
    pub client: Arc<TeamsClient>,
}

impl AgentRuntime {
    pub fn new(cred: AgentCredential) -> Result<Self> {
        let keys = Arc::new(cred.device_keys()?);
        let client = Arc::new(TeamsClient::new(
            &cred.server_url,
            cred.device_id,
            keys.clone(),
        )?);
        Ok(Self { cred, keys, client })
    }

    /// The agent's own Teams store (a separate database next to the credential),
    /// so it never mixes with a human's identity on the same machine.
    pub fn open_store(&self, config: &AppConfig) -> Result<(AppConfig, TeamsStore)> {
        let dir = agents_dir(config);
        std::fs::create_dir_all(&dir)?;
        let mut agent_config = config.clone();
        agent_config.database_path = dir.join(format!(
            "{}.{}.db",
            self.cred.team_name, self.cred.agent_name
        ));
        let store = TeamsStore::open(&agent_config)?;
        let db = store.database();
        db.conn.execute(
            "INSERT OR REPLACE INTO teams_identity (id, server_url, email, account_id, device_id, created_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                self.cred.server_url,
                format!("agent:{}", self.cred.agent_name),
                self.cred.account_id.to_string(),
                self.cred.device_id.to_string(),
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
        store.upsert_team(&TeamLocal {
            team_id: self.cred.team_id,
            server_url: self.cred.server_url.clone(),
            account_id: self.cred.account_id,
            device_id: self.cred.device_id,
            trust_root_ed25519: self.cred.trust_root,
            last_pulled_revision: 0,
            created_at: chrono::Utc::now(),
        })?;
        ensure_team_source(db, self.cred.team_id, &self.cred.team_name)?;
        Ok((agent_config, store))
    }

    /// Sync the vault (also uploads queued AI audit events), verify the signed
    /// policy and mirror the team's hosts into the main database.
    pub async fn refresh(
        &self,
        config: &AppConfig,
        agent_config: &AppConfig,
        store: &TeamsStore,
    ) -> Result<Refreshed> {
        let session = Session::with_keys(store, agent_config, self.keys.clone())?;
        let sync = session.sync_team(self.cred.team_id, vec![]).await?;
        if sync.waiting_for_key {
            bail!("this agent has no team key yet: an admin must run `bssh teams grant`");
        }
        let view = session
            .vault_view(self.cred.team_id)
            .await?
            .ok_or_else(|| anyhow!("no team key"))?;
        let candidates: Vec<SignedAgentPolicy> = view
            .agent_policies
            .iter()
            .map(|p| p.policy.clone())
            .collect();
        let dir = agents_dir(config);
        let min = load_min_version(&dir, &self.cred, &self.cred.policy);
        let policy = verify_policy(&self.cred, &candidates, &self.cred.policy, min)?;
        store_min_version(&dir, &self.cred, &self.cred.policy, policy.signed.version)?;

        let db = Database::new(config)?;
        ensure_team_source(&db, self.cred.team_id, &self.cred.team_name)?;
        session::materialize_into(&db, config, self.cred.team_id, &view)?;
        Ok(Refreshed {
            policy,
            hosts: view.connections.len(),
        })
    }

    /// Is this agent still a live member? An admin removing it is the kill
    /// switch: the gateway stops serving as soon as this fails.
    pub async fn check_active(&self) -> Result<()> {
        let agents = self.client.list_agents(self.cred.team_id).await?;
        match agents
            .iter()
            .find(|a| a.device.device_id == self.cred.device_id)
        {
            Some(a) if a.device.revoked_at.is_none() => Ok(()),
            _ => bail!("agent {} is no longer part of the team", self.cred.id()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bssh_teams_proto::wire::Fixed;
    use chrono::Utc;

    fn setup() -> (AgentCredential, DeviceKeys, Uuid) {
        let admin = DeviceKeys::generate();
        let admin_dev = Uuid::new_v4();
        let agent = DeviceKeys::generate();
        let approvers = vec![Peer {
            device_id: admin_dev,
            account_id: Uuid::new_v4(),
            label: "alice@x.io".into(),
            ed25519: admin.ed25519_public(),
            x25519: admin.x25519_public(),
        }];
        let cred = AgentCredential::new(
            "https://teams.example.com".into(),
            Uuid::new_v4(),
            "ops".into(),
            "deployer".into(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            &agent,
            admin.ed25519_public(),
            approvers,
            "default".into(),
        );
        (cred, admin, admin_dev)
    }

    fn signed(
        cred: &AgentCredential,
        admin: &DeviceKeys,
        dev: Uuid,
        v: u64,
        json: &str,
    ) -> SignedAgentPolicy {
        let mut p = SignedAgentPolicy {
            agent_device_id: cred.device_id,
            name: "default".into(),
            policy_json: json.into(),
            version: v,
            signed_at: Utc::now(),
            signer_device_id: dev,
            signature: Fixed::ZERO,
        };
        crypto::sign_policy(cred.team_id, &mut p, admin);
        p
    }

    #[test]
    fn credential_is_0600_and_round_trips() {
        let (cred, _, _) = setup();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/ops.deployer.json");
        cred.save(&path).unwrap();
        assert!(cred.save(&path).is_err(), "no overwrite");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let back = AgentCredential::load(&path).unwrap();
        assert_eq!(
            back.device_keys().unwrap().ed25519_public(),
            cred.device_keys().unwrap().ed25519_public()
        );
        assert_eq!(back.id(), "ops/deployer");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(AgentCredential::load(&path).is_err());
        }
    }

    #[test]
    fn only_a_pinned_valid_newest_policy_is_used() {
        let (cred, admin, dev) = setup();
        let v1 = signed(&cred, &admin, dev, 1, r#"{"profile":"observe"}"#);
        let v2 = signed(&cred, &admin, dev, 2, r#"{"profile":"operate"}"#);
        let ok = verify_policy(&cred, &[v1.clone(), v2.clone()], "default", 0).unwrap();
        assert_eq!(ok.signed.version, 2);

        // Widened after signing: bad signature, falls back to the older valid one.
        let mut widened = v2.clone();
        widened.version = 3;
        widened.policy_json = r#"{"profile":"oncall"}"#.into();
        assert_eq!(
            verify_policy(&cred, &[v1.clone(), widened], "default", 0)
                .unwrap()
                .signed
                .version,
            1
        );

        // Signed by an unpinned key: refused.
        let evil = DeviceKeys::generate();
        let forged = signed(&cred, &evil, dev, 9, r#"{"profile":"oncall"}"#);
        assert!(verify_policy(&cred, &[forged], "default", 0).is_err());
        // For another agent: refused.
        let mut other = v1.clone();
        other.agent_device_id = Uuid::new_v4();
        assert!(verify_policy(&cred, &[other], "default", 0).is_err());
        // Nothing signed at all.
        assert!(verify_policy(&cred, &[], "default", 0).is_err());
        // Rollback: server replays version 1 after we saw 2.
        assert!(verify_policy(&cred, &[v1], "default", 2).is_err());
    }

    #[test]
    fn min_version_only_goes_up() {
        let (cred, _, _) = setup();
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_min_version(dir.path(), &cred, "default"), 0);
        store_min_version(dir.path(), &cred, "default", 3).unwrap();
        store_min_version(dir.path(), &cred, "default", 2).unwrap();
        assert_eq!(load_min_version(dir.path(), &cred, "default"), 3);
    }
}
