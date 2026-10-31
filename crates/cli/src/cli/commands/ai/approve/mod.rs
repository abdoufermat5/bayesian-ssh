//! `bssh ai approve`.
//!
//! Connects to every live run socket, lists the approval requests waiting for a human, shows
//! the card (what it does as bssh read it, the commands, "the agent says") and sends the
//! decision. This is the trusted channel: the agent's process can't reach it. Deciding needs
//! a person at a terminal (stdin, stdout and stderr on a TTY), so a pipe can only list or
//! deny; approving is never scriptable.

mod canvas;
mod card;
mod command;
mod detail;
mod effects;
mod grant;
mod listing;
mod prompt;
mod view;
mod words;

#[cfg(test)]
mod card_tests;
#[cfg(test)]
mod fixtures;

pub(crate) use card::render_request;
pub use command::execute;
