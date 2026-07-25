use crate::cli::utils::resolve_connection;
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::SshService;
use anyhow::Result;
use std::time::Instant;
use tokio::process::Command;
use tokio::task::JoinSet;

pub async fn execute(
    target: Option<String>,
    all: bool,
    tag: Option<String>,
    timeout: Option<u64>,
    config: AppConfig,
) -> Result<()> {
    let ssh_service = SshService::new(config)?;
    let timeout_secs = timeout.unwrap_or(5);

    let targets: Vec<Connection> = if all {
        ssh_service.list_connections(None, false).await?
    } else if let Some(ref tag_name) = tag {
        ssh_service.list_connections(Some(tag_name), false).await?
    } else if let Some(ref target_str) = target {
        match resolve_connection(&ssh_service, target_str, "ping", true).await {
            Ok(c) => vec![c],
            Err(_) => {
                println!("❌ No connection selected.");
                return Ok(());
            }
        }
    } else {
        anyhow::bail!("Specify a connection target, --all, or -g/--tag");
    };

    if targets.is_empty() {
        println!("No connections matching criteria.");
        return Ok(());
    }

    if targets.len() == 1 {
        let connection = &targets[0];
        println!(
            "Testing connectivity to '{}' ({})...",
            connection.name, connection.host
        );
        let (success, duration, stderr) = ping_single_host(connection, timeout_secs).await;
        if success {
            println!(
                "✅ SSH ping to '{}' successful! (took {:.2?})",
                connection.name, duration
            );
        } else {
            println!(
                "❌ SSH ping to '{}' failed. (took {:.2?})",
                connection.name, duration
            );
            if !stderr.is_empty() {
                println!("Error output:\n{}", stderr.trim());
            }
        }
        return Ok(());
    }

    println!(
        "Testing parallel connectivity for {} host(s)... (timeout: {}s)\n",
        targets.len(),
        timeout_secs
    );
    let mut set = JoinSet::new();

    for conn in targets {
        set.spawn(async move {
            let (success, duration, _) = ping_single_host(&conn, timeout_secs).await;
            (conn, success, duration)
        });
    }

    let mut successful_count = 0;
    let total = set.len();

    while let Some(res) = set.join_next().await {
        if let Ok((conn, success, duration)) = res {
            if success {
                successful_count += 1;
                println!(
                    "  ✅ {:<20} {:<25} {:>8.2?}",
                    conn.name,
                    format!("({}:{})", conn.host, conn.port),
                    duration
                );
            } else {
                println!(
                    "  ❌ {:<20} {:<25} {:>8.2?} (FAILED)",
                    conn.name,
                    format!("({}:{})", conn.host, conn.port),
                    duration
                );
            }
        }
    }

    println!(
        "\nPing Summary: {}/{} hosts reachable.",
        successful_count, total
    );
    Ok(())
}

async fn ping_single_host(
    connection: &Connection,
    timeout_secs: u64,
) -> (bool, std::time::Duration, String) {
    let start_time = Instant::now();
    let mut cmd = Command::new("ssh");

    if let Some(bastion) = &connection.bastion {
        let bastion_user = connection
            .bastion_user
            .as_deref()
            .unwrap_or(&connection.user);

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
    } else {
        if connection.use_kerberos {
            cmd.args(["-o", "GSSAPIAuthentication=yes"]);
        }
        if let Some(key_path) = &connection.key_path {
            cmd.args(["-i", key_path]);
        }
        cmd.args(["-o", &format!("ConnectTimeout={}", timeout_secs)]);
        cmd.args(["-o", "BatchMode=yes"]);
        cmd.args(["-p", &connection.port.to_string()]);
        cmd.arg(format!("{}@{}", connection.user, connection.host));
        cmd.arg("exit 0");
    }

    match cmd.output().await {
        Ok(output) => {
            let duration = start_time.elapsed();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            (output.status.success(), duration, stderr)
        }
        Err(e) => (false, start_time.elapsed(), e.to_string()),
    }
}
