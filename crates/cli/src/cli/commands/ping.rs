use crate::cli::utils::{resolve_connection, MAX_PARALLEL_HOSTS};
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::SshService;
use anyhow::{bail, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub async fn execute(
    target: Option<String>,
    all: bool,
    tag: Option<String>,
    timeout: Option<u64>,
    config: AppConfig,
) -> Result<()> {
    let ssh_service = SshService::new(config.clone())?;
    let timeout_secs = timeout.unwrap_or(5);

    let targets: Vec<Connection> = if all {
        ssh_service.list_connections(None, false).await?
    } else if let Some(tag_name) = &tag {
        ssh_service.list_connections(Some(tag_name), false).await?
    } else if let Some(target_str) = &target {
        vec![resolve_connection(&ssh_service, target_str, "ping", true, &config).await?]
    } else {
        bail!("Specify a connection target, --all, or -g/--tag");
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
            return Ok(());
        }
        println!(
            "❌ SSH ping to '{}' failed. (took {:.2?})",
            connection.name, duration
        );
        if !stderr.is_empty() {
            println!("Error output:\n{}", stderr.trim());
        }
        bail!("'{}' is unreachable", connection.name);
    }

    println!(
        "Testing parallel connectivity for {} host(s)... (timeout: {}s)\n",
        targets.len(),
        timeout_secs
    );
    let total = targets.len();
    let semaphore = Arc::new(Semaphore::new(MAX_PARALLEL_HOSTS));
    let mut set = JoinSet::new();

    for conn in targets {
        let semaphore = Arc::clone(&semaphore);
        set.spawn(async move {
            // The semaphore is never closed, so acquire cannot fail.
            let _permit = semaphore.acquire_owned().await.ok();
            let (success, duration, _) = ping_single_host(&conn, timeout_secs).await;
            (conn, success, duration)
        });
    }

    let mut successful_count = 0;

    while let Some(res) = set.join_next().await {
        let Ok((conn, success, duration)) = res else {
            continue; // counted as unreachable via `total`
        };
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

    println!(
        "\nPing Summary: {}/{} hosts reachable.",
        successful_count, total
    );
    if successful_count < total {
        bail!(
            "{} of {} host(s) unreachable",
            total - successful_count,
            total
        );
    }
    Ok(())
}

/// Build the `ssh` argv for a non-interactive reachability probe.
///
/// All options precede the destination and `--` ends option parsing, so a
/// host/user value starting with `-` can never be read as an ssh option.
/// Bastion handling mirrors the subprocess transport: Kerberos + bastion is
/// an *interactive* bastion (ssh to the bastion on port 22, pass
/// `user@target` as its sole argument); otherwise the bastion is a classic
/// jump host reached via `-J`, so `ConnectTimeout`/`BatchMode` cover both hops.
fn ping_args(connection: &Connection, timeout_secs: u64) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-o".into(),
        format!("ConnectTimeout={}", timeout_secs),
        "-o".into(),
        "BatchMode=yes".into(),
    ];
    if connection.use_kerberos {
        args.extend(["-o".into(), "GSSAPIAuthentication=yes".into()]);
    }
    if let Some(key_path) = &connection.key_path {
        args.extend(["-i".into(), key_path.clone()]);
    }
    let target = format!("{}@{}", connection.user, connection.host);
    if let Some(bastion) = &connection.bastion {
        let bastion_user = connection
            .bastion_user
            .as_deref()
            .unwrap_or(&connection.user);
        let jump = format!("{}@{}", bastion_user, bastion);
        if connection.use_kerberos {
            args.extend([
                "-o".into(),
                "GSSAPIDelegateCredentials=yes".into(),
                "-p".into(),
                "22".into(),
                "--".into(),
                jump,
                target,
            ]);
            return args;
        }
        args.extend(["-J".into(), jump]);
    }
    args.extend([
        "-p".into(),
        connection.port.to_string(),
        "--".into(),
        target,
        "exit 0".into(),
    ]);
    args
}

async fn ping_single_host(connection: &Connection, timeout_secs: u64) -> (bool, Duration, String) {
    let start_time = Instant::now();
    let mut cmd = Command::new("ssh");
    cmd.args(ping_args(connection, timeout_secs))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);

    // ConnectTimeout only bounds the TCP connect; also cap the whole probe so
    // a stalled handshake or auth exchange cannot hang the command.
    let overall = Duration::from_secs(timeout_secs.saturating_mul(2).saturating_add(5));
    match tokio::time::timeout(overall, cmd.output()).await {
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            (output.status.success(), start_time.elapsed(), stderr)
        }
        Ok(Err(e)) => (false, start_time.elapsed(), e.to_string()),
        Err(_) => (
            false,
            start_time.elapsed(),
            format!("timed out after {}s", overall.as_secs()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(bastion: Option<&str>, kerberos: bool) -> Connection {
        Connection::new(
            "web".into(),
            "web.example.com".into(),
            "deploy".into(),
            2222,
            bastion.map(String::from),
            Some("jump".into()),
            kerberos,
            None,
        )
    }

    #[test]
    fn direct_probe_targets_host_after_option_terminator() {
        let args = ping_args(&conn(None, false), 5);
        let n = args.len();
        assert_eq!(&args[n - 3..], ["--", "deploy@web.example.com", "exit 0"]);
        assert!(args.contains(&"ConnectTimeout=5".to_string()));
        assert!(!args.contains(&"-J".to_string()));
    }

    #[test]
    fn jump_host_probe_uses_proxyjump() {
        let args = ping_args(&conn(Some("bastion.corp"), false), 5);
        let j = args.iter().position(|a| a == "-J").expect("-J present");
        assert_eq!(args[j + 1], "jump@bastion.corp");
        let n = args.len();
        assert_eq!(&args[n - 3..], ["--", "deploy@web.example.com", "exit 0"]);
        let p = args.iter().position(|a| a == "-p").unwrap();
        assert_eq!(args[p + 1], "2222");
    }

    #[test]
    fn interactive_bastion_probe_passes_target_as_sole_argument() {
        let args = ping_args(&conn(Some("bastion.corp"), true), 5);
        let n = args.len();
        assert_eq!(
            &args[n - 4..],
            ["22", "--", "jump@bastion.corp", "deploy@web.example.com"]
        );
        assert!(args.contains(&"BatchMode=yes".to_string()));
        assert!(!args.contains(&"-J".to_string()));
    }
}
