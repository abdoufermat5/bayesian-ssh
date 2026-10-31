//! HTTP client for the bssh Teams server (`/v1`). One method per endpoint the
//! client needs; the sync engine uses it through [`TeamsTransport`].
//!
//! Security notes (PERIMETER.md, "network attacker"): https only (plain http
//! is accepted solely for loopback hosts, for development), TLS 1.3 minimum,
//! redirects are never followed (so a token can't be forwarded and https can't
//! be downgraded), and every request has a timeout.
//!
//! [`TeamsTransport`]: super::sync::TeamsTransport

mod audit;
mod endpoints;
mod error;
mod http;
#[cfg(test)]
pub(crate) mod mock;
#[cfg(test)]
mod tests;
mod transport;

pub use audit::{walk_audit_pages, AuditWalk};
pub use error::{ClientResult, TeamsError};
pub use http::{validate_server_url, AuthSigner, TeamsClient, MAX_POLL_WAIT_SECS};
