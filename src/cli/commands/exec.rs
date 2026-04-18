use anyhow::{bail, Result};
use tracing::info;

use crate::cli::utils::fuzzy_select_connection;
use crate::config::AppConfig;
use crate::services::transport::types::SshTransport;
use crate::services::transport::{pick_kind, RusshTransport, SubprocessTransport, TransportKind};
use crate::services::SshService;

pub async fn execute(target: String, command: Vec<String>, config: AppConfig) -> Result<()> {
    if command.is_empty() {
        bail!("no command supplied — use: bssh exec <target> -- <command...>");
    }

    let ssh_service = SshService::new(config.clone())?;
    let mut conn_opt = ssh_service
        .get_connection(&target)
        .await
        .unwrap_or_default();

    if conn_opt.is_none() {
        conn_opt = fuzzy_select_connection(&ssh_service, &target, "exec", true).await?;
    }

    let connection = match conn_opt {
        Some(c) => c,
        None => bail!("no connection selected"),
    };

    let cmd_str = command.join(" ");
    info!("exec '{}' on {}", cmd_str, connection.host);

    let kind = pick_kind(&connection, &config);
    let output = match kind {
        TransportKind::Native => {
            let t = RusshTransport::new(config.clone());
            match t.exec(&connection, &cmd_str).await {
                Err(crate::services::transport::TransportError::Fallback(e)) => {
                    tracing::warn!("native exec fallback ({e}), retrying with subprocess");
                    SubprocessTransport::new(config)
                        .exec(&connection, &cmd_str)
                        .await
                }
                other => other,
            }
        }
        TransportKind::Subprocess => {
            SubprocessTransport::new(config)
                .exec(&connection, &cmd_str)
                .await
        }
    }
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Write stdout to stdout, stderr to stderr.
    use std::io::Write;
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
    Ok(())
}
