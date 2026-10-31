//! `bssh key` subcommands.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum KeyCommands {
    /// List local SSH keys in ~/.ssh/ with fingerprints and permission checks
    List,
    /// Generate a new SSH keypair (Ed25519 or RSA) with secure permissions
    Generate {
        /// Name of key file (e.g. id_ed25519 or my_company_key)
        #[arg(short = 'n', long)]
        name: String,
        /// Key type: ed25519 or rsa (default: ed25519)
        #[arg(short = 't', long, value_name = "TYPE")]
        key_type: Option<String>,
    },
    /// Copy public key to remote server (~/.ssh/authorized_keys)
    Copy {
        /// Connection name or alias target
        target: String,
        /// Path to public key file (default: autodetect ~/.ssh/id_*.pub)
        #[arg(short = 'k', long, value_name = "FILE")]
        key: Option<String>,
    },
}
