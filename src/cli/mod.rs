use crate::config::AppConfig;
use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[allow(unused_imports)]
use commands::*;

#[derive(Parser)]
#[command(name = "bayesian-ssh")]
#[command(about = "A fast and lightweight SSH session manager with Kerberos support")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Connect to a server
    Connect {
        /// Server name or hostname
        target: String,
        /// SSH user
        #[arg(short, long)]
        user: Option<String>,
        /// SSH port
        #[arg(short, long, default_value = "22")]
        port: Option<u16>,
        /// Use Kerberos authentication
        #[arg(short, long)]
        kerberos: Option<bool>,
        /// Bastion host
        #[arg(short, long)]
        bastion: Option<String>,
        /// Disable bastion (force direct connection)
        #[arg(long)]
        no_bastion: bool,
        /// Bastion user
        #[arg(short, long)]
        bastion_user: Option<String>,
        /// SSH key path
        #[arg(short, long)]
        key: Option<String>,
    },

    /// Add a new connection to history
    Add {
        /// Connection name
        name: String,
        /// Server hostname
        host: String,
        /// SSH user
        #[arg(short, long)]
        user: Option<String>,
        /// SSH port
        #[arg(short, long, default_value = "22")]
        port: Option<u16>,
        /// Use Kerberos authentication
        #[arg(short, long)]
        kerberos: Option<bool>,
        /// Bastion host
        #[arg(short, long)]
        bastion: Option<String>,
        /// Disable bastion (force direct connection)
        #[arg(long)]
        no_bastion: bool,
        /// Bastion user
        #[arg(short, long)]
        bastion_user: Option<String>,
        /// SSH key path
        #[arg(short, long)]
        key: Option<String>,
        /// Tags for organization
        #[arg(short, long)]
        tags: Vec<String>,
    },

    /// List all connections
    List {
        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,
        /// Show only recently used
        #[arg(short, long)]
        recent: bool,
        /// Show connection details
        #[arg(short, long)]
        detailed: bool,
    },

    /// Remove a connection
    Remove {
        /// Connection name or ID
        target: String,
    },

    /// Show connection details
    Show {
        /// Connection name or ID
        target: String,
    },

    /// Edit connection settings
    Edit {
        /// Connection name or ID
        target: String,
        /// New connection name
        #[arg(long)]
        name: Option<String>,
        /// New host
        #[arg(long)]
        host: Option<String>,
        /// New user
        #[arg(long)]
        user: Option<String>,
        /// New port
        #[arg(long)]
        port: Option<u16>,
        /// Toggle Kerberos
        #[arg(long)]
        kerberos: Option<bool>,
        /// New bastion
        #[arg(long)]
        bastion: Option<String>,
        /// Disable bastion (force direct connection)
        #[arg(long)]
        no_bastion: bool,
        /// New bastion user
        #[arg(long)]
        bastion_user: Option<String>,
        /// New SSH key
        #[arg(long)]
        key: Option<String>,
        /// Add tags
        #[arg(long)]
        add_tags: Vec<String>,
        /// Remove tags
        #[arg(long)]
        remove_tags: Vec<String>,
    },

    /// Configure application settings
    Config {
        /// Default user
        #[arg(long)]
        default_user: Option<String>,
        /// Default bastion
        #[arg(long)]
        default_bastion: Option<String>,
        /// Default bastion user
        #[arg(long)]
        default_bastion_user: Option<String>,
        /// Default port
        #[arg(long)]
        default_port: Option<u16>,
        /// Use Kerberos by default
        #[arg(long)]
        use_kerberos: Option<bool>,
        /// Log level
        #[arg(long)]
        log_level: Option<String>,
    },

    /// Show application statistics
    Stats,

    /// Import connections from SSH config
    Import {
        /// SSH config file path
        #[arg(short, long)]
        file: Option<String>,
        /// Force direct connections (no bastion) for imported hosts
        #[arg(long)]
        no_bastion: bool,
    },

    /// Generate shell completion script
    Completions {
        /// Shell type
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

impl Cli {
    pub async fn execute(self, config: AppConfig) -> Result<()> {
        match self.command {
            Commands::Connect {
                target,
                user,
                port,
                kerberos,
                bastion,
                no_bastion,
                bastion_user,
                key,
            } => {
                commands::connect::execute(
                    target,
                    user,
                    port,
                    kerberos,
                    bastion,
                    no_bastion,
                    bastion_user,
                    key,
                    config,
                )
                .await
            }
            Commands::Add {
                name,
                host,
                user,
                port,
                kerberos,
                bastion,
                no_bastion,
                bastion_user,
                key,
                tags,
            } => {
                commands::add::execute(
                    name,
                    host,
                    user,
                    port,
                    kerberos,
                    bastion,
                    no_bastion,
                    bastion_user,
                    key,
                    tags,
                    config,
                )
                .await
            }
            Commands::List {
                tag,
                recent,
                detailed,
            } => commands::list::execute(tag, recent, detailed, config).await,
            Commands::Remove { target } => commands::remove::execute(target, config).await,
            Commands::Show { target } => commands::show::execute(target, config).await,
            Commands::Edit {
                target,
                name,
                host,
                user,
                port,
                kerberos,
                bastion,
                no_bastion,
                bastion_user,
                key,
                add_tags,
                remove_tags,
            } => {
                commands::edit::execute(
                    target,
                    name,
                    host,
                    user,
                    port,
                    kerberos,
                    bastion,
                    no_bastion,
                    bastion_user,
                    key,
                    add_tags,
                    remove_tags,
                    config,
                )
                .await
            }
            Commands::Config {
                default_user,
                default_bastion,
                default_bastion_user,
                default_port,
                use_kerberos,
                log_level,
            } => {
                commands::config::execute(
                    default_user,
                    default_bastion,
                    default_bastion_user,
                    default_port,
                    use_kerberos,
                    log_level,
                    config,
                )
                .await
            }
            Commands::Stats => commands::stats::execute(config).await,
            Commands::Import { file, no_bastion } => {
                commands::import::execute(file, no_bastion, config).await
            }
            Commands::Completions { shell } => commands::completions::execute(shell, config).await,
        }
    }
}
