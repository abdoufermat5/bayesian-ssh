//! A fake `bssh ai mcp` run for developing the desktop AI view without an agent.
//!
//! Binds `<ai_dir>/runs/demo-<pid>.sock`, answers the control protocol
//! (`services::ai::ipc`) and plays a short script: a few calls, then an approval
//! request for a service reload. What the desktop sends back (Decide, Stop,
//! RevokeGrant) is printed.
//!
//!     cargo run -p bayesian-ssh-gui --example ai_fake_run
//!
//! Unix only; Ctrl+C removes the socket.

#[cfg(unix)]
mod demo {
    use bayesian_ssh::config::AppConfig;
    use bayesian_ssh::services::ai::ai_dir;
    use bayesian_ssh::services::ai::ipc::{ClientMsg, ServerMsg, IPC_VERSION};
    use bayesian_ssh::services::ai::types::{
        AiEvent, Analysis, ApprovalRequest, ApprovalSubject, Decision, RiskTag, Tier, ToolCall,
        ToolName,
    };
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    fn call(id: &str, tool: ToolName, command: &str) -> ToolCall {
        ToolCall {
            run_id: "demo".into(),
            call_id: id.into(),
            tool,
            host: Some("web-2".into()),
            command: Some(command.into()),
            path: None,
            args: serde_json::json!({ "reason": "nginx returns 502, upstream looks down" }),
        }
    }

    fn script() -> Vec<ServerMsg> {
        let read = call("c-1", ToolName::RunReadonly, "journalctl -u nginx -n 50");
        let reload = call("c-2", ToolName::Service, "systemctl reload nginx");
        let analysis = Analysis {
            commands: Vec::new(),
            tags: [RiskTag::ServiceChange].into_iter().collect(),
            unknown_reason: None,
        };
        vec![
            ServerMsg::Event {
                event: AiEvent::Call {
                    call: read,
                    decision: Decision::Allow {
                        reason: "read-only command".into(),
                    },
                },
            },
            ServerMsg::Event {
                event: AiEvent::Result {
                    run_id: "demo".into(),
                    call_id: "c-1".into(),
                    exit_code: Some(0),
                    output_sha256: "00".into(),
                    redactions: Vec::new(),
                },
            },
            ServerMsg::NewRequest {
                request: ApprovalRequest {
                    request_id: "req-1".into(),
                    run_id: "demo".into(),
                    client: "Fake agent 0.1".into(),
                    tier: Tier::OutOfBand,
                    subject: ApprovalSubject::Call {
                        call: reload,
                        analysis: Some(analysis),
                    },
                    created_at: chrono::Utc::now().to_rfc3339(),
                    expires_at: (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
                },
            },
        ]
    }

    async fn send(write: &mut tokio::net::unix::OwnedWriteHalf, msg: &ServerMsg) -> bool {
        let Ok(mut line) = serde_json::to_vec(msg) else {
            return false;
        };
        line.push(b'\n');
        write.write_all(&line).await.is_ok()
    }

    pub async fn run() -> anyhow::Result<()> {
        let config = AppConfig::load(None)?;
        let dir = ai_dir(&config).join("runs");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("demo-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        println!("listening on {}", path.display());

        let cleanup = path.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = std::fs::remove_file(&cleanup);
            std::process::exit(0);
        });

        loop {
            let (stream, _) = listener.accept().await?;
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                let mut played = false;
                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(msg) = serde_json::from_str::<ClientMsg>(&line) else {
                        continue;
                    };
                    println!("<- {line}");
                    let is_list_pending = matches!(msg, ClientMsg::ListPending);
                    let ok = match msg {
                        ClientMsg::Hello { .. } => {
                            send(
                                &mut write,
                                &ServerMsg::Hello {
                                    version: IPC_VERSION,
                                    run_id: "demo".into(),
                                    client: "Fake agent 0.1".into(),
                                    policy: "default".into(),
                                    started_at: chrono::Utc::now().to_rfc3339(),
                                },
                            )
                            .await
                        }
                        ClientMsg::Subscribe => send(&mut write, &ServerMsg::Ok).await,
                        ClientMsg::ListPending => {
                            send(&mut write, &ServerMsg::Pending { requests: vec![] }).await
                        }
                        _ => send(&mut write, &ServerMsg::Ok).await,
                    };
                    if !ok {
                        break;
                    }
                    // Play the script once the handshake is done.
                    if is_list_pending && !played {
                        played = true;
                        for msg in script() {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            if !send(&mut write, &msg).await {
                                return;
                            }
                        }
                    }
                }
            });
        }
    }
}

#[cfg(unix)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    demo::run().await
}

#[cfg(not(unix))]
fn main() {
    eprintln!("The run sockets are Unix sockets: this example is Unix only.");
}
