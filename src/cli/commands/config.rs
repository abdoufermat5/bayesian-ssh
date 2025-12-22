use crate::config::{AppConfig, AppConfigUpdates};
use anyhow::Result;
use tracing::info;

pub async fn execute(
    default_user: Option<String>,
    default_bastion: Option<String>,
    default_bastion_user: Option<String>,
    default_port: Option<u16>,
    use_kerberos: Option<bool>,
    log_level: Option<String>,
    clear_bastion: bool,
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

    let updates = AppConfigUpdates {
        default_user,
        default_bastion: bastion_update,
        default_bastion_user: bastion_user_update,
        default_port,
        use_kerberos_by_default: use_kerberos,
        log_level,
        auto_save_history: None,
        max_history_size: None,
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

    Ok(())
}
