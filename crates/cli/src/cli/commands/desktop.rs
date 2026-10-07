use crate::config::AppConfig;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const GUI_BINARIES: [&str; 3] = ["bayesian-ssh-gui", "bssh-gui", "bayesian-ssh-desktop"];

/// Launch the desktop app detached in the background
pub async fn execute(_config: AppConfig) -> Result<()> {
    // Prefer the GUI installed/built next to this executable (same install or
    // cargo target dir, so versions match), then fall back to PATH. The
    // current working directory is deliberately never searched: running
    // `bssh desktop` inside an untrusted checkout must not execute a
    // `target/*/bayesian-ssh-gui` planted there.
    let bin_path = find_next_to_current_exe()
        .or_else(|| GUI_BINARIES.iter().find_map(|bin| find_in_path(bin)))
        .ok_or_else(|| {
            anyhow!(
                "Could not find 'bayesian-ssh-gui' next to this executable or in PATH.\n\
                 Please build the desktop application using 'make release' or 'make build' first."
            )
        })?;

    println!("Launching {} in the background...", bin_path.display());

    let mut cmd = Command::new(&bin_path);
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null());

    // Detach the child process group so closing terminal does not kill the child
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    cmd.spawn()
        .with_context(|| format!("failed to launch {}", bin_path.display()))?;

    Ok(())
}

/// Look for the GUI in the directory of the running executable, then in a
/// sibling `release` directory (cargo `target/debug` → `target/release`).
fn find_next_to_current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let release_dir = exe_dir.parent().map(|p| p.join("release"));
    std::iter::once(exe_dir.to_path_buf())
        .chain(release_dir)
        .find_map(|dir| find_in_dir(&dir))
}

fn find_in_dir(dir: &Path) -> Option<PathBuf> {
    GUI_BINARIES
        .iter()
        .map(|bin| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

fn find_in_path(bin_name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        // Empty/relative PATH entries resolve against the CWD; skip them.
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(bin_name))
        .find(|candidate| candidate.is_file())
}
