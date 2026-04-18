//! SSH transport abstraction.
//!
//! The `SshTransport` trait lets callers (CLI, TUI, transfer service) stay
//! transport-agnostic. Concrete impls live in sibling modules.

#![allow(dead_code, unused_imports)]

pub mod dispatcher;
pub mod russh_impl;
pub mod scp_impl;
pub mod sftp_impl;
pub mod socks5;
pub mod subprocess_impl;
pub mod types;

pub use dispatcher::{pick_kind, TransportKind};
pub use russh_impl::RusshTransport;
pub use sftp_impl::RusshSftpSession;
pub use subprocess_impl::SubprocessTransport;
pub use types::*;
