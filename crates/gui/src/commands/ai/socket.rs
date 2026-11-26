//! Discovery of, and sessions on, the run sockets (Unix only).

use super::hub::{AiHub, RunHello};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bayesian_ssh::config::AppConfig;
use bayesian_ssh::services::ai::ai_dir;
use bayesian_ssh::services::ai::ipc::{ClientMsg, IPC_VERSION, ServerMsg};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::JoinHandle;

/// How the desktop introduces itself in `ClientMsg::Hello`.
const CLIENT_NAME: &str = "bssh-desktop";
/// How often `<ai_dir>/runs` is listed.
const SCAN_INTERVAL: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum size of a single JSONL IPC line accepted from a run socket (1 MiB).
const MAX_IPC_LINE_BYTES: usize = 1024 * 1024;

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
        let dir = runs_dir.to_path_buf();
        let found: Vec<(PathBuf, Option<SystemTime>)> = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "sock") {
                        let seen = mtime(&path);
                        out.push((path, seen));
                    }
                }
            }
            out
        })
        .await
        .unwrap_or_default();

        let found_set: HashSet<PathBuf> = found.iter().map(|(p, _)| p.clone()).collect();
        self.slots.retain(|path, slot| match slot {
            Slot::Live(handle) => !handle.is_finished() || found_set.contains(path),
            Slot::Dead(_) => found_set.contains(path),
        });
        for (path, seen_mtime) in found {
            match self.slots.get(&path) {
                Some(Slot::Live(handle)) if !handle.is_finished() => continue,
                Some(Slot::Dead(seen)) if *seen == seen_mtime => continue,
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
                    self.slots.insert(path, Slot::Dead(seen_mtime));
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

enum BoundedLine {
    Line(String),
    Eof,
    TooLong,
}

async fn read_bounded_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<BoundedLine> {
    let mut buf = Vec::new();
    let n = reader
        .take((max_bytes + 1) as u64)
        .read_until(b'\n', &mut buf)
        .await?;
    if n == 0 {
        return Ok(BoundedLine::Eof);
    }
    if buf.len() > max_bytes {
        return Ok(BoundedLine::TooLong);
    }
    while matches!(buf.last(), Some(b'\n' | b'\r')) {
        buf.pop();
    }
    let line = String::from_utf8(buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(BoundedLine::Line(line))
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

    let mut reader = BufReader::new(read);
    let mut attached: Option<(String, u64)> = None;
    loop {
        let line = match read_bounded_line(&mut reader, MAX_IPC_LINE_BYTES).await {
            Ok(BoundedLine::Line(line)) => line,
            Ok(BoundedLine::Eof) => break,
            Ok(BoundedLine::TooLong) => {
                tracing::warn!(
                    max_bytes = MAX_IPC_LINE_BYTES,
                    "closing ai socket: IPC line exceeded maximum length"
                );
                break;
            }
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
    async fn resolve(&mut self) -> Option<PathBuf> {
        let prev_env = self.env.clone();
        let has_dir = self.dir.is_some();
        let resolved = tokio::task::spawn_blocking(move || {
            let env = AppConfig::active_env();
            if prev_env.as_deref() != Some(env.as_str()) || !has_dir {
                let dir = AppConfig::load(None).ok().map(|config| {
                    bayesian_ssh::services::ai::control_client::runs_dir(&ai_dir(&config))
                });
                Some((env, dir))
            } else {
                None
            }
        })
        .await
        .ok()
        .flatten();

        if let Some((env, dir)) = resolved {
            self.env = Some(env);
            self.dir = dir;
        }
        self.dir.clone()
    }
}

/// Lists the runs directory forever, connecting to new sockets.
pub async fn discovery_loop(hub: Arc<AiHub>) {
    let mut resolver = RunsDirResolver::default();
    let mut discovery = Discovery::default();
    loop {
        if let Some(dir) = resolver.resolve().await {
            discovery.scan(&hub, &dir).await;
        }
        tokio::time::sleep(SCAN_INTERVAL).await;
    }
}
