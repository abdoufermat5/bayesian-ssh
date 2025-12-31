use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::filter::LevelFilter;

mod cli;
mod config;
mod database;
mod models;
mod services;
mod tui;

use cli::{Cli, Commands};
use config::AppConfig;

/// Convert log level string to tracing LevelFilter
fn parse_log_level(level: &str) -> LevelFilter {
    match level.to_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "info" => LevelFilter::INFO,
        "warn" | "warning" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        "off" | "none" => LevelFilter::OFF,
        _ => LevelFilter::INFO, // Default to INFO for invalid values
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command line arguments first
    let cli = Cli::parse();

    // Check if this is a completions command (don't log for completions)
    let is_completions = matches!(&cli.command, Commands::Completions { .. });

    // Load configuration first (before initializing logging)
    let config = AppConfig::load()?;

    if !is_completions {
        // Initialize logging with the configured log level
        let log_level = parse_log_level(&config.log_level);
        tracing_subscriber::fmt()
            .with_max_level(log_level)
            .init();

        if log_level >= LevelFilter::INFO {
            info!("Starting Bayesian SSH...");
        }
    }

    // Execute CLI command
    if let Err(e) = cli.execute(config).await {
        if !is_completions {
            error!("Error executing command: {}", e);
        }
        std::process::exit(1);
    }

    if !is_completions {
        info!("Bayesian SSH completed successfully");
    }
    Ok(())
}
