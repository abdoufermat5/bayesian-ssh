//! The top-level `bssh` argument parser.

use super::commands::Commands;
use clap::Parser;
use clap::builder::styling::{AnsiColor, Effects, Styles};

fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .usage(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .literal(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Green.on_default())
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Red.on_default() | Effects::BOLD)
}

const HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}
{usage-heading} {usage}

Options:
{options}

{after-help}";

const GROUPED_COMMANDS_HELP: &str = "\
Connections:
  connect      Connect to a saved server (supports fuzzy name matching)
  list         List saved connections (filterable by tag or recency)
  show         Show full details of a saved connection
  add          Save a new SSH connection
  edit         Edit one or more settings of an existing connection
  remove       Remove a saved connection (single or bulk by tag)
  duplicate    Clone an existing connection under a new name
  alias        Create, remove, or list short aliases for connections
  groups       List tag groups, or show connections in a specific group
  confirm      Accept the route changes a team sync made to a connection

Remote Execution, Transfers & Tunnels:
  exec         Run a command on remote host(s) and print output locally [alias: run]
  upload       Upload a file or directory to one or many remote hosts in parallel
  download     Download a file or directory from a remote host
  forward      Forward a local port to a remote address through an SSH tunnel [alias: tunnel]
  proxy        Start a SOCKS5 dynamic proxy through an SSH connection [alias: socks]
  close        Close active SSH sessions or clean up stale ones [alias: kill]

Sessions & Insights:
  history      Show past SSH session history (timestamps, durations, exit codes)
  replay       Replay a recorded SSH session in the terminal
  stats        Show usage statistics (total connections, sessions, top hosts)
  why          Explain the predictive ranking score of a connection in the current context

Inventory, Import & Backup:
  source       Manage inventory sources (add, list, sync, remove)
  import       Import SSH hosts from an OpenSSH config file or encrypted JSON backup
  export       Export connections to a file or stdout
  backup       Backup the connection database to a file
  restore      Restore the connection database from a previous backup

Teams, Organizations & AI:
  teams        Share connections with your team through an end-to-end encrypted vault
  org          Manage organizations: owners, teams, billing
  ai           Let AI agents operate servers safely (MCP proxy with policy, approvals and audit)

Security, Diagnostics & Setup:
  ping         Test SSH reachability of a saved connection (single or multi-host)
  doctor       Diagnose local bssh configuration, database, and SSH prerequisites
  audit        Audit security configuration, host keys, key permissions, and connection health
  key          Manage SSH keys (list, generate, copy public keys to remote servers)
  config       View or update global application settings
  env          Manage multi-environment profiles (separate connection databases)
  desktop      Launch the Tauri desktop GUI (runs detached in the background) [alias: gui]
  completions  Generate shell completion script for bash, zsh, fish, or powershell
  help         Print this message or the help of the given subcommand(s)";

#[derive(Parser)]
#[command(name = "bssh")]
#[command(styles = cli_styles())]
#[command(help_template = HELP_TEMPLATE)]
#[command(after_help = GROUPED_COMMANDS_HELP)]
#[command(
    about = "A fast and lightweight SSH session manager with Kerberos support",
    long_about = "bssh (bayesian-ssh) is a smart SSH session manager that learns from your usage patterns.\n\n\
        It ranks connections using Bayesian scoring, supports Kerberos authentication,\n\
        interactive bastion hosts, multi-environment profiles, and provides both a CLI and desktop GUI.\n\n\
        Common workflows:\n\
          bssh connect <name>     Connect to a saved server\n\
          bssh add <name> <host>  Save a new connection\n\
          bssh desktop            Launch the desktop GUI\n\
          bssh list               Show all saved connections"
)]
#[command(version)]
pub struct Cli {
    /// Override the active environment for this invocation (does not persist)
    #[arg(long, global = true, value_name = "ENV_NAME")]
    pub env: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn grouped_help_lists_every_subcommand() {
        let mut cmd = Cli::command();
        let rendered = cmd.render_help().to_string();
        assert!(rendered.contains("Usage: bssh"));
        for sub in cmd.get_subcommands() {
            let name = sub.get_name();
            assert!(
                rendered.contains(&format!("  {name} ")),
                "top-level help is missing subcommand '{name}'"
            );
        }
    }
}
