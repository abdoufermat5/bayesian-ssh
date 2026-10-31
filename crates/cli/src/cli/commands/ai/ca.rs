//! `bssh ai ca`.

use super::setup::{print_script, resolve_targets};
use crate::cli::parser_ai::{AiArgsCa, AiCaCommands};
use crate::cli::utils::confirm;
use crate::config::AppConfig;
use crate::services::ai::exec::AiExecutor;
use crate::services::ai::{cert, hostsetup};
use anyhow::{bail, Result};

/// Run the `ai ca` subcommand.
///
/// # Errors
/// Returns an error when the CA cannot be created, read, or trusted on a host.
pub async fn execute(args: AiArgsCa, config: AppConfig) -> Result<()> {
    match args.command {
        AiCaCommands::Init => init(&config),
        AiCaCommands::Show => show(&config),
        AiCaCommands::Trust {
            target,
            tag,
            dry_run,
            yes,
        } => {
            let step = if dry_run {
                TrustStep::DryRun
            } else if yes {
                TrustStep::AssumeYes
            } else {
                TrustStep::Ask
            };
            trust(
                &config,
                TrustRequest {
                    target: target.as_deref(),
                    tag: tag.as_deref(),
                    step,
                },
            )
            .await
        }
    }
}

fn init(config: &AppConfig) -> Result<()> {
    let passphrase = std::env::var(cert::CA_PASSPHRASE_ENV)
        .ok()
        .filter(|p| !p.is_empty());
    let pub_line = cert::init_ca(config, passphrase.as_deref())?;
    println!("Local CA created in {}", cert::ca_dir(config).display());
    println!("Fingerprint: {}", cert::ca_fingerprint(config)?);
    if passphrase.is_some() {
        println!(
            "The private key is sealed with the passphrase from {}: set it again whenever an \
             agent run needs to issue certificates.",
            cert::CA_PASSPHRASE_ENV
        );
    } else {
        println!(
            "The private key is a 0600 file in a 0700 directory. Agent runs have no terminal \
             to ask for a passphrase; to seal it, remove the CA directory and run `ca init` \
             again with {} set.",
            cert::CA_PASSPHRASE_ENV
        );
    }
    println!("\nPublic key:\n{pub_line}");
    println!("\nNext: `bssh ai ca trust <host>` makes a host accept certificates from this CA.");
    Ok(())
}

fn show(config: &AppConfig) -> Result<()> {
    let line = cert::ca_public_key(config)?;
    println!("{line}");
    eprintln!("fingerprint {}", cert::ca_fingerprint(config)?);
    Ok(())
}

/// How far `ca trust` goes: print, ask, or run without asking.
enum TrustStep {
    /// Print the script and stop (`--dry-run`).
    DryRun,
    /// Ask before running it.
    Ask,
    /// Run it without asking (`--yes`).
    AssumeYes,
}

/// The hosts `ca trust` covers and how far it may go.
struct TrustRequest<'a> {
    target: Option<&'a str>,
    tag: Option<&'a str>,
    step: TrustStep,
}

async fn trust(config: &AppConfig, request: TrustRequest<'_>) -> Result<()> {
    let TrustRequest { target, tag, step } = request;
    let ca_pub = cert::ca_public_key(config)?;
    let script = hostsetup::trust_script(&ca_pub)?;
    let targets = resolve_targets(target, tag, "ai ca trust", config).await?;
    if targets.is_empty() {
        println!("No connections match.");
        return Ok(());
    }
    let names: Vec<&str> = targets.iter().map(|c| c.name.as_str()).collect();
    println!(
        "bssh ai ca trust will run this script as root on {} host(s): {}\n",
        targets.len(),
        names.join(", ")
    );
    print_script(&script);
    println!(
        "\nsshd will accept certificates from the CA {} for the principal {} and only for the \
         account {} (checked with `sshd -t` before the reload; rolled back if it fails).\n",
        cert::ca_fingerprint(config)?,
        cert::AGENT_PRINCIPAL,
        hostsetup::AGENT_USER
    );
    match step {
        TrustStep::DryRun => {
            println!("Dry run: nothing was changed.");
            return Ok(());
        }
        TrustStep::Ask => {
            if !confirm("Run it?", false)? {
                println!("Cancelled.");
                return Ok(());
            }
        }
        TrustStep::AssumeYes => {}
    }

    let exec = AiExecutor::new(config);
    let mut failed = 0usize;
    let host_key_policy = config.transport.host_key_policy();
    for conn in &targets {
        println!("== {} ({}@{})", conn.name, conn.user, conn.host);
        match hostsetup::run_root_script(conn, &exec, &script, true, host_key_policy).await {
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
                eprintln!("   trust failed (exit {})", run.exit_code);
                continue;
            }
            Err(e) => {
                failed += 1;
                eprintln!("   trust failed: {e:#}");
                continue;
            }
        }
        let report = hostsetup::check_host(conn, &exec, Some(&ca_pub)).await;
        match (report.ca_trusted, report.agent_user) {
            (Some(true), true) => println!("   verified: CA trusted, certificates will work"),
            (Some(true), false) => println!(
                "   CA trusted, but the user {} does not exist yet: run `bssh ai setup {}`",
                hostsetup::AGENT_USER,
                conn.name
            ),
            _ => {
                failed += 1;
                eprintln!("   the script ran but sshd does not report the CA as trusted");
            }
        }
    }
    if failed > 0 {
        bail!("trust failed on {failed} host(s)");
    }
    Ok(())
}
