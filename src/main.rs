use clap::Parser;
use tracing::{error, info};

mod cli;
mod config;
mod database;
mod models;
mod services;
mod tui;

use cli::{Cli, Commands};
use config::AppConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command line arguments first
    let cli = Cli::parse();

    // Check if this is a completions command (don't log for completions)
    let is_completions = matches!(&cli.command, Commands::Completions { .. });

    if !is_completions {
        // Initialize logging only for non-completion commands
        tracing_subscriber::fmt::init();
        info!("Starting Bayesian SSH...");
    }

    // Load configuration
    let config = AppConfig::load()?;

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
