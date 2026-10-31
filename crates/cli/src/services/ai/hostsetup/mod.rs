//! bssh ai setup / doctor / ca trust: scripts for the host, running them as root, and the
//! per-host checks behind `bssh ai doctor`.
//!
//!
//! bssh needs root on a host for exactly two things, both shown before they run:
//! creating the `bssh-agent` user ([`agent_user_script`]) and making sshd trust the bssh CA
//! for that account ([`trust_script`]). No firewall rules and no sudoers files, ever:

mod check;
mod root;
mod scripts;
mod sudoers;

pub use check::{check_host, HostReport};
pub use root::{root_command, run_root_script, RootRun, SudoPassword};
pub use scripts::{
    agent_user_script, trust_script, AGENT_HOME, AGENT_USER, BACKUP_DIR, EGRESS_NOTE, EXIT_NO_SUDO,
    EXIT_SUDO_PASSWORD, HOST_CA_FILE, HOST_PRINCIPALS_DIR,
};
pub use sudoers::{sudoers_suggestions, SudoersSuggestion};

#[cfg(test)]
mod tests;
