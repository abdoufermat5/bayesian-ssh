//! SCP transfer via an interactive bastion (OVH "The Bastion" / `--osh scp`).
//!
//! When the bastion does not support TCP forwarding (no `-J`, no `-W`),
//! file transfers must go through a wrapper script that translates the
//! standard `scp` protocol into the bastion's own command interface.
//!
//! This module:
//! 1. Auto-generates the wrapper script at runtime in the bssh data dir.
//! 2. Exposes `scp_upload` / `scp_download` that shell out to `scp -S <wrapper>`.
//! 3. Supports recursive directory transfers via `scp -r`.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::process::Command as TokioCommand;
use tracing::{debug, info};

use crate::models::Connection;

/// Ensure the bastion SCP wrapper script exists and return its path.
///
/// The script is written once to `~/.local/share/bayesian-ssh/scp-wrapper.sh`.
/// It is safe to overwrite on every call so updates propagate automatically.
pub fn ensure_scp_wrapper() -> Result<PathBuf> {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("bayesian-ssh");
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("create data dir {}", data_dir.display()))?;

    let wrapper = data_dir.join("scp-wrapper.sh");

    let script = r#"#!/bin/sh
# bayesian-ssh SCP wrapper for interactive bastions (auto-generated)
# Translates scp's internal SSH invocation into the bastion --osh scp protocol.
while ! [ "$1" = "--" ] ; do
    if [ "$1" = "-l" ] ; then
        remoteuser="--user $2"
        shift 2
    elif [ "$1" = "-p" ] ; then
        remoteport="--port $2"
        shift 2
    else
        sshcmdline="$sshcmdline $1"
        shift
    fi
done
host="$2"
scpcmd=$(echo "$3" | sed -e 's/#/##/g;s/ /#/g')
username=$(echo "$remoteuser" | awk '{print $2}')
exec ssh -A -T -K "$username"@BASTION_PLACEHOLDER $sshcmdline -- $remoteuser $remoteport --host "$host" --osh scp --scp-cmd "$scpcmd"
"#;

    std::fs::write(&wrapper, script)
        .with_context(|| format!("write scp wrapper to {}", wrapper.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("chmod +x {}", wrapper.display()))?;
    }

    Ok(wrapper)
}

/// Create a connection-specific wrapper by substituting the bastion hostname.
///
/// Returns the path to the generated script.
fn create_bastion_wrapper(bastion: &str) -> Result<PathBuf> {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("bayesian-ssh");
    std::fs::create_dir_all(&data_dir)?;

    // Use a sanitised bastion name in the filename to avoid collisions.
    let safe_name: String = bastion
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect();
    let wrapper_path = data_dir.join(format!("scp-wrapper-{safe_name}.sh"));

    let template = ensure_scp_wrapper()?;
    let script = std::fs::read_to_string(&template)?;
    let script = script.replace("BASTION_PLACEHOLDER", bastion);

    std::fs::write(&wrapper_path, &script)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper_path, std::fs::Permissions::from_mode(0o755))?;
    }

    Ok(wrapper_path)
}

// ──────────────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────────────

/// Upload `local` to `remote_path` on the target behind the bastion.
///
/// `recursive` enables `-r` for directory trees.
pub async fn scp_upload(
    conn: &Connection,
    local: &Path,
    remote_path: &str,
    recursive: bool,
) -> Result<()> {
    let argv = build_scp_argv(conn, local, remote_path, recursive, Direction::Upload)?;
    run_scp(&argv).await
}

/// Download `remote_path` from the target to `local`.
pub async fn scp_download(
    conn: &Connection,
    remote_path: &str,
    local: &Path,
    recursive: bool,
) -> Result<()> {
    let argv = build_scp_argv(conn, local, remote_path, recursive, Direction::Download)?;
    run_scp(&argv).await
}

// ──────────────────────────────────────────────────────────────────────────
// Internals
// ──────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum Direction {
    Upload,
    Download,
}

fn build_scp_argv(
    conn: &Connection,
    local: &Path,
    remote_path: &str,
    recursive: bool,
    direction: Direction,
) -> Result<Vec<String>> {
    let mut argv: Vec<String> = vec!["scp".into()];

    // Use legacy SCP protocol (required for the wrapper).
    argv.push("-O".into());

    if recursive {
        argv.push("-r".into());
    }

    let remote_spec = format!("{}@{}:{}", conn.user, conn.host, remote_path);

    if conn.use_kerberos && conn.bastion.is_some() {
        // Interactive bastion mode — use the wrapper script.
        let bastion = conn.bastion.as_deref().unwrap();
        let wrapper = create_bastion_wrapper(bastion)?;
        info!("using SCP bastion wrapper: {}", wrapper.display());
        argv.push("-S".into());
        argv.push(wrapper.to_string_lossy().into_owned());

        match direction {
            Direction::Upload => {
                argv.push(local.to_string_lossy().into_owned());
                argv.push(remote_spec);
            }
            Direction::Download => {
                argv.push(remote_spec);
                argv.push(local.to_string_lossy().into_owned());
            }
        }
    } else if conn.bastion.is_some() {
        // Standard jump-host mode — use ProxyJump.
        let bastion = conn.bastion.as_deref().unwrap();
        let bu = conn.bastion_user.as_deref().unwrap_or(&conn.user);
        argv.push("-o".into());
        argv.push(format!("ProxyJump={bu}@{bastion}"));

        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        argv.push("-P".into());
        argv.push(conn.port.to_string());

        match direction {
            Direction::Upload => {
                argv.push(local.to_string_lossy().into_owned());
                argv.push(remote_spec);
            }
            Direction::Download => {
                argv.push(remote_spec);
                argv.push(local.to_string_lossy().into_owned());
            }
        }
    } else {
        // Direct connection — plain scp.
        if conn.use_kerberos {
            argv.push("-o".into());
            argv.push("GSSAPIAuthentication=yes".into());
        }
        if let Some(key) = &conn.key_path {
            argv.push("-i".into());
            argv.push(key.clone());
        }
        argv.push("-P".into());
        argv.push(conn.port.to_string());

        match direction {
            Direction::Upload => {
                argv.push(local.to_string_lossy().into_owned());
                argv.push(remote_spec);
            }
            Direction::Download => {
                argv.push(remote_spec);
                argv.push(local.to_string_lossy().into_owned());
            }
        }
    }

    debug!("scp argv: {:?}", argv);
    Ok(argv)
}

async fn run_scp(argv: &[String]) -> Result<()> {
    let (cmd_name, args) = argv.split_first().expect("argv non-empty");

    info!("running: {} {}", cmd_name, args.join(" "));

    let status = TokioCommand::new(cmd_name)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("failed to spawn scp process")?;

    if status.success() {
        info!("scp completed successfully");
        Ok(())
    } else {
        let code = status.code().unwrap_or(-1);
        Err(anyhow::anyhow!("scp exited with code {code}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn conn(kerb: bool, bastion: Option<&str>) -> Connection {
        Connection::new(
            "x".into(),
            "target.example".into(),
            "alice".into(),
            22,
            bastion.map(String::from),
            None,
            kerb,
            None,
        )
    }

    #[test]
    fn direct_upload_argv() {
        let argv = build_scp_argv(
            &conn(false, None),
            Path::new("/tmp/file.txt"),
            "/remote/file.txt",
            false,
            Direction::Upload,
        )
        .unwrap();

        assert_eq!(argv[0], "scp");
        assert!(argv.contains(&"-O".to_string()));
        assert!(argv.contains(&"-P".to_string()));
        assert!(argv.contains(&"22".to_string()));
        assert!(argv.last().unwrap().contains("alice@target.example:/remote/file.txt"));
    }

    #[test]
    fn bastion_kerberos_upload_uses_wrapper() {
        let argv = build_scp_argv(
            &conn(true, Some("bastion.example")),
            Path::new("/tmp/file.txt"),
            "/remote/file.txt",
            false,
            Direction::Upload,
        )
        .unwrap();

        assert!(argv.contains(&"-S".to_string()));
        assert!(argv.contains(&"-O".to_string()));
    }

    #[test]
    fn jump_host_upload_uses_proxy_jump() {
        let argv = build_scp_argv(
            &conn(false, Some("jump.example")),
            Path::new("/tmp/file.txt"),
            "/remote/file.txt",
            false,
            Direction::Upload,
        )
        .unwrap();

        assert!(argv.iter().any(|a| a.contains("ProxyJump")));
        assert!(!argv.contains(&"-S".to_string()));
    }

    #[test]
    fn recursive_flag_added() {
        let argv = build_scp_argv(
            &conn(false, None),
            Path::new("/tmp/dir"),
            "/remote/dir",
            true,
            Direction::Upload,
        )
        .unwrap();

        assert!(argv.contains(&"-r".to_string()));
    }
}
