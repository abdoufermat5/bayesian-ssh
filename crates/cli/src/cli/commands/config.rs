use crate::config::{AppConfig, AppConfigUpdates, HostKeyPolicy};
use anyhow::Result;
use tracing::info;

/// The `bssh config` flags; `None` leaves a setting untouched.
pub struct ConfigArgs {
    pub default_user: Option<String>,
    pub default_bastion: Option<String>,
    pub default_bastion_user: Option<String>,
    pub default_port: Option<u16>,
    pub use_kerberos: Option<bool>,
    pub log_level: Option<String>,
    /// Clear both the default bastion and its user.
    pub clear_bastion: bool,
    pub search_mode: Option<String>,
    pub strict_host_key_checking: Option<String>,
    pub recording: Option<String>,
    pub recording_tags: Option<String>,
    pub prediction: Option<String>,
    pub capture_cwd: Option<String>,
}

pub async fn execute(args: ConfigArgs, mut config: AppConfig) -> Result<()> {
    let ConfigArgs {
        default_user,
        default_bastion,
        default_bastion_user,
        default_port,
        use_kerberos,
        log_level,
        clear_bastion,
        search_mode,
        strict_host_key_checking,
        recording,
        recording_tags,
        prediction,
        capture_cwd,
    } = args;
    info!("Updating application configuration");

    // Reject an unknown policy here: storing it would leave the transports to
    // guess, which is how an unrecognised value once silently meant accept-new.
    let strict_host_key_checking = strict_host_key_checking
        .map(|value| {
            HostKeyPolicy::parse(&value).ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid --strict-host-key-checking '{}': use 'strict', 'accept-new' or 'off'",
                    value.escape_debug()
                )
            })
        })
        .transpose()?;

    // Handle bastion settings:
    // - If --clear-bastion is passed, set to Some(None) to clear
    // - If a value is provided, set to Some(Some(value))
    // - Otherwise, set to None (don't change)
    let bastion_update = if clear_bastion {
        Some(None)
    } else {
        default_bastion.map(Some)
    };

    let bastion_user_update = if clear_bastion {
        Some(None)
    } else {
        default_bastion_user.map(Some)
    };

    let recording_enabled = recording.as_deref().map(|s| s == "on");
    let recording_tags = recording_tags.map(|s| {
        s.split(',')
            .filter_map(crate::models::Connection::normalize_tag)
            .collect()
    });

    let updates = AppConfigUpdates {
        default_user,
        default_bastion: bastion_update,
        default_bastion_user: bastion_user_update,
        default_port,
        use_kerberos_by_default: use_kerberos,
        log_level,
        auto_save_history: None,
        max_history_size: None,
        search_mode,
        strict_host_key_checking,
        recording_enabled,
        recording_tags,
        prediction_enabled: prediction.as_deref().map(|v| v == "on"),
        capture_cwd: capture_cwd.as_deref().map(|v| v == "on"),
    };

    config.update(updates)?;

    println!("✅ Configuration updated successfully!");
    println!("\nCurrent settings:");
    println!("  Default user: {}", config.default_user);
    println!(
        "  Default bastion: {}",
        config.default_bastion.as_deref().unwrap_or("None")
    );
    println!(
        "  Default bastion user: {}",
        config.default_bastion_user.as_deref().unwrap_or("None")
    );
    println!("  Default port: {}", config.default_port);
    println!(
        "  Use Kerberos by default: {}",
        if config.use_kerberos_by_default {
            "Yes"
        } else {
            "No"
        }
    );
    println!("  Log level: {}", config.log_level);
    println!(
        "  Strict host key checking: {}",
        config.transport.strict_host_key_checking
    );
    println!(
        "  Search mode: {} {}",
        config.search_mode,
        if config.search_mode == "bayesian" {
            "(smart ranking with frequency/recency)"
        } else {
            "(simple pattern matching)"
        }
    );
    println!(
        "  Session recording: {} (tags: {}, retention: {}d)",
        if config.recording.enabled {
            "on"
        } else {
            "off"
        },
        if config.recording.tags.is_empty() {
            "all".to_string()
        } else {
            config.recording.tags.join(", ")
        },
        config.recording.retention_days
    );
    println!(
        "  Prediction: {}",
        if config.prediction.enabled {
            "on"
        } else {
            "off"
        }
    );
    println!(
        "  Capture cwd: {}",
        if config.prediction.capture_cwd {
            "on"
        } else {
            "off"
        }
    );

    Ok(())
}
