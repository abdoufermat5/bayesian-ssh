use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::filter::LevelFilter;

use bayesian_ssh::cli::{Cli, Commands};
use bayesian_ssh::config::AppConfig;
use bayesian_ssh::errors;

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
    let config = AppConfig::load(cli.env.clone())?;

    let env_prefix = format!("[{}] ", config.environment);

    if !is_completions {
        // Initialize logging with the configured log level and environment prefix
        let log_level = parse_log_level(&config.log_level);

        // Use a custom format block to prepend the [environment] tag to logs
        let format = tracing_subscriber::fmt::format()
            .with_target(false)
            .compact();

        // Logs go to stderr so stdout stays clean for data output
        // (`export` to stdout, `exec`, `completions`, piping in scripts).
        tracing_subscriber::fmt()
            .event_format(format)
            .with_max_level(log_level)
            .with_writer(std::io::stderr)
            .init();

        if log_level >= LevelFilter::INFO {
            info!("{}Starting Bayesian SSH...", env_prefix);
        }
    }

    // Execute CLI command
    if let Err(e) = cli.execute(config).await {
        errors::report_cli_error(&e);
        if !is_completions {
            error!("Error executing command: {}", e);
        }
        std::process::exit(1);
    }

    if !is_completions {
        info!("{}Bayesian SSH completed successfully", env_prefix);
    }
    Ok(())
}
