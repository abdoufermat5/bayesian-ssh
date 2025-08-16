use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
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
}

impl Default for AppConfig {
    fn default() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");

        Self {
            database_path: config_dir.join("history.db"),
            default_user: "admin".to_string(),
            default_bastion: None,
            default_bastion_user: None,
            default_port: 22,
            use_kerberos_by_default: true,
            ssh_config_path: dirs::home_dir().map(|h| h.join(".ssh/config")),
            log_level: "info".to_string(),
            auto_save_history: true,
            max_history_size: 1000,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");

        // Create config directory if it doesn't exist
        std::fs::create_dir_all(&config_dir)?;

        let config_file = config_dir.join("config.json");

        if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)?;
            let mut config: AppConfig = serde_json::from_str(&content)?;

            // Ensure database path is absolute
            if config.database_path.is_relative() {
                config.database_path = config_dir.join(&config.database_path);
            }

            Ok(config)
        } else {
            let config = AppConfig::default();
            config.save()?;
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("bayesian-ssh");

        std::fs::create_dir_all(&config_dir)?;

        let config_file = config_dir.join("config.json");
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
}
