//! Approval broker: in-client (elicitation hook), out-of-band over the control socket, expiry.
//!
//!
//! `LocalBroker::request` by tier:
//! - `InClient`: ask the elicitor; if there is none, or it can't answer, fall back to
//!   `OutOfBand`.
//! - `OutOfBand`: persist the request, push `ServerMsg::NewRequest` to every control client,
//!   wait for a `Decide` or for `approval_timeout_secs` (then `Expired`).
//! - `TwoPerson`: like out-of-band, but it needs two decisions from distinct people. Locally
//!   every approval comes from the same machine, so at most one is recorded (a second local
//!   one is refused) and the request ends denied on timeout with "needs Teams (two
//!   approvers)"; Teams' RelayBroker is the real T3 path.

mod broker;
mod control;

pub use broker::{Elicitor, LocalBroker, RunInfo, desktop_notify};
pub use control::{ResumeFn, serve_control};

#[cfg(test)]
pub(crate) mod tests;
