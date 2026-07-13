use anyhow::{bail, Result};
use tracing::info;

use crate::cli::utils::resolve_connection;
use crate::config::AppConfig;
use crate::services::transport::execute_with_fallback;
use crate::services::SshService;

pub async fn execute(target: String, command: Vec<String>, config: AppConfig) -> Result<()> {
    if command.is_empty() {
        bail!("no command supplied — use: bssh exec <target> -- <command...>");
    }

    let ssh_service = SshService::new(config.clone())?;
    let connection = resolve_connection(&ssh_service, &target, "exec", true).await?;

    let cmd_str = command.join(" ");
    info!("exec '{}' on {}", cmd_str, connection.host);

    let output = execute_with_fallback(&connection, &config, |transport| {
        let conn = connection.clone();
        let cmd = cmd_str.clone();
        Box::pin(async move { transport.exec(&conn, &cmd).await })
    })
    .await
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
