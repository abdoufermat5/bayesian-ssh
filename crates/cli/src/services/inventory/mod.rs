use crate::models::{DiscoveredHost, Source, SourceKind};
use anyhow::{bail, Result};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub mod ansible;
pub mod aws;
pub mod gcp;
pub mod ssh_config;
pub mod sync;
pub mod tailscale;

/// How long a provider command may run before it is killed.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// A place connections can be discovered from.
pub trait InventoryProvider {
    fn fetch(&self, source: &Source) -> Result<Vec<DiscoveredHost>>;
}

pub fn provider_for(kind: SourceKind) -> Box<dyn InventoryProvider> {
    match kind {
        SourceKind::SshConfig => Box::new(ssh_config::SshConfigProvider),
        SourceKind::Ansible => Box::new(ansible::AnsibleProvider),
        SourceKind::Tailscale => Box::new(tailscale::TailscaleProvider),
        SourceKind::Aws => Box::new(aws::AwsProvider),
        SourceKind::Gcp => Box::new(gcp::GcpProvider),
        SourceKind::Team => Box::new(TeamProvider),
    }
}

/// Team vaults are decrypted and applied by `bssh teams sync`, which needs
/// the device passphrase and the network; there is nothing to fetch here.
struct TeamProvider;

impl InventoryProvider for TeamProvider {
    fn fetch(&self, source: &Source) -> Result<Vec<DiscoveredHost>> {
        bail!(
            "'{}' is a team vault; sync it with `bssh teams sync`",
            source.name
        )
    }
}

/// Run an inventory tool with a fixed argv (never through a shell) and return
/// its stdout. `hint` is appended to the "not found" error so the user knows
/// what to install.
pub(crate) fn run_command(bin: &str, args: &[String], hint: &str) -> Result<String> {
    let mut child = match Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            bail!("`{}` not found in PATH — {}", bin, hint)
        }
        Err(e) => bail!("failed to run `{}`: {}", bin, e),
    };

    // Drain both pipes on separate threads so a large inventory cannot fill
    // the pipe buffer while we wait for the process to exit.
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= COMMAND_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out_reader.join();
            let _ = err_reader.join();
            bail!("`{}` timed out after 60s", bin);
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            bail!("`{}` exited with status {}", bin, status);
        }
        bail!("`{}` failed: {}", bin, stderr);
    }

    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// Expand a leading `~/` in a configured path.
pub(crate) fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

pub(crate) fn config_str(config: &serde_json::Value, key: &str) -> Option<String> {
    config
        .get(key)
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty())
}

pub(crate) fn config_bool(config: &serde_json::Value, key: &str) -> bool {
    config.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

pub(crate) fn config_strings(config: &serde_json::Value, key: &str) -> Vec<String> {
    config
        .get(key)
        .and_then(|v| v.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}
