use crate::config::AppConfig;
use anyhow::{anyhow, Result};
use std::process::Command;

/// Launch the desktop app detached in the background
pub async fn execute(_config: AppConfig) -> Result<()> {
    let possible_binaries = ["bayesian-ssh-desktop", "desktop"];
    let mut command_to_run = None;

    // Search in PATH
    for bin in &possible_binaries {
        if let Some(path) = find_in_path(bin) {
            command_to_run = Some(path.to_string_lossy().into_owned());
            break;
        }
    }

    // Search relative to the current executable directory
    if command_to_run.is_none() {
        if let Ok(mut exe_path) = std::env::current_exe() {
            exe_path.pop(); // Remove target name

            // Check debug target folder
            let debug_desktop = exe_path.join("desktop");
            // Check release target folder
            let release_desktop = exe_path.join("release").join("desktop");

            if debug_desktop.exists() {
                command_to_run = Some(debug_desktop.to_string_lossy().into_owned());
            } else if release_desktop.exists() {
                command_to_run = Some(release_desktop.to_string_lossy().into_owned());
            }
        }
    }

    // Development project fallback: check standard cargo output paths
    if command_to_run.is_none() {
        let dev_paths = [
            "desktop/src-tauri/target/debug/desktop",
            "desktop/src-tauri/target/release/desktop",
            "desktop/src-tauri/target/x86_64-unknown-linux-gnu/debug/desktop",
            "desktop/src-tauri/target/x86_64-unknown-linux-gnu/release/desktop",
        ];
        for path_str in &dev_paths {
            let path = std::path::Path::new(path_str);
            if path.exists() {
                command_to_run = Some(path.to_string_lossy().into_owned());
                break;
            }
        }
    }

    let bin_path = command_to_run.ok_or_else(|| {
        anyhow!(
            "Could not find 'bayesian-ssh-desktop' executable in PATH or target directories.\n\
             Please build the desktop application using 'make release-desktop' or 'make build-desktop' first."
        )
    })?;

    println!("Launching bayesian-ssh-desktop in the background...");

    // Spawn detached process
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let mut cmd = Command::new(&bin_path);
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null());

        // Detach the child process group so closing terminal does not kill the child
        cmd.process_group(0);

        cmd.spawn()?;
    }

    #[cfg(not(unix))]
    {
        Command::new(&bin_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .spawn()?;
    }

    Ok(())
}

fn find_in_path(bin_name: &str) -> Option<std::path::PathBuf> {
    if let Some(paths) = std::env::var_os("PATH") {
        for path in std::env::split_paths(&paths) {
            let bin_path = path.join(bin_name);
            if bin_path.is_file() {
                return Some(bin_path);
            }
        }
    }
    None
}
