//! `bssh source` subcommands.

use clap::Subcommand;

#[derive(Subcommand)]
#[expect(
    clippy::large_enum_variant,
    reason = "`Add` is the per-kind flag set parsed once per process; boxing it needs a flattened Args struct, which changes the clap derive shape"
)]
pub enum SourceCommands {
    /// Register a new inventory source (does not sync it)
    Add {
        /// Unique name for this source
        name: String,
        /// Source kind: ssh-config, ansible, tailscale, aws or gcp
        kind: String,
        /// Path to the SSH config file (ssh-config only)
        #[arg(long, value_name = "FILE")]
        path: Option<String>,
        /// Path to the Ansible inventory file (ansible only, required)
        #[arg(long, value_name = "FILE")]
        inventory: Option<String>,
        /// AWS profile to use (aws only)
        #[arg(long, value_name = "PROFILE")]
        profile: Option<String>,
        /// AWS region to scan (aws only, repeatable or comma-separated)
        #[arg(long, value_name = "REGION", value_delimiter = ',')]
        region: Vec<String>,
        /// Address to connect to: private/public/dns for AWS, internal/external for GCP
        #[arg(long, value_name = "MODE")]
        address: Option<String>,
        /// GCP project id (gcp only, required)
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,
        /// Default SSH username for discovered hosts
        #[arg(long, value_name = "USER")]
        user: Option<String>,
        /// Bastion/jump host applied to discovered connections
        #[arg(long, value_name = "HOST")]
        bastion: Option<String>,
        /// Username for the bastion host
        #[arg(long, value_name = "USER")]
        bastion_user: Option<String>,
        /// Include offline peers (tailscale only)
        #[arg(long)]
        include_offline: bool,
    },
    /// List configured sources and their host counts
    List,
    /// Reconcile a source's hosts with the connection database
    Sync {
        /// Source to sync (omit with --all)
        name: Option<String>,
        /// Sync every configured source, continuing after failures
        #[arg(short = 'a', long, conflicts_with = "name")]
        all: bool,
        /// Print the planned changes without writing anything
        #[arg(long)]
        dry_run: bool,
        /// Delete hosts that have been stale for longer than --prune-after days
        #[arg(long)]
        prune: bool,
        /// Age in days before a stale host is pruned (default: 30)
        #[arg(long, value_name = "DAYS")]
        prune_after: Option<u32>,
    },
    /// Remove a source, keeping or deleting its connections
    Remove {
        /// Name of the source to remove
        name: String,
        /// Delete the connections owned by the source instead of detaching them
        #[arg(long)]
        delete_hosts: bool,
        /// Skip the confirmation prompt
        #[arg(short = 'f', long)]
        force: bool,
    },
}
