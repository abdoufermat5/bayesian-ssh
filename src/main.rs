use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::filter::LevelFilter;

mod cli;
mod config;
mod database;
pub mod errors;
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

    // The TUI command runs an alternate-screen terminal UI. Any log output written
    // to stderr would corrupt the display, so we redirect tracing to a log file
    // instead when the tui command is active.
    let is_tui = matches!(&cli.command, Commands::Tui);

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

        if is_tui {
            // Route all log output to a file so the TUI display is never corrupted.
            let log_dir = dirs::data_local_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                .join("bayesian-ssh");
            let _ = std::fs::create_dir_all(&log_dir);
            let log_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_dir.join("tui.log"))
                .unwrap_or_else(|_| {
                    // Fall back to /dev/null if we can't open the log file
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open("/dev/null")
                        .expect("/dev/null always exists")
                });
            tracing_subscriber::fmt()
                .event_format(format)
                .with_max_level(log_level)
                .with_writer(std::sync::Mutex::new(log_file))
                .init();
        } else {
            tracing_subscriber::fmt()
                .event_format(format)
                .with_max_level(log_level)
                .init();

            if log_level >= LevelFilter::INFO {
                info!("{}Starting Bayesian SSH...", env_prefix);
            }
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
        info!("{}Bayesian SSH completed successfully", env_prefix);
    }
    Ok(())
}
