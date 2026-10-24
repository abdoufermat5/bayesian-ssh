//! `bssh ai ...` arguments. Each workstream owns its own `AiArgs*` struct.

use clap::{Args, Subcommand};

#[derive(Subcommand, Debug)]
pub enum AiCommands {
    /// Run the MCP server for an agent (stdio). Point your agent's MCP config here.
    Mcp(AiArgsMcp),
    /// Show how a command is parsed, tagged and decided
    Explain(AiArgsExplain),
    /// List, show or test agent policies
    Policy(AiArgsPolicy),
    /// Approve or deny a pending agent request
    Approve(AiArgsApprove),
    /// Stop a running agent session
    Stop(AiArgsStop),
    /// Show running agent sessions, pending approvals and grants
    Status(AiArgsStatus),
    /// Show what agents did: calls, decisions, approvals, changes
    Log(AiArgsLog),
    /// Replay the recorded output of an agent run
    Replay(AiArgsReplay),
    /// Configure an agent client (claude-code, codex, cursor, vscode, claude-desktop) to use bssh
    Install(AiArgsInstall),
    /// Prepare hosts for agents: create the low-privilege bssh-agent user (needs root once)
    Setup(AiArgsSetup),
    /// Check each host's enforcement: jail, certificates, agent user, client config
    Doctor(AiArgsDoctor),
    /// Local certificate authority for approval-bound certificates
    Ca(AiArgsCa),
    /// Serve MCP over HTTP for unattended agents (gateway)
    Serve(AiArgsServe),
    /// Issue a short-lived gateway token for an agent identity
    Token(AiArgsToken),
}

#[derive(Args, Debug)]
pub struct AiArgsMcp {
    /// Policy name from `ai.policies` in config.json
    #[arg(long, default_value = "default")]
    pub policy: String,
}

#[derive(Args, Debug)]
pub struct AiArgsExplain {
    /// Policy to decide with
    #[arg(long, default_value = "default")]
    pub policy: String,
    /// Host to decide for (its tags and enforcement matter)
    #[arg(long)]
    pub host: Option<String>,
    #[arg(long)]
    pub json: bool,
    /// The command, quoted or after --
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Args, Debug)]
pub struct AiArgsPolicy {
    #[command(subcommand)]
    pub command: AiPolicyCommands,
}

#[derive(Subcommand, Debug)]
pub enum AiPolicyCommands {
    /// List policies
    List,
    /// Print a policy as JSON (built-in profile defaults applied)
    Show {
        #[arg(default_value = "default")]
        name: String,
    },
    /// Run every command in FILE (one per line, `# expect: allow|ask|deny` optional) against a policy
    Test {
        file: String,
        #[arg(long, default_value = "default")]
        policy: String,
        #[arg(long)]
        host: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct AiArgsApprove {
    /// Request id (omit to pick from the pending list)
    pub request_id: Option<String>,
    /// Deny instead of approving
    #[arg(long)]
    pub deny: bool,
    #[arg(long)]
    pub reason: Option<String>,
    /// Only list pending requests
    #[arg(long)]
    pub list: bool,
    /// Also grant the same kind of call for this many minutes
    #[arg(long, value_name = "MINUTES")]
    pub grant: Option<u32>,
}

#[derive(Args, Debug)]
pub struct AiArgsStop {
    /// Run id (omit with --all, or when only one run is active)
    pub run: Option<String>,
    #[arg(long)]
    pub all: bool,
}

#[derive(Args, Debug)]
pub struct AiArgsStatus {
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct AiArgsLog {
    /// Run id (omit for the list of recent runs)
    pub run: Option<String>,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    #[arg(long)]
    pub json: bool,
    /// Print the run report (changes, diffs, undo points) instead of the event list
    #[arg(long)]
    pub report: bool,
}

#[derive(Args, Debug)]
pub struct AiArgsReplay {
    pub run: String,
    /// Only this call
    #[arg(long)]
    pub call: Option<String>,
}

#[derive(Args, Debug)]
pub struct AiArgsInstall {
    /// claude-code, codex, cursor, vscode or claude-desktop
    pub client: String,
    #[arg(long, default_value = "default")]
    pub policy: String,
    /// Also install a hook that steers raw ssh/scp to bssh tools
    #[arg(long)]
    pub guard: bool,
    /// Print what would change
    #[arg(long)]
    pub dry_run: bool,
    /// Remove bssh from the client's config
    #[arg(long)]
    pub uninstall: bool,
}

#[derive(Args, Debug)]
pub struct AiArgsSetup {
    /// Connection name (or use --tag)
    pub target: Option<String>,
    #[arg(short = 't', long)]
    pub tag: Option<String>,
    #[arg(long)]
    pub dry_run: bool,
    /// Don't ask for confirmation
    #[arg(short = 'y', long)]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct AiArgsDoctor {
    pub target: Option<String>,
    #[arg(short = 't', long)]
    pub tag: Option<String>,
    /// Policy whose write paths drive the sudoers suggestions
    #[arg(long, default_value = "default")]
    pub policy: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct AiArgsCa {
    #[command(subcommand)]
    pub command: AiCaCommands,
}

#[derive(Subcommand, Debug)]
pub enum AiCaCommands {
    /// Create the local CA key (sealed like the local vault)
    Init,
    /// Print the CA public key
    Show,
    /// Make hosts trust the CA for the bssh-agent principal (needs root once)
    Trust {
        target: Option<String>,
        #[arg(short = 't', long)]
        tag: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Args, Debug)]
pub struct AiArgsServe {
    /// Address to listen on (plain HTTP: keep it on localhost, put a TLS reverse proxy in front)
    #[arg(long, default_value = "127.0.0.1:8765")]
    pub listen: String,
    /// Policy name; Teams agents always run the admin-signed policy of their credential
    #[arg(long, default_value = "default")]
    pub policy: String,
    /// Agent identity to run, `<team>/<agent>` (see `bssh teams agent add`)
    #[arg(long, value_name = "TEAM/AGENT")]
    pub agent: String,
    /// Allow a non-loopback --listen address
    #[arg(long)]
    pub allow_remote: bool,
}

#[derive(Args, Debug)]
pub struct AiArgsToken {
    /// Agent identity (team/name)
    pub agent: String,
    #[arg(long, default_value = "1h")]
    pub ttl: String,
}
