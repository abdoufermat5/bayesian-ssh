//! `bssh ai ca`. Owner: W5.

use super::setup::{print_script, resolve_targets};
use crate::cli::parser_ai::{AiArgsCa, AiCaCommands};
use crate::cli::utils::confirm;
use crate::config::AppConfig;
use crate::services::ai::exec::AiExecutor;
use crate::services::ai::{cert, hostsetup};
use anyhow::{bail, Result};

pub async fn execute(args: AiArgsCa, config: AppConfig) -> Result<()> {
    match args.command {
        AiCaCommands::Init => init(&config),
        AiCaCommands::Show => show(&config),
        AiCaCommands::Trust {
            target,
            tag,
            dry_run,
            yes,
        } => trust(&config, target.as_deref(), tag.as_deref(), dry_run, yes).await,
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

async fn trust(
    config: &AppConfig,
    target: Option<&str>,
    tag: Option<&str>,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
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
    if dry_run {
        println!("Dry run: nothing was changed.");
        return Ok(());
    }
    if !yes && !confirm("Run it?", false)? {
        println!("Cancelled.");
        return Ok(());
    }

    let exec = AiExecutor::new(config);
    let mut failed = 0usize;
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
