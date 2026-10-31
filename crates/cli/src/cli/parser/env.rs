//! `bssh env` subcommands.

use clap::Subcommand;

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
