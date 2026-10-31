//! Discovery of, and sessions on, the run sockets (Unix only).

use super::hub::{AiHub, RunHello};
use std::collections::HashSet;
use std::sync::Arc;

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::services::ai::ipc::{ClientMsg, ServerMsg};

use bayesian_ssh::services::ai::ai_dir;
use bayesian_ssh::services::ai::ipc::IPC_VERSION;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::JoinHandle;

/// How the desktop introduces itself in `ClientMsg::Hello`.
const CLIENT_NAME: &str = "bssh-desktop";
/// How often `<ai_dir>/runs` is listed.
const SCAN_INTERVAL: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

enum Slot {
    /// A session task owns the socket.
    Live(JoinHandle<()>),
    /// Connecting failed: skipped until the socket file changes.
    Dead(Option<SystemTime>),
}

/// Tracks which `*.sock` files already have a session.
#[derive(Default)]
pub struct Discovery {
    slots: HashMap<PathBuf, Slot>,
}

impl Discovery {
    /// One pass over `<ai_dir>/runs`: connects to sockets without a session.
    pub async fn scan(&mut self, hub: &Arc<AiHub>, runs_dir: &Path) {
        let mut found = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(runs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "sock") {
                    found.insert(path);
                }
            }
        }
        self.slots.retain(|path, slot| match slot {
            Slot::Live(handle) => !handle.is_finished() || found.contains(path),
            Slot::Dead(_) => found.contains(path),
        });
        for path in found {
            match self.slots.get(&path) {
                Some(Slot::Live(handle)) if !handle.is_finished() => continue,
                Some(Slot::Dead(seen)) if *seen == mtime(&path) => continue,
                _ => {}
            }
            let stream = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(&path)).await;
            match stream {
                Ok(Ok(stream)) => {
                    let hub = Arc::clone(hub);
                    let handle = tokio::spawn(session(hub, stream));
                    self.slots.insert(path, Slot::Live(handle));
                }
                other => {
                    // Left behind by a process that died, or not ours.
                    tracing::debug!(path = %path.display(), "ai socket not reachable: {other:?}");
                    let seen = mtime(&path);
                    self.slots.insert(path, Slot::Dead(seen));
                }
            }
        }
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn encode(msg: &ClientMsg) -> Option<Vec<u8>> {
    let mut line = serde_json::to_vec(msg).ok()?;
    line.push(b'\n');
    Some(line)
}

/// Speaks the protocol on one connection until it closes.
async fn session(hub: Arc<AiHub>, stream: UnixStream) {
    let (read, mut write) = stream.into_split();
    let (tx, mut rx) = unbounded_channel::<ClientMsg>();
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let Some(line) = encode(&msg) else { continue };
            if write.write_all(&line).await.is_err() || write.flush().await.is_err() {
                break;
            }
        }
    });

    let _ = tx.send(ClientMsg::Hello {
        version: IPC_VERSION,
        client: CLIENT_NAME.to_string(),
    });
    let _ = tx.send(ClientMsg::Subscribe);
    let _ = tx.send(ClientMsg::ListPending);

    let mut lines = BufReader::new(read).lines();
    let mut attached: Option<(String, u64)> = None;
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => {
                tracing::debug!("ai socket read failed: {e}");
                break;
            }
        };
        let msg = match serde_json::from_str::<ServerMsg>(&line) {
            Ok(msg) => msg,
            Err(e) => {
                // A newer server may send variants this build doesn't know.
                tracing::warn!("ignoring unreadable ai socket message: {e}");
                continue;
            }
        };
        match (&attached, msg) {
            (
                None,
                ServerMsg::Hello {
                    version,
                    run_id,
                    client,
                    policy,
                    started_at,
                    ..
                },
            ) => {
                if version != IPC_VERSION {
                    tracing::warn!(version, "ai run speaks another protocol version");
                    hub.apply(|_, fx| {
                            fx.errors.push((
                                run_id.clone(),
                                format!(
                                    "Run {run_id} uses protocol version {version}; this app uses {IPC_VERSION}. Update bssh."
                                ),
                            ));
                        });
                    break;
                }
                let conn_id = hub.attach(
                    RunHello {
                        run_id: run_id.clone(),
                        client,
                        policy,
                        started_at,
                    },
                    tx.clone(),
                );
                attached = Some((run_id, conn_id));
            }
            (Some((run_id, _)), msg) => hub.on_message(run_id, msg),
            (None, _) => tracing::debug!("ai socket message before Hello ignored"),
        }
    }
    if let Some((run_id, conn_id)) = attached {
        hub.detach(&run_id, conn_id);
    }
    writer.abort();
}

/// `<ai_dir>/runs` of the active environment. The config is only reloaded
/// when the environment changes.
#[derive(Default)]
struct RunsDirResolver {
    env: Option<String>,
    dir: Option<PathBuf>,
}

impl RunsDirResolver {
    fn resolve(&mut self) -> Option<PathBuf> {
        let env = AppConfig::active_env();
        if self.env.as_deref() != Some(env.as_str()) || self.dir.is_none() {
            self.dir = AppConfig::load(None).ok().map(|config| {
                bayesian_ssh::services::ai::control_client::runs_dir(&ai_dir(&config))
            });
            self.env = Some(env);
        }
        self.dir.clone()
    }
}

/// Lists the runs directory forever, connecting to new sockets.
pub async fn discovery_loop(hub: Arc<AiHub>) {
    let mut resolver = RunsDirResolver::default();
    let mut discovery = Discovery::default();
    loop {
        if let Some(dir) = resolver.resolve() {
            discovery.scan(&hub, &dir).await;
        }
        tokio::time::sleep(SCAN_INTERVAL).await;
    }
}
