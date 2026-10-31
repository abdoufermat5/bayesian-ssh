//! The ssh-agent socket the desktop app uses.
//!
//! The app discovers, starts or is told about an agent while it runs. Writing
//! that into the process environment would race with every thread that reads
//! it (WebKitGTK, DNS lookups, spawned children), so the socket lives here
//! instead. Startup ([`crate::run`]) still adjusts the environment before any
//! thread exists and then seeds this state from it; afterwards ssh-family
//! children get the socket explicitly, and the core gets it through
//! `AuthConfig::agent_socket` (see [`crate::commands::load_config`]).

use std::sync::RwLock;

use bayesian_ssh::services::auth::SSH_AUTH_SOCK;

static SOCKET: RwLock<Option<String>> = RwLock::new(None);

/// Seed the state from `SSH_AUTH_SOCK`. Call once, after startup has settled
/// the process environment.
pub fn init_from_env() {
    set(std::env::var(SSH_AUTH_SOCK).ok().filter(|s| !s.is_empty()));
}

/// The socket in use, if any.
pub fn current() -> Option<String> {
    SOCKET.read().expect("agent socket lock poisoned").clone()
}

/// Use `socket` from now on; `None` means no agent.
pub fn set(socket: Option<String>) {
    *SOCKET.write().expect("agent socket lock poisoned") = socket;
}
