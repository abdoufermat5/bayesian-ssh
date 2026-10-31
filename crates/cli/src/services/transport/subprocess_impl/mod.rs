//! Subprocess transport — shells out to `ssh`/`scp`/`sftp`.
//!
//! This is the existing behavior extracted behind the `SshTransport` trait.

mod argv;
mod interactive;
mod process;
mod ssh_transport;

#[cfg(test)]
mod tests;

pub(crate) use argv::{HopOptions, jump_proxy_command, shell_quote, ssh_strict_host_key_value};

use crate::config::AppConfig;

pub struct SubprocessTransport {
    config: AppConfig,
}

impl SubprocessTransport {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }
}
