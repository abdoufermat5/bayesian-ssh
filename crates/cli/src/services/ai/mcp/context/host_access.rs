//! Host visibility, access grants and per-host enforcement facts.

use super::error::ToolError;
use super::run_context::AccessGrant;
use super::run_context::RunContext;
use crate::models::Connection;
use crate::services::ai::types::{Enforcement, ExecMode, HostFacts, JailKind, Scope};
use crate::services::ai::{jail, policy};
use anyhow::Result;
use std::time::{Duration, Instant};

impl RunContext {
    pub fn in_scope(&self, conn: &Connection) -> bool {
        if (self.deps.scope)(&self.policy, conn, self.tags_trusted(conn)) {
            return true;
        }
        let now = Instant::now();
        let Ok(mut grants) = self.access.lock() else {
            return false;
        };
        grants.retain(|g| g.expires > now);
        grants.iter().any(|g| {
            g.scope.hosts.iter().any(|h| h == &conn.name)
                || g.scope
                    .tags
                    .iter()
                    .any(|t| conn.tags.iter().any(|ct| ct == t))
        })
    }

    pub async fn visible_hosts(&self) -> Result<Vec<Connection>, ToolError> {
        let all = self.deps.hosts.list().await?;
        Ok(all.into_iter().filter(|c| self.in_scope(c)).collect())
    }

    /// Host by name or alias, only when in scope. Unknown and out-of-scope look the same to
    /// the agent; argument mistakes and probes never trip the circuit breaker.
    pub async fn resolve_host(&self, name: &str) -> Result<Connection, ToolError> {
        let all = self.deps.hosts.list().await?;
        let found = all
            .into_iter()
            .find(|c| c.name == name || c.aliases.iter().any(|a| a == name));
        match found {
            Some(c) if self.in_scope(&c) => Ok(c),
            _ => Err(ToolError::Invalid(format!(
                "unknown host '{name}' (use list_hosts to see the hosts you can reach)"
            ))),
        }
    }

    /// Grant wider scope for a while (after an approved `request_access`).
    pub fn grant_access(&self, scope: Scope, duration: Duration) {
        if let Ok(mut g) = self.access.lock() {
            g.push(AccessGrant {
                scope,
                expires: Instant::now() + duration,
            });
        }
    }

    /// Enforcement facts for a host, probed once per run.
    ///
    /// A probe that failed is *not* cached: caching "no jail, no certificates" would
    /// downgrade the host for the rest of the run after one network hiccup. The error is
    /// reported in [`HostFacts::probe_error`] and the next call tries again.
    pub async fn facts(&self, conn: &Connection) -> HostFacts {
        let cached = self.facts.lock().await.get(&conn.name).cloned();
        let (enforcement, probe_error) = match cached {
            Some(e) => (e, None),
            None => {
                let mut errors: Vec<String> = Vec::new();
                let interactive_bastion = conn.use_kerberos && conn.bastion.is_some();
                // One connection for the jail trials and the agent check: through a
                // bastion each exec is another login.
                let (jail_kind, agent_user) =
                    match jail::probe_with_agent(conn, self.deps.executor.as_ref()).await {
                        Ok(found) => found,
                        Err(e) => {
                            errors.push(format!("probe: {e:#}"));
                            (JailKind::None, false)
                        }
                    };
                // A certificate makes the target login bssh-agent, which cannot pass
                // through a bastion that logs in as the user: never promise it there.
                let e = Enforcement {
                    jail: jail_kind,
                    certificates: !interactive_bastion
                        && crate::services::ai::cert::ca_available(&self.config)
                        && agent_user,
                    agent_user,
                };
                let error = if errors.is_empty() {
                    self.facts.lock().await.insert(conn.name.clone(), e.clone());
                    None
                } else {
                    Some(errors.join("; "))
                };
                (e, error)
            }
        };
        let source = self.source_name(conn);
        HostFacts {
            name: conn.name.clone(),
            tags: conn.tags.clone(),
            tags_trusted: policy::tags_trusted(
                &self.config.ai.trusted_tag_sources,
                conn,
                source.as_deref(),
            )
            .is_trusted(),
            tag_source: source,
            enforcement,
            probe_error,
        }
    }

    /// The inventory source a connection came from, resolved once per source id.
    fn source_name(&self, conn: &Connection) -> Option<String> {
        let id = conn.source_id?;
        if let Ok(cache) = self.source_names.lock()
            && let Some(name) = cache.get(&id)
        {
            return name.clone();
        }
        let name = crate::services::ai::source_name(&self.config, conn);
        if let Ok(mut cache) = self.source_names.lock() {
            cache.insert(id, name.clone());
        }
        name
    }

    /// May this connection's tags relax the policy? Sources other than bssh's own
    /// connections count only when `ai.trusted_tag_sources` names them.
    fn tags_trusted(&self, conn: &Connection) -> policy::TagTrust {
        let source = self.source_name(conn);
        policy::tags_trusted(&self.config.ai.trusted_tag_sources, conn, source.as_deref())
    }

    /// Set a host's enforcement without probing it (gateway-side knowledge, tests).
    pub async fn prime_enforcement(&self, host: &str, e: Enforcement) {
        self.facts.lock().await.insert(host.to_string(), e);
    }

    /// Mode for read-only work: the host's jail, run as the low-privilege agent principal
    /// when the host trusts our CA (so deny paths are enforced by that user's permissions).
    pub fn read_mode(facts: &HostFacts) -> ExecMode {
        match facts.enforcement.jail {
            JailKind::None => ExecMode::Plain,
            k if facts.enforcement.certificates => ExecMode::JailedAsAgent(k),
            k => ExecMode::Jailed(k),
        }
    }

    /// Mode for writes: a forced-command certificate when the host trusts our CA.
    pub fn write_mode(facts: &HostFacts) -> ExecMode {
        if facts.enforcement.certificates {
            ExecMode::Certified
        } else {
            ExecMode::Plain
        }
    }
}
