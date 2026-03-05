use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub auto_save_history: bool,
    pub max_history_size: usize,
    #[serde(default = "default_search_mode")]
    pub search_mode: String, // "bayesian" or "fuzzy"
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
    pub fn get_active_env() -> String {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");
        let env_file = config_dir.join("active_env");
        if env_file.exists() {
            std::fs::read_to_string(&env_file)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "default".to_string())
        } else {
            "default".to_string()
        }
    }

    pub fn set_active_env(env: &str) -> Result<()> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");
        std::fs::create_dir_all(&config_dir)?;
        let env_file = config_dir.join("active_env");
        std::fs::write(env_file, env)?;
        Ok(())
    }

    fn migrate_legacy_config(config_dir: &std::path::Path) -> Result<()> {
        let default_env_dir = config_dir.join("environments").join("default");
        if !default_env_dir.exists() {
            std::fs::create_dir_all(&default_env_dir)?;
            
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
        Self::migrate_legacy_config(&config_dir)?;

        let environment = env_override.unwrap_or_else(Self::get_active_env);
        let env_dir = config_dir.join("environments").join(&environment);
        std::fs::create_dir_all(&env_dir)?;

        let config_file = env_dir.join("config.json");

        let mut config = if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)?;
            let mut cfg: AppConfig = serde_json::from_str(&content)?;
            cfg.environment = environment.clone();
            cfg
        } else {
            let cfg = Self::default_for_env(&environment);
            cfg.save()?;
            cfg
        };

        // Ensure database path is absolute and uses the environment dir
        if config.database_path.is_relative() || !config.database_path.starts_with(&env_dir) {
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
            log_level: "info".to_string(),
            auto_save_history: true,
            max_history_size: 1000,
            search_mode: "bayesian".to_string(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");

        let env_dir = config_dir.join("environments").join(&self.environment);
        std::fs::create_dir_all(&env_dir)?;

        let config_file = env_dir.join("config.json");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(config_file, content)?;

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
}
