use crate::config::AppConfig;
use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
pub mod utils;

#[allow(unused_imports)]
use commands::*;

#[derive(Parser)]
#[command(name = "bayesian-ssh")]
#[command(
    about = "A fast and lightweight SSH session manager with Kerberos support",
    long_about = "bayesian-ssh (bssh) is a smart SSH session manager that learns from your usage patterns.\n\n\
        It ranks connections using Bayesian scoring, supports Kerberos authentication,\n\
        interactive bastion hosts, multi-environment profiles, and provides both a CLI and TUI.\n\n\
        Common workflows:\n\
        \  bssh connect <name>     Connect to a saved server\n\
        \  bssh add <name> <host>  Save a new connection\n\
        \  bssh tui                Launch interactive dashboard\n\
        \  bssh list               Show all saved connections"
)]
#[command(version)]
pub struct Cli {
    /// Override the active environment for this invocation (does not persist)
    #[arg(long, global = true, value_name = "ENV_NAME")]
    pub env: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum EnvCommands {
    /// List all environments and show which one is active
    List,
    /// Switch the active environment (connections are scoped per environment)
    Use {
        /// Name of the environment to activate
        name: String,
    },
    /// Create a new isolated environment with its own connection database
    Create {
        /// Name for the new environment (must be unique)
        name: String,
    },
    /// Remove an environment and all its stored connections
    Remove {
        /// Name of the environment to delete
        name: String,
    },
}

#[derive(Subcommand)]
pub enum Commands {
    /// Connect to a saved server (supports fuzzy name matching)
    #[command(
        long_about = "Open an SSH session to a saved connection.\n\n\
            The target is matched by name, alias, or hostname using fuzzy search.\n\
            Bayesian scoring ranks the best match from your usage history.\n\
            Override any stored setting with the optional flags below.\n\n\
            Examples:\n\
            \  bssh connect web-prod\n\
            \  bssh connect db01 -u admin -p 2222\n\
            \  bssh connect backend -k true -b bastion.corp"
    )]
    Connect {
        /// Connection name, alias, or hostname (fuzzy-matched)
        target: String,
        /// Override the SSH username for this session
        #[arg(short = 'u', long, value_name = "USER")]
        user: Option<String>,
        /// Override the SSH port (default: 22)
        #[arg(short = 'p', long, default_value = "22", value_name = "PORT")]
        port: Option<u16>,
        /// Force Kerberos auth on or off for this session
        #[arg(short = 'k', long, value_name = "BOOL")]
        kerberos: Option<bool>,
        /// Route through this bastion/jump host
        #[arg(short = 'b', long, value_name = "HOST")]
        bastion: Option<String>,
        /// Bypass any configured bastion and connect directly
        #[arg(long)]
        no_bastion: bool,
        /// Username for the bastion host
        #[arg(short = 'B', long, value_name = "USER")]
        bastion_user: Option<String>,
        /// Path to an SSH private key to use
        #[arg(short = 'i', long, value_name = "FILE")]
        key: Option<String>,
    },

    /// Save a new SSH connection
    #[command(
        long_about = "Add a new named connection to the database.\n\n\
            The name is used as a friendly identifier for connect, upload, exec, etc.\n\
            Tags let you group related connections (e.g. --tags prod --tags eu-west).\n\n\
            Examples:\n\
            \  bssh add web-prod web.example.com -u deploy\n\
            \  bssh add db01 10.0.1.5 -p 2222 -k true -b bastion.corp\n\
            \  bssh add staging app.staging.internal -t staging -t backend"
    )]
    Add {
        /// Friendly name for this connection (must be unique)
        name: String,
        /// Server hostname or IP address
        host: String,
        /// SSH username (falls back to config default or $USER)
        #[arg(short = 'u', long, value_name = "USER")]
        user: Option<String>,
        /// SSH port (default: 22)
        #[arg(short = 'p', long, default_value = "22", value_name = "PORT")]
        port: Option<u16>,
        /// Enable Kerberos (GSSAPI) authentication
        #[arg(short = 'k', long, value_name = "BOOL")]
        kerberos: Option<bool>,
        /// Bastion/jump host to route through
        #[arg(short = 'b', long, value_name = "HOST")]
        bastion: Option<String>,
        /// Mark as direct connection (ignore default bastion)
        #[arg(long)]
        no_bastion: bool,
        /// Username for the bastion host
        #[arg(short = 'B', long, value_name = "USER")]
        bastion_user: Option<String>,
        /// Path to an SSH private key
        #[arg(short = 'i', long, value_name = "FILE")]
        key: Option<String>,
        /// Tags for grouping and filtering (repeatable)
        #[arg(short = 't', long, value_name = "TAG")]
        tags: Vec<String>,
    },

    /// List saved connections (filterable by tag or recency)
    #[command(
        long_about = "Display all saved connections in the active environment.\n\n\
            Connections are ranked by Bayesian score (most-used first).\n\
            Use --tag to filter by group, --recent for recently used, --detailed for full info.\n\n\
            Examples:\n\
            \  bssh list\n\
            \  bssh list -t prod -d\n\
            \  bssh list --recent"
    )]
    List {
        /// Show only connections with this tag
        #[arg(short = 't', long, value_name = "TAG")]
        tag: Option<String>,
        /// Show only recently used connections
        #[arg(short = 'r', long)]
        recent: bool,
        /// Show full connection details (host, port, bastion, auth)
        #[arg(short = 'd', long)]
        detailed: bool,
    },

    /// Remove a saved connection and its session history
    Remove {
        /// Connection name, alias, or ID to delete
        target: String,
        /// Skip the confirmation prompt
        #[arg(short = 'f', long)]
        force: bool,
    },

    /// Show full details of a saved connection
    Show {
        /// Connection name, alias, or ID
        target: String,
    },

    /// Edit one or more settings of an existing connection
    #[command(
        long_about = "Modify fields on a saved connection.\n\n\
            Only the fields you pass are updated; everything else stays unchanged.\n\n\
            Examples:\n\
            \  bssh edit web-prod --user deploy --port 2222\n\
            \  bssh edit db01 --bastion new-bastion.corp\n\
            \  bssh edit staging --add-tags canary --remove-tags legacy"
    )]
    Edit {
        /// Connection name, alias, or ID to edit
        target: String,
        /// Rename the connection
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// Change the hostname or IP
        #[arg(long, value_name = "HOST")]
        host: Option<String>,
        /// Change the SSH username
        #[arg(long, value_name = "USER")]
        user: Option<String>,
        /// Change the SSH port
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
        /// Enable or disable Kerberos authentication
        #[arg(long, value_name = "BOOL")]
        kerberos: Option<bool>,
        /// Set or change the bastion host
        #[arg(long, value_name = "HOST")]
        bastion: Option<String>,
        /// Remove the bastion (switch to direct connection)
        #[arg(long)]
        no_bastion: bool,
        /// Change the bastion username
        #[arg(long, value_name = "USER")]
        bastion_user: Option<String>,
        /// Set or change the SSH private key path
        #[arg(long, value_name = "FILE")]
        key: Option<String>,
        /// Add tags (repeatable)
        #[arg(long, value_name = "TAG")]
        add_tags: Vec<String>,
        /// Remove tags (repeatable)
        #[arg(long, value_name = "TAG")]
        remove_tags: Vec<String>,
    },

    /// View or update global application settings
    #[command(
        long_about = "Read or modify bssh global defaults.\n\n\
            Run with no flags to print current settings.\n\
            Pass one or more flags to update values.\n\n\
            Examples:\n\
            \  bssh config\n\
            \  bssh config --default-user deploy --use-kerberos true\n\
            \  bssh config --search-mode bayesian\n\
            \  bssh config --clear-bastion"
    )]
    Config {
        /// Default SSH username for new connections
        #[arg(long, value_name = "USER")]
        default_user: Option<String>,
        /// Default bastion host applied to new connections
        #[arg(long, value_name = "HOST")]
        default_bastion: Option<String>,
        /// Default username for the bastion host
        #[arg(long, value_name = "USER")]
        default_bastion_user: Option<String>,
        /// Default SSH port for new connections
        #[arg(long, value_name = "PORT")]
        default_port: Option<u16>,
        /// Enable Kerberos authentication by default for new connections
        #[arg(long, value_name = "BOOL")]
        use_kerberos: Option<bool>,
        /// Application log level (trace, debug, info, warn, error)
        #[arg(long, value_name = "LEVEL")]
        log_level: Option<String>,
        /// Remove the default bastion so new connections are direct
        #[arg(long)]
        clear_bastion: bool,
        /// Connection search algorithm: "bayesian" (usage-ranked) or "fuzzy" (substring)
        #[arg(long, value_parser = ["bayesian", "fuzzy"], value_name = "MODE")]
        search_mode: Option<String>,
    },

    /// Show usage statistics (total connections, sessions, top hosts)
    Stats,

    /// Export connections to a file or stdout
    #[command(
        long_about = "Serialize saved connections to JSON, TOML, or OpenSSH config format.\n\n\
            Writes to stdout by default; use -o to write to a file.\n\n\
            Examples:\n\
            \  bssh export --format json\n\
            \  bssh export --format ssh-config -o ~/.ssh/config.d/bssh\n\
            \  bssh export --format toml -t prod -o prod-hosts.toml"
    )]
    Export {
        /// Output format: json, toml, or ssh-config
        #[arg(long, value_name = "FMT")]
        format: Option<String>,
        /// Write output to this file instead of stdout
        #[arg(short = 'o', long, value_name = "FILE")]
        output: Option<String>,
        /// Export only connections matching this tag
        #[arg(short = 't', long, value_name = "TAG")]
        tag: Option<String>,
    },

    /// Backup the connection database to a file
    Backup {
        /// Destination path (defaults to ~/.local/share/bayesian-ssh/backups/<timestamp>.db)
        #[arg(short = 'o', long, value_name = "FILE")]
        output: Option<String>,
    },

    /// Restore the connection database from a previous backup
    Restore {
        /// Path to the backup file to restore
        file: String,
        /// Skip the confirmation prompt
        #[arg(short = 'f', long)]
        force: bool,
    },

    /// Clone an existing connection under a new name
    #[command(
        long_about = "Create a copy of a saved connection with a different name.\n\
            All settings (host, port, bastion, tags, etc.) are preserved.\n\n\
            Example:\n\
            \  bssh duplicate web-prod web-staging"
    )]
    Duplicate {
        /// Name of the connection to copy
        source: String,
        /// Name for the new connection
        new_name: String,
    },

    /// Test SSH reachability of a saved connection
    #[command(
        long_about = "Attempt a TCP connect (and optional SSH handshake) to verify the host is reachable.\n\
            Useful for verifying firewall rules or bastion routing before a full session.\n\n\
            Examples:\n\
            \  bssh ping web-prod\n\
            \  bssh ping db01 -t 10"
    )]
    Ping {
        /// Connection name, alias, or hostname
        target: String,
        /// Connection timeout in seconds (default: 5)
        #[arg(short = 't', long, value_name = "SECS")]
        timeout: Option<u64>,
    },

    /// List tag groups, or show connections in a specific group
    Groups {
        /// Tag name to filter by (omit to list all groups)
        group_name: Option<String>,
    },

    /// Import SSH hosts from an OpenSSH config file
    #[command(
        long_about = "Parse an OpenSSH config file and import each Host block as a connection.\n\
            Defaults to ~/.ssh/config when --file is omitted.\n\n\
            Examples:\n\
            \  bssh import\n\
            \  bssh import -f /etc/ssh/ssh_config\n\
            \  bssh import --no-bastion"
    )]
    Import {
        /// Path to the SSH config file (default: ~/.ssh/config)
        #[arg(short = 'f', long, value_name = "FILE")]
        file: Option<String>,
        /// Import all hosts as direct connections (ignore ProxyJump)
        #[arg(long)]
        no_bastion: bool,
    },

    /// Manage multi-environment profiles (separate connection databases)
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },

    /// Generate shell completion script for bash, zsh, fish, or powershell
    #[command(
        long_about = "Print a completion script to stdout.\n\
            Source or install it for your shell to enable tab-completion.\n\n\
            Examples:\n\
            \  bssh completions bash > ~/.local/share/bash-completion/completions/bssh\n\
            \  bssh completions zsh > ~/.zfunc/_bssh\n\
            \  bssh completions fish > ~/.config/fish/completions/bssh.fish"
    )]
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Show past SSH session history (timestamps, durations, exit codes)
    #[command(
        long_about = "Display a log of past SSH sessions.\n\
            Includes connection name, start time, duration, and exit status.\n\n\
            Examples:\n\
            \  bssh history\n\
            \  bssh history -c web-prod -n 50\n\
            \  bssh history --days 7 --failed"
    )]
    History {
        /// Show only sessions for this connection name
        #[arg(short = 'c', long, value_name = "NAME")]
        connection: Option<String>,
        /// Maximum number of entries to display
        #[arg(short = 'n', long, default_value = "20", value_name = "COUNT")]
        limit: usize,
        /// Limit to sessions from the last N days
        #[arg(short = 'd', long, value_name = "DAYS")]
        days: Option<u32>,
        /// Show only sessions that exited with an error
        #[arg(short = 'f', long)]
        failed: bool,
    },

    /// Launch the interactive terminal dashboard
    #[command(alias = "ui", long_about = "Open the full-screen TUI with connection list, session history,\n\
        file browser, and port-forwarding panels. Use arrow keys or vim bindings to navigate.")]
    Tui,

    /// Run a command on a remote host and print its output locally
    #[command(
        alias = "run",
        long_about = "Execute a one-off command over SSH without opening an interactive shell.\n\
            The remote stdout and stderr are printed locally.\n\
            Separate the remote command from bssh flags with --.\n\n\
            Examples:\n\
            \  bssh exec web-prod -- uname -a\n\
            \  bssh run db01 -- pg_dump mydb > backup.sql\n\
            \  bssh exec staging -- systemctl status nginx"
    )]
    Exec {
        /// Connection name, alias, or hostname
        target: String,
        /// Remote command and arguments (everything after --)
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },

    /// Upload a file or directory to a remote host (SFTP or SCP fallback)
    #[command(
        long_about = "Transfer a local file or directory to the remote host.\n\
            Uses SFTP for native connections and falls back to SCP automatically\n\
            when connecting through an interactive bastion.\n\n\
            Examples:\n\
            \  bssh upload web-prod ./deploy.tar.gz /opt/releases/deploy.tar.gz\n\
            \  bssh upload web-prod -r ./config/ /etc/myapp/ --mode 0o600\n\
            \  bssh upload db01 dump.sql /tmp/dump.sql --offset 1048576"
    )]
    Upload {
        /// Connection name, alias, or hostname
        target: String,
        /// Local file or directory to upload
        #[arg(value_name = "LOCAL")]
        local: std::path::PathBuf,
        /// Destination path on the remote host
        #[arg(value_name = "REMOTE")]
        remote: String,
        /// Resume an interrupted upload starting at this byte offset
        #[arg(long, default_value = "0", value_name = "BYTES")]
        offset: u64,
        /// Set file permissions on the remote side (octal, e.g. 0o644)
        #[arg(long, default_value = "0o644", value_parser = parse_octal, value_name = "OCTAL")]
        mode: u32,
        /// Recursively upload an entire directory tree
        #[arg(short = 'r', long)]
        recursive: bool,
    },

    /// Download a file or directory from a remote host (SFTP or SCP fallback)
    #[command(
        long_about = "Transfer a remote file or directory to the local machine.\n\
            Uses SFTP for native connections and falls back to SCP automatically\n\
            when connecting through an interactive bastion.\n\n\
            Examples:\n\
            \  bssh download web-prod /var/log/app.log ./app.log\n\
            \  bssh download db01 -r /backups/daily/ ./local-backups/"
    )]
    Download {
        /// Connection name, alias, or hostname
        target: String,
        /// Path to the file or directory on the remote host
        #[arg(value_name = "REMOTE")]
        remote: String,
        /// Local destination path
        #[arg(value_name = "LOCAL")]
        local: std::path::PathBuf,
        /// Recursively download an entire directory tree
        #[arg(short = 'r', long)]
        recursive: bool,
    },

    /// Forward a local port to a remote address through an SSH tunnel
    #[command(
        alias = "tunnel",
        long_about = "Create an SSH local port-forward (-L style).\n\
            Traffic to the local bind address is tunneled to the remote target.\n\n\
            Spec format: [bind_addr:]bind_port:remote_host:remote_port\n\
            (bind_addr defaults to 127.0.0.1 when omitted)\n\n\
            Examples:\n\
            \  bssh forward db01 -L 5432:localhost:5432\n\
            \  bssh tunnel web-prod -L 0.0.0.0:8080:internal-api:80"
    )]
    Forward {
        /// Connection name, alias, or hostname to tunnel through
        target: String,
        /// Forward spec: [bind_addr:]bind_port:remote_host:remote_port
        #[arg(short = 'L', value_name = "SPEC")]
        local: String,
    },

    /// Start a SOCKS5 dynamic proxy through an SSH connection
    #[command(
        alias = "socks",
        long_about = "Open an SSH dynamic port-forward (-D style) acting as a local SOCKS5 proxy.\n\
            Point your browser or application to the local port to route traffic through the remote host.\n\n\
            Examples:\n\
            \  bssh proxy web-prod -D 1080\n\
            \  bssh socks bastion -D 9050 --bind 0.0.0.0"
    )]
    Proxy {
        /// Connection name, alias, or hostname to proxy through
        target: String,
        /// Local port for the SOCKS5 listener
        #[arg(short = 'D', value_name = "PORT")]
        dynamic: u16,
        /// Bind address for the SOCKS5 listener
        #[arg(long, default_value = "127.0.0.1", value_name = "ADDR")]
        bind: String,
    },

    /// Create, remove, or list short aliases for connections
    #[command(
        long_about = "Aliases let you refer to connections by shorter names.\n\n\
            Examples:\n\
            \  bssh alias add wp web-prod\n\
            \  bssh alias list\n\
            \  bssh alias remove wp"
    )]
    Alias {
        #[command(subcommand)]
        action: AliasSubcommand,
    },

    /// Close active SSH sessions or clean up stale ones
    #[command(
        alias = "kill",
        long_about = "Terminate running SSH sessions started by bssh.\n\
            With no arguments, lists currently active sessions.\n\
            Use --cleanup to remove stale entries whose PIDs no longer exist.\n\n\
            Examples:\n\
            \  bssh close             # list active sessions\n\
            \  bssh close web-prod    # close one session\n\
            \  bssh kill --all -f     # force-close everything\n\
            \  bssh close --cleanup   # prune dead entries"
    )]
    Close {
        /// Connection name to close (omit to list active sessions)
        target: Option<String>,
        /// Close all active sessions at once
        #[arg(short = 'a', long)]
        all: bool,
        /// Remove stale session entries (PIDs that are no longer running)
        #[arg(long)]
        cleanup: bool,
        /// Skip confirmation prompts
        #[arg(short = 'f', long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum AliasSubcommand {
    /// Map a short alias to an existing connection name
    Add {
        /// The alias to create (e.g. "wp")
        alias: String,
        /// The connection name this alias points to
        target: String,
    },
    /// Delete an alias
    Remove {
        /// Alias name to delete
        alias: String,
    },
    /// List aliases (optionally filtered to one connection)
    List {
        /// Show only aliases for this connection (omit for all)
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
            Commands::Upload { target, local, remote, offset, mode, recursive } => {
                commands::transfer::execute_upload(target, local, remote, offset, mode, recursive, config).await
            }
            Commands::Download { target, remote, local, recursive } => {
                commands::transfer::execute_download(target, remote, local, recursive, config).await
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
