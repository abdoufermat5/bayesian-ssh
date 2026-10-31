//! `bssh teams`: teams, their members, invites, devices and agent approvals.
//!
//! [`execute`] opens the store and hands each subcommand to the submodule that
//! owns it; the session-facing helpers live next to the commands that use them.

mod agents;
mod audit;
mod billing;
mod device;
mod invite;
mod members;
mod passphrase;
mod registry;
mod run;
mod setup;
mod sharing;
mod sync;

pub use run::execute;

pub(crate) use billing::{open_in_browser, print_billing};
pub(crate) use passphrase::{passphrase, Passphrase};
