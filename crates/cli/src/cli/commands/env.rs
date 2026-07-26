use crate::cli::EnvCommands;
use crate::config::AppConfig;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::info;

pub async fn execute(command: EnvCommands) -> Result<()> {
    match command {
        EnvCommands::List => list_environments()?,
        EnvCommands::Use { name } => use_environment(&name)?,
        EnvCommands::Create { name } => create_environment(&name)?,
        EnvCommands::Remove { name } => remove_environment(&name)?,
    }
    Ok(())
}

fn get_environments_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("bayesian-ssh")
        .join("environments")
}

fn list_environments() -> Result<()> {
    let envs_dir = get_environments_dir();
    if !envs_dir.exists() {
        println!("No environments found.");
        return Ok(());
    }

    let active_env = AppConfig::get_active_env();
    let mut envs = Vec::new();

    for entry in std::fs::read_dir(&envs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                envs.push(name.to_string());
            }
        }
    }

    envs.sort();

    println!("Environments:");
    for env in envs {
        if env == active_env {
            println!("  * {} (active)", env);
        } else {
            println!("    {}", env);
        }
    }

    Ok(())
}

fn use_environment(name: &str) -> Result<()> {
    let env_dir = get_environments_dir().join(name);
    if !env_dir.exists() {
        anyhow::bail!(
            "Environment '{}' does not exist. Use `bayesian-ssh env create {}` to create it.",
            name,
            name
        );
    }

    AppConfig::set_active_env(name)?;
    println!("✅ Now using environment '{}'", name);
    Ok(())
}

fn create_environment(name: &str) -> Result<()> {
    let env_dir = get_environments_dir().join(name);
    if env_dir.exists() {
        anyhow::bail!("Environment '{}' already exists.", name);
    }

    std::fs::create_dir_all(&env_dir)?;

    // Create default config for this environment
    let config = AppConfig::default_for_env(name);
    config.save()?;

    println!("✅ Created new environment '{}'", name);
    println!("To start using it, run: bayesian-ssh env use {}", name);
    Ok(())
}

fn remove_environment(name: &str) -> Result<()> {
    let active_env = AppConfig::get_active_env();
    if name == active_env {
        anyhow::bail!(
            "Cannot remove the currently active environment '{}'. Change environment first.",
            name
        );
    }

    if name == "default" {
        anyhow::bail!("Cannot remove the built-in 'default' environment.");
    }

    let env_dir = get_environments_dir().join(name);
    if !env_dir.exists() {
        anyhow::bail!("Environment '{}' does not exist.", name);
    }

    std::fs::remove_dir_all(&env_dir).context("Failed to securely delete environment directory")?;

    println!("🗑️ Removed environment '{}'", name);
    info!("Removed environment {}", name);
    Ok(())
}
