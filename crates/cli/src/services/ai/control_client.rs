//! Client side of the local control socket (protocol in `ipc.rs`).
//!
//! Owner: W3. Used by `bssh ai approve | stop | status` and reusable by the desktop (W7).
//! Each running `bssh ai mcp` serves `<ai_dir>/runs/<run_id>.sock`; [`connect_all`] connects
//! to every live one and removes sockets whose process is gone.

use super::ipc::{ClientMsg, ServerMsg, IPC_VERSION};
use super::types::{ApprovalOutcome, ApprovalRequest, Grant};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the server said about its run in `ServerMsg::Hello`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RunHello {
    pub run_id: String,
    pub client: String,
    pub policy: String,
    pub started_at: String,
}

/// Directory holding the per-run sockets.
pub fn runs_dir(ai_dir: &Path) -> PathBuf {
    ai_dir.join("runs")
}

/// Socket path for a run.
pub fn socket_path(ai_dir: &Path, run_id: &str) -> PathBuf {
    runs_dir(ai_dir).join(format!("{run_id}.sock"))
}

/// Every `<ai_dir>/runs/*.sock`, sorted. Missing directory = no runs.
pub fn list_sockets(ai_dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = match std::fs::read_dir(runs_dir(ai_dir)) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "sock"))
            .collect(),
        Err(_) => Vec::new(),
    };
    out.sort();
    out
}

#[cfg(unix)]
pub use unix_impl::*;

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
    use tokio::net::UnixStream;

    /// One connection to a run's control socket.
    pub struct ControlClient {
        pub hello: RunHello,
        pub socket: PathBuf,
        reader: tokio::io::Lines<BufReader<OwnedReadHalf>>,
        writer: OwnedWriteHalf,
    }

    const IO_TIMEOUT: Duration = Duration::from_secs(10);

    impl ControlClient {
        /// Connect and do the Hello exchange. `name` identifies this client
        /// (`bssh-cli`, `bssh-desktop`).
        pub async fn connect(socket: &Path, name: &str) -> Result<Self> {
            let stream = tokio::time::timeout(IO_TIMEOUT, UnixStream::connect(socket))
                .await
                .map_err(|_| anyhow::anyhow!("timed out connecting to {}", socket.display()))?
                .with_context(|| format!("connect {}", socket.display()))?;
            let (rd, writer) = stream.into_split();
            let mut me = Self {
                hello: RunHello {
                    run_id: String::new(),
                    client: String::new(),
                    policy: String::new(),
                    started_at: String::new(),
                },
                socket: socket.to_path_buf(),
                reader: BufReader::new(rd).lines(),
                writer,
            };
            me.send(&ClientMsg::Hello {
                version: IPC_VERSION,
                client: name.to_string(),
            })
            .await?;
            match me.recv().await? {
                ServerMsg::Hello {
                    run_id,
                    client,
                    policy,
                    started_at,
                    ..
                } => {
                    me.hello = RunHello {
                        run_id,
                        client,
                        policy,
                        started_at,
                    };
                    Ok(me)
                }
                ServerMsg::Error { message } => bail!("{message}"),
                other => bail!("unexpected reply to hello: {other:?}"),
            }
        }

        pub async fn send(&mut self, msg: &ClientMsg) -> Result<()> {
            let mut line = serde_json::to_vec(msg)?;
            line.push(b'\n');
            tokio::time::timeout(IO_TIMEOUT, self.writer.write_all(&line))
                .await
                .map_err(|_| anyhow::anyhow!("timed out writing to the control socket"))??;
            Ok(())
        }

        /// Next message from the server (any kind). Errors when the server closes.
        pub async fn recv(&mut self) -> Result<ServerMsg> {
            match self.reader.next_line().await? {
                Some(l) => {
                    Ok(serde_json::from_str(&l).with_context(|| format!("bad message: {l}"))?)
                }
                None => bail!("the run closed the control socket"),
            }
        }

        /// Like [`recv`](Self::recv) with a timeout; `Ok(None)` on timeout.
        pub async fn recv_timeout(&mut self, wait: Duration) -> Result<Option<ServerMsg>> {
            match tokio::time::timeout(wait, self.recv()).await {
                Ok(r) => r.map(Some),
                Err(_) => Ok(None),
            }
        }

        /// Send a request and wait for its answer, skipping pushed messages
        /// (`NewRequest`, `Event`) that arrive in between.
        pub async fn request(&mut self, msg: &ClientMsg) -> Result<ServerMsg> {
            self.send(msg).await?;
            loop {
                let reply = tokio::time::timeout(IO_TIMEOUT, self.recv())
                    .await
                    .map_err(|_| anyhow::anyhow!("timed out waiting for the run to answer"))??;
                match reply {
                    ServerMsg::NewRequest { .. } | ServerMsg::Event { .. } => continue,
                    other => return Ok(other),
                }
            }
        }

        async fn expect_ok(&mut self, msg: &ClientMsg) -> Result<()> {
            match self.request(msg).await? {
                ServerMsg::Ok => Ok(()),
                ServerMsg::Error { message } => bail!("{message}"),
                other => bail!("unexpected reply: {other:?}"),
            }
        }

        pub async fn pending(&mut self) -> Result<Vec<ApprovalRequest>> {
            match self.request(&ClientMsg::ListPending).await? {
                ServerMsg::Pending { requests } => Ok(requests),
                ServerMsg::Error { message } => bail!("{message}"),
                other => bail!("unexpected reply: {other:?}"),
            }
        }

        pub async fn grants(&mut self) -> Result<Vec<Grant>> {
            match self.request(&ClientMsg::ListGrants).await? {
                ServerMsg::Grants { grants } => Ok(grants),
                ServerMsg::Error { message } => bail!("{message}"),
                other => bail!("unexpected reply: {other:?}"),
            }
        }

        pub async fn decide(&mut self, request_id: &str, outcome: ApprovalOutcome) -> Result<()> {
            self.expect_ok(&ClientMsg::Decide {
                request_id: request_id.to_string(),
                outcome,
            })
            .await
        }

        pub async fn revoke_grant(&mut self, grant_id: &str) -> Result<()> {
            self.expect_ok(&ClientMsg::RevokeGrant {
                grant_id: grant_id.to_string(),
            })
            .await
        }

        pub async fn stop(&mut self, reason: &str) -> Result<()> {
            self.expect_ok(&ClientMsg::Stop {
                reason: reason.to_string(),
            })
            .await
        }

        /// Start receiving events; read them with [`recv`](Self::recv).
        pub async fn subscribe(&mut self) -> Result<()> {
            self.expect_ok(&ClientMsg::Subscribe).await
        }
    }

    /// Connect to every live run socket under `ai_dir`. A socket nobody listens on
    /// (`ConnectionRefused`, left by a crashed run) is removed; other failures are skipped
    /// with a debug log.
    pub async fn connect_all(ai_dir: &Path, name: &str) -> Vec<ControlClient> {
        let mut out = Vec::new();
        for sock in list_sockets(ai_dir) {
            match ControlClient::connect(&sock, name).await {
                Ok(c) => out.push(c),
                Err(e) => {
                    let stale = e
                        .chain()
                        .filter_map(|c| c.downcast_ref::<std::io::Error>())
                        .any(|io| {
                            matches!(
                                io.kind(),
                                std::io::ErrorKind::ConnectionRefused
                                    | std::io::ErrorKind::NotFound
                            )
                        });
                    if stale {
                        tracing::debug!("removing stale socket {}", sock.display());
                        let _ = std::fs::remove_file(&sock);
                    } else {
                        tracing::debug!("skipping {}: {e}", sock.display());
                    }
                }
            }
        }
        out
    }
}

/// Placeholder so the CLI compiles on platforms without Unix sockets.
#[cfg(not(unix))]
pub struct ControlClient {
    pub hello: RunHello,
    pub socket: PathBuf,
}

#[cfg(not(unix))]
pub async fn connect_all(_ai_dir: &Path, _name: &str) -> Vec<ControlClient> {
    Vec::new()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::services::ai::approval::serve_control;
    use crate::services::ai::approval::tests::{broker, request, run_info, wait_pending};
    use crate::services::ai::types::{AiEvent, ApprovalBroker, Tier};
    use std::sync::Arc;
    use tokio::sync::{broadcast, watch};

    struct Server {
        _tmp: tempfile::TempDir,
        dir: PathBuf,
        broker: Arc<crate::services::ai::approval::LocalBroker>,
        events: broadcast::Sender<AiEvent>,
        stop: watch::Sender<bool>,
        handle: tokio::task::JoinHandle<Result<Option<String>>>,
    }

    async fn start() -> Server {
        let (tmp, b) = broker(30, None);
        let dir = tmp.path().join("ai");
        let sock = socket_path(&dir, "run1");
        let (events, _) = broadcast::channel(16);
        let (stop, stop_rx) = watch::channel(false);
        let handle = tokio::spawn(serve_control(
            sock.clone(),
            run_info(),
            b.clone(),
            events.clone(),
            stop_rx,
        ));
        for _ in 0..200 {
            if sock.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Server {
            _tmp: tmp,
            dir,
            broker: b,
            events,
            stop,
            handle,
        }
    }

    #[tokio::test]
    async fn round_trip_hello_pending_decide() {
        let s = start().await;
        let sock = socket_path(&s.dir, "run1");
        let mut c = ControlClient::connect(&sock, "test").await.unwrap();
        assert_eq!(c.hello.run_id, "run1");
        assert_eq!(c.hello.client, "test-client");
        assert!(c.pending().await.unwrap().is_empty());

        let b = s.broker.clone();
        let req = tokio::spawn(async move { b.request(request("q1", Tier::OutOfBand)).await });
        wait_pending(&s.broker, 1).await;
        // pushed without asking
        let pushed = c
            .recv_timeout(Duration::from_secs(5))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(pushed, ServerMsg::NewRequest { request } if request.request_id == "q1"));
        let pending = c.pending().await.unwrap();
        assert_eq!(pending.len(), 1);
        c.decide(
            "q1",
            ApprovalOutcome::Approved {
                by: "alice".into(),
                channel: "cli".into(),
                grant: None,
            },
        )
        .await
        .unwrap();
        let out = req.await.unwrap().unwrap();
        assert!(matches!(out, ApprovalOutcome::Approved { .. }));
        // deciding again is an error message, not a crash
        let err = c
            .decide(
                "q1",
                ApprovalOutcome::Denied {
                    by: "a".into(),
                    reason: None,
                },
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no pending request"));

        s.stop.send(true).unwrap();
        assert_eq!(s.handle.await.unwrap().unwrap(), None);
    }

    #[tokio::test]
    async fn two_clients_see_the_same_events() {
        let s = start().await;
        let sock = socket_path(&s.dir, "run1");
        let mut a = ControlClient::connect(&sock, "a").await.unwrap();
        let mut b = ControlClient::connect(&sock, "b").await.unwrap();
        a.subscribe().await.unwrap();
        b.subscribe().await.unwrap();
        let ev = AiEvent::RunEnded {
            run_id: "run1".into(),
            reason: "x".into(),
        };
        s.events.send(ev.clone()).unwrap();
        for c in [&mut a, &mut b] {
            match c
                .recv_timeout(Duration::from_secs(5))
                .await
                .unwrap()
                .unwrap()
            {
                ServerMsg::Event { event } => assert_eq!(event, ev),
                other => panic!("{other:?}"),
            }
        }
        s.stop.send(true).unwrap();
        s.handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn unsubscribed_client_gets_no_events() {
        let s = start().await;
        let sock = socket_path(&s.dir, "run1");
        let mut c = ControlClient::connect(&sock, "a").await.unwrap();
        s.events
            .send(AiEvent::RunEnded {
                run_id: "run1".into(),
                reason: "x".into(),
            })
            .unwrap();
        assert!(c
            .recv_timeout(Duration::from_millis(200))
            .await
            .unwrap()
            .is_none());
        s.stop.send(true).unwrap();
        s.handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn stop_message_ends_the_server_and_removes_the_socket() {
        let s = start().await;
        let sock = socket_path(&s.dir, "run1");
        let mut c = ControlClient::connect(&sock, "cli").await.unwrap();
        c.stop("user pressed stop").await.unwrap();
        let reason = s.handle.await.unwrap().unwrap();
        assert_eq!(reason.as_deref(), Some("user pressed stop"));
        assert!(!sock.exists(), "socket removed on exit");
    }

    #[tokio::test]
    async fn socket_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let s = start().await;
        let sock = socket_path(&s.dir, "run1");
        let mode = std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dmode = std::fs::metadata(runs_dir(&s.dir))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dmode, 0o700);
        s.stop.send(true).unwrap();
        s.handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn grants_list_and_revoke_over_the_socket() {
        use crate::services::ai::types::{Grant, ToolName};
        let s = start().await;
        s.broker
            .store()
            .add_grant(&Grant {
                grant_id: "g1".into(),
                run_id: Some("run1".into()),
                tool: ToolName::Service,
                hosts: "*".into(),
                target: None,
                expires_at: (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
            })
            .unwrap();
        let sock = socket_path(&s.dir, "run1");
        let mut c = ControlClient::connect(&sock, "cli").await.unwrap();
        assert_eq!(c.grants().await.unwrap().len(), 1);
        c.revoke_grant("g1").await.unwrap();
        assert!(c.grants().await.unwrap().is_empty());
        assert!(c.revoke_grant("g1").await.is_err());
        s.stop.send(true).unwrap();
        s.handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn wrong_version_and_missing_hello_are_rejected() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let s = start().await;
        let sock = socket_path(&s.dir, "run1");

        let stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
        let (rd, mut wr) = stream.into_split();
        wr.write_all(b"{\"type\":\"hello\",\"version\":999,\"client\":\"x\"}\n")
            .await
            .unwrap();
        let line = BufReader::new(rd)
            .lines()
            .next_line()
            .await
            .unwrap()
            .unwrap();
        assert!(line.contains("error") && line.contains("999"), "{line}");

        let stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
        let (rd, mut wr) = stream.into_split();
        wr.write_all(b"{\"type\":\"list_pending\"}\n")
            .await
            .unwrap();
        let line = BufReader::new(rd)
            .lines()
            .next_line()
            .await
            .unwrap()
            .unwrap();
        assert!(line.contains("hello"), "{line}");

        s.stop.send(true).unwrap();
        s.handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn connect_all_finds_live_runs_and_drops_stale_sockets() {
        let s = start().await;
        // A stale socket: bind then drop the listener, leaving the file.
        let stale = socket_path(&s.dir, "dead");
        {
            let l = std::os::unix::net::UnixListener::bind(&stale).unwrap();
            drop(l);
        }
        assert!(stale.exists());
        let clients = connect_all(&s.dir, "cli").await;
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].hello.run_id, "run1");
        assert!(!stale.exists(), "stale socket cleaned up");
        s.stop.send(true).unwrap();
        s.handle.await.unwrap().unwrap();
    }

    #[test]
    fn list_sockets_ignores_other_files_and_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(list_sockets(tmp.path()).is_empty());
        std::fs::create_dir_all(runs_dir(tmp.path())).unwrap();
        std::fs::write(runs_dir(tmp.path()).join("a.sock"), "").unwrap();
        std::fs::write(runs_dir(tmp.path()).join("b.txt"), "").unwrap();
        assert_eq!(list_sockets(tmp.path()).len(), 1);
    }
}
