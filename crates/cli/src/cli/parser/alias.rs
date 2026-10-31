//! `bssh alias` subcommands.

use clap::Subcommand;

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
