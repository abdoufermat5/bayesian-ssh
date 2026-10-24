//! `bssh ai setup`. Owner: W5.
//!
//! Creates the low-privilege `bssh-agent` user (decision Q6: that is the only thing that
//! needs root, no firewall, no sudoers). The script is printed, confirmed, then run with
//! sudo; a human runs setup, so an interactive sudo password is fine.

use crate::cli::parser_ai::AiArgsSetup;
use crate::cli::utils::{confirm, resolve_connection};
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::ai::exec::AiExecutor;
use crate::services::ai::hostsetup;
use crate::services::SshService;
use anyhow::{bail, Result};

/// Targets of a host command: one connection by name, or every connection with a tag.
pub(crate) async fn resolve_targets(
    target: Option<&str>,
    tag: Option<&str>,
    action: &str,
    config: &AppConfig,
) -> Result<Vec<Connection>> {
    let ssh = SshService::new(config.clone())?;
    match (target, tag) {
        (_, Some(tag)) => ssh.list_connections(Some(tag), false).await,
        (Some(target), None) => Ok(vec![
            resolve_connection(&ssh, target, action, true, config).await?,
        ]),
        (None, None) => bail!("name a connection, or use -t <tag>"),
    }
}

/// Print `script` indented, so it is clear where the text of the script ends.
pub(crate) fn print_script(script: &str) {
    for line in script.lines() {
        println!("    {line}");
    }
}

pub async fn execute(args: AiArgsSetup, config: AppConfig) -> Result<()> {
    let targets = resolve_targets(
        args.target.as_deref(),
        args.tag.as_deref(),
        "ai setup",
        &config,
    )
    .await?;
    if targets.is_empty() {
        println!("No connections match.");
        return Ok(());
    }

    let script = hostsetup::agent_user_script();
    let names: Vec<&str> = targets.iter().map(|c| c.name.as_str()).collect();
    println!(
        "bssh ai setup will run this script as root on {} host(s): {}\n",
        targets.len(),
        names.join(", ")
    );
    print_script(&script);
    println!(
        "\nIt creates the user `{}` (no password, no sudo) and its home with a backup directory.\n\
         It does not change firewalls or sudoers.\n",
        hostsetup::AGENT_USER
    );
    if args.dry_run {
        println!("Dry run: nothing was changed.");
        return Ok(());
    }
    if !args.yes && !confirm("Run it?", false)? {
        println!("Cancelled.");
        return Ok(());
    }

    let exec = AiExecutor::new(&config);
    let ca_pub = crate::services::ai::cert::ca_public_key(&config).ok();
    let mut failed = 0usize;
    // One host at a time: sudo may ask for a password on the terminal.
    for conn in &targets {
        println!("== {} ({}@{})", conn.name, conn.user, conn.host);
        match hostsetup::run_root_script(conn, &exec, &script, true).await {
            Ok(run) if run.exit_code == 0 => {
                for line in run.output.lines() {
                    println!("   {line}");
                }
            }
            Ok(run) => {
                failed += 1;
                for line in run.output.lines() {
                    eprintln!("   {line}");
                }
                eprintln!("   setup failed (exit {})", run.exit_code);
                continue;
            }
            Err(e) => {
                failed += 1;
                eprintln!("   setup failed: {e:#}");
                continue;
            }
        }
        let report = hostsetup::check_host(conn, &exec, ca_pub.as_deref()).await;
        if !report.agent_user {
            failed += 1;
            eprintln!(
                "   the script ran but the user {} is not visible",
                hostsetup::AGENT_USER
            );
            continue;
        }
        println!(
            "   agent user ok, read-only jail: {:?}, level: {}",
            report.jail,
            report.level()
        );
        if report.jail == crate::services::ai::types::JailKind::None {
            println!(
                "   no jail available here: install bubblewrap (`bwrap`) or util-linux >= 2.40, \
                 or put the bssh-jail helper in {}/.local/bin",
                report
                    .agent_home
                    .as_deref()
                    .unwrap_or(hostsetup::AGENT_HOME)
            );
        }
    }
    println!();
    println!("Next: `bssh ai ca init` and `bssh ai ca trust` for approval-bound certificates,");
    println!("then `bssh ai doctor` to check every host.");
    if failed > 0 {
        bail!("setup failed on {failed} host(s)");
    }
    Ok(())
}
