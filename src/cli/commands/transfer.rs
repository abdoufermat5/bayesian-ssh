use std::path::PathBuf;

use anyhow::{bail, Result};
use tracing::info;

use crate::config::AppConfig;
use crate::services::{SshService, TransferService};

pub async fn execute_upload(
    target: String,
    local: PathBuf,
    remote: String,
    offset: u64,
    mode: u32,
    recursive: bool,
    config: AppConfig,
) -> Result<()> {
    let ssh_service = SshService::new(config.clone())?;
    let connection = resolve_connection(&ssh_service, &target, "upload").await?;

    let transfer = TransferService::new(config)?;

    // When SFTP is unavailable (e.g. interactive bastion + Kerberos), use SCP.
    if !transfer.has_sftp(&connection) {
        info!("SFTP unavailable for this connection, falling back to SCP");
        transfer
            .scp_upload(&connection, &local, &remote, recursive)
            .await?;
        println!(
            "\u{2705} Uploaded {} \u{2192} {}:{remote} (via SCP)",
            local.display(),
            connection.host
        );
        return Ok(());
    }

    if recursive {
        if !local.is_dir() {
            bail!(
                "--recursive requires a local directory, but '{}' is not a directory",
                local.display()
            );
        }
        let progress: crate::services::transfer::ProgressFn = Box::new(|done, _total| {
            eprint!("\r  uploaded {done} bytes total   ");
        });
        let (files, bytes) = transfer
            .upload_recursive(&connection, &local, &remote, mode, Some(&progress))
            .await?;
        eprintln!();
        println!(
            "✅ Uploaded {} → {}:{remote} ({files} files, {bytes} bytes)",
            local.display(),
            connection.host
        );
    } else {
        if local.is_dir() {
            bail!(
                "'{}' is a directory — use --recursive (-r) to upload directories",
                local.display()
            );
        }
        let progress: crate::services::transfer::ProgressFn = Box::new(|done, total| {
            if let Some(t) = total {
                let pct = done * 100 / t.max(1);
                eprint!("\r  upload {done}/{t} bytes ({pct}%)   ");
            } else {
                eprint!("\r  upload {done} bytes   ");
            }
        });
        let written = transfer
            .upload(&connection, &local, &remote, offset, mode, Some(progress))
            .await?;
        eprintln!();
        info!("upload complete: {written} bytes");
        println!(
            "✅ Uploaded {} → {}:{remote} ({written} bytes)",
            local.display(),
            connection.host
        );
    }
    Ok(())
}

pub async fn execute_download(
    target: String,
    remote: String,
    local: PathBuf,
    recursive: bool,
    config: AppConfig,
) -> Result<()> {
    let ssh_service = SshService::new(config.clone())?;
    let connection = resolve_connection(&ssh_service, &target, "download").await?;

    let transfer = TransferService::new(config)?;

    // When SFTP is unavailable (e.g. interactive bastion + Kerberos), use SCP.
    if !transfer.has_sftp(&connection) {
        info!("SFTP unavailable for this connection, falling back to SCP");
        transfer
            .scp_download(&connection, &remote, &local, recursive)
            .await?;
        println!(
            "\u{2705} Downloaded {}:{remote} \u{2192} {} (via SCP)",
            connection.host,
            local.display()
        );
        return Ok(());
    }

    if recursive {
        let progress: crate::services::transfer::ProgressFn = Box::new(|done, _total| {
            eprint!("\r  downloaded {done} bytes total   ");
        });
        let (files, bytes) = transfer
            .download_recursive(&connection, &remote, &local, Some(&progress))
            .await?;
        eprintln!();
        println!(
            "✅ Downloaded {}:{remote} → {} ({files} files, {bytes} bytes)",
            connection.host,
            local.display()
        );
    } else {
        let progress: crate::services::transfer::ProgressFn = Box::new(|done, total| {
            if let Some(t) = total {
                let pct = done * 100 / t.max(1);
                eprint!("\r  download {done}/{t} bytes ({pct}%)   ");
            } else {
                eprint!("\r  download {done} bytes   ");
            }
        });
        let read = transfer
            .download(&connection, &remote, &local, Some(progress))
            .await?;
        eprintln!();
        info!("download complete: {read} bytes");
        println!(
            "✅ Downloaded {}:{remote} → {} ({read} bytes)",
            connection.host,
            local.display()
        );
    }
    Ok(())
}

async fn resolve_connection(
    ssh_service: &SshService,
    target: &str,
    action: &str,
) -> Result<crate::models::Connection> {
    let mut conn_opt = ssh_service.get_connection(target).await.unwrap_or_default();
    if conn_opt.is_none() {
        conn_opt =
            crate::cli::utils::fuzzy_select_connection(ssh_service, target, action, true).await?;
    }
    match conn_opt {
        Some(c) => Ok(c),
        None => bail!("no connection selected"),
    }
}
