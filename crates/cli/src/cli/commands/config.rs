use crate::config::{AppConfig, AppConfigUpdates};
use anyhow::Result;
use tracing::info;

#[allow(clippy::too_many_arguments)]
pub async fn execute(
    default_user: Option<String>,
    default_bastion: Option<String>,
    default_bastion_user: Option<String>,
    default_port: Option<u16>,
    use_kerberos: Option<bool>,
    log_level: Option<String>,
    clear_bastion: bool,
    search_mode: Option<String>,
    recording: Option<String>,
    recording_tags: Option<String>,
    prediction: Option<String>,
    capture_cwd: Option<String>,
    mut config: AppConfig,
) -> Result<()> {
    info!("Updating application configuration");

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
