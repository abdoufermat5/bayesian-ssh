//! The control socket a run serves to `bssh ai watch` and the desktop app.

use crate::services::ai::ipc::{ClientMsg, IPC_VERSION, ServerMsg};
use crate::services::ai::types::AiEvent;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use super::broker::{LocalBroker, RunInfo};

/// Clears a circuit-breaker pause from the control socket. `Err` is the reason it can't
/// (the run is stopped, or it is not paused).
pub type ResumeFn = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// Serve `<ai_dir>/runs/<run_id>.sock` (protocol in `ipc.rs`) until `stop` fires or a client
/// sends `Stop`.
///
/// Returns `Some(reason)` when a client asked to stop (the caller ends the run), `None` when
/// the `stop` watch fired. The socket file is removed on return.
#[cfg(unix)]
pub async fn serve_control(
    socket: PathBuf,
    run: RunInfo,
    broker: Arc<LocalBroker>,
    events: broadcast::Sender<AiEvent>,
    mut stop: tokio::sync::watch::Receiver<bool>,
    resume: ResumeFn,
) -> anyhow::Result<Option<String>> {
    use anyhow::Context;
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixListener;

    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        crate::config::enforce_secure_dir(dir);
    }
    // A leftover file from a crashed run would make bind fail.
    if socket.exists() {
        let _ = std::fs::remove_file(&socket);
    }
    let listener =
        UnixListener::bind(&socket).with_context(|| format!("bind {}", socket.display()))?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;

    let (client_stop_tx, mut client_stop_rx) = tokio::sync::watch::channel(None::<String>);
    let client_stop_tx = Arc::new(client_stop_tx);
    let (done_tx, done_rx) = tokio::sync::watch::channel(false);

    let result = loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        tokio::spawn(handle_conn(
                            stream,
                            run.clone(),
                            broker.clone(),
                            events.subscribe(),
                            client_stop_tx.clone(),
                            done_rx.clone(),
                            resume.clone(),
                        ));
                    }
                    Err(e) => tracing::warn!("control socket accept failed: {e}"),
                }
            }
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break None;
                }
            }
            changed = client_stop_rx.changed() => {
                if changed.is_ok() {
                    break client_stop_rx.borrow().clone();
                }
            }
        }
    };
    let _ = done_tx.send(true);
    drop(listener);
    let _ = std::fs::remove_file(&socket);
    Ok(result)
}

#[cfg(not(unix))]
pub async fn serve_control(
    socket: PathBuf,
    run: RunInfo,
    broker: Arc<LocalBroker>,
    events: broadcast::Sender<AiEvent>,
    stop: tokio::sync::watch::Receiver<bool>,
    resume: ResumeFn,
) -> anyhow::Result<Option<String>> {
    let _ = (socket, run, broker, events, stop, resume);
    anyhow::bail!("the AI control socket needs a Unix platform")
}

#[cfg(unix)]
async fn send_msg(
    wr: &mut tokio::net::unix::OwnedWriteHalf,
    msg: &ServerMsg,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut line = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    line.push(b'\n');
    wr.write_all(&line).await
}

#[cfg(unix)]
async fn handle_conn(
    stream: tokio::net::UnixStream,
    run: RunInfo,
    broker: Arc<LocalBroker>,
    mut events: broadcast::Receiver<AiEvent>,
    client_stop: Arc<tokio::sync::watch::Sender<Option<String>>>,
    mut done: tokio::sync::watch::Receiver<bool>,
    resume: ResumeFn,
) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let (rd, mut wr) = stream.into_split();
    let mut lines = BufReader::new(rd).lines();

    // Handshake: the first message must be a Hello with our version.
    let first = tokio::time::timeout(Duration::from_secs(10), lines.next_line()).await;
    let hello = match first {
        Ok(Ok(Some(l))) => serde_json::from_str::<ClientMsg>(&l).ok(),
        _ => None,
    };
    match hello {
        Some(ClientMsg::Hello { version, client }) if version == IPC_VERSION => {
            tracing::debug!("control client connected: {client}");
            let reply = ServerMsg::Hello {
                version: IPC_VERSION,
                run_id: run.run_id.clone(),
                client: broker.client(),
                policy: run.policy.clone(),
                started_at: run.started_at.clone(),
                on_behalf_of: run.on_behalf_of.clone(),
                hosts: run.hosts.clone(),
            };
            if send_msg(&mut wr, &reply).await.is_err() {
                return;
            }
        }
        Some(ClientMsg::Hello { version, .. }) => {
            let _ = send_msg(
                &mut wr,
                &ServerMsg::Error {
                    message: format!(
                        "protocol version {version} not supported (server speaks {IPC_VERSION})"
                    ),
                },
            )
            .await;
            return;
        }
        _ => {
            let _ = send_msg(
                &mut wr,
                &ServerMsg::Error {
                    message: "expected a hello message first".into(),
                },
            )
            .await;
            return;
        }
    }

    let mut subscribed = false;
    let mut new_requests = broker.subscribe_requests();
    loop {
        let reply: Option<ServerMsg> = tokio::select! {
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(l)) => l,
                    _ => break,
                };
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<ClientMsg>(&line) {
                    Ok(msg) => handle_msg(msg, &broker, &client_stop, &resume, &mut subscribed),
                    Err(e) => Some(ServerMsg::Error { message: format!("bad message: {e}") }),
                }
            }
            ev = events.recv(), if subscribed => match ev {
                Ok(event) => Some(ServerMsg::Event { event }),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!("control client missed {n} events");
                    None
                }
                Err(broadcast::error::RecvError::Closed) => None,
            },
            req = new_requests.recv() => match req {
                Ok(request) => Some(ServerMsg::NewRequest { request }),
                Err(_) => None,
            },
            _ = done.changed() => break,
        };
        if let Some(r) = reply
            && send_msg(&mut wr, &r).await.is_err()
        {
            break;
        }
    }
}

#[cfg(unix)]
fn handle_msg(
    msg: ClientMsg,
    broker: &LocalBroker,
    client_stop: &tokio::sync::watch::Sender<Option<String>>,
    resume: &ResumeFn,
    subscribed: &mut bool,
) -> Option<ServerMsg> {
    Some(match msg {
        ClientMsg::Hello { .. } => ServerMsg::Error {
            message: "already greeted".into(),
        },
        ClientMsg::Subscribe => {
            *subscribed = true;
            ServerMsg::Ok
        }
        ClientMsg::ListPending => ServerMsg::Pending {
            requests: broker.pending_requests(),
        },
        ClientMsg::ListGrants => {
            match broker
                .store()
                .active_grants(Some(&broker.run_info().run_id))
            {
                Ok(grants) => ServerMsg::Grants { grants },
                Err(e) => ServerMsg::Error {
                    message: format!("could not read grants: {e}"),
                },
            }
        }
        ClientMsg::Decide {
            request_id,
            outcome,
        } => match broker.decide(&request_id, outcome) {
            Ok(()) => ServerMsg::Ok,
            Err(e) => ServerMsg::Error {
                message: e.to_string(),
            },
        },
        ClientMsg::RevokeGrant { grant_id } => match broker.store().revoke_grant(&grant_id) {
            Ok(true) => ServerMsg::Ok,
            Ok(false) => ServerMsg::Error {
                message: format!("no active grant '{grant_id}'"),
            },
            Err(e) => ServerMsg::Error {
                message: e.to_string(),
            },
        },
        ClientMsg::Resume { by } => match resume(&by) {
            Ok(()) => ServerMsg::Ok,
            Err(message) => ServerMsg::Error { message },
        },
        ClientMsg::Stop { reason } => {
            let _ = client_stop.send(Some(reason));
            ServerMsg::Ok
        }
    })
}
