//! bssh Teams client: end-to-end encrypted shared vaults.
//!
//! The wire contract and design live in `crates/teams-proto` (see its
//! `PROTOCOL.md`). This module is the client side:
//!
//! - [`crypto`]: device keys, team-key wrapping, entry sealing, audit signing.
//!   All cryptography in bssh Teams is here, in the open (PERIMETER.md, P2).

#![allow(dead_code)]

pub mod crypto;
