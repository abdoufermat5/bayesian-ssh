use crate::cli::utils::fuzzy_select_connection;
use crate::config::AppConfig;
use crate::services::SshService;
use anyhow::{Context, Result};
use std::time::Instant;
use tokio::process::Command;
use tracing::info;

pub async fn execute(target: String, timeout: Option<u64>, config: AppConfig) -> Result<()> {
    let ssh_service = SshService::new(config)?;

    let mut target_conn = ssh_service.get_connection(&target).await.unwrap_or_default();

    if target_conn.is_none() {
        println!("🔍 Connection '{}' not found, searching...", target);
        target_conn = fuzzy_select_connection(&ssh_service, &target, "ping", true).await?;
    }

    let connection = match target_conn {
        Some(c) => c,
        None => {
            println!("❌ No connection selected.");
            return Ok(());
        }
    };

    println!("Testing connectivity to '{}' ({})...", connection.name, connection.host);

    let start_time = Instant::now();
    let timeout_secs = timeout.unwrap_or(5); // Default 5s timeout

    let mut cmd = Command::new("ssh");

    if let Some(bastion) = &connection.bastion {
        let bastion_user = connection.bastion_user.as_deref().unwrap_or(&connection.user);
        
        if connection.use_kerberos {
            cmd.args(["-o", "GSSAPIAuthentication=yes"]);
            cmd.args(["-o", "GSSAPIDelegateCredentials=yes"]);
        }
        if let Some(key_path) = &connection.key_path {
            cmd.args(["-i", key_path]);
        }
        
        cmd.arg("-p").arg("22");
        cmd.arg(format!("{}@{}", bastion_user, bastion));
        
        // Command to run ON the bastion (it expects user@target)
        cmd.arg(format!("{}@{}", connection.user, connection.host));
        if let Some(key_path) = &connection.key_path {
            cmd.args(["-i", key_path]);
        }
        
        cmd.args(["-o", &format!("ConnectTimeout={}", timeout_secs)]);
        cmd.args(["-o", "BatchMode=yes"]);
        cmd.args(["-p", &connection.port.to_string()]);
        cmd.arg(format!("{}@{}", connection.user, connection.host));
        cmd.arg("exit 0");
    }

    info!("Executing ping: {:?}", cmd);

    let result = cmd.output().await.context("Failed to execute ssh command")?;
    let duration = start_time.elapsed();

    if result.status.success() {
        println!("✅ SSH ping to '{}' successful! (took {:.2?})", connection.name, duration);
    } else {
        println!("❌ SSH ping to '{}' failed. (took {:.2?})", connection.name, duration);
        let stderr = String::from_utf8_lossy(&result.stderr);
        if !stderr.is_empty() {
            println!("Error output:\n{}", stderr.trim());
        }
    }

    Ok(())
}
