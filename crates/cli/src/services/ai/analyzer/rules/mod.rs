//! Rule database: program + subcommand + flags -> risk tags.
//!
//! Data first: `RULES` holds programs whose risk depends on the subcommand or flags,
//! `READ_ONLY` the ones that never change anything. Anything not listed is unknown.
//! Programs with logic that doesn't fit the table live in `special.rs`.
//!
//! Matching rules (first case that applies wins, so list dangerous cases first):
//! - `sub`: consecutive arguments right after the global options, e.g. `"rollout status"`.
//!   Words accept `a|b` alternatives and `*` globs; a trailing `$` word means "no more
//!   positional arguments". `;` separates whole alternative phrases (`config get;cache dir`).
//! - `has` / `lacks`: argument patterns (see `arg_matches`).
//! - A global option the table doesn't list makes the subcommand unknown, never a guess.

mod classify;
mod globals;
mod lists;
mod matching;
mod table;
mod types;

pub use classify::classify;
pub use matching::{arg_matches, glob_match};
pub use types::{Arg, Class};
