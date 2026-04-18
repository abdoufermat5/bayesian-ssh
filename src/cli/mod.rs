use crate::config::AppConfig;
use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
pub mod utils;

#[allow(unused_imports)]
use commands::*;

#[derive(Parser)]
#[command(name = "bayesian-ssh")]
#[command(about = "A fast and lightweight SSH session manager with Kerberos support")]
#[command(version)]
pub struct Cli {
    /// Override the active environment for this command
    #[arg(long, global = true)]
    pub env: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum EnvCommands {
    /// List all environments
    List,
    /// Set the active environment
    Use {
        /// Name of the environment
        name: String,
    },
    /// Create a new environment
    Create {
        /// Name of the new environment
        name: String,
    },
    /// Remove an environment
    Remove {
        /// Name of the environment to remove
        name: String,
    },
}

#[derive(Subcommand)]
pub enum Commands {
    /// Connect to a server
    Connect {
        /// Server name or hostname
        target: String,
        /// SSH user
        #[arg(short = 'i', long)]
        user: Option<String>,
        /// SSH port
        #[arg(short = 'i', long, default_value = "22")]
        port: Option<u16>,
        /// Use Kerberos authentication
        #[arg(short = 'i', long)]
        kerberos: Option<bool>,
        /// Bastion host
        #[arg(short = 'i', long)]
        bastion: Option<String>,
        /// Disable bastion (force direct connection)
        #[arg(long)]
        no_bastion: bool,
        /// Bastion user
        #[arg(short = 'i', long)]
        bastion_user: Option<String>,
        /// SSH key path
        #[arg(short = 'i', long)]
        key: Option<String>,
    },

    /// Add a new connection to history
    Add {
        /// Connection name
        name: String,
        /// Server hostname
        host: String,
        /// SSH user
        #[arg(short = 'i', long)]
        user: Option<String>,
        /// SSH port
        #[arg(short = 'i', long, default_value = "22")]
        port: Option<u16>,
        /// Use Kerberos authentication
        #[arg(short = 'i', long)]
        kerberos: Option<bool>,
        /// Bastion host
        #[arg(short = 'i', long)]
        bastion: Option<String>,
        /// Disable bastion (force direct connection)
        #[arg(long)]
        no_bastion: bool,
        /// Bastion user
        #[arg(short = 'i', long)]
        bastion_user: Option<String>,
        /// SSH key path
        #[arg(short = 'i', long)]
        key: Option<String>,
        /// Tags for organization
        #[arg(short = 'i', long)]
        tags: Vec<String>,
    },

    /// List all connections
    List {
        /// Filter by tag
        #[arg(short = 'i', long)]
        tag: Option<String>,
        /// Show only recently used
        #[arg(short = 'i', long)]
        recent: bool,
        /// Show connection details
        #[arg(short = 'i', long)]
        detailed: bool,
    },

    /// Remove a connection
    Remove {
        /// Connection name or ID
        target: String,
        /// Skip confirmation prompt
        #[arg(short = 'i', long)]
        force: bool,
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
        /// Clear default bastion settings
        #[arg(long)]
        clear_bastion: bool,
        /// Search mode: "bayesian" (smart ranking) or "fuzzy" (simple matching)
        #[arg(long, value_parser = ["bayesian", "fuzzy"])]
        search_mode: Option<String>,
    },

    /// Show application statistics
    Stats,

    /// Export connections to file or stdout
    Export {
        /// Output format (json, toml, ssh-config)
        #[arg(long)]
        format: Option<String>,
        /// Output file path
        #[arg(short = 'i', long)]
        output: Option<String>,
        /// Filter by tag
        #[arg(short = 'i', long)]
        tag: Option<String>,
    },

    /// Backup the database
    Backup {
        /// Output file path (defaults to a timestamped file in backups dir)
        #[arg(short = 'i', long)]
        output: Option<String>,
    },

    /// Restore the database from a backup
    Restore {
        /// Backup file path
        file: String,
        /// Skip confirmation prompt
        #[arg(short = 'i', long)]
        force: bool,
    },

    /// Duplicate an existing connection
    Duplicate {
        /// Name of the connection to duplicate
        source: String,
        /// New name for the duplicated connection
        new_name: String,
    },

    /// Test SSH connectivity to a connection
    Ping {
        /// Target connection name or alias
        target: String,
        /// Connect timeout in seconds (default 5)
        #[arg(short = 'i', long)]
        timeout: Option<u64>,
    },

    /// Manage connection groups (tags)
    Groups {
        /// Name of the group to list connections for
        group_name: Option<String>,
    },

    /// Import connections from SSH config
    Import {
        /// SSH config file path
        #[arg(short = 'i', long)]
        file: Option<String>,
        /// Force direct connections (no bastion) for imported hosts
        #[arg(long)]
        no_bastion: bool,
    },

    /// Manage multi-environment profiles
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },

    /// Generate shell completion script
    Completions {
        /// Shell type
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Show session history
    History {
        /// Filter by connection name
        #[arg(short = 'i', long)]
        connection: Option<String>,
        /// Maximum number of entries to show
        #[arg(short = 'i', long, default_value = "20")]
        limit: usize,
        /// Show only sessions from the last N days
        #[arg(short = 'i', long)]
        days: Option<u32>,
        /// Show only failed sessions
        #[arg(short = 'i', long)]
        failed: bool,
    },

    /// Launch interactive TUI mode
    #[command(alias = "ui")]
    Tui,

    /// Execute a command on a remote host and print the output
    #[command(alias = "run")]
    Exec {
        /// Connection name, hostname, or alias
        target: String,
        /// Command and its arguments (pass after --)
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },

    /// Upload a local file to a remote host via SFTP
    Upload {
        /// Connection name, hostname, or alias
        target: String,
        /// Local file path
        #[arg(value_name = "LOCAL")]
        local: std::path::PathBuf,
        /// Remote destination path
        #[arg(value_name = "REMOTE")]
        remote: String,
        /// Resume an interrupted upload at this byte offset
        #[arg(long, default_value = "0")]
        offset: u64,
        /// Unix file permission mode for the remote file (octal)
        #[arg(long, default_value = "0o644", value_parser = parse_octal)]
        mode: u32,
    },

    /// Download a remote file from a host via SFTP
    Download {
        /// Connection name, hostname, or alias
        target: String,
        /// Remote file path
        #[arg(value_name = "REMOTE")]
        remote: String,
        /// Local destination path
        #[arg(value_name = "LOCAL")]
        local: std::path::PathBuf,
    },

    /// Open a local TCP port-forward tunnel through an SSH connection
    #[command(alias = "tunnel")]
    Forward {
        /// Connection name, hostname, or alias
        target: String,
        /// Local forward spec: [bind_addr:]bind_port:remote_host:remote_port
        #[arg(short = 'L', value_name = "SPEC")]
        local: String,
    },

    /// Start a SOCKS5 dynamic proxy through an SSH connection
    #[command(alias = "socks")]
    Proxy {
        /// Connection name, hostname, or alias
        target: String,
        /// Local port to listen on as the SOCKS5 server
        #[arg(short = 'D', value_name = "PORT")]
        dynamic: u16,
        /// Bind address for the SOCKS5 listener (default: 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1", value_name = "ADDR")]
        bind: String,
    },

    /// Manage connection aliases
    Alias {
        #[command(subcommand)]
        action: AliasSubcommand,
    },

    /// Close active SSH sessions
    #[command(alias = "kill")]
    Close {
        /// Connection name to close (shows active sessions if omitted)
        target: Option<String>,
        /// Close all active sessions
        #[arg(short = 'i', long)]
        all: bool,
        /// Clean up stale sessions (PIDs no longer running)
        #[arg(long)]
        cleanup: bool,
        /// Skip confirmation prompts
        #[arg(short = 'i', long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum AliasSubcommand {
    /// Add a new alias for a connection
    Add {
        /// Alias name
        alias: String,
        /// Target connection name
        target: String,
    },
    /// Remove an alias
    Remove {
        /// Alias to remove
        alias: String,
    },
    /// List aliases
    List {
        /// Connection to list aliases for (optional)
        target: Option<String>,
    },
}

fn parse_octal(s: &str) -> Result<u32, String> {
    let s = s.trim_start_matches("0o").trim_start_matches("0O");
    u32::from_str_radix(s, 8).map_err(|e| format!("invalid octal mode '{s}': {e}"))
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
            Commands::Remove { target, force } => {
                commands::remove::execute(target, force, config).await
            }
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
                clear_bastion,
                search_mode,
            } => {
                commands::config::execute(
                    default_user,
                    default_bastion,
                    default_bastion_user,
                    default_port,
                    use_kerberos,
                    log_level,
                    clear_bastion,
                    search_mode,
                    config,
                )
                .await
            }
            Commands::Stats => commands::stats::execute(config).await,
            Commands::Export {
                format,
                output,
                tag,
            } => commands::export::execute(format, output, tag, config).await,
            Commands::Backup { output } => commands::backup::execute(output, config).await,
            Commands::Restore { file, force } => {
                commands::restore::execute(file, force, config).await
            }
            Commands::Duplicate { source, new_name } => {
                commands::duplicate::execute(source, new_name, config).await
            }
            Commands::Ping { target, timeout } => {
                commands::ping::execute(target, timeout, config).await
            }
            Commands::Groups { group_name } => commands::groups::execute(group_name, config).await,
            Commands::Env { command } => commands::env::execute(command).await,
            Commands::Import { file, no_bastion } => {
                commands::import::execute(file, no_bastion, config).await
            }
            Commands::Completions { shell } => commands::completions::execute(shell, config).await,
            Commands::History {
                connection,
                limit,
                days,
                failed,
            } => commands::history::execute(connection, limit, days, failed, config).await,
            Commands::Tui => commands::tui::execute(config).await,
            Commands::Exec { target, command } => {
                commands::exec::execute(target, command, config).await
            }
            Commands::Upload { target, local, remote, offset, mode } => {
                commands::transfer::execute_upload(target, local, remote, offset, mode, config).await
            }
            Commands::Download { target, remote, local } => {
                commands::transfer::execute_download(target, remote, local, config).await
            }
            Commands::Forward { target, local } => {
                commands::forward::execute(target, local, config).await
            }
            Commands::Proxy { target, dynamic, bind } => {
                commands::proxy::execute(target, dynamic, bind, config).await
            }
            Commands::Alias { action } => {
                let alias_action = match action {
                    AliasSubcommand::Add { alias, target } => {
                        commands::alias::AliasAction::Add { alias, target }
                    }
                    AliasSubcommand::Remove { alias } => {
                        commands::alias::AliasAction::Remove { alias }
                    }
                    AliasSubcommand::List { target } => {
                        commands::alias::AliasAction::List { target }
                    }
                };
                commands::alias::execute(alias_action, config).await
            }
            Commands::Close {
                target,
                all,
                cleanup,
                force,
            } => commands::close::execute(target, all, cleanup, force, config).await,
        }
    }
}
