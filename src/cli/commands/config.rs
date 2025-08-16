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
    mut config: AppConfig,
) -> Result<()> {
    info!("Updating application configuration");

    let updates = AppConfigUpdates {
        default_user,
        default_bastion: Some(default_bastion),
        default_bastion_user: Some(default_bastion_user),
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
