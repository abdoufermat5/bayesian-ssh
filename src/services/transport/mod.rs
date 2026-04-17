//! SSH transport abstraction.
//!
//! The `SshTransport` trait lets callers (CLI, TUI, transfer service) stay
//! transport-agnostic. Concrete impls live in sibling modules.

#![allow(dead_code, unused_imports)]

pub mod dispatcher;
pub mod subprocess_impl;
pub mod types;

pub use dispatcher::{pick_kind, TransportKind};
pub use subprocess_impl::SubprocessTransport;
pub use types::*;
