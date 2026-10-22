//! Wire contract for bssh Teams, shared by the open-source client and the
//! sync server.
//!
//! This crate holds no cryptography. It defines what goes over the wire
//! ([`api`]), the binary field types ([`wire`]), and the exact bytes that get
//! authenticated or signed ([`layout`]). Both sides build those bytes here, so
//! they can't drift apart. See `PROTOCOL.md` for the design.

pub mod api;
pub mod layout;
pub mod wire;

/// Protocol version spoken by this crate. Sent in [`PROTOCOL_HEADER`].
pub const PROTOCOL_VERSION: u32 = 1;

/// Request header carrying [`PROTOCOL_VERSION`].
pub const PROTOCOL_HEADER: &str = "Bssh-Protocol";

/// Request header carrying the client's semver (`CARGO_PKG_VERSION`). The
/// server answers `426` with [`api::ErrorCode::ClientTooOld`] below its minimum.
pub const CLIENT_VERSION_HEADER: &str = "Bssh-Client-Version";

/// Every path is under this prefix.
pub const API_PREFIX: &str = "/v1";
