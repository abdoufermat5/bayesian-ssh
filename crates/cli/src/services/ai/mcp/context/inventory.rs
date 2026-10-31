//! Where the inventory comes from, and which hosts a run may see.

use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::ai::types::Policy;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// Where the inventory comes from (the bssh database). A trait so tests use a fixed list.
#[expect(
    clippy::double_must_use,
    reason = "async_trait expands each method into a must_use boxed future and clippy flags the macro output"
)]
#[async_trait]
pub trait HostSource: Send + Sync {
    async fn list(&self) -> Result<Vec<Connection>>;
}

/// The real inventory.
pub struct DbHosts {
    config: AppConfig,
}

impl DbHosts {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

#[async_trait]
impl HostSource for DbHosts {
    async fn list(&self) -> Result<Vec<Connection>> {
        // Plain sync database read (SshService is not Sync, so keep it out of the future).
        crate::database::Database::new(&self.config)?.list_connections(None, false)
    }
}

/// Fixed inventory (tests, gateway previews).
pub struct StaticHosts(pub Vec<Connection>);

#[async_trait]
impl HostSource for StaticHosts {
    async fn list(&self) -> Result<Vec<Connection>> {
        Ok(self.0.clone())
    }
}

/// Visibility check: policy, connection, and whether that connection's tags may relax the
/// policy (`ai.trusted_tag_sources`). Most implementations are [`crate::services::ai::policy::host_in_scope`].
pub type ScopeFn = Arc<dyn Fn(&Policy, &Connection, bool) -> bool + Send + Sync>;
