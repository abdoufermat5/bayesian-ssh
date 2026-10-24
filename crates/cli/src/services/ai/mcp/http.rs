//! The MCP server on rmcp's streamable HTTP transport, mounted by the `bssh ai serve`
//! gateway (`gateway::McpBackend`). The gateway has already checked the agent's token; this
//! side builds the run: the signed policy, the relay broker for approvals, the local CA.
//!
//! One run per gateway, not per HTTP session: with protocol `2026-07-28` rmcp serves requests
//! statelessly and calls the factory for every request, which would reset budgets and the TTL
//! on each call. A new run starts only when the current one stopped (kill switch, circuit
//! breaker, TTL).

use super::context::{DbHosts, RunContext, RunDeps};
use super::files::SftpFiles;
use super::server::build_server;
use crate::services::ai::analyzer::ShellAnalyzer;
use crate::services::ai::audit::LocalAudit;
use crate::services::ai::exec::AiExecutor;
use crate::services::ai::gateway::{GatewayContext, McpBackend};
use crate::services::ai::policy::{self, DefaultPolicyEngine};
use crate::services::ai::redact::SecretRedactor;
use crate::services::ai::store::AiStore;
use crate::services::ai::types::AuditSink;
use anyhow::Result;
use axum::Router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use std::sync::{Arc, Mutex};
use tracing::info;

/// `McpBackend` backed by the real MCP server (replaces `gateway::UnwiredMcp`).
pub struct RmcpBackend;

impl McpBackend for RmcpBackend {
    fn router(&self, ctx: GatewayContext) -> Result<Router> {
        let runs = Arc::new(GatewayRuns {
            store: Arc::new(AiStore::open(&ctx.config)?),
            audit: Arc::new(LocalAudit::open(&ctx.config)?),
            gw: ctx,
            current: Mutex::new(None),
        });
        // Every request already carries a valid agent token (checked by the gateway's
        // middleware), so DNS rebinding can't reach a tool; the gateway may sit behind a
        // reverse proxy with any host name.
        let config = StreamableHttpServerConfig::default().disable_allowed_hosts();
        let service = StreamableHttpService::new(
            move || Ok(build_server(runs.current())),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        Ok(Router::new().fallback_service(service))
    }
}

struct GatewayRuns {
    gw: GatewayContext,
    store: Arc<AiStore>,
    audit: Arc<LocalAudit>,
    current: Mutex<Option<Arc<RunContext>>>,
}

impl GatewayRuns {
    /// The live run, or a new one when there is none or it has stopped.
    fn current(&self) -> Arc<RunContext> {
        let mut cur = match self.current.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(ctx) = cur.as_ref().filter(|c| !c.is_stopped()) {
            return ctx.clone();
        }
        let ctx = self.new_run();
        info!(
            "bssh ai serve: run {} for agent {} (policy {} {})",
            ctx.run_id, self.gw.agent, self.gw.policy_name, self.gw.policy_hash
        );
        *cur = Some(ctx.clone());
        ctx
    }

    fn new_run(&self) -> Arc<RunContext> {
        let gw = &self.gw;
        let policy = gw.policy.clone();
        let deps = RunDeps {
            hosts: Arc::new(DbHosts::new(&gw.config)),
            files: Arc::new(SftpFiles::new(&gw.config)),
            executor: Arc::new(
                AiExecutor::new(&gw.config).with_read_paths(policy.read_paths.clone()),
            ),
            broker: gw.broker.clone(),
            analyzer: Arc::new(ShellAnalyzer::new()),
            redactor: Arc::new(SecretRedactor::new(Vec::new(), &policy.redact)),
            engine: Arc::new(DefaultPolicyEngine::new(self.store.clone())),
            audit: self.audit.clone() as Arc<dyn AuditSink>,
            scope: Arc::new(policy::host_in_scope),
            // Nobody sits at an unattended agent's client: approvals go out of band.
            elicitor: None,
        };
        RunContext::new(gw.config.clone(), gw.policy_name.clone(), policy, deps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::services::ai::types::{ApprovalBroker, ApprovalOutcome, ApprovalRequest, Policy};
    use async_trait::async_trait;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct DenyAll;

    #[async_trait]
    impl ApprovalBroker for DenyAll {
        async fn request(&self, _req: ApprovalRequest) -> anyhow::Result<ApprovalOutcome> {
            Ok(ApprovalOutcome::Denied {
                by: "test".into(),
                reason: None,
            })
        }
    }

    #[tokio::test]
    async fn gateway_backend_answers_mcp_initialize_over_http() {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            database_path: dir.path().join("history.db"),
            ..AppConfig::default()
        };
        let ctx = GatewayContext {
            config,
            team_id: uuid::Uuid::nil(),
            agent: "ops/oncall".into(),
            policy_name: "default".into(),
            policy: Policy::default(),
            policy_hash: "test".into(),
            broker: Arc::new(DenyAll),
        };
        let app = RmcpBackend.router(ctx).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"ci-bot","version":"1"}}}"#;
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        );
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut resp = String::new();
        let read = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut buf = [0u8; 4096];
            loop {
                let n = s.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                resp.push_str(&String::from_utf8_lossy(&buf[..n]));
                if resp.contains("\"serverInfo\"") {
                    break;
                }
            }
        })
        .await;
        assert!(read.is_ok(), "no answer in time: {resp}");
        assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
        assert!(resp.contains("bssh-ai"), "{resp}");
    }
}
