//! Policy engine: scope, deny > ask > allow, tiers, grants, budgets.
//!
//! Owner: W3. Stub from the scaffold; replace freely.

use super::store::AiStore;
use super::types::{Analysis, Decision, HostFacts, Policy, PolicyEngine, ToolCall};
use crate::config::AppConfig;
use crate::models::Connection;
use std::sync::Arc;

/// Policy by name from `config.ai.policies`, with its profile's defaults applied.
/// A missing `default` policy resolves to the built-in `observe` profile.
pub fn resolve(config: &AppConfig, name: &str) -> anyhow::Result<Policy> {
    let _ = (config, name);
    anyhow::bail!("policy::resolve: not implemented yet")
}

/// Is this connection visible to the agent at all?
pub fn host_in_scope(policy: &Policy, conn: &Connection) -> bool {
    let _ = (policy, conn);
    false
}

pub struct DefaultPolicyEngine {
    store: Arc<AiStore>,
}

impl DefaultPolicyEngine {
    /// `store` gives access to active grants and budget counters.
    pub fn new(store: Arc<AiStore>) -> Self {
        Self { store }
    }
}

impl PolicyEngine for DefaultPolicyEngine {
    fn decide(
        &self,
        policy: &Policy,
        call: &ToolCall,
        analysis: Option<&Analysis>,
        host: &HostFacts,
    ) -> Decision {
        let _ = (&self.store, policy, call, analysis, host);
        Decision::Deny {
            reason: "policy engine not implemented yet".into(),
        }
    }
}
