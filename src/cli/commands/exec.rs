use anyhow::{bail, Result};
use std::io::Write;
use tokio::task::JoinSet;
use tracing::info;

use crate::cli::utils::resolve_connection;
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::transport::execute_with_fallback;
use crate::services::SshService;

pub async fn execute(
    target: Option<String>,
    all: bool,
    tag: Option<String>,
    dry_run: bool,
    command: Vec<String>,
    config: AppConfig,
) -> Result<()> {
    if command.is_empty() {
        bail!("no command supplied — use: bssh exec <target> -- <command...>");
    }

    let ssh_service = SshService::new(config.clone())?;

    let targets: Vec<Connection> = if all {
        ssh_service.list_connections(None, false).await?
    } else if let Some(ref tag_name) = tag {
        ssh_service.list_connections(Some(tag_name), false).await?
    } else if let Some(ref target_str) = target {
        vec![resolve_connection(&ssh_service, target_str, "exec", true).await?]
    } else {
        bail!("Specify a connection target, --all, or -g/--tag");
    };

    if targets.is_empty() {
        println!("No connections matching criteria.");
        return Ok(());
    }

    let cmd_str = command.join(" ");

    if dry_run {
        println!("🔍 [DRY-RUN PREVIEW]");
        println!("Command to execute: {}", cmd_str);
        println!("Target hosts ({}):", targets.len());
        for conn in &targets {
            println!("  - {} ({}@{})", conn.name, conn.user, conn.host);
        }
        println!("\nDry-run complete. No remote commands were executed.");
        return Ok(());
    }

    if targets.len() == 1 {
        let connection = &targets[0];
        info!("exec '{}' on {}", cmd_str, connection.host);
        let output = execute_with_fallback(connection, &config, |transport| {
            let conn = connection.clone();
            let cmd = cmd_str.clone();
            Box::pin(async move { transport.exec(&conn, &cmd).await })
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        if !output.stdout.is_empty() {
            std::io::stdout().write_all(&output.stdout)?;
            if !output.stdout.ends_with(b"\n") {
                println!();
            }
        }
        if !output.stderr.is_empty() {
            std::io::stderr().write_all(&output.stderr)?;
        }

        if output.exit_code != 0 {
            std::process::exit(output.exit_code);
        }
        return Ok(());
    }

    println!(
        "🚀 Executing '{}' across {} host(s)...\n",
        cmd_str,
        targets.len()
    );
    let mut set = JoinSet::new();

    for conn in targets {
        let cfg = config.clone();
        let cmd = cmd_str.clone();
        set.spawn(async move {
            let res = execute_with_fallback(&conn, &cfg, |transport| {
                let c = conn.clone();
                let cm = cmd.clone();
                Box::pin(async move { transport.exec(&c, &cm).await })
            })
            .await;
            (conn, res)
        });
    }

    while let Some(res) = set.join_next().await {
        if let Ok((conn, exec_res)) = res {
            println!("==================================================");
            println!("Host: {} ({}@{})", conn.name, conn.user, conn.host);
            println!("==================================================");
            match exec_res {
                Ok(output) => {
                    if !output.stdout.is_empty() {
                        let out_str = String::from_utf8_lossy(&output.stdout);
                        print!("{}", out_str);
                        if !out_str.ends_with('\n') {
                            println!();
                        }
                    }
                    if !output.stderr.is_empty() {
                        eprintln!("[STDERR]\n{}", String::from_utf8_lossy(&output.stderr));
                    }
                    println!("(Exit Code: {})\n", output.exit_code);
                }
                Err(e) => {
                    println!("❌ Failed to execute on {}: {}\n", conn.name, e);
                }
            }
        }
    }

    Ok(())
}
