//! Secret redaction before output reaches the model (design 8).
//!
//! [`SecretRedactor`] removes exact values from the bssh vault,
//! private-key blocks, common token formats, credentials embedded in URLs and
//! `key=value` secrets. Each hit becomes `«redacted:<kind>»` and is counted in
//! the returned [`Redaction`] list (one entry per kind, sorted by kind).
//!
//! Rules apply in a fixed order: private-key blocks, vault secrets, token
//! formats, URL credentials, then `key=value`. Markers contain no character
//! sequence a later rule looks for, and values that are already a marker are
//! skipped, so redacting the output again changes nothing and counts nothing.

mod patterns;
mod redactor;
mod rules;

pub use redactor::SecretRedactor;

#[cfg(test)]
mod tests;
