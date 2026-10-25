use anyhow::{bail, Result};
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::info;

use crate::cli::utils::{resolve_connection, MAX_PARALLEL_HOSTS};
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::transport::{execute_with_fallback, ExecOptions, LimitedOutput};
use crate::services::SshService;
use std::time::Duration;

/// Exit status `bssh exec --timeout` reports for a command it killed (same as timeout(1)).
const TIMEOUT_EXIT_CODE: i32 = 124;

/// Limits from `--timeout` / `--max-output`.
#[derive(Debug, Clone, Copy, Default)]
pub struct CliLimits {
    pub timeout: Option<Duration>,
    /// Per stream, per host.
    pub max_output_bytes: Option<usize>,
}

impl CliLimits {
    fn options(self) -> ExecOptions {
        ExecOptions {
            timeout: self.timeout,
            max_output_bytes: self.max_output_bytes,
            ..ExecOptions::default()
        }
    }
}

/// Run `cmd` on one host with the transport fallback policy.
async fn run_on(
    conn: &Connection,
    config: &AppConfig,
    cmd: &str,
    opts: &ExecOptions,
) -> Result<LimitedOutput, crate::services::transport::TransportError> {
    execute_with_fallback(conn, config, |transport| {
        let conn = conn.clone();
        let cmd = cmd.to_string();
        let opts = opts.clone();
        Box::pin(async move { transport.exec_limited(&conn, &cmd, &opts).await })
    })
    .await
}

/// The process exit status for one host's result: the remote code, 124 on timeout.
/// Out-of-range codes (e.g. -1, 256) map to 1 rather than wrapping to a false success.
fn exit_status(out: &LimitedOutput) -> i32 {
    if out.timed_out {
        return TIMEOUT_EXIT_CODE;
    }
    match out.exit_code {
        Some(0) => 0,
        Some(c) if (1..=255).contains(&c) => c,
        _ => 1,
    }
}

fn limits_notes(out: &LimitedOutput, limits: CliLimits) -> Vec<String> {
    let mut notes = Vec::new();
    if out.truncated {
        if let Some(n) = limits.max_output_bytes {
            notes.push(format!("output truncated at {n} bytes per stream"));
        }
    }
    if out.timed_out {
        if let Some(t) = limits.timeout {
            notes.push(format!("timed out after {}s", t.as_secs()));
        }
    }
    notes
}

pub async fn execute(
    target: Option<String>,
    all: bool,
    tag: Option<String>,
    dry_run: bool,
    limits: CliLimits,
    command: Vec<String>,
    config: AppConfig,
) -> Result<()> {
    if command.is_empty() {
        bail!("no command supplied — use: bssh exec <target> -- <command...>");
    }

    let ssh_service = SshService::new(config.clone())?;

    let targets: Vec<Connection> = if all {
        ssh_service.list_connections(None, false).await?
    } else if let Some(tag_name) = &tag {
        ssh_service.list_connections(Some(tag_name), false).await?
    } else if let Some(target_str) = &target {
        vec![resolve_connection(&ssh_service, target_str, "exec", true, &config).await?]
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

    // A team sync may have redirected any of these hosts. Ask once per
    // connection (interactive) or refuse before running anything.
    for connection in &targets {
        ssh_service.confirm_pending_change(connection)?;
    }

    if targets.len() == 1 {
        let connection = &targets[0];
        info!("exec '{}' on {}", cmd_str, connection.host);
        let opts = limits.options();
        let output = run_on(connection, &config, &cmd_str, &opts)
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
        for note in limits_notes(&output, limits) {
            eprintln!("bssh: {note}");
        }

        let code = exit_status(&output);
        if code != 0 {
            // Propagate the remote status (or 124 on timeout).
            std::process::exit(code);
        }
        return Ok(());
    }

    println!(
        "🚀 Executing '{}' across {} host(s)...\n",
        cmd_str,
        targets.len()
    );
    let total = targets.len();
    let semaphore = Arc::new(Semaphore::new(MAX_PARALLEL_HOSTS));
    let mut set = JoinSet::new();

    for conn in targets {
        let cfg = config.clone();
        let cmd = cmd_str.clone();
        let opts = limits.options();
        let semaphore = Arc::clone(&semaphore);
        set.spawn(async move {
            // The semaphore is never closed, so acquire cannot fail.
            let _permit = semaphore.acquire_owned().await.ok();
            let res = run_on(&conn, &cfg, &cmd, &opts).await;
            (conn, res)
        });
    }

    let mut failed = 0usize;
    while let Some(res) = set.join_next().await {
        let (conn, exec_res) = match res {
            Ok(pair) => pair,
            Err(e) => {
                failed += 1;
                eprintln!("❌ exec task aborted: {e}\n");
                continue;
            }
        };
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
                for note in limits_notes(&output, limits) {
                    println!("({note})");
                }
                match output.exit_code {
                    Some(code) if !output.timed_out => println!("(Exit Code: {code})\n"),
                    _ => println!("(Exit Code: none)\n"),
                }
                if exit_status(&output) != 0 {
                    failed += 1;
                }
            }
            Err(e) => {
                failed += 1;
                println!("❌ Failed to execute on {}: {}\n", conn.name, e);
            }
        }
    }

    if failed > 0 {
        bail!("command failed on {failed} of {total} host(s)");
    }
    Ok(())
}
