//! Run reports as documents someone can read without bssh: text, Markdown, HTML, PDF, JSON.
//!
//! Every document is built from a [`RunReport`], so from the audit log and the change
//! records on disk. Nothing here is written by a model.

mod doc;
mod format;
mod html;
mod markdown;
mod text;

pub use format::{file_name, render, ReportFormat};

#[cfg(test)]
mod tests;
