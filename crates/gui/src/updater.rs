// In-app updates: signed GitHub release artifacts, verified by tauri-plugin-updater.
//
// The minisign public key is baked in at compile time from
// BAYESIAN_SSH_UPDATER_PUBLIC_KEY. Builds without it (local dev, forks) keep
// every other feature but refuse to update, with an explicit error and before
// any network request. There is no unsigned or default-key path.
//
// Only the formats the updater can replace in place are updatable: the
// AppImage and the desktop .deb/.rpm built by the Tauri bundler (it stamps the
// bundle type into the binary). Everything else reports who owns updates and
// never registers the plugin:
// - the snap: read-only squashfs, the Snap Store delivers updates;
// - raw binaries (install.sh, release tarballs), the unified CLI+GUI .deb/.rpm
//   and source builds: the updater would otherwise overwrite them with an
//   AppImage or install a second, conflicting package.

use std::time::Duration;

use serde::Serialize;
use tauri::utils::config::BundleType;
use tauri::{AppHandle, Manager, Runtime, Url};
use tauri_plugin_updater::{Config, UpdaterExt};

use crate::commands::{close_all_ptys, PtyState};

const ENDPOINT: &str =
    "https://github.com/abdoufermat5/bayesian-ssh/releases/latest/download/latest.json";
const PUBLIC_KEY: Option<&str> = option_env!("BAYESIAN_SSH_UPDATER_PUBLIC_KEY");

const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
// Covers the whole download as well as the re-check, so it is generous.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

const MISSING_KEY: &str = "updates are unavailable: this build has no updater public key \
(BAYESIAN_SSH_UPDATER_PUBLIC_KEY was not set at compile time). Install a release build from GitHub.";

// snapd sets `SNAP` (to `/snap/<name>/<rev>`) for every process it launches and
// `SNAP_NAME` to the snap's name.
const SNAP_NAME: &str = "bayesian-ssh";

const SNAP_MANAGED: &str =
    "updates are installed by the Snap Store; the in-app updater is disabled in the snap.";
const MANUAL_MANAGED: &str = "this installation cannot update itself: only the AppImage and the \
bayesian-ssh-desktop .deb/.rpm from GitHub releases can. Update it the way it was installed.";

/// Who owns updates when the in-app updater does not apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateChannel {
    /// The Snap Store.
    Snap,
    /// The user: raw binary, unified package or source build.
    Manual,
}

impl UpdateChannel {
    fn as_str(self) -> &'static str {
        match self {
            UpdateChannel::Snap => "snap",
            UpdateChannel::Manual => "manual",
        }
    }

    fn message(self) -> &'static str {
        match self {
            UpdateChannel::Snap => SNAP_MANAGED,
            UpdateChannel::Manual => MANUAL_MANAGED,
        }
    }
}

/// `None` when the in-app updater can install updates for this process.
/// Takes the environment lookup and bundle type so it stays pure.
fn channel(
    env: impl Fn(&str) -> Option<String>,
    bundle: Option<BundleType>,
) -> Option<UpdateChannel> {
    let snap = env("SNAP").is_some_and(|v| !v.trim().is_empty())
        || env("SNAP_NAME").is_some_and(|v| v.trim() == SNAP_NAME);
    if snap {
        return Some(UpdateChannel::Snap);
    }
    match bundle {
        Some(BundleType::AppImage | BundleType::Deb | BundleType::Rpm) => None,
        _ => Some(UpdateChannel::Manual),
    }
}

fn channel_now() -> Option<UpdateChannel> {
    channel(
        |key| std::env::var(key).ok(),
        tauri::utils::platform::bundle_type(),
    )
}

fn refuse_if_managed() -> Result<(), String> {
    match channel_now() {
        Some(channel) => Err(channel.message().to_string()),
        None => Ok(()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

fn configured_key(raw: Option<&'static str>) -> Option<&'static str> {
    raw.map(str::trim).filter(|k| !k.is_empty())
}

fn require_key(raw: Option<&'static str>) -> Result<&'static str, String> {
    configured_key(raw).ok_or_else(|| MISSING_KEY.to_string())
}

fn same_version(a: &str, b: &str) -> bool {
    a.trim_start_matches('v') == b.trim_start_matches('v')
}

/// The updater plugin, or `None` when this build or installation cannot update
/// itself (no endpoint is ever known then).
pub fn plugin(
    context: &mut tauri::Context<tauri::Wry>,
) -> Option<tauri::plugin::TauriPlugin<tauri::Wry, Config>> {
    if channel_now().is_some() {
        return None;
    }
    let key = configured_key(PUBLIC_KEY)?;
    // The plugin deserializes its config before applying Builder overrides.
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({ "pubkey": key, "endpoints": [ENDPOINT] }),
    );
    Some(tauri_plugin_updater::Builder::new().pubkey(key).build())
}

fn updater<R: Runtime>(
    app: &AppHandle<R>,
    timeout: Duration,
) -> Result<tauri_plugin_updater::Updater, String> {
    require_key(PUBLIC_KEY)?;
    let endpoint = Url::parse(ENDPOINT).map_err(|e| e.to_string())?;
    app.updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| e.to_string())?
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())
}

/// Look for a newer signed release. `None` when this build is current.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    refuse_if_managed()?;
    let updater = updater(&app, CHECK_TIMEOUT)?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        current_version: u.current_version,
        notes: u.body,
    }))
}

/// Install `version`, the one the user was shown. The latest release is
/// fetched again and the install is refused if it moved on in the meantime.
/// The updater verifies the artifact's signature against the embedded key
/// before installing; open sessions are closed and the app restarts.
#[tauri::command]
pub async fn install_update(app: AppHandle, version: String) -> Result<(), String> {
    refuse_if_managed()?;
    let updater = updater(&app, INSTALL_TIMEOUT)?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("no update is available")?;
    if !same_version(&update.version, &version) {
        return Err(format!(
            "the latest release changed from {version} to {}; check for updates again",
            update.version
        ));
    }
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    // Same teardown as quitting: children are killed, not left orphaned.
    if let Some(state) = app.try_state::<PtyState>() {
        let _ = close_all_ptys(app.clone(), state);
    }
    app.restart()
}

/// How this installation receives updates: `"snap"` (Snap Store), `"manual"`
/// (the user updates it the way it was installed), or `null` when the in-app
/// updater applies. Read once by Settings, without hitting the endpoint.
#[tauri::command]
pub fn update_managed_by() -> Option<&'static str> {
    channel_now().map(UpdateChannel::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn missing_or_blank_key_is_an_explicit_error() {
        assert!(require_key(None)
            .unwrap_err()
            .contains("BAYESIAN_SSH_UPDATER_PUBLIC_KEY"));
        assert!(require_key(Some("  \n")).is_err());
        assert_eq!(require_key(Some(" abc\n")).unwrap(), "abc");
    }

    #[test]
    fn version_match_ignores_leading_v() {
        assert!(same_version("v1.2.3", "1.2.3"));
        assert!(!same_version("1.2.4", "1.2.3"));
    }

    #[test]
    fn bundler_formats_update_in_app() {
        for bundle in [BundleType::AppImage, BundleType::Deb, BundleType::Rpm] {
            assert_eq!(channel(no_env, Some(bundle.clone())), None, "{bundle:?}");
        }
    }

    #[test]
    fn unstamped_binaries_are_updated_manually() {
        // Raw release binaries, the unified package and source builds carry no
        // bundle type: the updater would replace them with an AppImage.
        assert_eq!(channel(no_env, None), Some(UpdateChannel::Manual));
    }

    #[test]
    fn snap_env_wins_over_bundle_type() {
        let snap = |key: &str| (key == "SNAP").then(|| "/snap/bayesian-ssh/7".to_string());
        assert_eq!(
            channel(snap, Some(BundleType::Deb)),
            Some(UpdateChannel::Snap)
        );
        let named = |key: &str| (key == "SNAP_NAME").then(|| SNAP_NAME.to_string());
        assert_eq!(channel(named, None), Some(UpdateChannel::Snap));
    }

    #[test]
    fn other_snaps_or_blank_values_do_not_count() {
        let other = |key: &str| (key == "SNAP_NAME").then(|| "other".to_string());
        assert_eq!(channel(other, Some(BundleType::AppImage)), None);
        let blank = |key: &str| (key == "SNAP").then(String::new);
        assert_eq!(channel(blank, Some(BundleType::AppImage)), None);
    }
}
