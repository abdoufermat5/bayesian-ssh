//! The top-level `bssh` argument parser.

use super::commands::Commands;
use clap::Parser;

#[derive(Parser)]
#[command(name = "bayesian-ssh")]
#[command(
    about = "A fast and lightweight SSH session manager with Kerberos support",
    long_about = "bayesian-ssh (bssh) is a smart SSH session manager that learns from your usage patterns.\n\n\
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
