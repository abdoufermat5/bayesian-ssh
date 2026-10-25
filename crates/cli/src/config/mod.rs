use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};

/// Maximum length of an environment name (it becomes a directory name).
const MAX_ENV_NAME_LEN: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// Override identity file paths (equivalent to `-i` flag). Empty = auto-discover.
    pub identity_files: Vec<PathBuf>,
    /// Prefer ssh-agent when available.
    pub use_agent: bool,
    /// Path to ssh-agent socket; `None` → read `SSH_AUTH_SOCK`.
    pub agent_socket: Option<PathBuf>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            identity_files: Vec::new(),
            use_agent: true,
            agent_socket: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TransportConfig {
    pub force_subprocess: bool,
    /// `strict` | `accept-new` | `off`; `yes`/`no` are accepted as aliases of
    /// `strict`/`off`. Parsed by [`HostKeyPolicy`], never matched as a raw string.
    pub strict_host_key_checking: String,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            force_subprocess: false,
            strict_host_key_checking: HostKeyPolicy::AcceptNew.as_str().into(),
        }
    }
}

impl TransportConfig {
    /// The host-key policy this config asks for. An unrecognised value fails
    /// closed to [`HostKeyPolicy::Strict`] — never to `accept-new`.
    pub fn host_key_policy(&self) -> HostKeyPolicy {
        HostKeyPolicy::from_config(&self.strict_host_key_checking)
    }

    /// Replace an unrecognised policy with `strict` and return the offending
    /// value so the caller can warn. Recognised aliases (`yes`/`no`) are left
    /// alone: the value round-trips through serde untouched.
    fn sanitise_host_key_policy(&mut self) -> Option<String> {
        if HostKeyPolicy::parse(&self.strict_host_key_checking).is_some() {
            return None;
        }
        Some(std::mem::replace(
            &mut self.strict_host_key_checking,
            HostKeyPolicy::Strict.as_str().to_string(),
        ))
    }
}

/// Host-key verification policy (`transport.strict_host_key_checking`).
///
/// A single parse feeds both transports and the audit checks, so `yes`/`no`
/// (OpenSSH's own spelling) cannot mean different things on different paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// Refuse hosts that are unknown or whose key changed.
    Strict,
    /// Refuse a changed key; trust a first-seen host key and pin it (TOFU).
    AcceptNew,
    /// Accept any host key, including a changed one.
    Off,
}

impl HostKeyPolicy {
    /// Canonical config value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::AcceptNew => "accept-new",
            Self::Off => "off",
        }
    }

    /// Parse a configured value. Accepts the documented values plus the
    /// OpenSSH `yes`/`no` aliases, case-insensitively; `None` means the value
    /// is not a policy at all.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "strict" | "yes" => Some(Self::Strict),
            "accept-new" => Some(Self::AcceptNew),
            "off" | "no" => Some(Self::Off),
            _ => None,
        }
    }

    /// Parse a configured value, failing closed to [`Self::Strict`].
    pub fn from_config(value: &str) -> Self {
        Self::parse(value).unwrap_or(Self::Strict)
    }
}

/// bssh AI settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    /// Named agent policies; `default` is used when none is given.
    pub policies: std::collections::BTreeMap<String, crate::services::ai::types::Policy>,
    /// Record `run` output of agent calls as asciicast, like interactive sessions.
    pub record_output: bool,
    /// Approval requests with no answer expire as denied after this many seconds.
    pub approval_timeout_secs: u64,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            policies: Default::default(),
            record_output: true,
            approval_timeout_secs: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingConfig {
    /// Master switch.
    pub enabled: bool,
    /// Record only connections carrying one of these tags; empty = all connections.
    pub tags: Vec<String>,
    /// Stop writing events once a recording reaches this size.
    pub max_bytes_per_session: u64,
    /// Recordings older than this are deleted (file, recording_path and text index). 0 = keep forever.
    pub retention_days: u32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tags: Vec::new(),
            max_bytes_per_session: 50 * 1024 * 1024,
            retention_days: 90,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PredictionConfig {
    pub enabled: bool,
    pub capture_cwd: bool,
    pub window_days: u32,
}

impl Default for PredictionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            capture_cwd: true,
            window_days: 180,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(skip, default = "default_environment")]
    pub environment: String,
    pub database_path: PathBuf,
    pub default_user: String,
    pub default_bastion: Option<String>,
    pub default_bastion_user: Option<String>,
    pub default_port: u16,
    pub use_kerberos_by_default: bool,
    pub ssh_config_path: Option<PathBuf>,
    pub log_level: String,
    /// Set once the pre-2.10.3 default `log_level = "info"` has been
    /// migrated to `"warn"`. After that, `"info"` is an explicit choice.
    #[serde(default)]
    pub log_level_migrated: bool,
    pub auto_save_history: bool,
    pub max_history_size: usize,
    #[serde(default = "default_search_mode")]
    pub search_mode: String, // "bayesian" or "fuzzy"
    #[serde(default)]
    pub transport: TransportConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub recording: RecordingConfig,
    #[serde(default)]
    pub prediction: PredictionConfig,
    #[serde(default)]
    pub ai: AiConfig,
}

fn default_search_mode() -> String {
    "bayesian".to_string()
}

fn default_environment() -> String {
    "default".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::default_for_env("default")
    }
}

impl AppConfig {
    /// Validate an environment name before it is joined onto the
    /// `environments/` directory. Names come from `--env`, the
    /// `active_env` file, the desktop UI and imported backups, so anything
    /// that could escape the directory (`..`, separators, absolute paths)
    /// must be rejected.
    pub fn validate_env_name(name: &str) -> Result<()> {
        let valid = !name.is_empty()
            && name.len() <= MAX_ENV_NAME_LEN
            && !name.starts_with('.')
            && name.trim() == name
            && !name
                .chars()
                .any(|c| matches!(c, '/' | '\\' | ':') || c.is_control());
        if !valid {
            anyhow::bail!(
                "invalid environment name '{}': use up to {MAX_ENV_NAME_LEN} characters, \
                 no path separators, ':' or leading '.'",
                name.escape_debug()
            );
        }
        Ok(())
    }

    pub fn get_active_env() -> String {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");
        let env_file = config_dir.join("active_env");
        let env = std::fs::read_to_string(&env_file)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if env.is_empty() {
            return "default".to_string();
        }
        if let Err(e) = Self::validate_env_name(&env) {
            eprintln!("[warn] ignoring '{}': {e}", env_file.display());
            return "default".to_string();
        }
        env
    }

    pub fn set_active_env(env: &str) -> Result<()> {
        Self::validate_env_name(env)?;
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");
        std::fs::create_dir_all(&config_dir)?;
        enforce_secure_dir(&config_dir);
        let env_file = config_dir.join("active_env");
        atomic_write(&env_file, env)?;
        enforce_secure_file(&env_file);
        Ok(())
    }

    fn migrate_legacy_config(config_dir: &std::path::Path) -> Result<()> {
        let default_env_dir = config_dir.join("environments").join("default");
        if !default_env_dir.exists() {
            std::fs::create_dir_all(&default_env_dir)?;
            enforce_secure_dir(&default_env_dir);

            let legacy_config = config_dir.join("config.json");
            if legacy_config.exists() {
                std::fs::rename(&legacy_config, default_env_dir.join("config.json"))?;
            }

            let legacy_db = config_dir.join("history.db");
            if legacy_db.exists() {
                std::fs::rename(&legacy_db, default_env_dir.join("history.db"))?;
            }
        }
        Ok(())
    }

    pub fn load(env_override: Option<String>) -> Result<Self> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");

        std::fs::create_dir_all(&config_dir)?;
        enforce_secure_dir(&config_dir);
        Self::migrate_legacy_config(&config_dir)?;

        let environment = env_override.unwrap_or_else(Self::get_active_env);
        Self::validate_env_name(&environment)?;
        let env_dir = config_dir.join("environments").join(&environment);
        std::fs::create_dir_all(&env_dir)?;
        enforce_secure_dir(&env_dir);

        let config_file = env_dir.join("config.json");

        let mut config = if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)?;
            let mut cfg: AppConfig = serde_json::from_str(&content)
                .with_context(|| format!("parse config file '{}'", config_file.display()))?;
            cfg.environment = environment.clone();
            // `info` used to be the default, so a stored `info` is almost
            // always that default and not a choice. Quiet it once; later
            // `config --log-level info` sticks because the flag is set.
            if !cfg.log_level_migrated {
                if cfg.log_level.eq_ignore_ascii_case("info") {
                    cfg.log_level = "warn".to_string();
                }
                cfg.log_level_migrated = true;
                // Best effort: a config dir that can't be written must not break
                // every command; the migration simply runs again next time.
                let _ = cfg.save();
            }
            cfg
        } else {
            let cfg = Self::default_for_env(&environment);
            cfg.save()?;
            cfg
        };

        // A hand-edited policy the parser does not recognise must not weaken
        // verification: fall back to `strict` (not `accept-new`) and say so.
        // Best effort write, like the log-level migration: an unwritable
        // config dir must not break every command.
        if let Some(bad) = config.transport.sanitise_host_key_policy() {
            eprintln!(
                "[warn] unknown transport.strict_host_key_checking '{}'; \
                 using 'strict' (valid: strict, accept-new, off)",
                bad.escape_debug()
            );
            let _ = config.save();
        }

        // Ensure database path is absolute and uses the environment dir.
        // `starts_with` is purely lexical, so also reject `..` components
        // (`<env_dir>/../../elsewhere.db` would otherwise pass).
        if config.database_path.is_relative()
            || !config.database_path.starts_with(&env_dir)
            || config
                .database_path
                .components()
                .any(|c| c == Component::ParentDir)
        {
            config.database_path = env_dir.join("history.db");
            config.save()?;
        }

        Ok(config)
    }

    pub fn default_for_env(env: &str) -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");
        let env_dir = config_dir.join("environments").join(env);

        Self {
            environment: env.to_string(),
            database_path: env_dir.join("history.db"),
            default_user: whoami::username(),
            default_bastion: None,
            default_bastion_user: None,
            default_port: 22,
            use_kerberos_by_default: false,
            ssh_config_path: dirs::home_dir().map(|h| h.join(".ssh/config")),
            log_level: "warn".to_string(),
            log_level_migrated: true,
            auto_save_history: true,
            max_history_size: 1000,
            search_mode: "bayesian".to_string(),
            transport: TransportConfig::default(),
            auth: AuthConfig::default(),
            recording: RecordingConfig::default(),
            prediction: PredictionConfig::default(),
            ai: AiConfig::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        Self::validate_env_name(&self.environment)?;
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");

        let env_dir = config_dir.join("environments").join(&self.environment);
        std::fs::create_dir_all(&env_dir)?;
        enforce_secure_dir(&config_dir);
        enforce_secure_dir(&env_dir);

        let config_file = env_dir.join("config.json");
        let content = serde_json::to_string_pretty(self)?;

        // Atomic write: write to a temp file in the same directory, then
        // rename into place. A crash mid-write can no longer corrupt the
        // config (previously `fs::write` truncated in place).
        atomic_write(&config_file, &content)?;
        enforce_secure_file(&config_file);

        Ok(())
    }

    pub fn update(&mut self, updates: AppConfigUpdates) -> Result<()> {
        if let Some(user) = updates.default_user {
            self.default_user = user;
        }
        if let Some(bastion) = updates.default_bastion {
            self.default_bastion = bastion;
        }
        if let Some(bastion_user) = updates.default_bastion_user {
            self.default_bastion_user = bastion_user;
        }
        if let Some(port) = updates.default_port {
            self.default_port = port;
        }
        if let Some(use_kerberos) = updates.use_kerberos_by_default {
            self.use_kerberos_by_default = use_kerberos;
        }
        if let Some(log_level) = updates.log_level {
            self.log_level = log_level;
        }
        if let Some(auto_save) = updates.auto_save_history {
            self.auto_save_history = auto_save;
        }
        if let Some(max_size) = updates.max_history_size {
            self.max_history_size = max_size;
        }
        if let Some(search_mode) = updates.search_mode {
            if search_mode == "bayesian" || search_mode == "fuzzy" {
                self.search_mode = search_mode;
            }
        }
        if let Some(enabled) = updates.recording_enabled {
            self.recording.enabled = enabled;
        }
        if let Some(tags) = updates.recording_tags {
            self.recording.tags = tags;
        }
        if let Some(prediction_enabled) = updates.prediction_enabled {
            self.prediction.enabled = prediction_enabled;
        }
        if let Some(capture_cwd) = updates.capture_cwd {
            self.prediction.capture_cwd = capture_cwd;
        }

        if let Some(policy) = updates.strict_host_key_checking {
            self.transport.strict_host_key_checking = policy.as_str().to_string();
        }

        self.save()
    }
}

#[derive(Debug, Clone)]
pub struct AppConfigUpdates {
    pub default_user: Option<String>,
    pub default_bastion: Option<Option<String>>,
    pub default_bastion_user: Option<Option<String>>,
    pub default_port: Option<u16>,
    pub use_kerberos_by_default: Option<bool>,
    pub log_level: Option<String>,
    pub auto_save_history: Option<bool>,
    pub max_history_size: Option<usize>,
    pub search_mode: Option<String>,
    /// Host-key policy to store; the caller must have parsed the value, so an
    /// unknown setting is rejected before it can reach the config file.
    pub strict_host_key_checking: Option<HostKeyPolicy>,
    pub recording_enabled: Option<bool>,
    pub recording_tags: Option<Vec<String>>,
    pub prediction_enabled: Option<bool>,
    pub capture_cwd: Option<bool>,
}

pub fn enforce_secure_dir(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)) {
            eprintln!("[warn] failed to chmod 700 '{}': {e}", path.display());
        }
    }
    let _ = path;
}

pub fn enforce_secure_file(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!("[warn] failed to chmod 600 '{}': {e}", path.display());
        }
    }
    let _ = path;
}

/// Atomically write `content` to `path` via a temp file + rename.
///
/// Unlike `fs::write` (which truncates the destination in place), this
/// guarantees the destination is either fully replaced or untouched, even
/// if the process is killed mid-write. Uses `tempfile::NamedTempFile` so
/// the temp file is world-unique, created 0600 and cleaned up automatically.
pub fn atomic_write(path: &std::path::Path, content: &str) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));

    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| anyhow::anyhow!("create temp config file: {e}"))?;
    tmp.write_all(content.as_bytes())
        .map_err(|e| anyhow::anyhow!("write temp config: {e}"))?;
    tmp.flush()
        .map_err(|e| anyhow::anyhow!("flush temp config: {e}"))?;
    tmp.as_file().sync_all().ok();

    tmp.persist(path)
        .map_err(|e| anyhow::anyhow!("rename temp config into place: {}", e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, HostKeyPolicy, TransportConfig};

    #[test]
    fn env_names_accept_ordinary_names() {
        for name in ["default", "prod", "staging-eu_1", "client.acme", "my env"] {
            assert!(AppConfig::validate_env_name(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn env_names_reject_path_traversal() {
        for name in [
            "", ".", "..", "../x", "a/b", "a\\b", "/etc", "C:", ".hidden", " x", "a\0b", "a\nb",
        ] {
            assert!(AppConfig::validate_env_name(name).is_err(), "{name:?}");
        }
        assert!(AppConfig::validate_env_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn default_log_level_is_warn() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.log_level, "warn");
        assert!(cfg.log_level_migrated);
    }

    #[test]
    fn legacy_config_without_flag_is_not_marked_migrated() {
        let mut v = serde_json::to_value(AppConfig::default()).unwrap();
        v.as_object_mut().unwrap().remove("log_level_migrated");
        let cfg: AppConfig = serde_json::from_value(v).unwrap();
        assert!(!cfg.log_level_migrated);
    }

    #[test]
    fn host_key_policy_parses_documented_values_and_ssh_aliases() {
        for (value, expected) in [
            ("strict", HostKeyPolicy::Strict),
            ("STRICT", HostKeyPolicy::Strict),
            ("Yes", HostKeyPolicy::Strict),
            ("accept-new", HostKeyPolicy::AcceptNew),
            ("Accept-New", HostKeyPolicy::AcceptNew),
            ("off", HostKeyPolicy::Off),
            ("NO", HostKeyPolicy::Off),
        ] {
            assert_eq!(HostKeyPolicy::parse(value), Some(expected), "{value}");
        }
        for value in ["", " accept-new", "accept_new", "ask", "maybe", "0"] {
            assert_eq!(HostKeyPolicy::parse(value), None, "{value:?}");
        }
    }

    #[test]
    fn canonical_host_key_policy_values_round_trip() {
        for policy in [
            HostKeyPolicy::Strict,
            HostKeyPolicy::AcceptNew,
            HostKeyPolicy::Off,
        ] {
            assert_eq!(HostKeyPolicy::parse(policy.as_str()), Some(policy));
        }
    }

    #[test]
    fn unknown_host_key_policy_fails_closed_to_strict() {
        assert_eq!(HostKeyPolicy::from_config("bogus"), HostKeyPolicy::Strict);
        // The *default* is accept-new; a typo must not become the default.
        assert_eq!(
            HostKeyPolicy::from_config(HostKeyPolicy::AcceptNew.as_str()),
            HostKeyPolicy::AcceptNew
        );
    }

    #[test]
    fn unknown_policy_is_replaced_by_strict_on_load() {
        let mut transport = TransportConfig {
            force_subprocess: false,
            strict_host_key_checking: "strict-ish".into(),
        };
        assert_eq!(
            transport.sanitise_host_key_policy().as_deref(),
            Some("strict-ish")
        );
        assert_eq!(transport.strict_host_key_checking, "strict");
        assert_eq!(transport.host_key_policy(), HostKeyPolicy::Strict);
    }

    #[test]
    fn recognised_aliases_are_kept_untouched() {
        let mut transport = TransportConfig {
            force_subprocess: false,
            strict_host_key_checking: "yes".into(),
        };
        assert_eq!(transport.sanitise_host_key_policy(), None);
        assert_eq!(transport.strict_host_key_checking, "yes");
        assert_eq!(transport.host_key_policy(), HostKeyPolicy::Strict);
    }
}
