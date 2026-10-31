use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tracing::info;
use uuid::Uuid;

use crate::cli::utils::{format_bytes, format_elapsed, resolve_connection};
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::upload::{
    local_basename, local_stats, resolve_remote_path, upload_to_hosts, HostUploadResult,
    UploadEvent, UploadEventFn, UploadRequest,
};
use crate::services::{SshService, TransferService};

// ─────────────────────────────────────────────────────────────────────────────
// Pure argument parsing (unit-tested)
// ─────────────────────────────────────────────────────────────────────────────

/// Parsed positional/flag layout for `bssh upload`.
#[derive(Debug, PartialEq, Eq)]
pub struct UploadArgs {
    /// Resolved target specs (comma-split); empty when `--all`/`--tag` is used.
    pub targets: Vec<String>,
    pub local: PathBuf,
    pub remote: String,
}

/// Split a comma-separated target list, trimming blanks.
pub fn split_targets(spec: &str) -> Vec<String> {
    spec.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Which hosts `bssh upload` sends to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostSelector {
    /// Hosts listed in the first positional argument.
    Named,
    /// Every saved connection (`--all`).
    All,
    /// Every connection carrying this tag (`--tag`).
    Tag(String),
}

impl HostSelector {
    /// Maps the mutually exclusive `--all` / `--tag` flags; `--all` wins if both were set.
    pub fn from_flags(all: bool, tag: Option<String>) -> Self {
        if all {
            Self::All
        } else if let Some(tag) = tag {
            Self::Tag(tag)
        } else {
            Self::Named
        }
    }
}

/// Transfer flags of `bssh upload`.
pub struct UploadOptions {
    pub dry_run: bool,
    /// Maximum number of hosts uploaded to concurrently.
    pub parallel: usize,
    /// Byte offset to resume from (single host only).
    pub offset: u64,
    /// Remote file permissions.
    pub mode: u32,
    pub recursive: bool,
}

/// Everything `bssh upload` was given on the command line.
pub struct UploadCommand {
    /// `[TARGETS] LOCAL REMOTE`, as typed.
    pub positional: Vec<String>,
    pub selector: HostSelector,
    pub options: UploadOptions,
}

/// Interpret the 2-or-3 positional arguments together with `--all`/`--tag`.
///
/// With `--all`/`--tag`, exactly two positionals (`LOCAL REMOTE`) are expected.
/// Otherwise three (`TARGETS LOCAL REMOTE`), where `TARGETS` is a
/// comma-separated list of names/aliases/hosts.
pub fn parse_upload_args(raw: &[String], selector: &HostSelector) -> Result<UploadArgs> {
    let use_selector = *selector != HostSelector::Named;
    let expected = if use_selector { 2 } else { 3 };
    if raw.len() != expected {
        let hint = if use_selector {
            "with --all/-t expected exactly: <LOCAL> <REMOTE>"
        } else {
            "expected exactly: <TARGETS> <LOCAL> <REMOTE> \
             (or use --all / -t/--tag with <LOCAL> <REMOTE>)"
        };
        bail!("invalid upload arguments — {hint}; got {}.", raw.len());
    }
    if use_selector {
        Ok(UploadArgs {
            targets: Vec::new(),
            local: PathBuf::from(&raw[0]),
            remote: raw[1].clone(),
        })
    } else {
        let targets = split_targets(&raw[0]);
        if targets.is_empty() {
            bail!("no target hosts given");
        }
        Ok(UploadArgs {
            targets,
            local: PathBuf::from(&raw[1]),
            remote: raw[2].clone(),
        })
    }
}

/// Reject flag combinations the transfer service cannot honour.
pub fn validate_upload_flags(options: &UploadOptions, host_count: usize) -> Result<()> {
    if options.offset > 0 && options.recursive {
        bail!("--offset cannot be combined with --recursive (-r)");
    }
    if options.offset > 0 && host_count > 1 {
        bail!("--offset can only be used when uploading to a single host");
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Upload
// ─────────────────────────────────────────────────────────────────────────────

pub async fn execute_upload(command: UploadCommand, config: AppConfig) -> Result<()> {
    let UploadCommand {
        positional,
        selector,
        options,
    } = command;
    let plan = parse_upload_args(&positional, &selector)?;

    let ssh_service = SshService::new(config.clone())?;
    let targets = resolve_targets(&ssh_service, &plan, &selector, &config).await?;
    if targets.is_empty() {
        println!("No connections matching criteria.");
        return Ok(());
    }
    validate_upload_flags(&options, targets.len())?;

    let meta = tokio::fs::metadata(&plan.local)
        .await
        .map_err(|e| anyhow::anyhow!("cannot read '{}': {e}", plan.local.display()))?;
    let local_is_dir = meta.is_dir();
    if local_is_dir && !options.recursive {
        bail!(
            "'{}' is a directory — pass --recursive (-r) to upload directories",
            plan.local.display()
        );
    }
    let (size_bytes, _files) = local_stats(&plan.local).await?;

    if options.dry_run {
        let resolved = resolve_remote_path(
            &plan.remote,
            &local_basename(&plan.local),
            local_is_dir,
            false,
        );
        println!("🔍 [DRY-RUN PREVIEW]");
        println!(
            "Upload {} ({}) → {} on {} host(s):",
            plan.local.display(),
            format_bytes(size_bytes),
            resolved,
            targets.len()
        );
        for conn in &targets {
            println!("  - {} ({}@{})", conn.name, conn.user, conn.host);
        }
        println!("\nDry-run complete. No files were transferred.");
        return Ok(());
    }

    for connection in &targets {
        ssh_service.confirm_pending_change(connection)?;
    }

    println!(
        "Uploading {} ({}) → {} on {} host(s)",
        local_basename(&plan.local),
        format_bytes(size_bytes),
        plan.remote,
        targets.len()
    );

    let interactive = std::io::stderr().is_terminal();
    let multi = MultiProgress::new();
    let bars: Arc<Mutex<HashMap<Uuid, ProgressBar>>> = Arc::new(Mutex::new(HashMap::new()));

    let on_event: Option<UploadEventFn> = Some(Arc::new({
        let bars = Arc::clone(&bars);
        let multi = multi.clone();
        move |event: UploadEvent| match event {
            UploadEvent::Started {
                connection_id,
                name,
                host,
            } => {
                if !interactive {
                    return;
                }
                let bar = multi.add(ProgressBar::new(0));
                bar.set_style(
                    ProgressStyle::with_template(
                        "{prefix:>20} {spinner} {bar:30} {bytes:>9}/{total_bytes:<9} \
                         {binary_bytes_per_sec:>11} ETA {eta:>5} {msg}",
                    )
                    .expect("valid indicatif template"),
                );
                bar.set_prefix(format!("{name} ({host})"));
                bar.enable_steady_tick(Duration::from_millis(100));
                bars.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(connection_id, bar);
            }
            UploadEvent::Progress {
                connection_id,
                bytes_done,
                bytes_total,
            } => {
                if !interactive {
                    return;
                }
                let bars = bars.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(bar) = bars.get(&connection_id) {
                    if let Some(total) = bytes_total {
                        bar.set_length(total);
                    }
                    bar.set_position(bytes_done);
                }
            }
            UploadEvent::Finished { result } => {
                if interactive {
                    let mut bars = bars.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(bar) = bars.remove(&result.connection_id) {
                        finish_bar(&bar, &result);
                    }
                } else {
                    eprintln!("{}", host_line(&result));
                }
            }
        }
    }));

    let request = UploadRequest {
        local: plan.local,
        remote: plan.remote,
        mode: options.mode,
        offset: options.offset,
        recursive: options.recursive,
    };

    let results =
        upload_to_hosts(&config, targets, request, options.parallel.max(1), on_event).await?;

    print_summary(&results);

    let failed = results.iter().filter(|r| !r.ok).count();
    if failed > 0 {
        bail!("upload failed on {failed} of {} host(s)", results.len());
    }
    Ok(())
}

fn finish_bar(bar: &ProgressBar, result: &HostUploadResult) {
    let via = if result.via_scp { " (via SCP)" } else { "" };
    if result.via_scp {
        // SCP reports no byte progress: drop the empty `0 B/0 B` bar so the
        // host ends on one clean status line.
        bar.set_style(
            ProgressStyle::with_template("{prefix:>20} {msg}").expect("valid indicatif template"),
        );
    }
    if result.ok {
        bar.finish_with_message(format!("✔ {}{via}", format_bytes(result.bytes)));
    } else {
        bar.abandon_with_message(format!(
            "✖ {}",
            result.error.as_deref().unwrap_or("upload failed")
        ));
    }
}

fn host_line(result: &HostUploadResult) -> String {
    let via = if result.via_scp { " via SCP" } else { "" };
    if result.ok {
        format!(
            "✔ {} ({}): {} in {}{via}",
            result.name,
            result.host,
            format_bytes(result.bytes),
            format_elapsed(chrono::Duration::milliseconds(result.elapsed_ms as i64)),
        )
    } else {
        format!(
            "✖ {} ({}): {}",
            result.name,
            result.host,
            result.error.as_deref().unwrap_or("upload failed")
        )
    }
}

fn print_summary(results: &[HostUploadResult]) {
    println!();
    println!("Upload summary:");
    for r in results {
        if r.ok {
            println!(
                "  ✔ {:<18} {:<26} {} in {}{}",
                r.name,
                r.host,
                format_bytes(r.bytes),
                format_elapsed(chrono::Duration::milliseconds(r.elapsed_ms as i64)),
                if r.via_scp { " (via SCP)" } else { "" },
            );
        } else {
            println!(
                "  ✖ {:<18} {:<26} {}",
                r.name,
                r.host,
                r.error.as_deref().unwrap_or("upload failed"),
            );
        }
    }
    let ok = results.iter().filter(|r| r.ok).count();
    println!("\nUploaded to {ok}/{} hosts", results.len());
}

/// Resolve the requested targets into connections, deduplicated by id.
async fn resolve_targets(
    ssh_service: &SshService,
    plan: &UploadArgs,
    selector: &HostSelector,
    config: &AppConfig,
) -> Result<Vec<Connection>> {
    let mut connections = if *selector == HostSelector::All {
        ssh_service.list_connections(None, false).await?
    } else if let HostSelector::Tag(tag) = selector {
        ssh_service.list_connections(Some(tag), false).await?
    } else if plan.targets.len() == 1 {
        // Keep the interactive fuzzy search for a single target.
        vec![resolve_connection(ssh_service, &plan.targets[0], "upload", true, config).await?]
    } else {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for name in &plan.targets {
            match ssh_service.connection(name).await? {
                Some(conn) => found.push(conn),
                None => missing.push(name.clone()),
            }
        }
        if !missing.is_empty() {
            bail!(
                "unknown connection(s): {}. Use 'bssh list' to see available names.",
                missing.join(", ")
            );
        }
        found
    };

    let mut seen = std::collections::HashSet::new();
    connections.retain(|c| seen.insert(c.id));
    Ok(connections)
}

// ─────────────────────────────────────────────────────────────────────────────
// Download
// ─────────────────────────────────────────────────────────────────────────────

/// The `bssh download` arguments.
pub struct DownloadArgs {
    pub target: String,
    pub remote: String,
    pub local: PathBuf,
    pub recursive: bool,
}

pub async fn execute_download(args: DownloadArgs, config: AppConfig) -> Result<()> {
    let DownloadArgs {
        target,
        remote,
        local,
        recursive,
    } = args;
    let ssh_service = SshService::new(config.clone())?;
    let connection = resolve_connection(&ssh_service, &target, "download", true, &config).await?;
    ssh_service.confirm_pending_change(&connection)?;

    let transfer = TransferService::new(config)?;

    // When SFTP is unavailable (e.g. interactive bastion + Kerberos), use SCP.
    if !transfer.has_sftp(&connection) {
        info!("SFTP unavailable for this connection, falling back to SCP");
        transfer
            .scp_download(&connection, &remote, &local, recursive)
            .await?;
        println!(
            "✔ Downloaded {}:{remote} → {} (via SCP)",
            connection.host,
            local.display()
        );
        return Ok(());
    }

    if recursive {
        let bar = progress_bar(BarStyle::Throughput);
        bar.set_message("downloading");
        let progress: crate::services::transfer::ProgressFn = {
            let bar = bar.clone();
            Box::new(move |done, _total| {
                bar.set_position(done);
                bar.set_message(format!("{} downloaded", format_bytes(done)));
            })
        };
        let (files, bytes) = transfer
            .download_recursive(&connection, &remote, &local, Some(&progress))
            .await?;
        bar.finish_with_message(format!("✔ {files} files, {}", format_bytes(bytes)));
        println!(
            "✔ Downloaded {}:{remote} → {} ({files} files, {})",
            connection.host,
            local.display(),
            format_bytes(bytes)
        );
    } else {
        let bar = progress_bar(BarStyle::Sized);
        bar.set_message("downloading");
        let progress: crate::services::transfer::ProgressFn = {
            let bar = bar.clone();
            Box::new(move |done, total| {
                if let Some(total) = total {
                    bar.set_length(total);
                }
                bar.set_position(done);
            })
        };
        let read = transfer
            .download(&connection, &remote, &local, Some(progress))
            .await?;
        bar.finish_with_message(format!("✔ {}", format_bytes(read)));
        info!("download complete: {read} bytes");
        println!(
            "✔ Downloaded {}:{remote} → {} ({})",
            connection.host,
            local.display(),
            format_bytes(read)
        );
    }
    Ok(())
}

/// How a download's progress is drawn.
enum BarStyle {
    /// Bytes and rate only: the total is unknown up front (recursive downloads).
    Throughput,
    /// Bar with total and ETA.
    Sized,
}

/// A progress bar for a single download; hidden when stderr is not a TTY.
fn progress_bar(style: BarStyle) -> ProgressBar {
    if !std::io::stderr().is_terminal() {
        return ProgressBar::hidden();
    }
    let template = match style {
        BarStyle::Sized => {
            "{spinner} {bar:30} {bytes:>10}/{total_bytes:<10} \
         {binary_bytes_per_sec:>12} ETA {eta:>6} {msg}"
        }
        BarStyle::Throughput => "{spinner} {bytes:>10} {binary_bytes_per_sec:>12} {msg}",
    };
    let bar = ProgressBar::new(0);
    bar.set_style(ProgressStyle::with_template(template).expect("valid indicatif template"));
    bar.enable_steady_tick(Duration::from_millis(100));
    bar
}

#[cfg(test)]
mod tests {
    use super::{
        parse_upload_args, split_targets, validate_upload_flags, HostSelector, UploadOptions,
    };
    use std::path::PathBuf;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn options(offset: u64, recursive: bool) -> UploadOptions {
        UploadOptions {
            dry_run: false,
            parallel: 1,
            offset,
            mode: 0o644,
            recursive,
        }
    }

    #[test]
    fn three_positionals_are_targets_local_remote() {
        let plan = parse_upload_args(
            &args(&["web1,web2", "./a.tar", "/opt/"]),
            &HostSelector::Named,
        )
        .expect("valid");
        assert_eq!(plan.targets, vec!["web1", "web2"]);
        assert_eq!(plan.local, PathBuf::from("./a.tar"));
        assert_eq!(plan.remote, "/opt/");
    }

    #[test]
    fn two_positionals_require_all_or_tag() {
        assert!(parse_upload_args(&args(&["./a.tar", "/opt/"]), &HostSelector::Named).is_err());
        let tagged = parse_upload_args(
            &args(&["./a.tar", "/opt/"]),
            &HostSelector::Tag("prod".to_string()),
        )
        .expect("valid with tag");
        assert!(tagged.targets.is_empty());
        assert_eq!(tagged.remote, "/opt/");
        let all = parse_upload_args(&args(&["./a.tar", "/opt/"]), &HostSelector::All)
            .expect("valid --all");
        assert!(all.targets.is_empty());
    }

    #[test]
    fn three_positionals_rejected_with_all() {
        assert!(parse_upload_args(&args(&["web1", "./a", "/opt/"]), &HostSelector::All).is_err());
    }

    #[test]
    fn selector_from_flags_prefers_all_over_tag() {
        assert_eq!(HostSelector::from_flags(false, None), HostSelector::Named);
        assert_eq!(HostSelector::from_flags(true, None), HostSelector::All);
        assert_eq!(
            HostSelector::from_flags(false, Some("prod".to_string())),
            HostSelector::Tag("prod".to_string())
        );
        assert_eq!(
            HostSelector::from_flags(true, Some("prod".to_string())),
            HostSelector::All
        );
    }

    #[test]
    fn comma_targets_are_trimmed_and_deduped_by_parser() {
        assert_eq!(
            split_targets(" web1 , web2 ,, db1 "),
            vec!["web1", "web2", "db1"]
        );
        assert!(split_targets(" , ").is_empty());
        // Empty target list is an error even at the right arity.
        assert!(parse_upload_args(&args(&[" , ", "./a", "/opt/"]), &HostSelector::Named).is_err());
    }

    #[test]
    fn offset_rejected_for_multiple_hosts_or_recursive() {
        assert!(validate_upload_flags(&options(0, false), 4).is_ok());
        assert!(validate_upload_flags(&options(1024, false), 1).is_ok());
        assert!(validate_upload_flags(&options(1024, false), 2).is_err());
        assert!(validate_upload_flags(&options(1024, true), 1).is_err());
    }
}
