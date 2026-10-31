//! The `bssh <command>` subcommand tree.

use super::alias::AliasSubcommand;
use super::env::EnvCommands;
use super::key::KeyCommands;
use super::org::OrgCommands;
use super::source::SourceCommands;
use super::teams::TeamsCommands;
use crate::cli::parser_ai::AiCommands;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum Commands {
    /// Connect to a saved server (supports fuzzy name matching)
    #[command(long_about = "Open an SSH session to a saved connection.\n\n\
            The target is matched by name, alias, or hostname using fuzzy search.\n\
            Bayesian scoring ranks the best match from your usage history.\n\
            Override any stored setting with the optional flags below.\n\n\
            Examples:\n\
              bssh connect web-prod\n\
              bssh connect db01 -u admin -p 2222\n\
              bssh connect backend -k true -b bastion.corp")]
    Connect {
        /// Connection name, alias, or hostname (fuzzy-matched)
        target: String,
        /// Override the SSH username for this session
        #[arg(short = 'u', long, value_name = "USER")]
        user: Option<String>,
        /// Override the stored SSH port for this session
        #[arg(short = 'p', long, value_name = "PORT", value_parser = clap::value_parser!(u16).range(1..))]
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
        /// Record this session's output (overrides config)
        #[arg(long, conflicts_with = "no_record")]
        record: bool,
        /// Do not record this session's output (overrides config)
        #[arg(long, conflicts_with = "record")]
        no_record: bool,
    },

    /// Save a new SSH connection
    #[command(long_about = "Add a new named connection to the database.\n\n\
            The name is used as a friendly identifier for connect, upload, exec, etc.\n\
            Tags let you group related connections (e.g. --tags prod --tags eu-west).\n\n\
            Examples:\n\
              bssh add web-prod web.example.com -u deploy\n\
              bssh add db01 10.0.1.5 -p 2222 -k true -b bastion.corp\n\
              bssh add staging app.staging.internal -t staging -t backend")]
    Add {
        /// Friendly name for this connection (must be unique)
        name: String,
        /// Server hostname or IP address
        host: String,
        /// SSH username (falls back to config default or $USER)
        #[arg(short = 'u', long, value_name = "USER")]
        user: Option<String>,
        /// SSH port (default: the configured default port, usually 22)
        #[arg(short = 'p', long, value_name = "PORT", value_parser = clap::value_parser!(u16).range(1..))]
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
        /// Forward your ssh-agent to the server (ssh -A). Off by default
        #[arg(long, conflicts_with = "no_forward_agent")]
        forward_agent: bool,
        /// Explicitly do not forward your ssh-agent (the default)
        #[arg(long, conflicts_with = "forward_agent")]
        no_forward_agent: bool,
        /// Tags for grouping and filtering (repeatable or comma-separated)
        #[arg(short = 't', long, value_name = "TAG", value_delimiter = ',')]
        tags: Vec<String>,
    },

    /// List saved connections (filterable by tag or recency)
    #[command(
        long_about = "Display all saved connections in the active environment.\n\n\
            Connections are ranked by Bayesian score (most-used first).\n\
            Use --tag to filter by group, --recent for recently used, --detailed for full info.\n\n\
            Examples:\n\
              bssh list\n\
              bssh list -t prod -d\n\
              bssh list --recent"
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

    /// Remove a saved connection (single or bulk by tag)
    Remove {
        /// Connection name, alias, or ID to delete
        target: Option<String>,
        /// Remove connections matching this tag
        #[arg(short = 't', long, value_name = "TAG")]
        tag: Option<String>,
        /// Skip the confirmation prompt
        #[arg(short = 'f', long)]
        force: bool,
    },

    /// Show full details of a saved connection
    Show {
        /// Connection name, alias, or ID
        target: String,
    },

    /// Accept the route changes a team sync made to a connection
    #[command(
        long_about = "A team member can edit a shared connection. When a sync changes its host, \
            port, user, bastion or Kerberos setting, bssh ignores the new route until you confirm it \
            (agent forwarding also stays off).\n\n\
            Examples:\n\
              bssh confirm db-prod\n\
              bssh connect db-prod      # asks y/N interactively, or points here"
    )]
    Confirm {
        /// Connection name, alias, or ID with a pending change
        target: String,
    },

    /// Edit one or more settings of an existing connection
    #[command(long_about = "Modify fields on a saved connection.\n\n\
            Only the fields you pass are updated; everything else stays unchanged.\n\n\
            Examples:\n\
              bssh edit web-prod --user deploy --port 2222\n\
              bssh edit db01 --bastion new-bastion.corp\n\
              bssh edit staging --add-tags canary --remove-tags legacy")]
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
        #[arg(long, value_name = "PORT", value_parser = clap::value_parser!(u16).range(1..))]
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
        /// Forward your ssh-agent to the server (ssh -A)
        #[arg(long, conflicts_with = "no_forward_agent")]
        forward_agent: bool,
        /// Stop forwarding your ssh-agent
        #[arg(long, conflicts_with = "forward_agent")]
        no_forward_agent: bool,
        /// Add tags (repeatable or comma-separated)
        #[arg(long, value_name = "TAG", value_delimiter = ',')]
        add_tags: Vec<String>,
        /// Remove tags (repeatable or comma-separated)
        #[arg(long, value_name = "TAG", value_delimiter = ',')]
        remove_tags: Vec<String>,
    },

    /// View or update global application settings
    #[command(long_about = "Read or modify bssh global defaults.\n\n\
            Run with no flags to print current settings.\n\
            Pass one or more flags to update values.\n\n\
            Examples:\n\
              bssh config\n\
              bssh config --default-user deploy --use-kerberos true\n\
              bssh config --search-mode bayesian\n\
              bssh config --clear-bastion")]
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
        #[arg(long, value_name = "PORT", value_parser = clap::value_parser!(u16).range(1..))]
        default_port: Option<u16>,
        /// Enable Kerberos authentication by default for new connections
        #[arg(long, value_name = "BOOL")]
        use_kerberos: Option<bool>,
        /// Application log level (trace, debug, info, warn, error, off)
        #[arg(long, value_parser = ["trace", "debug", "info", "warn", "error", "off"], value_name = "LEVEL")]
        log_level: Option<String>,
        /// Remove the default bastion so new connections are direct
        #[arg(long)]
        clear_bastion: bool,
        /// Connection search algorithm: "bayesian" (usage-ranked) or "fuzzy" (substring)
        #[arg(long, value_parser = ["bayesian", "fuzzy"], value_name = "MODE")]
        search_mode: Option<String>,
        /// Host-key verification: "strict", "accept-new" (pin first-seen) or "off"
        /// ("yes"/"no" are accepted as aliases of strict/off)
        #[arg(long, value_name = "POLICY")]
        strict_host_key_checking: Option<String>,
        /// Enable or disable session output recording ("on" or "off")
        #[arg(long, value_parser = ["on", "off"], value_name = "STATE")]
        recording: Option<String>,
        /// Record only connections carrying these comma-separated tags (empty string clears)
        #[arg(long, value_name = "TAGS")]
        recording_tags: Option<String>,
        /// Enable or disable context-aware predictive ranking ("on" or "off")
        #[arg(long, value_parser = ["on", "off"], value_name = "STATE")]
        prediction: Option<String>,
        /// Enable or disable capturing working directory and git root ("on" or "off")
        #[arg(long, value_parser = ["on", "off"], value_name = "STATE")]
        capture_cwd: Option<String>,
    },

    /// Show usage statistics (total connections, sessions, top hosts)
    Stats,

    /// Audit security configuration, host keys, key permissions, and connection health
    Audit {
        /// Automatically repair overly permissive file and directory permissions
        #[arg(short = 'f', long)]
        fix: bool,
    },

    /// Manage SSH keys (list, generate, copy public keys to remote servers)
    Key {
        #[command(subcommand)]
        command: KeyCommands,
    },

    /// Export connections to a file or stdout
    #[command(
        long_about = "Serialize saved connections to JSON, TOML, or OpenSSH config format.\n\n\
            Writes to stdout by default; use -o to write to a file.\n\n\
            Examples:\n\
              bssh export --format json\n\
              bssh export --format ssh-config -o ~/.ssh/config.d/bssh\n\
              bssh export --format toml -t prod -o prod-hosts.toml"
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
        /// Encrypt export payload with a secret passphrase
        #[arg(short = 'p', long, value_name = "PASSPHRASE")]
        passphrase: Option<String>,
    },

    /// Backup the connection database to a file
    Backup {
        /// Destination path (default: ENV_DIR/backups/backup-TIMESTAMP.db); must not already exist
        #[arg(short = 'o', long, value_name = "FILE")]
        output: Option<String>,
    },

    /// Diagnose local bssh configuration, database, and SSH prerequisites
    #[command(long_about = "Run local health checks for bssh.\n\n\
            Verifies the active configuration, database initialization, SSH client availability,\n\
            optional SSH config path, ssh-agent environment, and Kerberos helper commands.\n\n\
            Examples:\n\
              bssh doctor\n\
              bssh --env staging doctor")]
    Doctor,

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
              bssh duplicate web-prod web-staging"
    )]
    Duplicate {
        /// Name of the connection to copy
        source: String,
        /// Name for the new connection
        new_name: String,
    },

    /// Test SSH reachability of a saved connection (single or multi-host)
    #[command(
        long_about = "Attempt a TCP connect (and optional SSH handshake) to verify hosts are reachable.\n\
            Supports single host, all hosts (--all), or filtering by tag (-g/--tag).\n\n\
            Examples:\n\
              bssh ping web-prod\n\
              bssh ping --all\n\
              bssh ping -g prod -t 10"
    )]
    Ping {
        /// Connection name, alias, or hostname
        target: Option<String>,
        /// Ping all saved connections in parallel
        #[arg(short = 'a', long)]
        all: bool,
        /// Ping connections matching this tag
        #[arg(short = 'g', long, value_name = "TAG")]
        tag: Option<String>,
        /// Connection timeout in seconds (default: 5)
        #[arg(short = 't', long, value_name = "SECS", value_parser = clap::value_parser!(u64).range(1..=600))]
        timeout: Option<u64>,
    },

    /// List tag groups, or show connections in a specific group
    Groups {
        /// Tag name to filter by (omit to list all groups)
        group_name: Option<String>,
    },

    /// Import SSH hosts from an OpenSSH config file or encrypted JSON backup
    #[command(
        long_about = "Parse an OpenSSH config file or encrypted JSON backup and import each host as a connection.\n\
            Defaults to ~/.ssh/config when --file is omitted.\n\n\
            Imported keys: HostName, User, Port, IdentityFile, ProxyJump and ProxyCommand\n\
            (only 'ssh -W %h:%p <host>'). ProxyJump becomes the bastion: an alias defined in\n\
            the same file is resolved to its HostName/User/Port, 'none' means direct, and for a\n\
            chain only the first hop is kept (with a warning).\n\n\
            Examples:\n\
              bssh import\n\
              bssh import -f backup.enc --passphrase secret\n\
              bssh import --no-bastion"
    )]
    Import {
        /// Path to the SSH config file or backup file (default: ~/.ssh/config)
        #[arg(short = 'f', long, value_name = "FILE")]
        file: Option<String>,
        /// Import all hosts as direct connections (ignore ProxyJump)
        #[arg(long)]
        no_bastion: bool,
        /// Passphrase to decrypt encrypted backup file
        #[arg(short = 'p', long, value_name = "PASSPHRASE")]
        passphrase: Option<String>,
    },

    /// Manage inventory sources (add, list, sync, remove)
    #[command(
        long_about = "Configure inventories that connections can be discovered from.\n\n\
            A source remembers how to find hosts (SSH config, Ansible, Tailscale, AWS, GCP).\n\
            Adding a source does not import anything: run `bssh source sync <NAME>` to\n\
            reconcile its hosts with the connections database.\n\n\
            Examples:\n\
              bssh source add prod aws --region eu-west-1 --profile work\n\
              bssh source add local ssh-config\n\
              bssh source add cloud gcp --project my-project\n\
              bssh source sync prod --dry-run\n\
              bssh source sync --all --prune"
    )]
    Source {
        #[command(subcommand)]
        command: SourceCommands,
    },

    /// Share connections with your team through an end-to-end encrypted vault
    #[command(
        long_about = "bssh Teams: shared connections, encrypted on your devices.\n\n\
            The sync server stores only ciphertext. Team connections appear in\n\
            `bssh list` and `bssh connect` like any other host, and connecting to one\n\
            is recorded in the team's signed audit log at the next sync.\n\n\
            Your device keys are protected by a passphrase (prompted, or read from\n\
            BSSH_TEAMS_PASSPHRASE for scripts).",
        after_help = "Examples:\n\
          bssh teams init --server https://teams.example.com --email you@example.com\n\
          bssh teams create platform\n\
          bssh teams share db-prod --team platform\n\
          bssh teams invite platform ana@example.com\n\
          bssh teams init --server https://teams.example.com --email ana@example.com --invite <TOKEN>\n\
          bssh teams init --server https://teams.example.com --email you@example.com --email-code <CODE>\n\
          bssh teams join <TOKEN> --fingerprint SHA256:...      (on Ana's machine)\n\
          bssh teams grant platform --fingerprint SHA256:...    (admin, Ana's device)\n\
          bssh teams sync"
    )]
    Teams {
        #[command(subcommand)]
        command: TeamsCommands,
    },

    /// Manage organizations: owners, teams, billing
    #[command(
        long_about = "An organization owns teams and, on servers with billing, their\n\
            subscription and seats. Seats are counted per organization: someone in\n\
            several of its teams takes one seat, and a pending invite holds one.\n\n\
            An organization has no name on the server; it is shown by its owners'\n\
            emails. Owners manage billing and owners, and create teams in it.",
        after_help = "Examples:\n\
          bssh org list\n\
          bssh org show\n\
          bssh org owners add platform ana@example.com\n\
          bssh org billing --checkout --seats 8\n\
          bssh org billing --manage\n\
          bssh teams create infra --org platform"
    )]
    Org {
        #[command(subcommand)]
        command: OrgCommands,
    },

    /// Let AI agents operate servers safely (MCP proxy with policy, approvals and audit)
    #[command(
        long_about = "bssh AI: your agent gets your servers, not your keys.\n\n\
            `bssh ai mcp` is an MCP server for Claude Code, Codex, Cursor and other agents.\n\
            Every call is parsed, checked against a policy, run in a read-only jail or\n\
            approved by you, recorded and signed.",
        after_help = "Examples:\n\
          bssh ai install claude-code\n\
          bssh ai explain 'systemctl restart nginx && tail /var/log/x'\n\
          bssh ai setup -t staging\n\
          bssh ai approve --list\n\
          bssh ai log"
    )]
    Ai {
        #[command(subcommand)]
        command: AiCommands,
    },

    /// Manage multi-environment profiles (separate connection databases)
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },

    /// Generate shell completion script for bash, zsh, fish, or powershell
    #[command(long_about = "Print a completion script to stdout.\n\
            Source or install it for your shell to enable tab-completion.\n\n\
            Examples:\n\
              bssh completions bash > ~/.local/share/bash-completion/completions/bssh\n\
              bssh completions zsh > ~/.zfunc/_bssh\n\
              bssh completions fish > ~/.config/fish/completions/bssh.fish")]
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Show past SSH session history (timestamps, durations, exit codes)
    #[command(long_about = "Display a log of past SSH sessions.\n\
            Includes connection name, start time, duration, and exit status.\n\n\
            Examples:\n\
              bssh history\n\
              bssh history -c web-prod -n 50\n\
              bssh history --days 7 --failed")]
    History {
        /// Show only sessions for this connection name
        #[arg(short = 'c', long, value_name = "NAME")]
        connection: Option<String>,
        /// Maximum number of entries to display
        #[arg(short = 'n', long, default_value = "20", value_name = "COUNT")]
        limit: usize,
        /// Limit to sessions from the last N days
        #[arg(short = 'D', long, value_name = "DAYS")]
        days: Option<u32>,
        /// Show only sessions that exited with an error
        #[arg(short = 'f', long)]
        failed: bool,
        /// Search recorded session output for a text phrase
        #[arg(short = 'g', long, value_name = "TEXT")]
        grep: Option<String>,
    },

    /// Replay a recorded SSH session in the terminal
    #[command(
        long_about = "Play back a recorded asciicast v2 session in the terminal.\n\
            Find session IDs with `bssh history` or `bssh history --grep <text>`.\n\n\
            Examples:\n\
              bssh replay 3f2a91c0\n\
              bssh replay 3f2a91c0 --speed 2.0 --idle-cap 1.0"
    )]
    Replay {
        /// Session ID or unique prefix (at least 6 characters)
        session: String,
        /// Playback speed multiplier
        #[arg(short = 's', long, default_value_t = 1.0, value_name = "FACTOR")]
        speed: f64,
        /// Cap idle pauses to at most this many seconds
        #[arg(long, default_value_t = 2.0, value_name = "SECONDS")]
        idle_cap: f64,
    },

    /// Launch the Tauri desktop GUI (runs detached in the background)
    #[command(
        alias = "gui",
        long_about = "Launch the bayesian-ssh desktop GUI application in the background (detached from terminal)."
    )]
    Desktop,

    /// Run a command on remote host(s) and print output locally
    #[command(
        alias = "run",
        long_about = "Execute a one-off command over SSH without opening an interactive shell.\n\
            Supports single host, all hosts (--all), or filtering by tag (-t/--tag).\n\
            Use --dry-run to preview target hosts before running.\n\n\
            IMPORTANT: Use -- to separate the remote command from bssh flags.\n\n\
            Examples:\n\
              bssh exec web-prod -- uname -a\n\
              bssh exec --all --dry-run -- uptime\n\
              bssh exec -t prod -- systemctl status nginx\n\
              bssh exec --timeout 30 --max-output 65536 web-prod -- journalctl -n 1000"
    )]
    Exec {
        /// Connection name, alias, or hostname
        target: Option<String>,
        /// Run command on all saved connections in parallel
        #[arg(short = 'a', long)]
        all: bool,
        /// Run command on connections matching this tag
        #[arg(short = 't', long, value_name = "TAG")]
        tag: Option<String>,
        /// Preview target hosts and command without executing
        #[arg(long)]
        dry_run: bool,
        /// Kill the command after this many seconds (exit code 124, like timeout(1))
        #[arg(long, value_name = "SECS", value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        /// Keep at most this many bytes of stdout and of stderr per host
        #[arg(long, value_name = "BYTES", value_parser = clap::value_parser!(u64).range(1..))]
        max_output: Option<u64>,
        /// Remote command and arguments (put -- before the command)
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },

    /// Upload a file or directory to one or many remote hosts in parallel
    #[command(
        override_usage = "bssh upload [OPTIONS] [TARGETS] <LOCAL> <REMOTE>",
        long_about = "Transfer a local file or directory to one or more hosts, in parallel.\n\
            Uses SFTP for native connections and falls back to SCP automatically\n\
            when connecting through an interactive bastion.\n\n\
            Targets are comma-separated names/aliases/hosts, e.g. `web1,web2,db1`.\n\
            Use --all or -t/--tag to select many hosts without listing them.\n\n\
            Examples:\n\
              bssh upload web1,web2 ./app.tar.gz /opt/app/\n\
              bssh upload -t prod ./app.tar.gz /opt/app/\n\
              bssh upload --all --dry-run ./app.tar.gz /opt/app/\n\
              bssh upload web-prod -r ./config/ /etc/myapp/ --mode 0o600"
    )]
    Upload {
        /// Targets, local path and remote path: [TARGETS] LOCAL REMOTE
        #[arg(value_names = ["TARGETS", "LOCAL", "REMOTE"], num_args = 1..=3)]
        args: Vec<String>,
        /// Upload to all saved connections in parallel
        #[arg(short = 'a', long, conflicts_with = "tag")]
        all: bool,
        /// Upload to connections matching this tag
        #[arg(short = 't', long, value_name = "TAG", conflicts_with = "all")]
        tag: Option<String>,
        /// Preview target hosts and resolved remote paths without connecting
        #[arg(long)]
        dry_run: bool,
        /// Maximum number of hosts to upload to concurrently
        #[arg(short = 'j', long, value_name = "N", default_value_t = crate::cli::utils::MAX_PARALLEL_HOSTS)]
        parallel: usize,
        /// Resume an interrupted upload starting at this byte offset (single host only)
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
              bssh download web-prod /var/log/app.log ./app.log\n\
              bssh download db01 -r /backups/daily/ ./local-backups/"
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
              bssh forward db01 -L 5432:localhost:5432\n\
              bssh tunnel web-prod -L 0.0.0.0:8080:internal-api:80"
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
              bssh proxy web-prod -D 1080\n\
              bssh socks bastion -D 9050 --bind 0.0.0.0"
    )]
    Proxy {
        /// Connection name, alias, or hostname to proxy through
        target: String,
        /// Local port for the SOCKS5 listener
        #[arg(short = 'D', value_name = "PORT", value_parser = clap::value_parser!(u16).range(1..))]
        dynamic: u16,
        /// Bind address for the SOCKS5 listener
        #[arg(long, default_value = "127.0.0.1", value_name = "ADDR")]
        bind: String,
    },

    /// Create, remove, or list short aliases for connections
    #[command(
        long_about = "Aliases let you refer to connections by shorter names.\n\n\
            Examples:\n\
              bssh alias add wp web-prod\n\
              bssh alias list\n\
              bssh alias remove wp"
    )]
    Alias {
        #[command(subcommand)]
        action: AliasSubcommand,
    },

    /// Close active SSH sessions or clean up stale ones
    #[command(
        alias = "kill",
        long_about = "Terminate running SSH sessions started by bssh.\n\
            With no arguments, lists currently active sessions.\n\n\
            Examples:\n\
              bssh close             # list active sessions\n\
              bssh close web-prod    # close one session\n\
              bssh kill --all -f     # force-close everything"
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

    /// Explain the predictive ranking score of a connection in the current context
    #[command(
        long_about = "Break down the Bayesian ranking score of a saved connection factor by factor.\n\
            Shows frequency prior, recency, success rate, and context likelihood ratios\n\
            (repository, hour of day, and previous connection).\n\n\
            Example:\n\
              bssh why web-prod"
    )]
    Why {
        /// Connection name, alias, or ID
        target: String,
    },
}

fn parse_octal(s: &str) -> Result<u32, String> {
    let digits = s.trim_start_matches("0o").trim_start_matches("0O");
    let mode =
        u32::from_str_radix(digits, 8).map_err(|e| format!("invalid octal mode '{s}': {e}"))?;
    if mode > 0o7777 {
        return Err(format!("mode '{s}' exceeds 0o7777"));
    }
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::parse_octal;

    #[test]
    fn parse_octal_bounds() {
        assert_eq!(parse_octal("0o644"), Ok(0o644));
        assert_eq!(parse_octal("755"), Ok(0o755));
        assert_eq!(parse_octal("0o7777"), Ok(0o7777));
        assert!(parse_octal("0o10000").is_err());
        assert!(parse_octal("0o").is_err());
        assert!(parse_octal("9").is_err());
    }
}
